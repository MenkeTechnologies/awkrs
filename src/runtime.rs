use std::borrow::Cow;
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;

/// Fast hash map for awk variables and arrays. Uses FxHash (no DoS resistance,
/// but ~2× faster than SipHash for short string keys typical in awk programs).
pub type AwkMap<K, V> = rustc_hash::FxHashMap<K, V>;
use socket2::{Domain, Socket, Type};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::bytecode::CompiledProgram;
use crate::error::{Error, Result};
use gettext::Catalog;
use rug::float::Round;
use rug::ops::Pow as _;
use rug::Float;

thread_local! {
    static NON_DECIMAL_PARSE: Cell<bool> = const { Cell::new(false) };
}

/// Set how string→number coercion parses literals (gawk `--non-decimal-data` / `-n`).
pub fn set_numeric_parse_mode(enabled: bool) {
    NON_DECIMAL_PARSE.with(|c| c.set(enabled));
}

/// Whether [`parse_number`] uses hex/octal rules like gawk `strtonum`.
#[inline]
pub fn numeric_parse_mode() -> bool {
    NON_DECIMAL_PARSE.with(|c| c.get())
}
use memchr::memmem;
use regex::bytes::Regex as BytesRegex;
use regex::{Regex, RegexBuilder};

/// Initial capacity for stdout batching (`print` accumulates here until flush).
/// Large END blocks (e.g. `for (k in a) print …`) grow this heavily; starting larger
/// avoids repeated `Vec` reallocations without a hard upper bound on output size.
const DEFAULT_PRINT_BUF_CAPACITY: usize = 512 * 1024;

pub(crate) type SharedInputReader = Arc<Mutex<BufReader<Box<dyn Read + Send>>>>;

/// Default precision for [`Value::Mpfr`] when `-M` / `--bignum` is enabled (MPFR bits).
pub const MPFR_PREC: u32 = 256;

/// POSIX / gawk: string ordering via `strcoll` on Unix (used by `for-in` value sorts and comparisons).
pub fn awk_locale_str_cmp(a: &str, b: &str) -> Ordering {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        match (CString::new(a), CString::new(b)) {
            (Ok(ca), Ok(cb)) => unsafe {
                let r = libc::strcoll(ca.as_ptr(), cb.as_ptr());
                r.cmp(&0)
            },
            _ => a.cmp(b),
        }
    }
    #[cfg(not(unix))]
    {
        a.cmp(b)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SortedInMode {
    Unsorted,
    IndStrAsc,
    IndStrDesc,
    IndNumAsc,
    IndNumDesc,
    ValStrAsc,
    ValStrDesc,
    ValNumAsc,
    ValNumDesc,
    ValTypeAsc,
    ValTypeDesc,
    /// gawk: `PROCINFO["sorted_in"] = "cmp"` — user function `(i1, i2)` returns &lt;0 / 0 / &gt;0 (index sort).
    CustomFn(String),
}

fn is_sorted_in_user_fn_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    if !(c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_sorted_in_at_token(t: &str) -> Option<SortedInMode> {
    match t {
        "@unsorted" => Some(SortedInMode::Unsorted),
        "@ind_str_asc" => Some(SortedInMode::IndStrAsc),
        "@ind_str_desc" => Some(SortedInMode::IndStrDesc),
        "@ind_num_asc" => Some(SortedInMode::IndNumAsc),
        "@ind_num_desc" => Some(SortedInMode::IndNumDesc),
        "@val_str_asc" => Some(SortedInMode::ValStrAsc),
        "@val_str_desc" => Some(SortedInMode::ValStrDesc),
        "@val_num_asc" => Some(SortedInMode::ValNumAsc),
        "@val_num_desc" => Some(SortedInMode::ValNumDesc),
        "@val_type_asc" => Some(SortedInMode::ValTypeAsc),
        "@val_type_desc" => Some(SortedInMode::ValTypeDesc),
        _ => None,
    }
}

pub(crate) fn sorted_in_mode(rt: &Runtime) -> SortedInMode {
    if rt.posix {
        return SortedInMode::Unsorted;
    }
    match rt.get_global_var("PROCINFO") {
        Some(Value::Array(m)) => {
            let Some(v) = m.get("sorted_in") else {
                return SortedInMode::Unsorted;
            };
            let s = v.as_str();
            let t = s.trim();
            if t.is_empty() {
                return SortedInMode::Unsorted;
            }
            if t.starts_with('@') {
                if let Some(mode) = parse_sorted_in_at_token(t) {
                    return mode;
                }
                if !rt.sorted_in_warned.get() {
                    rt.sorted_in_warned.set(true);
                    eprintln!(
                        "awkrs: PROCINFO[\"sorted_in\"]={s:?}: unknown @… token (expected @ind_* / @val_* / @unsorted)"
                    );
                }
                return SortedInMode::Unsorted;
            }
            if is_sorted_in_user_fn_name(t) {
                return SortedInMode::CustomFn(t.to_string());
            }
            SortedInMode::Unsorted
        }
        _ => SortedInMode::Unsorted,
    }
}

#[inline]
fn val_type_rank(v: &Value) -> u8 {
    match v {
        Value::Uninit => 0,
        Value::Num(_) | Value::Mpfr(_) => 1,
        Value::Str(_) | Value::Regexp(_) => 2,
        Value::Array(_) => 3,
    }
}

pub(crate) fn sort_for_in_keys(
    keys: &mut [String],
    arr: &AwkMap<String, Value>,
    mode: SortedInMode,
) {
    use SortedInMode::*;
    match mode {
        Unsorted => {}
        CustomFn(_) => {}
        IndStrAsc => keys.sort(),
        IndStrDesc => keys.sort_by(|a, b| b.cmp(a)),
        IndNumAsc => keys.sort_by(|a, b| {
            parse_number(a)
                .partial_cmp(&parse_number(b))
                .unwrap_or(Ordering::Equal)
        }),
        IndNumDesc => keys.sort_by(|a, b| {
            parse_number(b)
                .partial_cmp(&parse_number(a))
                .unwrap_or(Ordering::Equal)
        }),
        ValStrAsc => keys.sort_by(|ka, kb| {
            let sa = arr.get(ka).map(|v| v.as_str()).unwrap_or_default();
            let sb = arr.get(kb).map(|v| v.as_str()).unwrap_or_default();
            awk_locale_str_cmp(&sa, &sb)
        }),
        ValStrDesc => keys.sort_by(|ka, kb| {
            let sa = arr.get(ka).map(|v| v.as_str()).unwrap_or_default();
            let sb = arr.get(kb).map(|v| v.as_str()).unwrap_or_default();
            awk_locale_str_cmp(&sb, &sa)
        }),
        ValNumAsc => keys.sort_by(|ka, kb| {
            let na = arr.get(ka).map(|v| v.as_number()).unwrap_or(0.0);
            let nb = arr.get(kb).map(|v| v.as_number()).unwrap_or(0.0);
            na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
        }),
        ValNumDesc => keys.sort_by(|ka, kb| {
            let na = arr.get(ka).map(|v| v.as_number()).unwrap_or(0.0);
            let nb = arr.get(kb).map(|v| v.as_number()).unwrap_or(0.0);
            nb.partial_cmp(&na).unwrap_or(Ordering::Equal)
        }),
        ValTypeAsc => keys.sort_by(|ka, kb| {
            let va = arr.get(ka.as_str());
            let vb = arr.get(kb.as_str());
            let ra = va.map(val_type_rank).unwrap_or(0);
            let rb = vb.map(val_type_rank).unwrap_or(0);
            ra.cmp(&rb).then_with(|| {
                let sa = va.map(|v| v.as_str()).unwrap_or_default();
                let sb = vb.map(|v| v.as_str()).unwrap_or_default();
                awk_locale_str_cmp(&sa, &sb)
            })
        }),
        ValTypeDesc => keys.sort_by(|ka, kb| {
            let va = arr.get(ka.as_str());
            let vb = arr.get(kb.as_str());
            let ra = va.map(val_type_rank).unwrap_or(0);
            let rb = vb.map(val_type_rank).unwrap_or(0);
            rb.cmp(&ra).then_with(|| {
                let sa = va.map(|v| v.as_str()).unwrap_or_default();
                let sb = vb.map(|v| v.as_str()).unwrap_or_default();
                awk_locale_str_cmp(&sb, &sa)
            })
        }),
    }
}

#[cfg(unix)]
fn wait_fd_read_timeout(fd: std::os::unix::io::RawFd, timeout_ms: i32) -> crate::error::Result<()> {
    if timeout_ms <= 0 {
        return Ok(());
    }
    let mut fds = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let rc = unsafe { libc::poll(&mut fds, 1, timeout_ms) };
    if rc < 0 {
        return Err(crate::error::Error::Io(std::io::Error::last_os_error()));
    }
    if rc == 0 {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "read timeout (PROCINFO[\"READ_TIMEOUT\"])",
        )));
    }
    Ok(())
}

