use std::borrow::Cow;
use std::collections::HashMap;

/// Fast hash map for awk variables and arrays. Uses FxHash (no DoS resistance,
/// but ~2× faster than SipHash for short string keys typical in awk programs).
pub type AwkMap<K, V> = rustc_hash::FxHashMap<K, V>;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};
use memchr::memmem;
use regex::Regex;

type SharedInputReader = Arc<Mutex<BufReader<Box<dyn Read + Send>>>>;

/// Open two-way pipe to `sh -c` (gawk-style `|&` / `<&`).
pub struct CoprocHandle {
    pub child: Child,
    pub stdin: BufWriter<ChildStdin>,
    pub stdout: BufReader<ChildStdout>,
}

#[derive(Debug, Clone)]
pub enum Value {
    /// Never assigned (missing global, missing function argument, or fresh slot).
    /// String/number contexts treat this like `""` / `0` (same as gawk *untyped*).
    Uninit,
    Str(String),
    Num(f64),
    Array(AwkMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Uninit => String::new(),
            Value::Str(s) => s.clone(),
            Value::Num(n) => format_number(*n),
            Value::Array(_) => String::new(),
        }
    }

    /// For `&str` APIs (e.g. `gsub`) without allocating when the value is already `Str`.
    #[inline]
    pub fn as_str_cow(&self) -> Cow<'_, str> {
        match self {
            Value::Uninit => Cow::Borrowed(""),
            Value::Str(s) => Cow::Borrowed(s.as_str()),
            Value::Num(n) => Cow::Owned(format_number(*n)),
            Value::Array(_) => Cow::Borrowed(""),
        }
    }

    /// Borrow the inner string without cloning. Returns `None` for Num/Array.
    #[inline]
    #[allow(dead_code)]
    pub fn str_ref(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Write the string representation directly into a byte buffer — zero allocation
    /// for the Str case, one `write!` for Num.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        match self {
            Value::Uninit => {}
            Value::Str(s) => buf.extend_from_slice(s.as_bytes()),
            Value::Num(n) => {
                use std::io::Write;
                let n = *n;
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(buf, "{}", n as i64);
                } else {
                    let _ = write!(buf, "{n}");
                }
            }
            Value::Array(_) => {}
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) => *n,
            Value::Str(s) => parse_number(s),
            Value::Array(_) => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty() && s.parse::<f64>().map(|n| n != 0.0).unwrap_or(true),
            Value::Array(a) => !a.is_empty(),
        }
    }

    /// Take ownership of the inner String, converting numbers to string form.
    /// Avoids clone when the Value is already a Str variant.
    #[inline]
    pub fn into_string(self) -> String {
        match self {
            Value::Uninit => String::new(),
            Value::Str(s) => s,
            Value::Num(n) => format_number(n),
            Value::Array(_) => String::new(),
        }
    }

    /// Append this value's string representation to an existing String.
    /// Avoids intermediate allocation compared to `format!("{a}{b}")`.
    #[inline]
    pub fn append_to_string(&self, buf: &mut String) {
        match self {
            Value::Uninit => {}
            Value::Str(s) => buf.push_str(s),
            Value::Num(n) => {
                use std::fmt::Write;
                let n = *n;
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(buf, "{}", n as i64);
                } else {
                    let _ = write!(buf, "{n}");
                }
            }
            Value::Array(_) => {}
        }
    }

    /// POSIX-style: true if the value is numeric (including string that looks like number).
    pub fn is_numeric_str(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(_) => true,
            Value::Str(s) => {
                let t = s.trim();
                !t.is_empty() && t.parse::<f64>().is_ok()
            }
            Value::Array(_) => false,
        }
    }
}

/// Format a number to string (awk rules: integer form if no fractional part).
#[inline]
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Parse a string to f64, returning 0.0 for non-numeric. Handles leading/trailing whitespace.
#[inline]
fn parse_number(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    // Hot path: decimal integers (e.g. `seq`, many data columns) without float parsing.
    if let Some(n) = parse_ascii_integer(s) {
        return n as f64;
    }
    s.parse().unwrap_or(0.0)
}

/// Returns `Some(n)` only for strings that are exactly an optional sign + ASCII digits (awk-style int).
#[inline]
fn parse_ascii_integer(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let neg = match b.first().copied() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    if i >= b.len() {
        return None;
    }
    let mut acc: i64 = 0;
    while i < b.len() {
        let d = b[i];
        if !d.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((d - b'0') as i64)?;
        i += 1;
    }
    Some(if neg { -acc } else { acc })
}

