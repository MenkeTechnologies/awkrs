//! Pure-Rust implementations of APIs traditionally shipped as gawk loadable extensions
//! (`filefuncs`, `time`, `ordchr`, `readfile`, `rwarray`, etc.). Call these as ordinary
//! builtins; `@load "filefuncs.so"` is not required in awkrs.

use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `chdir(path)` — return 0 on success, -1 on failure (sets **`ERRNO`**).
pub(crate) fn chdir(rt: &mut Runtime, path: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    match std::env::set_current_dir(path) {
        Ok(()) => Ok(Value::Num(0.0)),
        Err(e) => {
            rt.set_errno_io(&e);
            Ok(Value::Num(-1.0))
        }
    }
}

/// `stat(path, arr)` — populate **`arr`** with file metadata; return 0 or -1.
pub(crate) fn stat(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            rt.set_errno_io(&e);
            return Ok(Value::Num(-1.0));
        }
    };
    rt.array_delete(arr_name, None);
    let file_type = if meta.is_dir() {
        "directory"
    } else if meta.is_symlink() {
        "symlink"
    } else if meta.is_file() {
        "file"
    } else {
        "other"
    };
    rt.array_set(arr_name, "type".into(), Value::Str(file_type.into()));
    rt.array_set(arr_name, "size".into(), Value::Num(meta.len() as f64));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        rt.array_set(arr_name, "dev".into(), Value::Num(meta.dev() as f64));
        rt.array_set(arr_name, "ino".into(), Value::Num(meta.ino() as f64));
        rt.array_set(arr_name, "mode".into(), Value::Num(meta.mode() as f64));
        rt.array_set(arr_name, "nlink".into(), Value::Num(meta.nlink() as f64));
        rt.array_set(arr_name, "uid".into(), Value::Num(meta.uid() as f64));
        rt.array_set(arr_name, "gid".into(), Value::Num(meta.gid() as f64));
        rt.array_set(arr_name, "rdev".into(), Value::Num(meta.rdev() as f64));
        rt.array_set(
            arr_name,
            "blksize".into(),
            Value::Num(meta.blksize() as f64),
        );
        rt.array_set(arr_name, "blocks".into(), Value::Num(meta.blocks() as f64));
        rt.array_set(arr_name, "atime".into(), Value::Num(meta.atime() as f64));
        rt.array_set(arr_name, "mtime".into(), Value::Num(meta.mtime() as f64));
        rt.array_set(arr_name, "ctime".into(), Value::Num(meta.ctime() as f64));
    }
    #[cfg(not(unix))]
    {
        rt.array_set(arr_name, "dev".into(), Value::Num(0.0));
        rt.array_set(arr_name, "ino".into(), Value::Num(0.0));
        rt.array_set(arr_name, "mode".into(), Value::Num(0.0));
        rt.array_set(arr_name, "nlink".into(), Value::Num(1.0));
        rt.array_set(arr_name, "uid".into(), Value::Num(0.0));
        rt.array_set(arr_name, "gid".into(), Value::Num(0.0));
        rt.array_set(arr_name, "rdev".into(), Value::Num(0.0));
        rt.array_set(arr_name, "blksize".into(), Value::Num(0.0));
        rt.array_set(arr_name, "blocks".into(), Value::Num(0.0));
        if let Ok(t) = meta.accessed() {
            rt.array_set(
                arr_name,
                "atime".into(),
                Value::Num(
                    t.duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0),
                ),
            );
        }
        if let Ok(t) = meta.modified() {
            rt.array_set(
                arr_name,
                "mtime".into(),
                Value::Num(
                    t.duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0),
                ),
            );
        }
    }
    Ok(Value::Num(0.0))
}

/// `statvfs(path, arr)` — Unix only; returns -1 on unsupported platforms or errors.
pub(crate) fn statvfs(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;
        let c = CString::new(path).map_err(|_| Error::Runtime("statvfs: path".into()))?;
        let mut v: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
        let r = unsafe { libc::statvfs(c.as_ptr(), v.as_mut_ptr()) };
        if r != 0 {
            let e = std::io::Error::last_os_error();
            rt.set_errno_io(&e);
            return Ok(Value::Num(-1.0));
        }
        let v = unsafe { v.assume_init() };
        rt.array_delete(arr_name, None);
        rt.array_set(arr_name, "f_bsize".into(), Value::Num(v.f_bsize as f64));
        rt.array_set(arr_name, "f_frsize".into(), Value::Num(v.f_frsize as f64));
        rt.array_set(arr_name, "f_blocks".into(), Value::Num(v.f_blocks as f64));
        rt.array_set(arr_name, "f_bfree".into(), Value::Num(v.f_bfree as f64));
        rt.array_set(arr_name, "f_bavail".into(), Value::Num(v.f_bavail as f64));
        rt.array_set(arr_name, "f_files".into(), Value::Num(v.f_files as f64));
        rt.array_set(arr_name, "f_ffree".into(), Value::Num(v.f_ffree as f64));
        rt.array_set(arr_name, "f_favail".into(), Value::Num(v.f_favail as f64));
        rt.array_set(arr_name, "f_fsid".into(), Value::Num(0.0));
        rt.array_set(arr_name, "f_flag".into(), Value::Num(v.f_flag as f64));
        rt.array_set(arr_name, "f_namemax".into(), Value::Num(v.f_namemax as f64));
        Ok(Value::Num(0.0))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, arr_name);
        rt.set_errno_str("statvfs: not supported on this platform");
        Ok(Value::Num(-1.0))
    }
}

