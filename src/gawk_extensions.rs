//! Pure-Rust implementations of APIs traditionally shipped as gawk loadable extensions
//! (`filefuncs`, `time`, `ordchr`, `readfile`, `rwarray`, etc.). Call these as ordinary
//! builtins; `@load "filefuncs.so"` is not required in awkrs.

use crate::awkstr::AwkStr;
use crate::error::{Error, Result};
use crate::runtime::{AwkArray, Runtime, Value};
use std::fs::{self, File};
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
    let parts: Vec<AwkStr> = paths.iter().map(|p| AwkStr::from(p.as_str())).collect();
    rt.split_into_array(arr_name, &parts);
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
    Ok(Value::Str(s.into()))
}

/// `readfile(path)` — read entire file as a string (empty on failure; **`ERRNO`** set).
pub(crate) fn readfile(rt: &mut Runtime, path: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    match fs::read_to_string(path) {
        Ok(s) => Ok(Value::Str(s.into())),
        Err(e) => {
            rt.set_errno_io(&e);
            Ok(Value::Str(String::new().into()))
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
        Ok(_) => Ok(Value::Str(tmp_s.into())),
        Err(e) => {
            rt.set_errno_io(&e);
            Ok(Value::Str(String::new().into()))
        }
    }
}

/// `inplace_commit(tmp, dest)` — atomic `rename(tmp, dest)`.
pub(crate) fn inplace_commit(rt: &mut Runtime, tmp: &str, dest: &str) -> Result<Value> {
    rename(rt, tmp, dest)
}

// ── rwarray: port of gawk's extension/rwarray.c file format (major 4, minor 1) ──
//
// File: the magic `awkrulz\n`, the major and minor version as big-endian u32s,
// then one array. An array is a u32 element count followed by its elements; an
// element is a u32 index length, the index bytes, then a value: a u32 type code
// and its payload. Strings, strnums, regexps and undefined values carry a u32
// length and their bytes, a boolean carries "TRUE" / "FALSE" the same way, a
// double carries a NUL-terminated `%.17g` rendering, a GMP integer is
// `mpz_out_raw` (signed big-endian byte count, then the magnitude), an MPFR
// float is `mpfr_out_str` in base 62 plus a space, and a subarray is an array.

const RW_MAGIC: &[u8] = b"awkrulz\n";
const RW_MAJOR: u32 = 4;
const RW_MINOR: u32 = 1;
const VT_STRING: u32 = 1;
const VT_NUMBER: u32 = 2;
const VT_GMP: u32 = 3;
const VT_MPFR: u32 = 4;
const VT_ARRAY: u32 = 5;
const VT_REGEX: u32 = 6;
const VT_STRNUM: u32 = 7;
const VT_BOOL: u32 = 8;
const VT_UNDEFINED: u32 = 20;

fn rw_put_u32(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_be_bytes());
}

fn rw_put_bytes(out: &mut Vec<u8>, code: u32, bytes: &[u8]) {
    rw_put_u32(out, code);
    rw_put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

fn rw_write_array(out: &mut Vec<u8>, a: &AwkArray) {
    rw_put_u32(out, a.len() as u32);
    for (k, v) in a.iter() {
        rw_put_u32(out, k.as_bytes().len() as u32);
        out.extend_from_slice(k.as_bytes());
        rw_write_value(out, v);
    }
}

fn rw_write_value(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Array(a) => {
            rw_put_u32(out, VT_ARRAY);
            rw_write_array(out, a);
        }
        Value::Num(n) => {
            let mut text = crate::format::awk_sprintf("%.17g", &[Value::Num(*n)])
                .unwrap_or_else(|_| n.to_string())
                .into_bytes();
            text.push(0);
            rw_put_bytes(out, VT_NUMBER, &text);
        }
        Value::Mpfr(f) => match f.to_integer().filter(|_| f.is_integer()) {
            Some(i) => {
                rw_put_u32(out, VT_GMP);
                let mag = i.to_digits::<u8>(rug::integer::Order::MsfBe);
                let len = mag.len() as i32;
                let signed = if i < 0 { -len } else { len };
                out.extend_from_slice(&signed.to_be_bytes());
                out.extend_from_slice(&mag);
            }
            None => {
                rw_put_u32(out, VT_MPFR);
                out.extend_from_slice(mpfr_out_str_base62(f).as_bytes());
                out.push(b' ');
            }
        },
        Value::Str(s) if v.is_numeric_str() => rw_put_bytes(out, VT_STRNUM, s.as_bytes()),
        Value::Str(s) | Value::StrLit(s) => rw_put_bytes(out, VT_STRING, s.as_bytes()),
        Value::Regexp(s) => rw_put_bytes(out, VT_REGEX, s.as_bytes()),
        Value::Uninit => rw_put_bytes(out, VT_UNDEFINED, b""),
    }
}