/// Convert a [`Value`] to a [`Float`] for MPFR arithmetic (`-M`).
pub fn value_to_float(v: &Value, prec: u32) -> Float {
    match v {
        Value::Mpfr(f) => f.clone(),
        Value::Num(n) => Float::with_val(prec, *n),
        Value::Str(s) => Float::with_val(prec, parse_number(s.trim())),
        Value::Regexp(s) => Float::with_val(prec, parse_number(s.trim())),
        Value::Uninit => Float::with_val(prec, 0),
        Value::Array(_) => Float::with_val(prec, 0),
    }
}

/// Binary `+=` / `-=` / … for compound assignment; uses MPFR when `use_mpfr` is true.
pub fn awk_binop_values(
    op: crate::ast::BinOp,
    old: &Value,
    rhs: &Value,
    use_mpfr: bool,
    rt: &Runtime,
) -> crate::error::Result<Value> {
    use crate::ast::BinOp;
    use crate::error::Error;
    if !use_mpfr {
        let a = old.as_number();
        let b = rhs.as_number();
        let n = match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Mod => a % b,
            BinOp::Pow => a.powf(b),
            _ => return Err(Error::Runtime("invalid compound assignment op".into())),
        };
        return Ok(Value::Num(n));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let a = value_to_float(old, prec);
    let b = value_to_float(rhs, prec);
    let r = match op {
        BinOp::Add => Float::with_val_round(prec, &a + &b, round).0,
        BinOp::Sub => Float::with_val_round(prec, &a - &b, round).0,
        BinOp::Mul => Float::with_val_round(prec, &a * &b, round).0,
        BinOp::Div => Float::with_val_round(prec, &a / &b, round).0,
        BinOp::Mod => Float::with_val_round(prec, &a % &b, round).0,
        BinOp::Pow => Float::with_val_round(prec, a.pow(&b), round).0,
        _ => return Err(Error::Runtime("invalid compound assignment op".into())),
    };
    Ok(Value::Mpfr(r))
}

/// Parse gawk-style `/inet/tcp/lport/host/rport` (local port `0` = ephemeral client).
pub fn parse_inet_tcp(path: &str) -> Option<(u16, String, u16)> {
    parse_inet_l4(path, "/inet/tcp/")
}

/// Parse gawk-style `/inet/udp/lport/host/rport`.
pub fn parse_inet_udp(path: &str) -> Option<(u16, String, u16)> {
    parse_inet_l4(path, "/inet/udp/")
}

fn parse_inet_l4(path: &str, prefix: &str) -> Option<(u16, String, u16)> {
    let rest = path.strip_prefix(prefix)?;
    let mut it = rest.split('/');
    let lport = it.next()?.parse().ok()?;
    let host = it.next()?.to_string();
    let rport = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((lport, host, rport))
}