/// `fts(root, arr)` — recursive directory walk; fills **`arr[1]`…`arr[n]`** with paths (sorted).
pub(crate) fn fts(rt: &mut Runtime, root: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    let root_path = Path::new(root);
    if !root_path.exists() {
        rt.set_errno_str("fts: path does not exist");
        return Ok(Value::Num(-1.0));
    }
    let mut paths: Vec<String> = Vec::new();
    let walker = walkdir::WalkDir::new(root_path).follow_links(false);
    for e in walker.into_iter().filter_map(|e| e.ok()) {
        paths.push(e.path().to_string_lossy().into_owned());
    }
    paths.sort();
    rt.split_into_array(arr_name, &paths);
    Ok(Value::Num(paths.len() as f64))
}

/// `gettimeofday(arr)` — sets **`sec`** and **`usec`** (fractional epoch).
pub(crate) fn gettimeofday(rt: &mut Runtime, arr_name: &str) -> Result<Value> {
    rt.clear_errno();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    rt.array_delete(arr_name, None);
    rt.array_set(arr_name, "sec".into(), Value::Num(now.as_secs_f64()));
    rt.array_set(
        arr_name,
        "usec".into(),
        Value::Num(now.subsec_micros() as f64),
    );
    Ok(Value::Num(0.0))
}

/// `sleep(sec)` — sleep for a fractional number of seconds.
pub(crate) fn sleep_secs(_rt: &mut Runtime, sec: f64) -> Result<Value> {
    if sec < 0.0 {
        return Err(Error::Runtime("sleep: negative duration".into()));
    }
    std::thread::sleep(Duration::from_secs_f64(sec));
    Ok(Value::Num(0.0))
}

/// `ord(str)` — numeric codepoint of the first character (0 if empty).
pub(crate) fn ord(_rt: &mut Runtime, s: &str) -> Result<Value> {
    let n = s.chars().next().map(|c| c as u32).unwrap_or(0);
    Ok(Value::Num(f64::from(n)))
}

/// `chr(n)` — single UTF-32 character as string (empty if invalid).
pub(crate) fn chr(_rt: &mut Runtime, n: f64) -> Result<Value> {
    let u = n as u32;
    let s = char::from_u32(u).map(|c| c.to_string()).unwrap_or_default();
    Ok(Value::Str(s))
}

/// `readfile(path)` — read entire file as a string (empty on failure; **`ERRNO`** set).
pub(crate) fn readfile(rt: &mut Runtime, path: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    match fs::read_to_string(path) {
        Ok(s) => Ok(Value::Str(s)),
        Err(e) => {
            rt.set_errno_io(&e);
            Ok(Value::Str(String::new()))
        }
    }
}

/// `revoutput(s)` / demo: reverse a string (Unicode scalar order).
pub(crate) fn revoutput(_rt: &mut Runtime, s: &str) -> Result<Value> {
    Ok(Value::Str(s.chars().rev().collect()))
}

/// Same as [`revoutput`] (gawk `revtwoway` demo).
pub(crate) fn revtwoway(rt: &mut Runtime, s: &str) -> Result<Value> {
    revoutput(rt, s)
}

/// `rename(old, new)` — return 0 on success, -1 on failure.
pub(crate) fn rename(rt: &mut Runtime, old: &str, new: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    match fs::rename(old, new) {
        Ok(()) => Ok(Value::Num(0.0)),
        Err(e) => {
            rt.set_errno_io(&e);
            Ok(Value::Num(-1.0))
        }
    }
}

