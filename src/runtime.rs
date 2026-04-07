use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};

type SharedInputReader = Arc<Mutex<BufReader<Box<dyn Read + Send>>>>;

/// Open two-way pipe to `sh -c` (gawk-style `|&` / `<&`).
pub struct CoprocHandle {
    pub child: Child,
    pub stdin: BufWriter<ChildStdin>,
    pub stdout: BufReader<ChildStdout>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Num(f64),
    Array(HashMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Array(_) => "".into(),
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Str(s) => s.parse().unwrap_or(0.0),
            Value::Array(_) => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty() && s.parse::<f64>().map(|n| n != 0.0).unwrap_or(true),
            Value::Array(a) => !a.is_empty(),
        }
    }

    /// POSIX-style: true if the value is numeric (including string that looks like number).
    pub fn is_numeric_str(&self) -> bool {
        match self {
            Value::Num(_) => true,
            Value::Str(s) => {
                let t = s.trim();
                !t.is_empty() && t.parse::<f64>().is_ok()
            }
            Value::Array(_) => false,
        }
    }
}

pub struct Runtime {
    pub vars: HashMap<String, Value>,
    /// Post-`BEGIN` globals shared across parallel record workers (`Arc` clone is O(1)).
    /// Reads resolve `vars` first (per-record overlay), then this map. Not used in the main thread.
    pub global_readonly: Option<Arc<HashMap<String, Value>>>,
    pub fields: Vec<String>,
    pub record: String,
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
}

impl Runtime {
    pub fn new() -> Self {
        let mut vars = HashMap::new();
        vars.insert("OFS".into(), Value::Str(" ".into()));
        vars.insert("ORS".into(), Value::Str("\n".into()));
        vars.insert("OFMT".into(), Value::Str("%.6g".into()));
        // POSIX octal \034 — multidimensional array subscript separator
        vars.insert("SUBSEP".into(), Value::Str("\x1c".into()));
        Self {
            vars,
            global_readonly: None,
            fields: Vec::new(),
            record: String::new(),
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
        }
    }

    /// Worker runtime for parallel record processing: empty overlay `vars`, shared read-only globals.
    pub fn for_parallel_worker(
        shared_globals: Arc<HashMap<String, Value>>,
        filename: String,
        rand_seed: u64,
        numeric_decimal: char,
    ) -> Self {
        Self {
            vars: HashMap::new(),
            global_readonly: Some(shared_globals),
            fields: Vec::new(),
            record: String::new(),
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
        }
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
        self.record = line.to_string();
        if fs.is_empty() {
            self.fields = line.chars().map(|c| c.to_string()).collect();
        } else if fs == " " {
            let mut fields = Vec::with_capacity(line.split_whitespace().count().max(8));
            for w in line.split_whitespace() {
                fields.push(w.to_string());
            }
            self.fields = fields;
        } else {
            let mut fields = Vec::with_capacity(8);
            for p in line.split(fs) {
                fields.push(p.to_string());
            }
            self.fields = fields;
        }
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
    }

    pub fn field(&self, i: i32) -> Value {
        if i < 0 {
            return Value::Str(String::new());
        }
        let idx = i as usize;
        if idx == 0 {
            return Value::Str(self.record.clone());
        }
        self.fields
            .get(idx - 1)
            .cloned()
            .map(Value::Str)
            .unwrap_or_else(|| Value::Str(String::new()))
    }

    pub fn set_field(&mut self, i: i32, val: &str) {
        if i < 1 {
            return;
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

    fn rebuild_record(&mut self) {
        let ofs = self
            .vars
            .get("OFS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.record = self.fields.join(&ofs);
    }

    pub fn set_record_from_line(&mut self, line: &str) {
        let fs = self
            .vars
            .get("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.set_field_sep_split(&fs, line.trim_end_matches(['\n', '\r']));
    }

    pub fn array_get(&self, name: &str, key: &str) -> Value {
        match self.get_global_var(name) {
            Some(Value::Array(a)) => a.get(key).cloned().unwrap_or(Value::Str(String::new())),
            _ => Value::Str(String::new()),
        }
    }

    pub fn array_set(&mut self, name: &str, key: String, val: Value) {
        if !self.vars.contains_key(name) {
            if let Some(Value::Array(a)) = self.global_readonly.as_ref().and_then(|g| g.get(name)) {
                self.vars.insert(name.to_string(), Value::Array(a.clone()));
            }
        }
        let e = self
            .vars
            .entry(name.to_string())
            .or_insert_with(|| Value::Array(HashMap::new()));
        match e {
            Value::Array(a) => {
                a.insert(key, val);
            }
            _ => {
                *e = Value::Array(HashMap::from([(key, val)]));
            }
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
                    .insert(name.to_string(), Value::Array(HashMap::new()));
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
            record: self.record.clone(),
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