/// `writea(file, arr)` — gawk's rwarray `writea`: 1 on success, 0 on failure
/// with `ERRNO` set (a partly written file is removed).
pub(crate) fn writea(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    let mut out = RW_MAGIC.to_vec();
    rw_put_u32(&mut out, RW_MAJOR);
    rw_put_u32(&mut out, RW_MINOR);
    match rt.get_global_var(arr_name) {
        Some(Value::Array(a)) => rw_write_array(&mut out, a),
        _ => rw_put_u32(&mut out, 0),
    }
    if let Err(e) = fs::write(path, &out) {
        rt.set_errno_io(&e);
        let _ = fs::remove_file(path);
        return Ok(Value::Num(0.0));
    }
    Ok(Value::Num(1.0))
}

struct RwReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl RwReader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.buf.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn array(&mut self, rt: &Runtime) -> Option<AwkArray> {
        let count = self.u32()?;
        let mut a = AwkArray::new();
        for _ in 0..count {
            let len = self.u32()? as usize;
            let key = self.take(len)?.to_vec();
            let val = self.value(rt)?;
            a.insert_bytes(&key, val);
        }
        Some(a)
    }

    fn value(&mut self, rt: &Runtime) -> Option<Value> {
        let code = self.u32()?;
        match code {
            VT_ARRAY => Some(Value::Array(self.array(rt)?)),
            VT_NUMBER => {
                let len = self.u32()? as usize;
                let text = self.take(len)?;
                let text = text.split(|&b| b == 0).next().unwrap_or_default();
                let n = std::str::from_utf8(text).ok()?.trim().parse::<f64>().unwrap_or(0.0);
                Some(Value::Num(n))
            }
            VT_GMP => {
                let size = i32::from_be_bytes(self.take(4)?.try_into().ok()?);
                let mag = self.take(size.unsigned_abs() as usize)?;
                let mut i = rug::Integer::from_digits(mag, rug::integer::Order::MsfBe);
                if size < 0 {
                    i = -i;
                }
                Some(rw_number(rt, rug::Float::with_val(rt.mpfr_prec_bits().max(i.significant_bits()), i)))
            }
            VT_MPFR => {
                let end = self.buf[self.pos..].iter().position(|&b| b == b' ')?;
                let text = std::str::from_utf8(self.take(end)?).ok()?.to_string();
                self.take(1)?;
                Some(rw_number(rt, mpfr_parse_base62(&text, rt.mpfr_prec_bits())?))
            }
            _ => {
                let len = self.u32()? as usize;
                let bytes = self.take(len)?;
                let s = AwkStr::from(bytes.to_vec());
                Some(match code {
                    VT_STRNUM => Value::Str(s),
                    VT_REGEX => Value::Regexp(s),
                    VT_UNDEFINED => Value::Uninit,
                    VT_BOOL => Value::Num(if bytes == b"TRUE" { 1.0 } else { 0.0 }),
                    // gawk: "treating recovered value with unknown type code
                    // as a string" — VT_STRING lands here too.
                    _ => Value::StrLit(s),
                })
            }
        }
    }
}