fn tcp_connect_with_local_port(host: &str, lport: u16, rport: u16) -> Result<TcpStream> {
    let mut addrs = format!("{host}:{rport}")
        .to_socket_addrs()
        .map_err(|e| Error::Runtime(format!("inet resolve `{host}`: {e}")))?;
    let addr = addrs
        .next()
        .ok_or_else(|| Error::Runtime(format!("inet: no address for `{host}:{rport}`")))?;
    let domain = match addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::STREAM, None)
        .map_err(|e| Error::Runtime(format!("inet socket: {e}")))?;
    let bind_addr = match addr {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, lport)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, lport)),
    };
    socket
        .bind(&bind_addr.into())
        .map_err(|e| Error::Runtime(format!("inet bind local port {lport}: {e}")))?;
    socket.set_nonblocking(false).ok();
    socket
        .connect(&addr.into())
        .map_err(|e| Error::Runtime(format!("inet connect `{host}:{rport}`: {e}")))?;
    Ok(socket.into())
}

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
    /// gawk: `@/regex/` regexp constant — distinct from [`Value::Str`] for `typeof` and typed `~`.
    Regexp(String),
    Num(f64),
    /// GNU MPFR arbitrary-precision float (`-M` / `--bignum`).
    Mpfr(Float),
    Array(AwkMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> String {
        match self {
            Value::Uninit => String::new(),
            Value::Str(s) => s.clone(),
            Value::Regexp(s) => s.clone(),
            Value::Num(n) => format_number(*n),
            Value::Mpfr(f) => f.to_string(),
            Value::Array(_) => String::new(),
        }
    }

    /// For `&str` APIs (e.g. `gsub`) without allocating when the value is already `Str`.
    #[inline]
    pub fn as_str_cow(&self) -> Cow<'_, str> {
        match self {
            Value::Uninit => Cow::Borrowed(""),
            Value::Str(s) => Cow::Borrowed(s.as_str()),
            Value::Regexp(s) => Cow::Borrowed(s.as_str()),
            Value::Num(n) => Cow::Owned(format_number(*n)),
            Value::Mpfr(f) => Cow::Owned(f.to_string()),
            Value::Array(_) => Cow::Borrowed(""),
        }
    }

    /// Borrow the inner string without cloning. Returns `None` for Num/Array.
    #[inline]
    #[allow(dead_code)]
    pub fn str_ref(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Regexp(s) => Some(s),
            _ => None,
        }
    }

    /// Write the string representation directly into a byte buffer — zero allocation
    /// for the Str case, one `write!` for Num.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        match self {
            Value::Uninit => {}
            Value::Str(s) => buf.extend_from_slice(s.as_bytes()),
            Value::Regexp(s) => buf.extend_from_slice(s.as_bytes()),
            Value::Num(n) => {
                use std::io::Write;
                let n = *n;
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(buf, "{}", n as i64);
                } else {
                    let _ = write!(buf, "{n}");
                }
            }
            Value::Mpfr(f) => buf.extend_from_slice(f.to_string().as_bytes()),
            Value::Array(_) => {}
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) => *n,
            Value::Str(s) => parse_number(s),
            Value::Regexp(s) => parse_number(s),
            Value::Mpfr(f) => f.to_f64(),
            Value::Array(_) => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty() && s.parse::<f64>().map(|n| n != 0.0).unwrap_or(true),
            Value::Regexp(s) => !s.is_empty(),
            Value::Mpfr(f) => !f.is_zero(),
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
            Value::Regexp(s) => s,
            Value::Num(n) => format_number(n),
            Value::Mpfr(f) => f.to_string(),
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
            Value::Regexp(s) => buf.push_str(s),
            Value::Num(n) => {
                use std::fmt::Write;
                let n = *n;
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(buf, "{}", n as i64);
                } else {
                    let _ = write!(buf, "{n}");
                }
            }
            Value::Mpfr(f) => buf.push_str(&f.to_string()),
            Value::Array(_) => {}
        }
    }

    /// POSIX-style: true if the value is numeric (including string that looks like number).
    pub fn is_numeric_str(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(_) => true,
            Value::Mpfr(_) => true,
            Value::Str(s) => {
                let t = s.trim();
                !t.is_empty() && t.parse::<f64>().is_ok()
            }
            Value::Regexp(_) => false,
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

/// gawk `strtonum`-style parse (hex `0x…`, octal `0…`, else float).
#[inline]
fn parse_number_strtonum(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if t.starts_with("0x") || t.starts_with("0X") {
        return u64::from_str_radix(&t[2..], 16)
            .map(|v| v as f64)
            .unwrap_or(0.0);
    }
    if t.len() > 1 && t.starts_with('0') && !t.contains('.') && !t.contains('e') && !t.contains('E')
    {
        return i64::from_str_radix(t, 8).map(|v| v as f64).unwrap_or(0.0);
    }
    t.parse::<f64>().unwrap_or(0.0)
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
    if numeric_parse_mode() {
        return parse_number_strtonum(s);
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
fn split_fields_fpat(
    record: &str,
    fpat: &str,
    field_ranges: &mut Vec<(u32, u32)>,
    ignore_case: bool,
) -> bool {
    field_ranges.clear();
    let mut b = RegexBuilder::new(fpat);
    b.case_insensitive(ignore_case);
    match b.build() {
        Ok(re) => {
            for m in re.find_iter(record) {
                field_ranges.push((m.start() as u32, m.end() as u32));
            }
            true
        }
        Err(_) => false,
    }
}

fn split_fields_fieldwidths(record: &str, widths: &[usize], field_ranges: &mut Vec<(u32, u32)>) {
    field_ranges.clear();
    if widths.is_empty() {
        return;
    }
    let b = record.as_bytes();
    let n = b.len();
    let mut pos = 0usize;
    let len_w = widths.len();
    for (i, &w) in widths.iter().enumerate() {
        let end = if i == len_w - 1 { n } else { (pos + w).min(n) };
        field_ranges.push((pos as u32, end as u32));
        pos = end;
        if pos >= n {
            break;
        }
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
fn split_fields_into(
    record: &str,
    fs: &str,
    field_ranges: &mut Vec<(u32, u32)>,
    ignore_case: bool,
) {
    field_ranges.clear();
    // Rough NF estimate from record length reduces per-line `Vec` growth for whitespace/FS splits.
    if !record.is_empty() {
        let want = (record.len() / 16).saturating_add(4).clamp(8, 2048);
        if field_ranges.capacity() < want {
            field_ranges.reserve(want - field_ranges.capacity());
        }
    }
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
        let mut b = RegexBuilder::new(fs);
        b.case_insensitive(ignore_case);
        match b.build() {
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
    /// Directory iteration for `getline var < dir` (gawk **readdir** extension semantics).
    pub dir_read: HashMap<String, (Vec<String>, usize)>,
    /// Open files for `print … > path` / `print … >> path` / `fflush` / `close`.
    pub output_handles: HashMap<String, BufWriter<File>>,
    /// `print`/`printf` `| "cmd"` — stdin of `sh -c cmd` (key is the command string).
    pub pipe_stdin: HashMap<String, BufWriter<ChildStdin>>,
    pub pipe_children: HashMap<String, Child>,
    /// `print`/`printf` `|& "cmd"` / `getline <& "cmd"` — two-way `sh -c` (same key for both directions).
    pub coproc_handles: HashMap<String, CoprocHandle>,
    /// gawk `/inet/tcp/...` TCP streams (read half).
    pub inet_tcp_read: HashMap<String, BufReader<TcpStream>>,
    /// gawk `/inet/tcp/...` TCP streams (write half).
    pub inet_tcp_write: HashMap<String, TcpStream>,
    /// gawk `/inet/udp/...` connected UDP sockets (one per path; `recv` / `send` datagrams).
    pub inet_udp: HashMap<String, UdpSocket>,
    /// Last `bindtextdomain` directory (gettext stub / future real i18n).
    pub gettext_dir: String,
    /// `-M` / `--bignum`: use MPFR ([`Value::Mpfr`]) for arithmetic in the VM.
    pub bignum: bool,
    pub rand_seed: u64,
    /// Radix for `%f` / `%g` / etc. and `print` of numbers when `-N` / `--use-lc-numeric` is set (Unix).
    pub numeric_decimal: char,
    /// Thousands separator for gawk **`%'`** (`printf` / `sprintf` integer grouping), from `localeconv()` when available.
    pub numeric_thousands_sep: Option<char>,
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
    /// Scratch buffer for JIT numeric slot marshaling (reused across records).
    pub jit_slot_buf: Vec<f64>,
    /// `-k` / `--csv` (gawk-style): use [`split_csv_gawk_fields`] instead of `FPAT` / `FS` for `$n`.
    pub csv_mode: bool,
    /// gawk: `RS` longer than one character is a regex delimiter (cached here).
    pub rs_pattern_for_regex: String,
    pub rs_regex_bytes: Option<BytesRegex>,
    /// gawk `--sandbox` / `-S`: disallow file redirects, pipes, coprocesses, inet, `system()`.
    pub sandbox: bool,
    /// gawk `-b` / `--characters-as-bytes`: `length` / `substr` / `index` use byte units (otherwise UTF-8 character units).
    pub characters_as_bytes: bool,
    /// gawk `--posix` / `-P` (reserved; stricter POSIX checks may be added incrementally).
    pub posix: bool,
    /// gawk `--traditional` / `-c` (reserved; traditional awk rules may be added incrementally).
    pub traditional: bool,
    /// Bytecode JIT (`-s` / `--no-optimize` disables when set).
    pub jit_enabled: bool,
    /// GNU MO catalogs loaded by `bindtextdomain` (domain → catalog).
    pub gettext_catalogs: AwkMap<String, Arc<Catalog>>,
    /// Copy of [`crate::bytecode::CompiledProgram::slot_map`] for SYMTAB / `array_keys` without VM context.
    pub symtab_slot_map: HashMap<String, u16>,
    /// `-p` / `--profile`: invocation count per **record** rule (index matches `CompiledProgram::record_rules`).
    pub profile_record_hits: Vec<u64>,
    /// One-shot: warn once when `PROCINFO["sorted_in"]` is set to an unsupported custom comparator name.
    pub sorted_in_warned: Cell<bool>,
}

impl Runtime {
    pub fn new() -> Self {
        let mut vars = AwkMap::default();
        vars.insert("OFS".into(), Value::Str(" ".into()));
        vars.insert("ORS".into(), Value::Str("\n".into()));
        vars.insert("OFMT".into(), Value::Str("%.6g".into()));
        // POSIX: number→string coercion (distinct from OFMT, which is for print).
        vars.insert("CONVFMT".into(), Value::Str("%.6g".into()));
        // POSIX record separator (default newline).
        vars.insert("RS".into(), Value::Str("\n".into()));
        // Text of the input record separator for the last record read (gawk).
        vars.insert("RT".into(), Value::Str(String::new()));
        vars.insert("ERRNO".into(), Value::Str(String::new()));
        vars.insert("ARGIND".into(), Value::Num(0.0));
        // Process environment (gawk associative array).
        let mut environ = AwkMap::default();
        for (k, v) in std::env::vars() {
            environ.insert(k, Value::Str(v));
        }
        vars.insert("ENVIRON".into(), Value::Array(environ));
        // Stub gawk special arrays (full semantics not implemented).
        vars.insert("PROCINFO".into(), Value::Array(AwkMap::default()));
        vars.insert("SYMTAB".into(), Value::Array(AwkMap::default()));
        vars.insert("FUNCTAB".into(), Value::Array(AwkMap::default()));
        // POSIX octal \034 — multidimensional array subscript separator
        vars.insert("SUBSEP".into(), Value::Str("\x1c".into()));
        // Empty FPAT means use FS for field splitting (gawk).
        vars.insert("FPAT".into(), Value::Str(String::new()));
        vars.insert("FIELDWIDTHS".into(), Value::Str(String::new()));
        vars.insert("IGNORECASE".into(), Value::Num(0.0));
        vars.insert("BINMODE".into(), Value::Num(0.0));
        vars.insert("LINT".into(), Value::Num(0.0));
        vars.insert("TEXTDOMAIN".into(), Value::Str(String::new()));
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
            inet_tcp_read: HashMap::new(),
            inet_tcp_write: HashMap::new(),
            inet_udp: HashMap::new(),
            gettext_dir: String::new(),
            bignum: false,
            file_handles: HashMap::new(),
            dir_read: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed: 1,
            numeric_decimal: '.',
            numeric_thousands_sep: crate::locale_numeric::thousands_sep_from_locale().or(Some(',')),
            slots: Vec::new(),
            regex_cache: AwkMap::default(),
            memmem_finder_cache: AwkMap::default(),
            print_buf: Vec::with_capacity(DEFAULT_PRINT_BUF_CAPACITY),
            ofs_bytes: b" ".to_vec(),
            ors_bytes: b"\n".to_vec(),
            vm_stack: Vec::with_capacity(64),
            jit_slot_buf: Vec::new(),
            csv_mode: false,
            rs_pattern_for_regex: String::new(),
            rs_regex_bytes: None,
            sandbox: false,
            characters_as_bytes: false,
            posix: false,
            traditional: false,
            jit_enabled: true,
            gettext_catalogs: AwkMap::default(),
            symtab_slot_map: HashMap::new(),
            profile_record_hits: Vec::new(),
            sorted_in_warned: Cell::new(false),
        }
    }

    /// True when the **`LINT`** variable is set to a truthy value (after `BEGIN`, includes `-v LINT=1`).
    pub fn lint_runtime_active(&self) -> bool {
        self.get_global_var("LINT")
            .map(|v| v.truthy())
            .unwrap_or(false)
    }

    /// gawk **`PROCINFO["prec"]`**: MPFR precision in bits when **`-M`** / **`--bignum`** is active.
    pub fn mpfr_prec_bits(&self) -> u32 {
        if !self.bignum {
            return MPFR_PREC;
        }
        match self.get_global_var("PROCINFO") {
            Some(Value::Array(m)) => m
                .get("prec")
                .map(|v| v.as_number() as u32)
                .filter(|&p| (53..=1_000_000).contains(&p))
                .unwrap_or(MPFR_PREC),
            _ => MPFR_PREC,
        }
    }

    /// gawk **`PROCINFO["roundmode"]`**: MPFR rounding (`N` nearest, `Z` zero, `U` up, `D` down, `A` away).
    pub fn mpfr_round(&self) -> Round {
        let s = match self.get_global_var("PROCINFO") {
            Some(Value::Array(m)) => m.get("roundmode").map(|v| v.as_str()).unwrap_or_default(),
            _ => String::new(),
        };
        let c = s.trim().chars().next().unwrap_or('N');
        match c.to_ascii_uppercase() {
            'N' => Round::Nearest,
            'Z' => Round::Zero,
            'U' => Round::Up,
            'D' => Round::Down,
            'A' => Round::AwayZero,
            _ => Round::Nearest,
        }
    }

    /// gawk **`PROCINFO["READ_TIMEOUT"]`**: positive = milliseconds for blocking reads on files / inet; **`0`** = no timeout.
    pub fn read_timeout_ms(&self) -> i32 {
        match self.get_global_var("PROCINFO") {
            Some(Value::Array(m)) => m
                .get("READ_TIMEOUT")
                .map(|v| v.as_number() as i32)
                .unwrap_or(0),
            _ => 0,
        }
    }

    /// Refresh **`PROCINFO`**, **`FUNCTAB`**, and a **`SYMTAB`** mirror of globals (best-effort vs gawk introspection).
    pub fn refresh_special_arrays(&mut self, cp: &CompiledProgram, bin_name: &str) {
        self.procinfo_refresh(bin_name);
        self.functab_refresh(cp);
        self.symtab_mirror_refresh(cp);
    }

    fn procinfo_refresh(&mut self, bin_name: &str) {
        let mut p = AwkMap::default();
        if let Some(Value::Array(old)) = self.vars.get("PROCINFO") {
            for (k, v) in old.iter() {
                p.insert(k.clone(), v.clone());
            }
        }
        p.insert(
            "version".into(),
            Value::Str(env!("CARGO_PKG_VERSION").into()),
        );
        p.insert("api".into(), Value::Str("awkrs".into()));
        p.insert("program".into(), Value::Str(bin_name.into()));
        p.insert("platform".into(), Value::Str(std::env::consts::OS.into()));
        p.insert("pid".into(), Value::Num(std::process::id() as f64));
        #[cfg(unix)]
        {
            unsafe {
                p.insert("ppid".into(), Value::Num(libc::getppid() as f64));
                p.insert("uid".into(), Value::Num(libc::getuid() as f64));
                p.insert("euid".into(), Value::Num(libc::geteuid() as f64));
                p.insert("gid".into(), Value::Num(libc::getgid() as f64));
                p.insert("egid".into(), Value::Num(libc::getegid() as f64));
            }
        }
        if self.bignum && !p.contains_key("prec") {
            p.insert("prec".into(), Value::Num(MPFR_PREC as f64));
        }
        if !p.contains_key("roundmode") {
            p.insert("roundmode".into(), Value::Str("N".into()));
        }
        if !p.contains_key("READ_TIMEOUT") {
            p.insert("READ_TIMEOUT".into(), Value::Num(0.0));
        }
        let binmode = self
            .get_global_var("BINMODE")
            .map(|v| v.as_number())
            .unwrap_or(0.0);
        p.insert("awkrs_binmode".into(), Value::Num(binmode));
        self.vars.insert("PROCINFO".into(), Value::Array(p));
    }

    fn functab_refresh(&mut self, cp: &CompiledProgram) {
        let mut ft = AwkMap::default();
        for (name, f) in &cp.functions {
            let mut meta = AwkMap::default();
            meta.insert("type".into(), Value::Str("user".into()));
            meta.insert("arity".into(), Value::Num(f.params.len() as f64));
            ft.insert(name.clone(), Value::Array(meta));
        }
        self.vars.insert("FUNCTAB".into(), Value::Array(ft));
    }

    fn symtab_mirror_refresh(&mut self, cp: &CompiledProgram) {
        self.symtab_slot_map = cp.slot_map.clone();
        // SYMTAB subscripts resolve live via [`VmCtx`] / [`Runtime::symtab_elem_get`]; keep empty placeholder.
        self.vars
            .insert("SYMTAB".into(), Value::Array(AwkMap::default()));
    }

    /// Resize [`Self::jit_slot_buf`] for JIT (`n` elements; no shrink).
    #[inline]
    pub fn ensure_jit_slot_buf(&mut self, n: usize) {
        if self.jit_slot_buf.len() < n {
            self.jit_slot_buf.resize(n, 0.0);
        } else if self.jit_slot_buf.len() > n {
            self.jit_slot_buf.truncate(n);
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
    #[allow(clippy::too_many_arguments)]
    pub fn for_parallel_worker(
        shared_globals: Arc<AwkMap<String, Value>>,
        filename: String,
        rand_seed: u64,
        numeric_decimal: char,
        numeric_thousands_sep: Option<char>,
        csv_mode: bool,
        bignum: bool,
        sandbox: bool,
        characters_as_bytes: bool,
        posix: bool,
        traditional: bool,
        jit_enabled: bool,
        gettext_catalogs: AwkMap<String, Arc<Catalog>>,
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
            inet_tcp_read: HashMap::new(),
            inet_tcp_write: HashMap::new(),
            inet_udp: HashMap::new(),
            gettext_dir: String::new(),
            bignum,
            file_handles: HashMap::new(),
            dir_read: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed,
            numeric_decimal,
            numeric_thousands_sep,
            slots: Vec::new(),
            regex_cache: AwkMap::default(),
            memmem_finder_cache: AwkMap::default(),
            print_buf: Vec::new(),
            ofs_bytes: b" ".to_vec(),
            ors_bytes: b"\n".to_vec(),
            vm_stack: Vec::with_capacity(64),
            jit_slot_buf: Vec::new(),
            csv_mode,
            rs_pattern_for_regex: String::new(),
            rs_regex_bytes: None,
            sandbox,
            characters_as_bytes,
            posix,
            traditional,
            jit_enabled,
            gettext_catalogs,
            symtab_slot_map: HashMap::new(),
            profile_record_hits: Vec::new(),
            sorted_in_warned: Cell::new(false),
        }
    }

    /// Refused when [`Self::sandbox`] is set (gawk-style `-S`).
    pub fn require_unsandboxed_io(&self) -> Result<()> {
        if self.sandbox {
            return Err(Error::Runtime(
                "sandbox: file I/O, pipes, coprocesses, inet, and system() are disabled".into(),
            ));
        }
        Ok(())
    }

    /// Ensure a regex is compiled and cached. Call before `regex_ref()`.
    pub fn ensure_regex(&mut self, pat: &str) -> std::result::Result<(), String> {
        let ic = self.ignore_case_flag();
        let key = format!("{ic}\x1c{pat}");
        use std::collections::hash_map::Entry;
        if let Entry::Vacant(e) = self.regex_cache.entry(key) {
            let mut b = RegexBuilder::new(pat);
            b.case_insensitive(ic);
            let re = b.build().map_err(|e| e.to_string())?;
            e.insert(re);
        }
        Ok(())
    }

    /// Get a cached regex (must call `ensure_regex` first).
    pub fn regex_ref(&self, pat: &str) -> &Regex {
        let ic = self.ignore_case_flag();
        let key = format!("{ic}\x1c{pat}");
        &self.regex_cache[&key]
    }

    /// gawk **`IGNORECASE`**: truthy value enables case-insensitive regex and string compares.
    #[inline]
    pub fn ignore_case_flag(&self) -> bool {
        self.get_global_var("IGNORECASE")
            .map(|v| v.truthy())
            .unwrap_or(false)
    }

    pub fn clear_errno(&mut self) {
        self.vars.insert("ERRNO".into(), Value::Str(String::new()));
    }

    pub fn set_errno_io(&mut self, e: &std::io::Error) {
        self.vars.insert("ERRNO".into(), Value::Str(e.to_string()));
    }

    pub fn set_errno_str(&mut self, msg: impl Into<String>) {
        self.vars.insert("ERRNO".into(), Value::Str(msg.into()));
    }

    pub fn ensure_rs_regex_bytes(&mut self) -> Result<()> {
        let rs = self.rs_string();
        if self.rs_pattern_for_regex == rs {
            return Ok(());
        }
        self.rs_pattern_for_regex.clear();
        self.rs_pattern_for_regex.push_str(&rs);
        if rs == "\n" || rs.is_empty() {
            self.rs_regex_bytes = None;
            return Ok(());
        }
        if rs.chars().count() <= 1 {
            self.rs_regex_bytes = None;
            return Ok(());
        }
        self.rs_regex_bytes = Some(
            BytesRegex::new(&rs).map_err(|e| Error::Runtime(format!("invalid RS regex: {e}")))?,
        );
        Ok(())
    }

    pub fn set_rt_from_bytes(&mut self, sep: &[u8]) {
        let t = if sep.is_empty() {
            String::new()
        } else {
            String::from_utf8_lossy(sep).into_owned()
        };
        self.vars.insert("RT".into(), Value::Str(t));
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
    #[inline]
    pub fn get_global_var(&self, name: &str) -> Option<&Value> {
        self.vars
            .get(name)
            .or_else(|| self.global_readonly.as_ref()?.get(name))
    }

    /// `print … | "cmd"` / `printf … | "cmd"` — append bytes to the coprocess stdin (spawn on first use).
    pub fn write_pipe_line(&mut self, cmd: &str, data: &str) -> Result<()> {
        self.require_unsandboxed_io()?;
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
        self.require_unsandboxed_io()?;
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

    /// `expr | getline` — one line from `sh -c expr` stdout (new subprocess each call).
    pub fn read_line_pipe(&mut self, cmd: &str) -> Result<Option<String>> {
        self.require_unsandboxed_io()?;
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Runtime(format!("pipe getline `{cmd}`: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Runtime(format!("pipe getline `{cmd}`: no stdout")))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(Error::Io)?;
        let _ = child.wait();
        if n == 0 {
            Ok(None)
        } else {
            Ok(Some(line))
        }
    }

    /// Write one `print` line (including `ORS`) to `path`. First open uses truncate (`>`) or
    /// append (`>>`); later writes reuse the same handle until `close`.
    pub fn write_output_line(&mut self, path: &str, data: &str, append: bool) -> Result<()> {
        self.require_unsandboxed_io()?;
        if path.starts_with("/inet/udp/") {
            let _ = append;
            self.ensure_inet_udp(path)?;
            let s = self.inet_udp.get_mut(path).unwrap();
            s.send(data.as_bytes())
                .map_err(|e| Error::Runtime(format!("inet udp send `{path}`: {e}")))?;
            return Ok(());
        }
        if path.starts_with("/inet/tcp/") {
            let _ = append;
            self.ensure_inet_tcp_pair(path)?;
            let w = self.inet_tcp_write.get_mut(path).unwrap();
            w.write_all(data.as_bytes()).map_err(Error::Io)?;
            return Ok(());
        }
        self.ensure_output_writer(path, append)?;
        let w = self.output_handles.get_mut(path).unwrap();
        w.write_all(data.as_bytes()).map_err(Error::Io)?;
        Ok(())
    }

    fn ensure_output_writer(&mut self, path: &str, append: bool) -> Result<()> {
        if path.starts_with("/inet/udp/") {
            return self.ensure_inet_udp(path);
        }
        if path.starts_with("/inet/tcp/") {
            return self.ensure_inet_tcp_pair(path);
        }
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
        if let Some(w) = self.inet_tcp_write.get_mut(key) {
            w.flush().map_err(Error::Io)?;
            return Ok(());
        }
        if self.inet_udp.contains_key(key) {
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

    /// Current [`RS`](https://www.gnu.org/software/gawk/manual/html_node/Built_002din-Variables.html) value.
    pub fn rs_string(&self) -> String {
        match self.get_global_var("RS") {
            Some(Value::Str(s)) => s.clone(),
            Some(v) => v.as_str(),
            None => "\n".to_string(),
        }
    }

    /// POSIX / gawk: format a number using **`CONVFMT`** (string coercion).
    pub fn num_to_string_convfmt(&self, n: f64) -> String {
        let fmt = self
            .get_global_var("CONVFMT")
            .map(|v| v.as_str())
            .unwrap_or_else(|| "%.6g".to_string());
        crate::format::awk_sprintf_with_decimal(
            &fmt,
            &[Value::Num(n)],
            self.numeric_decimal,
            self.numeric_thousands_sep,
            None,
        )
        .unwrap_or_else(|_| format_number(n))
    }

    /// POSIX: `print` formats numbers with **`OFMT`** (distinct from [`Self::num_to_string_convfmt`]).
    pub fn num_to_string_ofmt(&self, n: f64) -> String {
        let fmt = self
            .get_global_var("OFMT")
            .map(|v| v.as_str())
            .unwrap_or_else(|| "%.6g".to_string());
        crate::format::awk_sprintf_with_decimal(
            &fmt,
            &[Value::Num(n)],
            self.numeric_decimal,
            self.numeric_thousands_sep,
            None,
        )
        .unwrap_or_else(|_| format_number(n))
    }

    /// `CONVFMT` formatting for an MPFR value (`-M`).
    pub fn mpfr_to_string_convfmt(&self, f: &Float) -> String {
        let fmt = self
            .get_global_var("CONVFMT")
            .map(|v| v.as_str())
            .unwrap_or_else(|| "%.6g".to_string());
        crate::format::awk_sprintf_with_decimal(
            &fmt,
            &[Value::Mpfr(f.clone())],
            self.numeric_decimal,
            self.numeric_thousands_sep,
            Some((self.mpfr_prec_bits(), self.mpfr_round())),
        )
        .unwrap_or_else(|_| f.to_string())
    }

    /// `OFMT` formatting for an MPFR value (`-M`).
    pub fn mpfr_to_string_ofmt(&self, f: &Float) -> String {
        let fmt = self
            .get_global_var("OFMT")
            .map(|v| v.as_str())
            .unwrap_or_else(|| "%.6g".to_string());
        crate::format::awk_sprintf_with_decimal(
            &fmt,
            &[Value::Mpfr(f.clone())],
            self.numeric_decimal,
            self.numeric_thousands_sep,
            Some((self.mpfr_prec_bits(), self.mpfr_round())),
        )
        .unwrap_or_else(|_| f.to_string())
    }

    /// Write `$n` from an MPFR using **`CONVFMT`**-style string (field materialization).
    pub fn set_field_from_mpfr(&mut self, i: i32, f: &Float) {
        let s = self.mpfr_to_string_convfmt(f);
        self.set_field(i, &s);
    }

    /// Next **record** from the primary input stream (respects `RS`), used by `getline` with no redirection.
    pub fn read_line_primary(&mut self) -> Result<Option<String>> {
        let Some(reader) = self.input_reader.clone() else {
            return Err(Error::Runtime(
                "`getline` with no file is only valid during normal input".into(),
            ));
        };
        let rs = self.rs_string();
        self.ensure_rs_regex_bytes()?;
        let mut rt_sep = Vec::new();
        if !crate::record_io::read_next_record(
            &reader,
            &rs,
            &mut self.line_buf,
            &mut rt_sep,
            self.rs_regex_bytes.as_ref(),
        )? {
            return Ok(None);
        }
        self.set_rt_from_bytes(&rt_sep);
        let end = if rs == "\n" {
            crate::record_io::trim_end_record_bytes(&self.line_buf)
        } else {
            self.line_buf.len()
        };
        Ok(Some(
            String::from_utf8_lossy(&self.line_buf[..end]).into_owned(),
        ))
    }

    /// `getline var < filename` — one line from a kept-open file handle.
    pub fn read_line_file(&mut self, path: &str) -> Result<Option<String>> {
        self.require_unsandboxed_io()?;
        if path.starts_with("/inet/udp/") {
            self.ensure_inet_udp(path)?;
            let s = self.inet_udp.get_mut(path).unwrap();
            let mut buf = [0u8; 65536];
            let n = s
                .recv(&mut buf)
                .map_err(|e| Error::Runtime(format!("inet udp recv `{path}`: {e}")))?;
            if n == 0 {
                return Ok(None);
            }
            return Ok(Some(String::from_utf8_lossy(&buf[..n]).into_owned()));
        }
        if path.starts_with("/inet/tcp/") {
            self.ensure_inet_tcp_pair(path)?;
            let reader = self.inet_tcp_read.get_mut(path).unwrap();
            let mut line = String::new();
            let n = reader.read_line(&mut line).map_err(Error::Io)?;
            if n == 0 {
                return Ok(None);
            }
            return Ok(Some(line));
        }
        if path.starts_with("/inet/") {
            return Err(Error::Runtime(format!(
                "unsupported inet path `{path}` (use /inet/tcp/... or /inet/udp/...)"
            )));
        }
        let p = Path::new(path);
        if p.is_dir() {
            self.require_unsandboxed_io()?;
            if !self.dir_read.contains_key(path) {
                let mut names: Vec<String> = std::fs::read_dir(p)
                    .map_err(|e| Error::Runtime(format!("read_dir {path}: {e}")))?
                    .filter_map(|e| e.ok().map(|x| x.file_name().to_string_lossy().into_owned()))
                    .collect();
                names.sort();
                self.dir_read.insert(path.to_string(), (names, 0));
            }
            let (names, i) = self.dir_read.get_mut(path).unwrap();
            if *i >= names.len() {
                return Ok(None);
            }
            let name = names[*i].clone();
            *i += 1;
            return Ok(Some(name));
        }
        if !self.file_handles.contains_key(path) {
            let f = File::open(p).map_err(|e| Error::Runtime(format!("open {path}: {e}")))?;
            self.file_handles
                .insert(path.to_string(), BufReader::new(f));
        }
        let to = self.read_timeout_ms();
        let reader = self.file_handles.get_mut(path).unwrap();
        #[cfg(unix)]
        if to > 0 {
            use std::os::unix::io::AsRawFd;
            let fd = reader.get_ref().as_raw_fd();
            wait_fd_read_timeout(fd, to)?;
        }
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line))
    }

    fn ensure_inet_tcp_pair(&mut self, path: &str) -> Result<()> {
        if self.inet_tcp_read.contains_key(path) {
            return Ok(());
        }
        let (lport, host, rport) = parse_inet_tcp(path)
            .ok_or_else(|| Error::Runtime(format!("invalid /inet/tcp/ path `{path}`")))?;
        let stream = if lport == 0 {
            TcpStream::connect((host.as_str(), rport))
                .map_err(|e| Error::Runtime(format!("inet connect `{path}`: {e}")))?
        } else {
            tcp_connect_with_local_port(&host, lport, rport)?
        };
        let w = stream
            .try_clone()
            .map_err(|e| Error::Runtime(format!("inet: {e}")))?;
        let to = self.read_timeout_ms();
        if to > 0 {
            let d = Duration::from_millis(to as u64);
            stream
                .set_read_timeout(Some(d))
                .map_err(|e| Error::Runtime(format!("inet tcp read timeout: {e}")))?;
        }
        self.inet_tcp_read
            .insert(path.to_string(), BufReader::new(stream));
        self.inet_tcp_write.insert(path.to_string(), w);
        Ok(())
    }

    fn ensure_inet_udp(&mut self, path: &str) -> Result<()> {
        if self.inet_udp.contains_key(path) {
            return Ok(());
        }
        let (lport, host, rport) = parse_inet_udp(path)
            .ok_or_else(|| Error::Runtime(format!("invalid /inet/udp/ path `{path}`")))?;
        let mut addrs = format!("{host}:{rport}")
            .to_socket_addrs()
            .map_err(|e| Error::Runtime(format!("inet udp resolve `{host}`: {e}")))?;
        let addr = addrs
            .next()
            .ok_or_else(|| Error::Runtime(format!("inet udp: no address for `{host}:{rport}`")))?;
        let socket = match addr {
            SocketAddr::V4(_) => UdpSocket::bind((Ipv4Addr::UNSPECIFIED, lport)),
            SocketAddr::V6(_) => UdpSocket::bind((Ipv6Addr::UNSPECIFIED, lport)),
        }
        .map_err(|e| Error::Runtime(format!("inet udp bind `{path}`: {e}")))?;
        socket
            .connect(addr)
            .map_err(|e| Error::Runtime(format!("inet udp connect `{path}`: {e}")))?;
        let to = self.read_timeout_ms();
        if to > 0 {
            socket
                .set_read_timeout(Some(Duration::from_millis(to as u64)))
                .map_err(|e| Error::Runtime(format!("inet udp read timeout: {e}")))?;
        }
        self.inet_udp.insert(path.to_string(), socket);
        Ok(())
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
        let _ = self.dir_read.remove(path);
        let _ = self.inet_tcp_read.remove(path);
        let _ = self.inet_tcp_write.remove(path);
        let _ = self.inet_udp.remove(path);
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
    /// Uses `cached_fs` when available (set by `set_field_sep_split`) to avoid per-record
    /// HashMap lookups and String allocations for the common case.
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
        let ic = self.ignore_case_flag();
        if let Some(fw) = self.fieldwidths_vec() {
            if !fw.is_empty() {
                split_fields_fieldwidths(record, &fw, &mut self.field_ranges);
                self.fields.clear();
                self.fields_dirty = false;
                return;
            }
        }
        // Check FPAT: use Cow to avoid heap alloc when the value is already a string.
        let has_fpat = self
            .get_global_var("FPAT")
            .map(|v| match v {
                Value::Str(s) => !s.trim().is_empty(),
                _ => false,
            })
            .unwrap_or(false);
        if has_fpat {
            let fp = self
                .get_global_var("FPAT")
                .map(|v| v.as_str())
                .unwrap_or_default();
            let fp_trimmed = fp.trim();
            if !fp_trimmed.is_empty()
                && split_fields_fpat(record, fp_trimmed, &mut self.field_ranges, ic)
            {
                return;
            }
        }
        // Use cached_fs (set by set_field_sep_split) to avoid HashMap lookup + String clone.
        if !self.cached_fs.is_empty() {
            split_fields_into(record, &self.cached_fs, &mut self.field_ranges, ic);
        } else {
            let fs_str = self
                .get_global_var("FS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".to_string());
            split_fields_into(record, &fs_str, &mut self.field_ranges, ic);
        }
    }

    fn fieldwidths_vec(&self) -> Option<Vec<usize>> {
        let t = self.get_global_var("FIELDWIDTHS")?.as_str();
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        let v: Vec<usize> = t
            .split_whitespace()
            .filter_map(|w| w.parse::<usize>().ok())
            .filter(|&w| w > 0)
            .collect();
        if v.is_empty() {
            None
        } else {
            Some(v)
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
        let rs = self.rs_string();
        let mut end = self.line_buf.len();
        if rs == "\n" {
            while end > 0 && (self.line_buf[end - 1] == b'\n' || self.line_buf[end - 1] == b'\r') {
                end -= 1;
            }
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
        // Sync cached_fs from vars (non-allocating check; only copies when changed).
        let fs_changed = match self.vars.get("FS") {
            Some(Value::Str(s)) => s.as_str() != self.cached_fs,
            _ => false,
        };
        if fs_changed {
            if let Some(Value::Str(s)) = self.vars.get("FS") {
                self.cached_fs.clear();
                self.cached_fs.push_str(s);
            }
        }
        // Split using current FPAT or FS
        self.fields_dirty = false;
        self.fields.clear();
        self.field_ranges.clear();
        self.split_record_fields();
    }

    /// `SYMTAB[name]` — live global / slot value (gawk introspection).
    pub fn symtab_elem_get(&self, key: &str) -> Value {
        if let Some(&slot) = self.symtab_slot_map.get(key) {
            let i = slot as usize;
            if i < self.slots.len() {
                return self.slots[i].clone();
            }
        }
        self.get_global_var(key)
            .cloned()
            .unwrap_or_else(|| self.builtin_scalar_symtab(key))
    }

    fn builtin_scalar_symtab(&self, name: &str) -> Value {
        match name {
            "NR" => Value::Num(self.nr),
            "FNR" => Value::Num(self.fnr),
            "NF" => Value::Num(if self.fields_dirty {
                self.fields.len()
            } else {
                self.field_ranges.len()
            } as f64),
            "FILENAME" => Value::Str(self.filename.clone()),
            _ => Value::Uninit,
        }
    }

    /// Enumerate SYMTAB keys (globals, slot-backed names, special scalars).
    pub fn symtab_keys_reflect(&self) -> Vec<String> {
        use rustc_hash::FxHashSet;
        let mut seen = FxHashSet::default();
        for k in self.vars.keys() {
            if matches!(k.as_str(), "SYMTAB" | "FUNCTAB" | "PROCINFO") {
                continue;
            }
            seen.insert(k.clone());
        }
        if let Some(g) = &self.global_readonly {
            for k in g.keys() {
                if matches!(k.as_str(), "SYMTAB" | "FUNCTAB" | "PROCINFO") {
                    continue;
                }
                seen.insert(k.clone());
            }
        }
        for k in self.symtab_slot_map.keys() {
            seen.insert(k.clone());
        }
        for &s in crate::namespace::SPECIAL_GLOBAL_NAMES {
            seen.insert((*s).to_string());
        }
        let mut out: Vec<_> = seen.into_iter().collect();
        out.sort();
        out
    }

    fn symtab_has_key(&self, key: &str) -> bool {
        if self.symtab_slot_map.contains_key(key) {
            return true;
        }
        if self.vars.contains_key(key) && !matches!(key, "SYMTAB" | "FUNCTAB" | "PROCINFO") {
            return true;
        }
        if self
            .global_readonly
            .as_ref()
            .is_some_and(|g| g.contains_key(key))
        {
            return true;
        }
        !matches!(self.symtab_elem_get(key), Value::Uninit)
    }

    /// `SYMTAB[name] = v` — assign global or slot (not a materialized mirror array).
    pub fn symtab_elem_set(&mut self, key: &str, val: Value) {
        if let Some(&slot) = self.symtab_slot_map.get(key) {
            let i = slot as usize;
            if i < self.slots.len() {
                self.slots[i] = val;
                return;
            }
        }
        match key {
            "OFS" => self.ofs_bytes = val.as_str().into_bytes(),
            "ORS" => self.ors_bytes = val.as_str().into_bytes(),
            _ => {}
        }
        self.vars.insert(key.to_string(), val);
    }

    #[inline]
    pub fn array_get(&self, name: &str, key: &str) -> Value {
        if name == "SYMTAB" {
            return self.symtab_elem_get(key);
        }
        match self.get_global_var(name) {
            Some(Value::Array(a)) => match a.get(key) {
                Some(Value::Num(n)) => Value::Num(*n),
                Some(v) => v.clone(),
                None => Value::Str(String::new()),
            },
            _ => Value::Str(String::new()),
        }
    }

    pub fn array_set(&mut self, name: &str, key: String, val: Value) {
        if name == "SYMTAB" {
            self.symtab_elem_set(&key, val);
            return;
        }
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

    /// Fused `a[$field] += delta` (constant field index, e.g. `$5`): build the key from
    /// the split record once and update the array in one map pass.
    ///
    /// Avoids `field(i).as_str()` which allocated twice per call (field string + clone for
    /// `as_str()`), and avoids separate `array_get` + `array_set` lookups.
    ///
    /// Uses a substring of `record` / `fields` as `&str` for `get_mut` so repeated field
    /// values do not allocate a `String` per line; inserts still allocate once for the key.
    pub fn array_field_add_delta(&mut self, name: &str, field: i32, delta: f64) {
        self.ensure_fields_split();
        if field < 1 {
            Self::apply_array_numeric_delta(&mut self.vars, &self.global_readonly, name, "", delta);
            return;
        }
        let idx = (field - 1) as usize;
        if self.fields_dirty {
            let key = self.fields.get(idx).map(|s| s.as_str()).unwrap_or("");
            Self::apply_array_numeric_delta(
                &mut self.vars,
                &self.global_readonly,
                name,
                key,
                delta,
            );
            return;
        }
        let (s, e) = match self.field_ranges.get(idx) {
            Some(&(s, e)) => (s as usize, e as usize),
            None => {
                Self::apply_array_numeric_delta(
                    &mut self.vars,
                    &self.global_readonly,
                    name,
                    "",
                    delta,
                );
                return;
            }
        };
        let key = &self.record[s..e];
        Self::apply_array_numeric_delta(&mut self.vars, &self.global_readonly, name, key, delta);
    }

    /// Shared body for [`array_field_add_delta`](Self::array_field_add_delta); separate from
    /// `&mut self` so callers can borrow `record` / `fields` for `key` while mutating `vars`.
    fn apply_array_numeric_delta(
        vars: &mut AwkMap<String, Value>,
        global_readonly: &Option<Arc<AwkMap<String, Value>>>,
        name: &str,
        key: &str,
        delta: f64,
    ) {
        if let Some(existing) = vars.get_mut(name) {
            match existing {
                Value::Array(a) => {
                    if let Some(v) = a.get_mut(key) {
                        let n = v.as_number() + delta;
                        *v = Value::Num(n);
                    } else {
                        a.insert(key.to_string(), Value::Num(delta));
                    }
                    return;
                }
                _ => {
                    let mut m = AwkMap::default();
                    m.insert(key.to_string(), Value::Num(delta));
                    *existing = Value::Array(m);
                    return;
                }
            }
        }
        if let Some(Value::Array(a)) = global_readonly.as_ref().and_then(|g| g.get(name)) {
            let mut copy = a.clone();
            let old = copy.get(key).map(|v| v.as_number()).unwrap_or(0.0);
            copy.insert(key.to_string(), Value::Num(old + delta));
            vars.insert(name.to_string(), Value::Array(copy));
        } else {
            let mut m = AwkMap::default();
            m.insert(key.to_string(), Value::Num(delta));
            vars.insert(name.to_string(), Value::Array(m));
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

    /// Keys for `for (k in arr)` / `SYMTAB` in **sorted** order. When `PROCINFO["sorted_in"]` names a
    /// **user function**, sorting requires VM/interpreter context — use [`crate::vm::VmCtx::for_in_keys`]
    /// or the interpreter path; this method returns **unsorted** hash iteration order in that case.
    pub fn array_keys(&self, name: &str) -> Vec<String> {
        if name == "SYMTAB" {
            let mut keys = self.symtab_keys_reflect();
            if self.posix {
                return keys;
            }
            let mode = sorted_in_mode(self);
            if matches!(mode, SortedInMode::CustomFn(_)) {
                return keys;
            }
            let mut tmp: AwkMap<String, Value> = AwkMap::default();
            for k in &keys {
                tmp.insert(k.clone(), self.symtab_elem_get(k));
            }
            sort_for_in_keys(&mut keys, &tmp, mode);
            return keys;
        }
        let Some(Value::Array(a)) = self.get_global_var(name) else {
            return Vec::new();
        };
        let mut keys: Vec<String> = a.keys().cloned().collect();
        if self.posix {
            return keys;
        }
        let mode = sorted_in_mode(self);
        if matches!(mode, SortedInMode::CustomFn(_)) {
            return keys;
        }
        sort_for_in_keys(&mut keys, a, mode);
        keys
    }

    /// `key in arr` — true iff `arr` is an array that has `key` (POSIX: subscript was used).
    #[inline]
    pub fn array_has(&self, name: &str, key: &str) -> bool {
        if name == "SYMTAB" {
            return self.symtab_has_key(key);
        }
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

/// Field-splitting for `split(s, a [, fs])` — same algorithm as [`crate::bytecode::Op::Split`].
pub fn split_string_by_field_separator(s: &str, fs: &str, ignore_case: bool) -> Vec<String> {
    if fs.is_empty() {
        s.chars().map(|c| c.to_string()).collect()
    } else if fs == " " {
        s.split_whitespace().map(String::from).collect()
    } else if fs.len() == 1 {
        s.split(fs).map(String::from).collect()
    } else {
        let mut b = RegexBuilder::new(fs);
        b.case_insensitive(ignore_case);
        match b.build() {
            Ok(re) => re.split(s).map(String::from).collect(),
            Err(_) => s.split(fs).map(String::from).collect(),
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
            inet_tcp_read: HashMap::new(),
            inet_tcp_write: HashMap::new(),
            inet_udp: HashMap::new(),
            gettext_dir: self.gettext_dir.clone(),
            bignum: self.bignum,
            file_handles: HashMap::new(),
            dir_read: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed: self.rand_seed,
            numeric_decimal: self.numeric_decimal,
            numeric_thousands_sep: self.numeric_thousands_sep,
            slots: self.slots.clone(),
            regex_cache: self.regex_cache.clone(),
            memmem_finder_cache: self.memmem_finder_cache.clone(),
            print_buf: Vec::new(),
            ofs_bytes: self.ofs_bytes.clone(),
            ors_bytes: self.ors_bytes.clone(),
            vm_stack: Vec::with_capacity(64),
            jit_slot_buf: Vec::new(),
            csv_mode: self.csv_mode,
            rs_pattern_for_regex: self.rs_pattern_for_regex.clone(),
            rs_regex_bytes: self.rs_regex_bytes.clone(),
            sandbox: self.sandbox,
            characters_as_bytes: self.characters_as_bytes,
            posix: self.posix,
            traditional: self.traditional,
            jit_enabled: self.jit_enabled,
            gettext_catalogs: self.gettext_catalogs.clone(),
            symtab_slot_map: self.symtab_slot_map.clone(),
            profile_record_hits: Vec::new(),
            sorted_in_warned: Cell::new(self.sorted_in_warned.get()),
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