/// Split `record` using gawk-style **FPAT** (each regex match is one field).
/// Returns `false` if `fpat` is not a valid regex (caller may fall back to FS).
fn split_fields_fpat(record: &str, fpat: &str, field_ranges: &mut Vec<(u32, u32)>) -> bool {
    field_ranges.clear();
    match Regex::new(fpat) {
        Ok(re) => {
            for m in re.find_iter(record) {
                field_ranges.push((m.start() as u32, m.end() as u32));
            }
            true
        }
        Err(_) => false,
    }
}

/// gawk `--csv` / `-k` field splitting: comma-separated, `"..."` for quoting, `""` for a literal `"`.
/// Field ranges are **value** byte ranges (no surrounding quote characters), matching gawk’s `$n` text.
fn split_csv_gawk_fields(record: &str, field_ranges: &mut Vec<(u32, u32)>) {
    field_ranges.clear();
    let bytes = record.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if bytes[i] == b',' {
            field_ranges.push((i as u32, i as u32));
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            i += 1;
            let val_start = i;
            while i < n {
                if bytes[i] == b'"' {
                    if i + 1 < n && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            let val_end = i;
            field_ranges.push((val_start as u32, val_end as u32));
            if i < n && bytes[i] == b'"' {
                i += 1;
            }
        } else {
            let val_start = i;
            while i < n && bytes[i] != b',' {
                i += 1;
            }
            field_ranges.push((val_start as u32, i as u32));
        }
        if i < n && bytes[i] == b',' {
            i += 1;
            if i == n {
                field_ranges.push((n as u32, n as u32));
            }
        }
    }
}

/// Split `record` into `field_ranges` (replaces contents). Shared by lazy split and stdin path.
fn split_fields_into(record: &str, fs: &str, field_ranges: &mut Vec<(u32, u32)>) {
    field_ranges.clear();
    if fs.is_empty() {
        for (i, c) in record.char_indices() {
            field_ranges.push((i as u32, (i + c.len_utf8()) as u32));
        }
    } else if fs == " " {
        let bytes = record.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        while i < len {
            let start = i;
            while i < len && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            field_ranges.push((start as u32, i as u32));
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
    } else if fs.len() == 1 {
        let sep = fs.as_bytes()[0];
        let bytes = record.as_bytes();
        let mut start = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == sep {
                field_ranges.push((start as u32, i as u32));
                start = i + 1;
            }
        }
        field_ranges.push((start as u32, bytes.len() as u32));
    } else {
        // POSIX: multi-character FS is treated as a regular expression.
        match Regex::new(fs) {
            Ok(re) => {
                let mut last = 0;
                for m in re.find_iter(record) {
                    field_ranges.push((last as u32, m.start() as u32));
                    last = m.end();
                }
                field_ranges.push((last as u32, record.len() as u32));
            }
            Err(_) => {
                // Fall back to literal split if the FS is not a valid regex.
                let mut pos = 0;
                for part in record.split(fs) {
                    let end = pos + part.len();
                    field_ranges.push((pos as u32, end as u32));
                    pos = end + fs.len();
                }
            }
        }
    }
}

pub struct Runtime {
    pub vars: AwkMap<String, Value>,
    /// Post-`BEGIN` globals shared across parallel record workers (`Arc` clone is O(1)).
    /// Reads resolve `vars` first (per-record overlay), then this map. Not used in the main thread.
    pub global_readonly: Option<Arc<AwkMap<String, Value>>>,
    /// Owned field strings — only populated when a field is modified via `set_field`.
    pub fields: Vec<String>,
    /// Zero-copy field byte-ranges into `record`. Each `(start, end)` is a byte offset.
    pub field_ranges: Vec<(u32, u32)>,
    /// True when `set_field` has been called and `fields` vec is authoritative.
    pub fields_dirty: bool,
    /// True when record has been set but fields have not been split yet.
    pub fields_pending_split: bool,
    /// Cached FS for lazy field splitting.
    pub cached_fs: String,
    pub record: String,
    /// Reusable buffer for input line reading (avoids per-line allocation).
    pub line_buf: Vec<u8>,
    pub nr: f64,
    pub fnr: f64,
    pub filename: String,
    /// Set by `exit`; END rules run before process exit (POSIX).
    pub exit_pending: bool,
    pub exit_code: i32,
    /// Primary input stream for `getline` without `< file` (same as main record loop).
    pub input_reader: Option<SharedInputReader>,
    /// Open files for `getline < path` / `close`.
    pub file_handles: HashMap<String, BufReader<File>>,
    /// Open files for `print … > path` / `print … >> path` / `fflush` / `close`.
    pub output_handles: HashMap<String, BufWriter<File>>,
    /// `print`/`printf` `| "cmd"` — stdin of `sh -c cmd` (key is the command string).
    pub pipe_stdin: HashMap<String, BufWriter<ChildStdin>>,
    pub pipe_children: HashMap<String, Child>,
    /// `print`/`printf` `|& "cmd"` / `getline <& "cmd"` — two-way `sh -c` (same key for both directions).
    pub coproc_handles: HashMap<String, CoprocHandle>,
    pub rand_seed: u64,
    /// Radix for `%f` / `%g` / etc. and `print` of numbers when `-N` / `--use-lc-numeric` is set (Unix).
    pub numeric_decimal: char,
    /// Indexed variable slots for the bytecode VM (fast Vec access instead of HashMap).
    pub slots: Vec<Value>,
    /// Compiled regex cache — avoids recompiling the same pattern every record.
    pub regex_cache: AwkMap<String, Regex>,
    /// Cached substring searchers for literal `sub`/`gsub` patterns — faster than `str::contains` per line.
    pub memmem_finder_cache: AwkMap<String, memmem::Finder<'static>>,
    /// Persistent stdout buffer — shared across record iterations, flushed at file boundaries.
    pub print_buf: Vec<u8>,
    /// Cached OFS bytes — avoids HashMap lookup + Vec alloc on every `print` call.
    pub ofs_bytes: Vec<u8>,
    /// Cached ORS bytes — avoids HashMap lookup + Vec alloc on every `print` call.
    pub ors_bytes: Vec<u8>,
    /// Reusable VM stack — avoids malloc/free per VmCtx creation.
    pub vm_stack: Vec<Value>,
    /// `-k` / `--csv` (gawk-style): use [`split_csv_gawk_fields`] instead of `FPAT` / `FS` for `$n`.
    pub csv_mode: bool,
}

impl Runtime {
    pub fn new() -> Self {
        let mut vars = AwkMap::default();
        vars.insert("OFS".into(), Value::Str(" ".into()));
        vars.insert("ORS".into(), Value::Str("\n".into()));
        vars.insert("OFMT".into(), Value::Str("%.6g".into()));
        // POSIX octal \034 — multidimensional array subscript separator
        vars.insert("SUBSEP".into(), Value::Str("\x1c".into()));
        // Empty FPAT means use FS for field splitting (gawk).
        vars.insert("FPAT".into(), Value::Str(String::new()));
        Self {
            vars,
            global_readonly: None,
            fields: Vec::new(),
            field_ranges: Vec::new(),
            fields_dirty: false,
            fields_pending_split: false,
            cached_fs: " ".into(),
            record: String::new(),
            line_buf: Vec::with_capacity(256),
            nr: 0.0,
            fnr: 0.0,
            filename: String::new(),
            exit_pending: false,
            exit_code: 0,
            input_reader: None,
            file_handles: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed: 1,
            numeric_decimal: '.',
            slots: Vec::new(),
            regex_cache: AwkMap::default(),
            memmem_finder_cache: AwkMap::default(),
            print_buf: Vec::with_capacity(65536),
            ofs_bytes: b" ".to_vec(),
            ors_bytes: b"\n".to_vec(),
            vm_stack: Vec::with_capacity(64),
            csv_mode: false,
        }
    }

    /// Initialize POSIX **`ARGC`** / **`ARGV`**: **`ARGV[0]`** is the process name; **`ARGV[1..]`** are input file paths (none when reading stdin only).
    pub fn init_argv(&mut self, files: &[std::path::PathBuf]) {
        use std::env;
        let bin = env::args().next().unwrap_or_else(|| "awkrs".to_string());
        let mut argv = vec![bin];
        for f in files {
            argv.push(f.to_string_lossy().into_owned());
        }
        let argc = argv.len();
        self.vars.insert("ARGC".into(), Value::Num(argc as f64));
        let mut map = AwkMap::default();
        for (i, s) in argv.iter().enumerate() {
            map.insert(i.to_string(), Value::Str(s.clone()));
        }
        self.vars.insert("ARGV".into(), Value::Array(map));
    }

    /// Worker runtime for parallel record processing: empty overlay `vars`, shared read-only globals.
    pub fn for_parallel_worker(
        shared_globals: Arc<AwkMap<String, Value>>,
        filename: String,
        rand_seed: u64,
        numeric_decimal: char,
        csv_mode: bool,
    ) -> Self {
        Self {
            vars: AwkMap::default(),
            global_readonly: Some(shared_globals),
            fields: Vec::new(),
            field_ranges: Vec::new(),
            fields_dirty: false,
            fields_pending_split: false,
            cached_fs: " ".into(),
            record: String::new(),
            line_buf: Vec::new(),
            nr: 0.0,
            fnr: 0.0,
            filename,
            exit_pending: false,
            exit_code: 0,
            input_reader: None,
            file_handles: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed,
            numeric_decimal,
            slots: Vec::new(),
            regex_cache: AwkMap::default(),
            memmem_finder_cache: AwkMap::default(),
            print_buf: Vec::new(),
            ofs_bytes: b" ".to_vec(),
            ors_bytes: b"\n".to_vec(),
            vm_stack: Vec::with_capacity(64),
            csv_mode,
        }
    }

    /// Ensure a regex is compiled and cached. Call before `regex_ref()`.
    pub fn ensure_regex(&mut self, pat: &str) -> std::result::Result<(), String> {
        if !self.regex_cache.contains_key(pat) {
            let re = Regex::new(pat).map_err(|e| e.to_string())?;
            self.regex_cache.insert(pat.to_string(), re);
        }
        Ok(())
    }

    /// Get a cached regex (must call `ensure_regex` first).
    pub fn regex_ref(&self, pat: &str) -> &Regex {
        &self.regex_cache[pat]
    }

    /// Cached [`memmem::Finder`] for a literal pattern string (non-empty).
    /// Used by literal `gsub`/`sub` to scan records with SIMD-friendly substring search.
    pub fn literal_substring_finder(&mut self, pat: &str) -> &memmem::Finder<'static> {
        if !self.memmem_finder_cache.contains_key(pat) {
            let f = memmem::Finder::new(pat.as_bytes()).into_owned();
            self.memmem_finder_cache.insert(pat.to_string(), f);
        }
        &self.memmem_finder_cache[pat]
    }

    /// Resolve a global name: per-record overlay, then shared `BEGIN` snapshot.
    pub fn get_global_var(&self, name: &str) -> Option<&Value> {
        self.vars
            .get(name)
            .or_else(|| self.global_readonly.as_ref()?.get(name))
    }

    /// `print … | "cmd"` / `printf … | "cmd"` — append bytes to the coprocess stdin (spawn on first use).
    pub fn write_pipe_line(&mut self, cmd: &str, data: &str) -> Result<()> {
        if self.coproc_handles.contains_key(cmd) {
            return Err(Error::Runtime(format!(
                "one-way pipe `|` conflicts with two-way `|&` for `{cmd}`"
            )));
        }
        if !self.pipe_stdin.contains_key(cmd) {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| Error::Runtime(format!("pipe `{cmd}`: {e}")))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| Error::Runtime(format!("pipe `{cmd}`: no stdin")))?;
            self.pipe_children.insert(cmd.to_string(), child);
            self.pipe_stdin
                .insert(cmd.to_string(), BufWriter::new(stdin));
        }
        let w = self.pipe_stdin.get_mut(cmd).unwrap();
        w.write_all(data.as_bytes()).map_err(Error::Io)?;
        Ok(())
    }

    fn ensure_coproc(&mut self, cmd: &str) -> Result<()> {
        if self.coproc_handles.contains_key(cmd) {
            return Ok(());
        }
        if self.pipe_stdin.contains_key(cmd) {
            return Err(Error::Runtime(format!(
                "two-way pipe `|&` conflicts with one-way `|` for `{cmd}`"
            )));
        }
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Runtime(format!("coprocess `{cmd}`: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Runtime(format!("coprocess `{cmd}`: no stdin")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Runtime(format!("coprocess `{cmd}`: no stdout")))?;
        self.coproc_handles.insert(
            cmd.to_string(),
            CoprocHandle {
                child,
                stdin: BufWriter::new(stdin),
                stdout: BufReader::new(stdout),
            },
        );
        Ok(())
    }

    /// `print … |& "cmd"` / `printf … |& "cmd"` — append bytes to the two-way pipe stdin.
    pub fn write_coproc_line(&mut self, cmd: &str, data: &str) -> Result<()> {
        self.ensure_coproc(cmd)?;
        let w = self.coproc_handles.get_mut(cmd).unwrap();
        w.stdin.write_all(data.as_bytes()).map_err(Error::Io)?;
        Ok(())
    }

    /// `getline … <& "cmd"` — one line from the coprocess stdout.
    pub fn read_line_coproc(&mut self, cmd: &str) -> Result<Option<String>> {
        self.ensure_coproc(cmd)?;
        let h = self.coproc_handles.get_mut(cmd).unwrap();
        let mut line = String::new();
        let n = h.stdout.read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }

    /// Write one `print` line (including `ORS`) to `path`. First open uses truncate (`>`) or
    /// append (`>>`); later writes reuse the same handle until `close`.
    pub fn write_output_line(&mut self, path: &str, data: &str, append: bool) -> Result<()> {
        self.ensure_output_writer(path, append)?;
        let w = self.output_handles.get_mut(path).unwrap();
        w.write_all(data.as_bytes()).map_err(Error::Io)?;
        Ok(())
    }

    fn ensure_output_writer(&mut self, path: &str, append: bool) -> Result<()> {
        if self.output_handles.contains_key(path) {
            return Ok(());
        }
        let f = if append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
        }
        .map_err(|e| Error::Runtime(format!("open {path}: {e}")))?;
        self.output_handles
            .insert(path.to_string(), BufWriter::new(f));
        Ok(())
    }

    /// Flush buffered output for a file or pipe opened with `print`/`printf` redirection.
    pub fn flush_redirect_target(&mut self, key: &str) -> Result<()> {
        if let Some(w) = self.output_handles.get_mut(key) {
            w.flush().map_err(Error::Io)?;
            return Ok(());
        }
        if let Some(w) = self.pipe_stdin.get_mut(key) {
            w.flush().map_err(Error::Io)?;
            return Ok(());
        }
        if let Some(h) = self.coproc_handles.get_mut(key) {
            h.stdin.flush().map_err(Error::Io)?;
            return Ok(());
        }
        Err(Error::Runtime(format!(
            "fflush: {key} is not an open output file, pipe, or coprocess"
        )))
    }

    pub fn attach_input_reader(&mut self, r: SharedInputReader) {
        self.input_reader = Some(r);
    }

    pub fn detach_input_reader(&mut self) {
        self.input_reader = None;
    }

    /// Next line from the primary input stream (used by `getline` with no redirection).
    pub fn read_line_primary(&mut self) -> Result<Option<String>> {
        let Some(r) = &self.input_reader else {
            return Err(Error::Runtime(
                "`getline` with no file is only valid during normal input".into(),
            ));
        };
        let mut line = String::new();
        let mut guard = r
            .lock()
            .map_err(|_| Error::Runtime("input reader lock poisoned".into()))?;
        let n = guard.read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }

    /// `getline var < filename` — one line from a kept-open file handle.
    pub fn read_line_file(&mut self, path: &str) -> Result<Option<String>> {
        let p = Path::new(path);
        if !self.file_handles.contains_key(path) {
            let f = File::open(p).map_err(|e| Error::Runtime(format!("open {path}: {e}")))?;
            self.file_handles
                .insert(path.to_string(), BufReader::new(f));
        }
        let reader = self.file_handles.get_mut(path).unwrap();
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }

    pub fn close_handle(&mut self, path: &str) -> f64 {
        if let Some(h) = self.coproc_handles.remove(path) {
            let _ = shutdown_coproc(h);
        }
        if let Some(mut w) = self.output_handles.remove(path) {
            let _ = w.flush();
        }
        if let Some(mut w) = self.pipe_stdin.remove(path) {
            let _ = w.flush();
        }
        if let Some(mut ch) = self.pipe_children.remove(path) {
            let _ = ch.wait();
        }
        let _ = self.file_handles.remove(path);
        0.0
    }

    pub fn rand(&mut self) -> f64 {
        self.rand_seed = self.rand_seed.wrapping_mul(1103515245).wrapping_add(12345);
        f64::from((self.rand_seed >> 16) as u32 & 0x7fff) / 32768.0
    }

    pub fn srand(&mut self, n: Option<u32>) -> f64 {
        let prev = self.rand_seed;
        self.rand_seed = n.map(|x| x as u64).unwrap_or(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() ^ (d.subsec_nanos() as u64))
                .unwrap_or(1),
        );
        (prev & 0xffff_ffff) as f64
    }

    pub fn set_field_sep_split(&mut self, fs: &str, line: &str) {
        self.record.clear();
        self.record.push_str(line);
        self.fields_dirty = false;
        self.fields_pending_split = true;
        self.cached_fs.clear();
        self.cached_fs.push_str(fs);
        self.fields.clear();
        self.field_ranges.clear();
    }

    /// Like [`set_field_sep_split`](Self::set_field_sep_split) but takes an owned line (avoids extra
    /// copies when the caller already has a `String`, e.g. `gsub` replacing `$0`).
    pub fn set_field_sep_split_owned(&mut self, fs: &str, line: String) {
        self.record = line;
        self.fields_dirty = false;
        self.fields_pending_split = true;
        self.cached_fs.clear();
        self.cached_fs.push_str(fs);
        self.fields.clear();
        self.field_ranges.clear();
    }

    /// Ensure fields are split. Called lazily before any field access.
    /// Uses **`FPAT`** when set to a non-empty pattern (gawk-style field-by-content); otherwise **`FS`**.
    #[inline]
    pub fn ensure_fields_split(&mut self) {
        if self.fields_pending_split {
            self.fields_pending_split = false;
            self.split_record_fields();
        }
    }

    /// Split `self.record` into `field_ranges` using current **`FPAT`** (if non-empty) or **`FS`**.
    fn split_record_fields(&mut self) {
        let record = self.record.as_str();
        if self.csv_mode {
            split_csv_gawk_fields(record, &mut self.field_ranges);
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                let raw = &record[s as usize..e as usize];
                // CSV doubled-quote escape: `""` → `"` inside a quoted field (gawk / RFC 4180).
                self.fields.push(raw.replace("\"\"", "\""));
            }
            self.fields_dirty = true;
            return;
        }
        let fp_raw = self
            .get_global_var("FPAT")
            .map(|v| v.as_str())
            .unwrap_or_default();
        let fp = fp_raw.trim();
        if !fp.is_empty() {
            if !split_fields_fpat(record, fp, &mut self.field_ranges) {
                let fs_str = self
                    .get_global_var("FS")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| " ".to_string());
                split_fields_into(record, &fs_str, &mut self.field_ranges);
            }
        } else {
            let fs_str = self
                .get_global_var("FS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".to_string());
            split_fields_into(record, &fs_str, &mut self.field_ranges);
        }
    }

    pub fn field(&mut self, i: i32) -> Value {
        if i < 0 {
            return Value::Str(String::new());
        }
        let idx = i as usize;
        if idx == 0 {
            return Value::Str(self.record.clone());
        }
        self.ensure_fields_split();
        if self.fields_dirty {
            self.fields
                .get(idx - 1)
                .cloned()
                .map(Value::Str)
                .unwrap_or_else(|| Value::Str(String::new()))
        } else {
            self.field_ranges
                .get(idx - 1)
                .map(|&(s, e)| Value::Str(self.record[s as usize..e as usize].to_string()))
                .unwrap_or_else(|| Value::Str(String::new()))
        }
    }

    /// Get field value as f64 directly without allocating a String.
    #[inline]
    pub fn field_as_number(&mut self, i: i32) -> f64 {
        if i < 0 {
            return 0.0;
        }
        let idx = i as usize;
        if idx == 0 {
            return parse_number(&self.record);
        }
        self.ensure_fields_split();
        if self.fields_dirty {
            self.fields
                .get(idx - 1)
                .map(|s| parse_number(s))
                .unwrap_or(0.0)
        } else {
            self.field_ranges
                .get(idx - 1)
                .map(|&(s, e)| parse_number(&self.record[s as usize..e as usize]))
                .unwrap_or(0.0)
        }
    }

    /// Write field bytes directly into print_buf without allocating a String.
    /// Uses split borrowing within the method to avoid borrow conflicts.
    #[inline]
    pub fn print_field_to_buf(&mut self, idx: usize) {
        if idx == 0 {
            self.print_buf.extend_from_slice(self.record.as_bytes());
            return;
        }
        self.ensure_fields_split();
        if self.fields_dirty {
            if let Some(s) = self.fields.get(idx - 1) {
                self.print_buf.extend_from_slice(s.as_bytes());
            }
        } else if let Some(&(s, e)) = self.field_ranges.get(idx - 1) {
            self.print_buf
                .extend_from_slice(&self.record.as_bytes()[s as usize..e as usize]);
        }
    }

    /// Get a field as &str without allocating (zero-copy from record).
    #[allow(dead_code)]
    pub fn field_str(&self, i: usize) -> &str {
        if i == 0 {
            return &self.record;
        }
        if self.fields_dirty {
            self.fields.get(i - 1).map(|s| s.as_str()).unwrap_or("")
        } else {
            self.field_ranges
                .get(i - 1)
                .map(|&(s, e)| &self.record[s as usize..e as usize])
                .unwrap_or("")
        }
    }

    /// Number of fields in the current record.
    #[inline]
    #[allow(dead_code)]
    pub fn nf(&mut self) -> usize {
        self.ensure_fields_split();
        if self.fields_dirty {
            self.fields.len()
        } else {
            self.field_ranges.len()
        }
    }

    /// True when `$i` is out of range for the current record (`i >= 1` and `i > NF`).
    #[inline]
    pub fn field_is_unassigned(&mut self, i: i32) -> bool {
        if i < 1 {
            return false;
        }
        (i as usize) > self.nf()
    }

    pub fn set_field(&mut self, i: i32, val: &str) {
        if i < 1 {
            return;
        }
        // Materialize owned fields from ranges if needed
        if !self.fields_dirty {
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                self.fields
                    .push(self.record[s as usize..e as usize].to_string());
            }
            self.fields_dirty = true;
        }
        let idx = (i - 1) as usize;
        if self.fields.len() <= idx {
            self.fields.resize(idx + 1, String::new());
        }
        self.fields[idx] = val.to_string();
        self.rebuild_record();
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
    }

    /// Set a field to a numeric value directly, formatting in-place without
    /// allocating a temporary `Value::Num` and round-tripping through `as_str()`.
    pub fn set_field_num(&mut self, i: i32, n: f64) {
        if i < 1 {
            return;
        }
        if !self.fields_dirty {
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                self.fields
                    .push(self.record[s as usize..e as usize].to_string());
            }
            self.fields_dirty = true;
        }
        let idx = (i - 1) as usize;
        if self.fields.len() <= idx {
            self.fields.resize(idx + 1, String::new());
        }
        // Format number into the existing String, reusing its allocation.
        self.fields[idx].clear();
        if n.fract() == 0.0 && n.abs() < 1e15 {
            use std::fmt::Write;
            let _ = write!(self.fields[idx], "{}", n as i64);
        } else {
            use std::fmt::Write;
            let _ = write!(self.fields[idx], "{n}");
        }
        self.rebuild_record();
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
    }

    fn rebuild_record(&mut self) {
        let ofs = self
            .vars
            .get("OFS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.record = self.fields.join(&ofs);
    }

    pub fn set_record_from_line(&mut self, line: &str) {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let fs = self
            .vars
            .get("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.set_field_sep_split(&fs, trimmed);
    }

    /// Parse the current `line_buf` as a record. Avoids the borrow-checker conflict
    /// of borrowing `line_buf` and calling `set_field_sep_split` simultaneously.
    pub fn set_record_from_line_buf(&mut self) {
        // Trim trailing \n\r
        let mut end = self.line_buf.len();
        while end > 0 && (self.line_buf[end - 1] == b'\n' || self.line_buf[end - 1] == b'\r') {
            end -= 1;
        }
        // Copy the trimmed line into record (reuses allocation)
        self.record.clear();
        // Valid UTF-8 fast path (common for text data)
        match std::str::from_utf8(&self.line_buf[..end]) {
            Ok(s) => self.record.push_str(s),
            Err(_) => {
                let lossy = String::from_utf8_lossy(&self.line_buf[..end]);
                self.record.push_str(&lossy);
            }
        }
        // Split using current FPAT or FS
        self.fields_dirty = false;
        self.fields.clear();
        self.field_ranges.clear();
        self.split_record_fields();
    }

    pub fn array_get(&self, name: &str, key: &str) -> Value {
        match self.get_global_var(name) {
            Some(Value::Array(a)) => a.get(key).cloned().unwrap_or(Value::Str(String::new())),
            _ => Value::Str(String::new()),
        }
    }

    pub fn array_set(&mut self, name: &str, key: String, val: Value) {
        // Fast path: array already exists in vars — no name allocation needed.
        if let Some(existing) = self.vars.get_mut(name) {
            match existing {
                Value::Array(a) => {
                    a.insert(key, val);
                    return;
                }
                _ => {
                    let mut m = AwkMap::default();
                    m.insert(key, val);
                    *existing = Value::Array(m);
                    return;
                }
            }
        }
        // Slow path: first access — copy from readonly globals or create new.
        if let Some(Value::Array(a)) = self.global_readonly.as_ref().and_then(|g| g.get(name)) {
            let mut copy = a.clone();
            copy.insert(key, val);
            self.vars.insert(name.to_string(), Value::Array(copy));
        } else {
            let mut m = AwkMap::default();
            m.insert(key, val);
            self.vars.insert(name.to_string(), Value::Array(m));
        }
    }

    pub fn array_delete(&mut self, name: &str, key: Option<&str>) {
        if let Some(k) = key {
            if let Some(Value::Array(a)) = self.vars.get_mut(name) {
                a.remove(k);
            } else if let Some(Value::Array(a)) =
                self.global_readonly.as_ref().and_then(|g| g.get(name))
            {
                let mut copy = a.clone();
                copy.remove(k);
                self.vars.insert(name.to_string(), Value::Array(copy));
            }
        } else {
            self.vars.remove(name);
            if self
                .global_readonly
                .as_ref()
                .is_some_and(|g| g.contains_key(name))
            {
                self.vars
                    .insert(name.to_string(), Value::Array(AwkMap::default()));
            }
        }
    }

    pub fn array_keys(&self, name: &str) -> Vec<String> {
        match self.get_global_var(name) {
            Some(Value::Array(a)) => a.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// `key in arr` — true iff `arr` is an array that has `key` (POSIX: subscript was used).
    pub fn array_has(&self, name: &str, key: &str) -> bool {
        match self.get_global_var(name) {
            Some(Value::Array(a)) => a.contains_key(key),
            _ => false,
        }
    }

    pub fn split_into_array(&mut self, arr_name: &str, parts: &[String]) {
        self.array_delete(arr_name, None);
        for (i, p) in parts.iter().enumerate() {
            self.array_set(arr_name, format!("{}", i + 1), Value::Str(p.clone()));
        }
    }
}

fn shutdown_coproc(mut h: CoprocHandle) -> Result<()> {
    h.stdin.flush().map_err(Error::Io)?;
    drop(h.stdin);
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = h.stdout.read_line(&mut buf).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
    }
    drop(h.stdout);
    let _ = h.child.wait();
    Ok(())
}

impl Clone for Runtime {
    fn clone(&self) -> Self {
        Self {
            vars: self.vars.clone(),
            global_readonly: self.global_readonly.clone(),
            fields: self.fields.clone(),
            field_ranges: self.field_ranges.clone(),
            fields_dirty: self.fields_dirty,
            fields_pending_split: self.fields_pending_split,
            cached_fs: self.cached_fs.clone(),
            record: self.record.clone(),
            line_buf: Vec::new(),
            nr: self.nr,
            fnr: self.fnr,
            filename: self.filename.clone(),
            exit_pending: self.exit_pending,
            exit_code: self.exit_code,
            input_reader: None,
            file_handles: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed: self.rand_seed,
            numeric_decimal: self.numeric_decimal,
            slots: self.slots.clone(),
            regex_cache: self.regex_cache.clone(),
            memmem_finder_cache: self.memmem_finder_cache.clone(),
            print_buf: Vec::new(),
            ofs_bytes: self.ofs_bytes.clone(),
            ors_bytes: self.ors_bytes.clone(),
            vm_stack: Vec::with_capacity(64),
            csv_mode: self.csv_mode,
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        for (_, h) in self.coproc_handles.drain() {
            let _ = shutdown_coproc(h);
        }
        for (_, mut w) in self.output_handles.drain() {
            let _ = w.flush();
        }
        for (_, mut w) in self.pipe_stdin.drain() {
            let _ = w.flush();
        }
        for (_, mut ch) in self.pipe_children.drain() {
            let _ = ch.wait();
        }
    }
}

#[cfg(test)]
mod value_tests {
    use super::Value;

    #[test]
    fn value_as_number_from_int_string() {
        assert_eq!(Value::Str("42".into()).as_number(), 42.0);
    }

    #[test]
    fn value_as_number_empty_string_zero() {
        assert_eq!(Value::Str("".into()).as_number(), 0.0);
    }

    #[test]
    fn value_truthy_numeric_string_zero() {
        assert!(!Value::Str("0".into()).truthy());
    }

    #[test]
    fn value_truthy_non_numeric_string() {
        assert!(Value::Str("hello".into()).truthy());
    }

    #[test]
    fn value_truthy_nonempty_array() {
        let mut m = super::AwkMap::default();
        m.insert("k".into(), Value::Num(1.0));
        assert!(Value::Array(m).truthy());
    }

    #[test]
    fn value_is_numeric_str_detects_decimal() {
        assert!(Value::Str("3.14".into()).is_numeric_str());
        assert!(!Value::Str("x".into()).is_numeric_str());
    }

    #[test]
    fn value_append_to_string_concat() {
        let mut buf = String::from("a");
        Value::Str("b".into()).append_to_string(&mut buf);
        Value::Num(7.0).append_to_string(&mut buf);
        assert_eq!(buf, "ab7");
    }

    #[test]
    fn value_into_string_from_num_integer_form() {
        assert_eq!(Value::Num(12.0).into_string(), "12");
    }

    #[test]
    fn value_write_to_buf_str_and_num() {
        let mut v = Vec::new();
        Value::Str("ok".into()).write_to(&mut v);
        Value::Num(5.0).write_to(&mut v);
        assert_eq!(v, b"ok5");
    }

    #[test]
    fn value_truthy_num_zero() {
        assert!(!Value::Num(0.0).truthy());
    }

    #[test]
    fn value_truthy_num_nonzero() {
        assert!(Value::Num(-3.0).truthy());
    }

    #[test]
    fn value_empty_array_not_truthy() {
        let m = super::AwkMap::default();
        assert!(!Value::Array(m).truthy());
    }

    #[test]
    fn value_as_number_negative_float_string() {
        assert_eq!(Value::Str("-2.5".into()).as_number(), -2.5);
    }

    #[test]
    fn value_as_number_scientific_notation_string() {
        assert_eq!(Value::Str("1e2".into()).as_number(), 100.0);
    }

    #[test]
    fn value_into_string_float_fraction() {
        let s = Value::Num(0.25).into_string();
        assert!(s.contains('2') && s.contains('5'), "{s}");
    }

    #[test]
    fn csv_mode_quoted_comma_three_fields() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", r#"a,"b,c",d"#);
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
        assert_eq!(rt.field(1).as_str(), "a");
        assert_eq!(rt.field(2).as_str(), "b,c");
        assert_eq!(rt.field(3).as_str(), "d");
    }

    #[test]
    fn csv_mode_escape_double_quote_in_field() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", "\"a\"\"b\"");
        rt.ensure_fields_split();
        assert_eq!(rt.field(1).as_str(), "a\"b");
    }

    #[test]
    fn csv_mode_trailing_comma_empty_field() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", "a,");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 2);
        assert_eq!(rt.field(1).as_str(), "a");
        assert_eq!(rt.field(2).as_str(), "");
    }
}