/// `inplace_tmpfile(path)` — unique temp path in the same directory as **`path`** (for safe edit + rename).
pub(crate) fn inplace_tmpfile(rt: &mut Runtime, path: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    let p = Path::new(path);
    let dir = p
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{}.awkrs_inplace.{}",
        p.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
        stamp
    ));
    let tmp_s = tmp.to_string_lossy().into_owned();
    match File::create(&tmp) {
        Ok(_) => Ok(Value::Str(tmp_s)),
        Err(e) => {
            rt.set_errno_io(&e);
            Ok(Value::Str(String::new()))
        }
    }
}

/// `inplace_commit(tmp, dest)` — atomic `rename(tmp, dest)`.
pub(crate) fn inplace_commit(rt: &mut Runtime, tmp: &str, dest: &str) -> Result<Value> {
    rename(rt, tmp, dest)
}

fn escape_rw(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o
}

fn unescape_rw(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('r') => o.push('\r'),
                Some('t') => o.push('\t'),
                Some('\\') => o.push('\\'),
                Some(x) => {
                    o.push('\\');
                    o.push(x);
                }
                None => o.push('\\'),
            }
        } else {
            o.push(c);
        }
    }
    o
}

/// `writea(filename, arr)` — text format (awkrs **`rwarray`** v1); returns 0 or -1.
pub(crate) fn writea(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    let keys = rt.array_keys(arr_name);
    let mut f = match File::create(path) {
        Ok(f) => f,
        Err(e) => {
            rt.set_errno_io(&e);
            return Ok(Value::Num(-1.0));
        }
    };
    writeln!(f, "awkrs-rwarray-v1").map_err(Error::Io)?;
    for k in keys {
        let v = rt.array_get(arr_name, &k);
        let line = format!("{}\t{}\n", escape_rw(&k), escape_rw(&v.as_str()));
        f.write_all(line.as_bytes()).map_err(Error::Io)?;
    }
    Ok(Value::Num(0.0))
}

/// `reada(filename, arr)` — replaces **`arr`** contents from **`writea`** format.
pub(crate) fn reada(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            rt.set_errno_io(&e);
            return Ok(Value::Num(-1.0));
        }
    };
    let mut reader = BufReader::new(f);
    let mut magic = String::new();
    reader.read_line(&mut magic).map_err(Error::Io)?;
    if magic.trim() != "awkrs-rwarray-v1" {
        rt.set_errno_str("reada: not an awkrs rwarray file");
        return Ok(Value::Num(-1.0));
    }
    rt.array_delete(arr_name, None);
    let mut line = String::new();
    while reader.read_line(&mut line).map_err(Error::Io)? > 0 {
        let s = line.trim_end_matches(['\r', '\n']);
        if s.is_empty() {
            line.clear();
            continue;
        }
        let mut parts = s.splitn(2, '\t');
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("");
        rt.array_set(arr_name, unescape_rw(key), Value::Str(unescape_rw(val)));
        line.clear();
    }
    Ok(Value::Num(0.0))
}

/// `intdiv0(a,b)` — like **`intdiv`** but returns 0 when **`b == 0`** (no error).
pub(crate) fn intdiv0(rt: &mut Runtime, a: &Value, b: &Value) -> Result<Value> {
    match crate::bignum::awk_intdiv_values(a, b, rt) {
        Ok(v) => Ok(v),
        Err(_) => {
            if rt.bignum {
                let prec = rt.mpfr_prec_bits();
                let round = rt.mpfr_round();
                Ok(Value::Mpfr(rug::Float::with_val_round(prec, 0, round).0))
            } else {
                Ok(Value::Num(0.0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    #[test]
    fn ord_chr_roundtrip() {
        let mut rt = Runtime::new();
        let o = ord(&mut rt, "A").unwrap();
        assert_eq!(o.as_number(), 65.0);
        let c = chr(&mut rt, 65.0).unwrap();
        assert_eq!(c.as_str(), "A");
    }

    #[test]
    fn intdiv0_zero_divisor() {
        let mut rt = Runtime::new();
        let v = intdiv0(
            &mut rt,
            &Value::Num(10.0),
            &Value::Num(0.0),
        )
        .unwrap();
        assert_eq!(v.as_number(), 0.0);
    }

    #[test]
    fn writea_reada_roundtrip() {
        let mut rt = Runtime::new();
        rt.array_set("a", "x".into(), Value::Str("hello".into()));
        let dir = std::env::temp_dir();
        let p = dir.join("awkrs_rwarray_test.tmp");
        let _ = std::fs::remove_file(&p);
        writea(&mut rt, p.to_str().unwrap(), "a").unwrap();
        reada(&mut rt, p.to_str().unwrap(), "b").unwrap();
        assert_eq!(rt.array_get("b", "x").as_str(), "hello");
        let _ = std::fs::remove_file(&p);
    }
}