/// A GMP or MPFR number read back: kept arbitrary-precision under `-M`, a
/// double otherwise.
fn rw_number(rt: &Runtime, f: rug::Float) -> Value {
    if rt.bignum {
        Value::Mpfr(f)
    } else {
        Value::Num(f.to_f64())
    }
}

/// mpfr's digit alphabet for bases above 36: `0-9`, `A-Z`, `a-z`.
const B62_DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `mpfr_out_str(fp, 62, 0, f, MPFR_RNDN)`: `[-]d.ddd@e` with the digit count
/// `mpfr_get_str` picks for `n = 0` (`1 + ceil(prec * log 2 / log 62)`), the
/// significand rounded to nearest-even, and a decimal exponent of 62; the
/// singular values are `@NaN@`, `@Inf@`, `-@Inf@`, `0`, `-0`.
fn mpfr_out_str_base62(f: &rug::Float) -> String {
    use rug::ops::Pow;
    if f.is_nan() {
        return "@NaN@".into();
    }
    if f.is_infinite() {
        return if f.is_sign_negative() { "-@Inf@" } else { "@Inf@" }.into();
    }
    if f.is_zero() {
        return if f.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let m = 1 + (f64::from(f.prec()) * std::f64::consts::LN_2 / 62f64.ln()).ceil() as u32;
    let x = rug::Rational::try_from(f).expect("finite").abs();
    let pow = |e: i64| -> rug::Rational {
        let p = rug::Integer::from(62).pow(e.unsigned_abs() as u32);
        if e >= 0 { rug::Rational::from(p) } else { rug::Rational::from((1, p)) }
    };
    // Exponent e with 62^(e-1) <= x < 62^e, from an estimate corrected exactly.
    let mut e = (x.to_f64().log(62.0)).floor() as i64 + 1;
    while x >= pow(e) {
        e += 1;
    }
    while x < pow(e - 1) {
        e -= 1;
    }
    let scaled = |e: i64| -> rug::Integer {
        let r = rug::Rational::from(&x * &pow(i64::from(m) - e));
        let (frac, fl) = r.fract_floor(rug::Integer::new());
        let half = rug::Rational::from((1, 2));
        if frac > half || (frac == half && fl.is_odd()) { fl + 1 } else { fl }
    };
    let mut n = scaled(e);
    if n >= rug::Integer::from(62).pow(m) {
        e += 1;
        n = scaled(e);
    }
    let mut ds = Vec::new();
    let mut v = n;
    while v > 0 {
        let (q, r) = v.div_rem_euc(rug::Integer::from(62));
        ds.push(B62_DIGITS[r.to_usize().expect("digit")]);
        v = q;
    }
    ds.reverse();
    let mut s = String::new();
    if f.is_sign_negative() {
        s.push('-');
    }
    s.push(ds[0] as char);
    s.push('.');
    s.extend(ds[1..].iter().map(|&b| b as char));
    s.push_str(&format!("@{}", e - 1));
    s
}

/// `mpfr_inp_str(op, fp, 62, MPFR_RNDN)` for what [`mpfr_out_str_base62`]
/// writes: base-62 digits with an optional `.`, then an optional `@` and a
/// decimal power of 62.
fn mpfr_parse_base62(text: &str, prec: u32) -> Option<rug::Float> {
    use rug::ops::Pow;
    let (neg, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let special = match body {
        "@NaN@" => Some(rug::Float::with_val(prec, rug::float::Special::Nan)),
        "@Inf@" => Some(rug::Float::with_val(prec, rug::float::Special::Infinity)),
        _ => None,
    };
    if let Some(s) = special {
        return Some(if neg { -s } else { s });
    }
    let (mant, exp) = match body.split_once('@') {
        Some((m, e)) => (m, e.parse::<i64>().ok()?),
        None => (body, 0),
    };
    let mut n = rug::Integer::new();
    let mut frac_digits = 0i64;
    let mut seen_point = false;
    for c in mant.bytes() {
        if c == b'.' && !seen_point {
            seen_point = true;
            continue;
        }
        let d = B62_DIGITS.iter().position(|&b| b == c)?;
        n = n * 62 + d as u32;
        if seen_point {
            frac_digits += 1;
        }
    }
    let shift = exp - frac_digits;
    let p = rug::Integer::from(62).pow(shift.unsigned_abs() as u32);
    let r = if shift >= 0 { rug::Rational::from(n * p) } else { rug::Rational::from((n, p)) };
    let f = rug::Float::with_val(prec, r);
    Some(if neg { -f } else { f })
}

/// `reada(file, arr)` — gawk's rwarray `reada`: replaces `arr` with the array
/// stored by `writea` and returns 1; on a missing file, a bad magic or version,
/// or a truncated file it returns 0 with `ERRNO` set. The array is cleared
/// only once the header has been accepted, as in `read_backend`.
pub(crate) fn reada(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    let buf = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            rt.set_errno_io(&e);
            return Ok(Value::Num(0.0));
        }
    };
    let mut r = RwReader { buf: &buf, pos: 0 };
    let header_ok = r.take(RW_MAGIC.len()) == Some(RW_MAGIC)
        && r.u32() == Some(RW_MAJOR)
        && r.u32() == Some(RW_MINOR);
    if !header_ok {
        rt.set_errno_io(&std::io::Error::from_raw_os_error(libc::EBADF));
        return Ok(Value::Num(0.0));
    }
    rt.array_delete(arr_name, None);
    let Some(arr) = r.array(rt) else {
        rt.set_errno_io(&std::io::Error::from_raw_os_error(libc::EBADF));
        return Ok(Value::Num(0.0));
    };
    for (k, v) in arr.iter() {
        rt.array_set_bytes(arr_name, k.as_bytes(), v.clone());
    }
    Ok(Value::Num(1.0))
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

/// `readdir(path, arr)` — populate **`arr`** with directory entries; returns count or -1.
pub(crate) fn readdir(rt: &mut Runtime, path: &str, arr_name: &str) -> Result<Value> {
    rt.require_unsandboxed_io()?;
    rt.clear_errno();
    let entries = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => {
            rt.set_errno_io(&e);
            return Ok(Value::Num(-1.0));
        }
    };
    rt.array_delete(arr_name, None);
    let mut count = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                rt.set_errno_io(&e);
                continue;
            }
        };
        let fname = entry.file_name().to_string_lossy().into_owned();
        let ftype = entry
            .file_type()
            .map(|ft| {
                if ft.is_dir() {
                    "d"
                } else if ft.is_symlink() {
                    "l"
                } else if ft.is_file() {
                    "f"
                } else {
                    "u"
                }
            })
            .unwrap_or("u");
        // gawk readdir: arr[count] = "filename/filetype"
        count += 1;
        rt.array_set(
            arr_name,
            count.to_string(),
            Value::Str(format!("{fname}/{ftype}").into()),
        );
    }
    Ok(Value::Num(count as f64))
}

/// `getlocaltime(arr [, timestamp])` — populate **`arr`** with broken-down local time fields.
/// Returns seconds since epoch. If **`timestamp`** is given, use it; otherwise use current time.
pub(crate) fn getlocaltime(rt: &mut Runtime, arr_name: &str, ts: Option<f64>) -> Result<Value> {
    rt.clear_errno();
    let epoch_secs = ts.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    });
    let secs_i64 = epoch_secs as i64;
    // Use libc localtime_r on Unix for proper timezone handling
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        let mut tm: MaybeUninit<libc::tm> = MaybeUninit::uninit();
        let time_t = secs_i64 as libc::time_t;
        let result = unsafe { libc::localtime_r(&time_t, tm.as_mut_ptr()) };
        if result.is_null() {
            rt.set_errno_str("getlocaltime: localtime_r failed");
            return Ok(Value::Num(-1.0));
        }
        let tm = unsafe { tm.assume_init() };
        rt.array_delete(arr_name, None);
        rt.array_set(arr_name, "sec".into(), Value::Num(tm.tm_sec as f64));
        rt.array_set(arr_name, "min".into(), Value::Num(tm.tm_min as f64));
        rt.array_set(arr_name, "hour".into(), Value::Num(tm.tm_hour as f64));
        rt.array_set(arr_name, "mday".into(), Value::Num(tm.tm_mday as f64));
        rt.array_set(arr_name, "mon".into(), Value::Num((tm.tm_mon + 1) as f64));
        rt.array_set(
            arr_name,
            "year".into(),
            Value::Num((tm.tm_year + 1900) as f64),
        );
        rt.array_set(arr_name, "wday".into(), Value::Num(tm.tm_wday as f64));
        rt.array_set(arr_name, "yday".into(), Value::Num((tm.tm_yday + 1) as f64));
        rt.array_set(arr_name, "isdst".into(), Value::Num(tm.tm_isdst as f64));
    }
    #[cfg(not(unix))]
    {
        let _ = secs_i64;
        rt.array_delete(arr_name, None);
        rt.set_errno_str("getlocaltime: not supported on this platform");
    }
    Ok(Value::Num(epoch_secs))
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
        let v = intdiv0(&mut rt, &Value::Num(10.0), &Value::Num(0.0)).unwrap();
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

    #[test]
    fn ord_empty_string_zero() {
        let mut rt = Runtime::new();
        assert_eq!(ord(&mut rt, "").unwrap().as_number(), 0.0);
    }

    #[test]
    fn chr_invalid_codepoint_empty_string() {
        let mut rt = Runtime::new();
        let v = chr(&mut rt, f64::from(0xD800)).unwrap();
        assert_eq!(v.as_str(), "");
    }

    #[test]
    fn gettimeofday_sets_sec_and_usec() {
        let mut rt = Runtime::new();
        gettimeofday(&mut rt, "ts").unwrap();
        assert!(rt.array_get("ts", "sec").as_number() > 0.0);
        let usec = rt.array_get("ts", "usec").as_number();
        assert!((0.0..=999_999.0).contains(&usec), "usec={usec}");
    }

    #[test]
    fn sleep_negative_errors() {
        let mut rt = Runtime::new();
        let e = sleep_secs(&mut rt, -1.0).unwrap_err();
        assert!(e.to_string().contains("sleep"), "unexpected error: {e}");
    }

    #[test]
    fn sleep_zero_returns_ok_without_panicking() {
        let mut rt = Runtime::new();
        sleep_secs(&mut rt, 0.0).unwrap();
    }

    #[test]
    fn revoutput_reverses_scalar_order() {
        let mut rt = Runtime::new();
        let v = revoutput(&mut rt, "ab").unwrap();
        assert_eq!(v.as_str(), "ba");
    }

    #[test]
    fn readfile_missing_yields_empty_string() {
        let mut rt = Runtime::new();
        let dir = std::env::temp_dir();
        let p = dir.join(format!("awkrs_no_such_readfile_{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let v = readfile(&mut rt, p.to_str().unwrap()).unwrap();
        assert_eq!(v.as_str(), "");
    }

    #[test]
    fn mpfr_base62_matches_gawk_rwarray_bytes() {
        // Renderings copied from files `gawk -M -l rwarray` wrote (PREC 53).
        for (x, text) in [
            (1.5, "1.V00000000@0"),
            (0.1, "6.COnbCOnbH@-1"),
            (1e100, "Q.CyvrY2MJt@55"),
            (-2.75e-30, "-8.7waQ3SLvA@-17"),
        ] {
            let f = rug::Float::with_val(53, x);
            assert_eq!(mpfr_out_str_base62(&f), text, "{x}");
            assert_eq!(mpfr_parse_base62(text, 53).unwrap().to_f64(), x, "{text}");
        }
        let inf = rug::Float::with_val(53, f64::NEG_INFINITY);
        assert_eq!(mpfr_out_str_base62(&inf), "-@Inf@");
        assert!(mpfr_parse_base62("@NaN@", 53).unwrap().is_nan());
    }

    #[test]
    fn reada_rejects_bad_magic() {
        let mut rt = Runtime::new();
        let dir = std::env::temp_dir();
        let p = dir.join(format!("awkrs_reada_bad_{}", std::process::id()));
        // gawk's read_backend answers 0 (not -1) and sets ERRNO.
        std::fs::write(&p, "not-magic\n").unwrap();
        let n = reada(&mut rt, p.to_str().unwrap(), "z").unwrap();
        assert_eq!(n.as_number(), 0.0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn stat_populates_type_and_size_for_file() {
        let mut rt = Runtime::new();
        let dir = std::env::temp_dir();
        let p = dir.join(format!("awkrs_stat_test_{}", std::process::id()));
        std::fs::write(&p, b"hi").unwrap();
        let code = stat(&mut rt, p.to_str().unwrap(), "st").unwrap();
        assert_eq!(code.as_number(), 0.0);
        assert_eq!(rt.array_get("st", "type").as_str(), "file");
        assert_eq!(rt.array_get("st", "size").as_number(), 2.0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ord_first_char_unicode_scalar() {
        let mut rt = Runtime::new();
        let n = ord(&mut rt, "πx").unwrap().as_number();
        assert_eq!(n, f64::from('π' as u32));
    }

    #[test]
    fn chr_ascii_roundtrip_with_ord() {
        let mut rt = Runtime::new();
        let c = chr(&mut rt, 65.0).unwrap();
        assert_eq!(c.as_str(), "A");
        let n = ord(&mut rt, "A").unwrap().as_number();
        assert_eq!(n, 65.0);
    }

    #[test]
    fn revoutput_unicode_preserves_scalar_boundaries() {
        let mut rt = Runtime::new();
        let v = revoutput(&mut rt, "aπb").unwrap();
        assert_eq!(v.as_str(), "bπa");
    }

    #[test]
    fn readdir_current_dir_lists_files() {
        let mut rt = Runtime::new();
        let n = readdir(&mut rt, ".", "d").unwrap().as_number();
        assert!(n > 0.0);
        // readdir format is "name/type"
        let e1 = rt.array_get("d", "1").as_str();
        assert!(e1.contains('/'));
        assert!(e1.ends_with("/f") || e1.ends_with("/d"));
    }

    #[test]
    fn fts_current_dir_recursive() {
        let mut rt = Runtime::new();
        let n = fts(&mut rt, "src", "f").unwrap().as_number();
        assert!(n > 0.0);
        // fts should return paths sorted.
        let p1 = rt.array_get("f", "1").as_str();
        assert!(p1.starts_with("src"));
    }

    #[test]
    fn rename_move_file() {
        let mut rt = Runtime::new();
        let dir = std::env::temp_dir();
        let p1 = dir.join(format!("awkrs_rn1_{}", std::process::id()));
        let p2 = dir.join(format!("awkrs_rn2_{}", std::process::id()));
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);

        std::fs::write(&p1, b"x").unwrap();
        let res = rename(&mut rt, p1.to_str().unwrap(), p2.to_str().unwrap()).unwrap();
        assert_eq!(res.as_number(), 0.0);
        assert!(p2.exists());
        assert!(!p1.exists());

        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn chdir_to_current_dir_ok_v2() {
        let mut rt = Runtime::new();
        let res = chdir(&mut rt, ".").unwrap();
        assert_eq!(res.as_number(), 0.0);
    }

    #[test]
    fn revtwoway_same_as_revoutput_v2() {
        let mut rt = Runtime::new();
        let v = revtwoway(&mut rt, "abc").unwrap();
        assert_eq!(v.as_str(), "cba");
    }

    #[test]
    fn statvfs_on_root_v2() {
        let mut rt = Runtime::new();
        let res = statvfs(&mut rt, "/", "sv").unwrap();
        // On non-unix it returns -1, on unix it might succeed or fail depending on permissions
        let n = res.as_number();
        assert!(n == 0.0 || n == -1.0);
    }

    #[test]
    fn getlocaltime_current_time_v2() {
        let mut rt = Runtime::new();
        let res = getlocaltime(&mut rt, "tm", None).unwrap();
        assert!(res.as_number() > 0.0);
    }
}
