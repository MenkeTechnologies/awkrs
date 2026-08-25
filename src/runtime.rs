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
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::awkstr::AwkStr;
use crate::bignum::value_to_mpfr;
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
    /// `Unsorted` variant.
    Unsorted,
    /// `IndStrAsc` variant.
    IndStrAsc,
    /// `IndStrDesc` variant.
    IndStrDesc,
    /// `IndNumAsc` variant.
    IndNumAsc,
    /// `IndNumDesc` variant.
    IndNumDesc,
    /// `ValStrAsc` variant.
    ValStrAsc,
    /// `ValStrDesc` variant.
    ValStrDesc,
    /// `ValNumAsc` variant.
    ValNumAsc,
    /// `ValNumDesc` variant.
    ValNumDesc,
    /// `ValTypeAsc` variant.
    ValTypeAsc,
    /// `ValTypeDesc` variant.
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
        Value::Str(_) | Value::StrLit(_) | Value::Regexp(_) => 2,
        Value::Array(_) => 3,
    }
}

pub(crate) fn sort_for_in_keys(keys: &mut [AwkStr], arr: &AwkArray, mode: SortedInMode) {
    use SortedInMode::*;
    match mode {
        Unsorted => {}
        CustomFn(_) => {}
        IndStrAsc => keys.sort(),
        IndStrDesc => keys.sort_by(|a, b| b.cmp(a)),
        IndNumAsc => keys.sort_by(|a, b| {
            parse_number(&a.to_str_lossy())
                .partial_cmp(&parse_number(&b.to_str_lossy()))
                .unwrap_or(Ordering::Equal)
        }),
        IndNumDesc => keys.sort_by(|a, b| {
            parse_number(&b.to_str_lossy())
                .partial_cmp(&parse_number(&a.to_str_lossy()))
                .unwrap_or(Ordering::Equal)
        }),
        ValStrAsc => keys.sort_by(|ka, kb| {
            let sa = arr.get_bytes(ka).map(|v| v.as_str()).unwrap_or_default();
            let sb = arr.get_bytes(kb).map(|v| v.as_str()).unwrap_or_default();
            awk_locale_str_cmp(&sa, &sb)
        }),
        ValStrDesc => keys.sort_by(|ka, kb| {
            let sa = arr.get_bytes(ka).map(|v| v.as_str()).unwrap_or_default();
            let sb = arr.get_bytes(kb).map(|v| v.as_str()).unwrap_or_default();
            awk_locale_str_cmp(&sb, &sa)
        }),
        ValNumAsc => keys.sort_by(|ka, kb| {
            let na = arr.get_bytes(ka).map(|v| v.as_number()).unwrap_or(0.0);
            let nb = arr.get_bytes(kb).map(|v| v.as_number()).unwrap_or(0.0);
            na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
        }),
        ValNumDesc => keys.sort_by(|ka, kb| {
            let na = arr.get_bytes(ka).map(|v| v.as_number()).unwrap_or(0.0);
            let nb = arr.get_bytes(kb).map(|v| v.as_number()).unwrap_or(0.0);
            nb.partial_cmp(&na).unwrap_or(Ordering::Equal)
        }),
        ValTypeAsc => keys.sort_by(|ka, kb| {
            let va = arr.get_bytes(ka);
            let vb = arr.get_bytes(kb);
            let ra = va.map(val_type_rank).unwrap_or(0);
            let rb = vb.map(val_type_rank).unwrap_or(0);
            ra.cmp(&rb).then_with(|| {
                let sa = va.map(|v| v.as_str()).unwrap_or_default();
                let sb = vb.map(|v| v.as_str()).unwrap_or_default();
                awk_locale_str_cmp(&sa, &sb)
            })
        }),
        ValTypeDesc => keys.sort_by(|ka, kb| {
            let va = arr.get_bytes(ka);
            let vb = arr.get_bytes(kb);
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
/// Coerce to MPFR for `-M` (see [`crate::bignum::value_to_mpfr`]).
#[inline]
pub fn value_to_float(v: &Value, prec: u32, round: Round) -> Float {
    value_to_mpfr(v, prec, round)
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
    old.reject_if_array_scalar()?;
    rhs.reject_if_array_scalar()?;
    if !use_mpfr {
        let a = old.as_number();
        let b = rhs.as_number();
        let n = match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => {
                if b == 0.0 {
                    return Err(Error::Runtime("division by zero attempted".into()));
                }
                a / b
            }
            BinOp::Mod => {
                if b == 0.0 {
                    return Err(Error::Runtime("division by zero attempted in `%'".into()));
                }
                a % b
            }
            BinOp::Pow => a.powf(b),
            _ => return Err(Error::Runtime("invalid compound assignment op".into())),
        };
        return Ok(Value::Num(n));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let a = value_to_mpfr(old, prec, round);
    let b = value_to_mpfr(rhs, prec, round);
    let r = match op {
        BinOp::Add => Float::with_val_round(prec, &a + &b, round).0,
        BinOp::Sub => Float::with_val_round(prec, &a - &b, round).0,
        BinOp::Mul => Float::with_val_round(prec, &a * &b, round).0,
        BinOp::Div => {
            if b.is_zero() {
                return Err(Error::Runtime("division by zero attempted".into()));
            }
            Float::with_val_round(prec, &a / &b, round).0
        }
        BinOp::Mod => {
            if b.is_zero() {
                return Err(Error::Runtime("division by zero attempted in `%'".into()));
            }
            Float::with_val_round(prec, &a % &b, round).0
        }
        BinOp::Pow => Float::with_val_round(prec, a.pow(&b), round).0,
        _ => return Err(Error::Runtime("invalid compound assignment op".into())),
    };
    Ok(Value::Mpfr(r))
}

/// Does this redirection target name the program's own standard output?
///
/// `print "x" > "/dev/stdout"` must land in the same byte stream as a plain
/// `print`, interleaved in program order. Opening `/dev/stdout` as an ordinary
/// file gives it a second, independent buffer, and awkrs then emitted
/// `A`/`B`/`C` from `print "A"; print "B" > "/dev/stdout"; print "C"` as
/// `A C B` — the separate buffer drained at exit, after the main one. gawk,
/// mawk and one-true-awk all print `A B C`. Recognising the name and writing
/// through the ordinary print buffer makes the ordering fall out for free
/// instead of depending on flush timing.
fn is_program_stdout(path: &str) -> bool {
    path == "/dev/stdout"
}

/// The file `getline < path` should actually open.
///
/// POSIX gives the operand `-` the meaning "standard input", and gawk, mawk and
/// one-true-awk all honour it for `getline < "-"` as well as for a file
/// operand: all three read stdin for `while ((getline l < "-") > 0)`. awkrs
/// opened a file literally named `-`, which does not exist, so the read
/// returned -1 and the loop never ran. `/dev/stdin` is the same stream and is
/// already handled correctly, so `-` is redirected onto it.
///
/// Only the *input* side is remapped. On the output side the references
/// disagree — gawk writes `print > "-"` to stdout while mawk and one-true-awk
/// create a file named `-` — so there is no single behavior to match and awkrs
/// keeps the mawk / one-true-awk reading.
fn getline_open_path(p: &Path) -> &Path {
    if p.as_os_str() == "-" {
        return Path::new("/dev/stdin");
    }
    p
}

/// The number awk reports for a finished child: `system()` and `close()` on a
/// pipe both answer with this.
///
/// A child that exited normally reports its exit code. A child killed by a
/// signal has no exit code at all, and `ExitStatus::code()` answers `None` for
/// it — reporting that as `-1` loses which signal fired and collides with the
/// "could not wait" answer. gawk, mawk and one-true-awk all report `256 + signo`
/// instead, so `system("kill -TERM $$")` is 271 in every one of them; awkrs
/// answered -1 from `system()` while already answering 271 from `close()`,
/// because the encoding lived in `close_handle` and nowhere else. It lives here
/// now, and both callers go through it.
pub fn awk_process_status(status: std::io::Result<ExitStatus>) -> f64 {
    let Ok(status) = status else { return -1.0 };
    if let Some(code) = status.code() {
        return code as f64;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return (256 + sig) as f64;
        }
    }
    -1.0
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
    /// `child` field.
    pub child: Child,
    /// `stdin` field.
    pub stdin: BufWriter<ChildStdin>,
    /// `stdout` field.
    pub stdout: BufReader<ChildStdout>,
}
/// `Value` — see variants for the choices.
#[derive(Debug, Clone)]
pub enum Value {
    /// Never assigned (missing global, missing function argument, or fresh slot).
    /// String/number contexts treat this like `""` / `0` (same as gawk *untyped*).
    Uninit,
    /// Dynamic string (fields, concat, I/O, etc.) — may be a POSIX *numeric string* in comparisons.
    Str(AwkStr),
    /// String literal from program text (`"..."`) — not a numeric string for relational ops (POSIX).
    StrLit(AwkStr),
    /// gawk: `@/regex/` regexp constant — distinct from [`Value::Str`] for `typeof` and typed `~`.
    Regexp(AwkStr),
    /// `Num` variant.
    Num(f64),
    /// GNU MPFR arbitrary-precision float (`-M` / `--bignum`).
    Mpfr(Float),
    /// `Array` variant.
    Array(AwkArray),
}

/// Default **`-M`** number→string when no [`Runtime`] is available (POSIX default **CONVFMT** **`%.6g`**).
///
/// Uses each MPFR `Float`'s own precision (`Float::prec()`) for `sprintf` MPFR mode so values
/// allocated at high `PROCINFO["prec"]` are not rounded down through a hardcoded bit count during
/// formatting (round mode defaults to nearest when `Runtime` is not in scope).
#[inline]
fn mpfr_value_default_display(f: &Float) -> String {
    let prec = f.prec();
    crate::format::awk_sprintf_with_decimal(
        "%.6g",
        &[Value::Mpfr(f.clone())],
        '.',
        Some(','),
        Some((prec, Round::Nearest)),
    )
    .unwrap_or_else(|_| crate::bignum::mpfr_string_trim_trailing_zeros(f.to_string()))
}

/// Longest leading substring of `s` that `f64::from_str` accepts (POSIX awk string→number prefix rule).
/// Parseability is not monotonic in prefix length (e.g. `1` ok, `1e` err, `1e2` ok), so we scan downward.
///
/// gawk parity: Rust's `f64::from_str` accepts bare `"inf"`, `"nan"`, `"infinity"`, `"Infinity"`, etc.
/// gawk does NOT — bare special names coerce to 0. The only special-name forms gawk accepts are
/// sign-prefixed three-letter `inf` / `nan` (case-insensitive), AND only when not followed by more
/// alphanumeric characters (so `"+infzzz"` and `"+infinity"` both coerce to 0, not `+inf`).
#[inline]
pub(crate) fn longest_f64_prefix(s: &str) -> Option<&str> {
    if s.is_empty() {
        return None;
    }
    // f64 numeric prefixes are entirely ASCII (digits, signs, dot, e/E, inf, nan).
    // Limit the byte-index iteration to the ASCII prefix so `&s[..end]` never
    // lands inside a multi-byte UTF-8 character. (Without this, a string like
    // "ÿ" panicked: "end byte index 1 is not a char boundary; it is inside 'ÿ'".)
    let ascii_len = s.as_bytes().iter().take_while(|&&b| b.is_ascii()).count();
    if ascii_len == 0 {
        return None;
    }
    for end in (1..=ascii_len).rev() {
        let p = &s[..end];
        if !awk_numeric_prefix_acceptable(p, end < s.len() && next_byte_is_alnum(s, end)) {
            continue;
        }
        if p.parse::<f64>().is_ok() {
            return Some(p);
        }
    }
    None
}

#[inline]
fn next_byte_is_alnum(s: &str, end: usize) -> bool {
    s.as_bytes()
        .get(end)
        .map(|c| c.is_ascii_alphanumeric())
        .unwrap_or(false)
}

/// gawk numeric coercion rule: accept ordinary decimal/float prefixes (must contain a digit), plus
/// signed three-letter `inf` / `nan` (case-insensitive). The special-name form is rejected when
/// followed by an alphanumeric byte in the source (gawk requires the keyword to stand alone, so
/// `"+infzzz"` and `"+infinity"` both coerce to 0).
#[inline]
fn awk_numeric_prefix_acceptable(p: &str, has_trailing_alnum: bool) -> bool {
    if p.bytes().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if has_trailing_alnum {
        return false;
    }
    let b = p.as_bytes();
    if b.len() != 4 {
        return false;
    }
    if !matches!(b[0], b'+' | b'-') {
        return false;
    }
    let tail = &p[1..];
    tail.eq_ignore_ascii_case("inf") || tail.eq_ignore_ascii_case("nan")
}

impl Value {
    /// gawk-style fatal: whole arrays cannot be coerced to a string in scalar contexts (`print a`, concat, etc.).
    #[inline]
    pub fn reject_if_array_scalar(&self) -> Result<()> {
        if matches!(self, Value::Array(_)) {
            return Err(Error::Runtime(
                "attempt to use an array in a scalar context".into(),
            ));
        }
        Ok(())
    }
    /// `as_str` — see implementation for the contract.
    pub fn as_str(&self) -> String {
        match self {
            Value::Uninit => String::new(),
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => s.to_lossy_string(),
            Value::Num(n) => format_number(*n),
            Value::Mpfr(f) => mpfr_value_default_display(f),
            Value::Array(_) => String::new(),
        }
    }

    /// For `&str` APIs (e.g. `gsub`) without allocating when the value is already `Str`.
    #[inline]
    pub fn as_str_cow(&self) -> Cow<'_, str> {
        match self {
            Value::Uninit => Cow::Borrowed(""),
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => s.to_str_lossy(),
            Value::Num(n) => Cow::Owned(format_number(*n)),
            Value::Mpfr(f) => Cow::Owned(mpfr_value_default_display(f)),
            Value::Array(_) => Cow::Borrowed(""),
        }
    }

    /// Borrow the inner byte string without cloning. `None` for Num/Array.
    #[inline]
    #[allow(dead_code)]
    pub fn str_ref(&self) -> Option<&AwkStr> {
        match self {
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => Some(s),
            _ => None,
        }
    }

    /// The value's **bytes**, borrowing when it already holds them.
    ///
    /// This is the byte-exact counterpart of [`Self::as_str_cow`], and the one
    /// any path the awk program can observe should use: `as_str`/`as_str_cow`
    /// render through `U+FFFD` and cannot round-trip a byte that is not part of
    /// valid UTF-8. A number renders through `CONVFMT` exactly as before — that
    /// text is always ASCII, so the two agree for every numeric value.
    #[inline]
    pub fn as_bytes_cow(&self) -> Cow<'_, [u8]> {
        match self {
            Value::Uninit | Value::Array(_) => Cow::Borrowed(b""),
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => Cow::Borrowed(s.as_bytes()),
            Value::Num(n) => Cow::Owned(format_number(*n).into_bytes()),
            Value::Mpfr(f) => Cow::Owned(mpfr_value_default_display(f).into_bytes()),
        }
    }

    /// Write the string representation directly into a byte buffer — zero allocation
    /// for the Str case, one `write!` for Num.
    pub fn write_to(&self, buf: &mut Vec<u8>) {
        match self {
            Value::Uninit => {}
            Value::Str(s) | Value::StrLit(s) => buf.extend_from_slice(s.as_bytes()),
            Value::Regexp(s) => buf.extend_from_slice(s.as_bytes()),
            Value::Num(n) => {
                use std::io::Write;
                let n = *n;
                if n.is_finite() && n.fract() == 0.0 {
                    let _ = write!(buf, "{:.0}", n);
                } else {
                    let _ = write!(buf, "{n}");
                }
            }
            Value::Mpfr(f) => buf.extend_from_slice(mpfr_value_default_display(f).as_bytes()),
            Value::Array(_) => {}
        }
    }
    /// `as_number` — see implementation for the contract.
    pub fn as_number(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) => *n,
            Value::Str(s) | Value::StrLit(s) => parse_number(&s.to_str_lossy()),
            Value::Regexp(s) => parse_number(&s.to_str_lossy()),
            Value::Mpfr(f) => f.to_f64(),
            Value::Array(_) => 0.0,
        }
    }
    /// `truthy` — see implementation for the contract.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) => *n != 0.0,
            // POSIX: string LITERALS (Value::StrLit, from source) are truthy
            // iff non-empty — "0", "false", " " are all truthy strings.
            Value::StrLit(s) => !s.is_empty(),
            // `Value::Str` comes from input (fields, -v, getline) and may be a
            // "numeric string": if it parses cleanly as a number, use numeric
            // truthiness; otherwise non-empty.
            Value::Str(s) => {
                if s.is_empty() {
                    false
                } else if let Ok(n) = s.to_str_lossy().parse::<f64>() {
                    n != 0.0
                } else {
                    true
                }
            }
            Value::Regexp(s) => !s.is_empty(),
            Value::Mpfr(f) => !f.is_zero(),
            Value::Array(a) => !a.is_empty(),
        }
    }

    /// Boolean tests in `if` / `while` / `for` / `?:` — whole arrays are a fatal error (gawk).
    pub fn truthy_cond(&self) -> crate::error::Result<bool> {
        self.reject_if_array_scalar()?;
        Ok(match self {
            Value::Uninit => false,
            Value::Num(n) => *n != 0.0,
            // Same rule as `truthy()` — see comments there.
            Value::StrLit(s) => !s.is_empty(),
            Value::Str(s) => {
                if s.is_empty() {
                    false
                } else if let Ok(n) = s.to_str_lossy().parse::<f64>() {
                    n != 0.0
                } else {
                    true
                }
            }
            Value::Regexp(s) => !s.is_empty(),
            Value::Mpfr(f) => !f.is_zero(),
            Value::Array(_) => unreachable!(),
        })
    }

    /// Take ownership of the inner String, converting numbers to string form.
    /// Avoids clone when the Value is already a Str variant.
    #[inline]
    pub fn into_string(self) -> String {
        match self {
            Value::Uninit => String::new(),
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => s.to_lossy_string(),
            Value::Num(n) => format_number(n),
            Value::Mpfr(f) => mpfr_value_default_display(&f),
            Value::Array(_) => String::new(),
        }
    }

    /// Append this value's string representation to an existing String.
    /// Avoids intermediate allocation compared to `format!("{a}{b}")`.
    #[allow(dead_code)] // tested but no production call sites currently
    #[inline]
    pub fn append_to_string(&self, buf: &mut String) {
        match self {
            Value::Uninit => {}
            Value::Str(s) | Value::StrLit(s) => buf.push_str(&s.to_str_lossy()),
            Value::Regexp(s) => buf.push_str(&s.to_str_lossy()),
            Value::Num(n) => {
                use std::fmt::Write;
                let n = *n;
                if n.is_finite() && n.fract() == 0.0 {
                    let _ = write!(buf, "{:.0}", n);
                } else {
                    let _ = write!(buf, "{n}");
                }
            }
            Value::Mpfr(f) => buf.push_str(&mpfr_value_default_display(f)),
            Value::Array(_) => {}
        }
    }

    /// POSIX-style: true if the value is numeric (including string that looks like number).
    pub fn is_numeric_str(&self) -> bool {
        match self {
            // Uninitialized scalar has dual 0 / "" — participates in numeric comparisons like 0.
            Value::Uninit => true,
            Value::Num(_) => true,
            Value::Mpfr(_) => true,
            Value::StrLit(_) => false,
            Value::Str(s) => {
                // POSIX / gawk: a string is "numeric" only if the ENTIRE trimmed text is a
                // valid number — `"42abc"` is NOT numeric (typeof is `"string"`), even though
                // `+"42abc"` yields 42 via prefix parsing. Earlier we used the prefix check
                // alone, which made `typeof($1)` report `strnum` for noisy fields like
                // `"42abc"` and made `"42abc" == 42` a numeric compare instead of string.
                let s = s.to_str_lossy();
                let t = s.trim();
                if t.is_empty() {
                    return false;
                }
                match longest_f64_prefix(t) {
                    Some(p) => p.len() == t.len(),
                    None => false,
                }
            }
            Value::Regexp(_) => false,
            Value::Array(_) => false,
        }
    }
}

/// Format a number to string (awk rules: integer form if no fractional part).
/// Uses `{:.0}` for integer-valued floats so values past `i64::MAX` (e.g. 1e25)
/// still display as decimals — matches gawk's `printf("%.0f", n)` behavior.
#[inline]
fn format_number(n: f64) -> String {
    if !n.is_finite() {
        // gawk: "+inf", "-inf", "+nan", "-nan" — emit the same spelling that `print` /
        // OFMT-driven `%g` use so `printf "%s"` of a NaN matches `print` of the same value.
        let sign = if n.is_sign_negative() { '-' } else { '+' };
        let body = if n.is_nan() { "nan" } else { "inf" };
        return format!("{sign}{body}");
    }
    if n.fract() == 0.0 {
        // gawk parity: `print -0.0` produces `"0"`, not `"-0"`. Normalize negative
        // zero so the two awks agree on numeric zero output.
        if n == 0.0 {
            return "0".to_string();
        }
        format!("{:.0}", n)
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
    longest_f64_prefix(t)
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(0.0)
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
    longest_f64_prefix(s)
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(0.0)
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
/// Split a top-level alternation regex into its alternatives (split on `|`
/// outside of `[]` and `()`). Backslash-escaped chars are passed through
/// without splitting. Returns the original pattern as a single element when
/// no top-level `|` is present.
fn split_toplevel_alternatives(pat: &str) -> Vec<String> {
    let mut alts = Vec::new();
    let mut cur = String::new();
    let mut brackets = 0i32;
    let mut parens = 0i32;
    let mut chars = pat.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push(c);
                if let Some(&next) = chars.peek() {
                    cur.push(next);
                    chars.next();
                }
            }
            '[' => {
                brackets += 1;
                cur.push(c);
            }
            ']' if brackets > 0 => {
                brackets -= 1;
                cur.push(c);
            }
            '(' => {
                parens += 1;
                cur.push(c);
            }
            ')' if parens > 0 => {
                parens -= 1;
                cur.push(c);
            }
            '|' if brackets == 0 && parens == 0 => {
                alts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    alts.push(cur);
    alts
}

/// FPAT splitting follows gawk's **leftmost-longest** semantic (POSIX-style),
/// NOT Rust's regex crate default of leftmost-first. With a pattern like
/// `[^,]*|"[^"]*"` against `abc,"def, ghi",xyz`, leftmost-first would pick the
/// first alternative at every position (splitting on commas inside quoted
/// regions); leftmost-longest correctly preserves quoted fields.
///
/// We approximate POSIX semantics by:
///   1. Splitting the FPAT into top-level alternatives on `|`.
///   2. At each position, trying every alternative anchored at that position
///      and picking the longest non-empty match.
///   3. Skipping positions with no non-empty match.
///
/// For a single-alternative pattern this collapses to the original behavior
/// (with empty-match skipping for safety).
fn split_fields_fpat(record: &[u8], fpat: &str, field_ranges: &mut Vec<(u32, u32)>) -> bool {
    field_ranges.clear();
    if record.is_empty() {
        return true;
    }
    // "We compile them once" used to mean once per *call*, and this runs once
    // per record — `FPAT="[0-9]+"` over 300 000 records cost 3.04 s of CPU
    // against gawk's 1.39 s. Memoised like the `FS` and `split()` engines.
    with_fpat_regexes(fpat, |compiled| {
        let Some(compiled) = compiled else {
            return false;
        };
        fpat_scan(record, compiled, field_ranges)
    })
}

/// The leftmost-longest scan, once the alternatives are compiled.
fn fpat_scan(record: &[u8], compiled: &[BytesRegex], field_ranges: &mut Vec<(u32, u32)>) -> bool {
    let n = record.len();
    let bytes = record;
    let mut pos = 0usize;
    while pos < n {
        let tail = &record[pos..];
        let mut best_end: Option<usize> = None;
        for re in compiled {
            if let Some(m) = re.find(tail) {
                // `^(?:…)` ensures m.start() == 0.
                let end = m.end();
                if end > 0 && best_end.is_none_or(|b| end > b) {
                    best_end = Some(end);
                }
            }
        }
        if let Some(end) = best_end {
            let abs_end = pos + end;
            field_ranges.push((pos as u32, abs_end as u32));
            pos = abs_end;
        } else {
            // Advance one char (UTF-8 safe).
            pos += utf8_char_len_at(bytes, pos);
        }
    }
    true
}

#[inline]
fn utf8_char_len_at(bytes: &[u8], pos: usize) -> usize {
    if pos >= bytes.len() {
        return 1;
    }
    let b = bytes[pos];
    // b < 0xC0 covers both ASCII (< 0x80) and continuation bytes (0x80..0xC0);
    // both advance by 1 byte. Continuation bytes shouldn't appear at a char
    // boundary, but treat them safely as 1-byte advances.
    if b < 0xC0 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// A FIELDWIDTHS spec entry: skip `skip` bytes before the field, then take `width` bytes.
/// `width == usize::MAX` means "take everything remaining" (gawk's `*` token).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldwidthsSpec {
    pub(crate) skip: usize,
    pub(crate) width: usize,
}

const FIELDWIDTHS_REST: usize = usize::MAX;

fn split_fields_fieldwidths(
    record: &[u8],
    specs: &[FieldwidthsSpec],
    field_ranges: &mut Vec<(u32, u32)>,
) {
    field_ranges.clear();
    if specs.is_empty() {
        return;
    }
    let b = record;
    let n = b.len();
    let mut pos = 0usize;
    for spec in specs {
        // Skip leading bytes (gawk `skip:width`); cannot read past end.
        pos = (pos + spec.skip).min(n);
        let end = if spec.width == FIELDWIDTHS_REST {
            n
        } else {
            (pos + spec.width).min(n)
        };
        field_ranges.push((pos as u32, end as u32));
        pos = end;
        if pos >= n {
            break;
        }
    }
}

/// gawk `--csv` / `-k` field splitting: comma-separated, `"..."` for quoting, `""` for a literal `"`.
/// Field ranges are **value** byte ranges (no surrounding quote characters), matching gawk’s `$n` text.
/// gawk `--csv` field splitting (RFC 4180-ish): commas separate fields,
/// double-quoted fields may contain commas, and `""` inside a quoted field is
/// an escaped quote. Empty record → 0 fields; otherwise the field count is
/// always `(commas at top level) + 1`.
fn split_csv_gawk_fields(record: &[u8], field_ranges: &mut Vec<(u32, u32)>) {
    field_ranges.clear();
    let bytes = record;
    let n = bytes.len();
    if n == 0 {
        return;
    }
    let mut i = 0usize;
    loop {
        // Each iteration emits exactly one field starting at position `i`.
        if i < n && bytes[i] == b'"' {
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
        // After the field, expect EOR or a comma separator.
        if i >= n {
            return;
        }
        // bytes[i] == ',' — consume it and loop to emit the next (possibly
        // empty) field. This is what makes `,,,` produce 4 empty fields, not 3.
        debug_assert_eq!(bytes[i], b',');
        i += 1;
    }
}

/// Compile a regex `FS`. `None` when the pattern is not a valid regex, which
/// makes the splitter fall back to a literal split.
///
/// The one place the flags are set, so the memoised engine and the ad-hoc one
/// can never disagree: `IGNORECASE` applies (only a multi-character `FS` honours
/// it), and gawk lets `.` match a newline in an ERE, which matters once `RS`
/// allows a record to contain one.
///
/// Runs the same [`translate_awk_re_to_rust`] rewrite the `~` operator gets.
/// Without it a separator reached Rust's parser raw, so the awk spellings that
/// only the rewrite understands were lost: `split(s, a, "\\101")` and
/// `split(s, a, "[\\101]")` failed to compile and fell back to a literal split
/// where all three references split on `A`, `"\\8"` likewise where they split
/// on `8`, and `"\\d"` compiled as Rust's digit class where all three match a
/// literal `d`.
fn build_fs_regex(fs: &str, ignore_case: bool) -> Option<BytesRegex> {
    let translated = translate_awk_re_to_rust(fs);
    let mut b = regex::bytes::RegexBuilder::new(&translated);
    // Same locale rule as the `~` engine — see `Runtime::ensure_regex`.
    b.unicode(crate::locale_numeric::ctype_is_utf8());
    b.case_insensitive(ignore_case);
    b.dot_matches_new_line(true);
    b.build().ok()
}

/// The memoised engine for `fs`, or [`FsRegex::Unknown`] when the cache holds a
/// different pattern.
///
/// Takes the cache and the pattern as separate borrows rather than `&self`:
/// the caller needs `&mut self.field_ranges` at the same time, and a whole-`self`
/// borrow would rule that out.
fn memoised_fs_regex<'a>(
    cache: &'a Option<(String, bool, Option<BytesRegex>)>,
    fs: &str,
) -> FsRegex<'a> {
    match cache {
        Some((pat, _, engine)) if pat == fs => match engine {
            Some(re) => FsRegex::Compiled(re),
            None => FsRegex::Invalid,
        },
        _ => FsRegex::Unknown,
    }
}

/// Separator engines for `split()`, memoised per thread by pattern text.
///
/// Same finding as the record loop's `FS`, one call site along: the separator is
/// a pure function of its text and `IGNORECASE`, and `split()` inside a record
/// loop recompiled it for every record. Keyed by the pattern alone with the flag
/// stored beside the engine, so a hit costs no allocation; a pattern that does
/// not compile is memoised as `None` so the literal fallback does not pay for
/// the failed compile again either.
///
/// Bounded rather than unbounded: a program that splits on many distinct
/// computed separators would otherwise grow this without limit, and dropping the
/// whole map is correct at any moment — every entry is reconstructible.
const SPLIT_REGEX_MEMO_MAX: usize = 256;

thread_local! {
    static SPLIT_REGEX_MEMO: std::cell::RefCell<AwkMap<String, (bool, Option<BytesRegex>)>> =
        std::cell::RefCell::new(AwkMap::default());
}

/// Run `f` with the memoised engine for `fs` (`None` when `fs` is not a valid
/// regex). The engine is handed to a closure rather than returned because it
/// lives inside the thread-local map.
fn with_split_regex<R>(fs: &str, ignore_case: bool, f: impl FnOnce(Option<&BytesRegex>) -> R) -> R {
    SPLIT_REGEX_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        let fresh = match memo.get(fs) {
            Some((ic, _)) => *ic != ignore_case,
            None => true,
        };
        if fresh {
            if memo.len() >= SPLIT_REGEX_MEMO_MAX {
                memo.clear();
            }
            let compiled = build_fs_regex(fs, ignore_case);
            memo.insert(fs.to_string(), (ignore_case, compiled));
        }
        let entry = memo.get(fs).expect("just inserted");
        f(entry.1.as_ref())
    })
}

// Compiled `FPAT` alternatives, memoised per thread — the `with_split_regex`
// treatment for the other per-record regex. `None` records an `FPAT` that does
// not compile, which is what makes the splitter fall back to `FS`.
//
// No `IGNORECASE` in the key because `FPAT` does not honour it. gawk is the only
// reference that implements `FPAT` at all, and it matches case-sensitively
// regardless:
//
//   printf 'AB cd EF\n' | gawk 'BEGIN{FPAT="[a-z]+";IGNORECASE=1}{print NF, $1}'
//   1 cd          # awkrs answered `3 AB` — the alternatives were compiled
//                 # case-insensitively, so `AB` and `EF` became fields too.
thread_local! {
    static FPAT_REGEX_MEMO: std::cell::RefCell<AwkMap<String, Option<Vec<BytesRegex>>>> =
        std::cell::RefCell::new(AwkMap::default());
}

fn with_fpat_regexes<R>(fpat: &str, f: impl FnOnce(Option<&[BytesRegex]>) -> R) -> R {
    FPAT_REGEX_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if !memo.contains_key(fpat) {
            if memo.len() >= SPLIT_REGEX_MEMO_MAX {
                memo.clear();
            }
            // Every alternative is anchored at the candidate position, so a
            // single failure makes the whole `FPAT` unusable.
            let compiled = split_toplevel_alternatives(fpat)
                .iter()
                .map(|alt| build_fs_regex(&format!("^(?:{alt})"), false))
                .collect::<Option<Vec<BytesRegex>>>();
            memo.insert(fpat.to_string(), compiled);
        }
        let entry = memo.get(fpat).expect("just inserted");
        f(entry.as_deref())
    })
}

/// What the splitter should use for a regex `FS`.
///
/// A multi-character `FS` is a regular expression, and the splitter used to
/// compile it from scratch inside every call — which is once per record in the
/// main loop. `awk -F'[;,]' '{ print $3 }'` over a million lines therefore built
/// the same automaton a million times: 1.98 s of CPU against mawk's 0.16 s on
/// the same file. The record loop now hands in the engine it memoised.
pub enum FsRegex<'a> {
    /// Nothing memoised — compile here. The `split()` builtin and the unit tests
    /// come in this way; they are not the per-record path.
    Unknown,
    /// The engine for exactly this `FS` and `IGNORECASE`.
    Compiled(&'a BytesRegex),
    /// This `FS` is not a valid regex, and the caller already knows it. Falls
    /// back to a literal split without paying for the failed compile again.
    Invalid,
}

/// Split `record` into `field_ranges` (replaces contents). Shared by lazy split and stdin path.
/// Split `record` into field byte-ranges using `fs`.
///
/// `paragraph_mode` is `RS == ""`. POSIX: *"When RS is null … <newline> shall
/// always be a field separator, no matter what the value of FS is."* In practice
/// gawk and one-true-awk apply that only to a **single-character** FS (gawk
/// rewrites FS to `[<fs>\n]` there and leaves a regex FS untouched); mawk does
/// not apply it at all. The default `FS == " "` needs nothing extra — newline is
/// already whitespace — so the flag is only consulted in the single-char branch.
/// Every occurrence of `from` in `hay` replaced by `to`.
fn byte_replace(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut start = 0usize;
    while let Some(off) = memchr::memmem::find(&hay[start..], from) {
        out.extend_from_slice(&hay[start..start + off]);
        out.extend_from_slice(to);
        start += off + from.len();
    }
    out.extend_from_slice(&hay[start..]);
    out
}

/// `[AwkStr]::join` — the pieces separated by `sep`, as one `AwkStr`.
fn join_awkstrs(parts: &[AwkStr], sep: &[u8]) -> AwkStr {
    let mut out = AwkStr::with_capacity(
        parts.iter().map(|p| p.len()).sum::<usize>() + sep.len() * parts.len().saturating_sub(1),
    );
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push_bytes(sep);
        }
        out.push_awkstr(p);
    }
    out
}

/// Byte length of the UTF-8 character starting at `b[0]`, or 1 when the bytes
/// there are not a valid encoding.
///
/// An unpaired byte counting as one character is what makes a character-wise
/// operation total over arbitrary input: the references have no multi-byte
/// character to fold it into either.
pub(crate) fn utf8_char_len(b: &[u8]) -> usize {
    let want = match b[0] {
        0x00..=0x7f => return 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return 1,
    };
    if b.len() >= want && std::str::from_utf8(&b[..want]).is_ok() {
        want
    } else {
        1
    }
}

/// `str::split` for byte strings: the pieces of `hay` between each
/// non-overlapping occurrence of `needle`. An empty `needle` yields `hay` whole,
/// matching what the literal-split fallback wants.
fn byte_split<'h>(hay: &'h [u8], needle: &[u8]) -> Vec<&'h [u8]> {
    if needle.is_empty() {
        return vec![hay];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some(off) = memchr::memmem::find(&hay[start..], needle) {
        out.push(&hay[start..start + off]);
        start += off + needle.len();
    }
    out.push(&hay[start..]);
    out
}

fn split_fields_into(
    record: &[u8],
    fs: &str,
    field_ranges: &mut Vec<(u32, u32)>,
    ignore_case: bool,
    characters_as_bytes: bool,
    paragraph_mode: bool,
    fs_re: FsRegex<'_>,
) {
    field_ranges.clear();
    // POSIX: an empty record has zero fields, whatever FS is. Without this the
    // single-char and regex branches below both push one empty range and report
    // `NF == 1` for a blank line under `FS=":"` — gawk, mawk and one-true-awk
    // all report 0. Only the default (" ") and empty-FS branches got this right.
    if record.is_empty() {
        return;
    }
    // Rough NF estimate from record length reduces per-line `Vec` growth for whitespace/FS splits.
    let want = (record.len() / 16).saturating_add(4).clamp(8, 2048);
    if field_ranges.capacity() < want {
        field_ranges.reserve(want - field_ranges.capacity());
    }
    if fs.is_empty() {
        if characters_as_bytes {
            for i in 0..record.len() {
                field_ranges.push((i as u32, (i + 1) as u32));
            }
        } else {
            // Character split. A byte that does not begin a valid UTF-8
            // character is its own field — the references have no multi-byte
            // character to group it into either.
            let mut i = 0usize;
            while i < record.len() {
                let step = utf8_char_len(&record[i..]);
                field_ranges.push((i as u32, (i + step) as u32));
                i += step;
            }
        }
    } else if fs == " " {
        let bytes = record;
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
        // POSIX / gawk: single-char FS is always a **literal** match; IGNORECASE
        // explicitly does not apply (only multi-char regex FS honors IGNORECASE).
        let sep = fs.as_bytes()[0];
        let bytes = record;
        let mut start = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == sep || (paragraph_mode && b == b'\n') {
                field_ranges.push((start as u32, i as u32));
                start = i + 1;
            }
        }
        field_ranges.push((start as u32, bytes.len() as u32));
    } else {
        // POSIX: multi-character FS is treated as a regular expression. The
        // paragraph-mode newline rule deliberately does NOT apply here — gawk,
        // mawk and one-true-awk all leave a regex FS alone (gawk only rewrites
        // FS to `[<fs>\n]` in the single-character branch), so
        // `RS=""; FS="[0-9]+"` on "a12b\nc345d" is three fields in every
        // reference, with "b\nc" as $2.
        let owned;
        let re: Option<&BytesRegex> = match fs_re {
            FsRegex::Compiled(re) => Some(re),
            FsRegex::Invalid => None,
            FsRegex::Unknown => {
                owned = build_fs_regex(fs, ignore_case);
                owned.as_ref()
            }
        };
        match re {
            Some(re) => {
                let mut last = 0;
                for m in re.find_iter(record) {
                    field_ranges.push((last as u32, m.start() as u32));
                    last = m.end();
                }
                field_ranges.push((last as u32, record.len() as u32));
            }
            None => {
                // Fall back to literal split if the FS is not a valid regex.
                let mut pos = 0;
                for part in byte_split(record, fs.as_bytes()) {
                    let end = pos + part.len();
                    field_ranges.push((pos as u32, end as u32));
                    pos = end + fs.len();
                }
            }
        }
    }
}
/// Value type shared by the fuse_chunk_cache + fuse_prefix_chunk_cache pair:
/// `Some(Some(Arc<(chunk, written_slots)>))` = cached + eligible;
/// `Some(None)` = checked + not eligible (short-circuit without rebuilding).
/// `None` (HashMap miss) = never checked.
pub type FuseChunkSlot = Option<Arc<(fusevm::Chunk, Vec<u16>)>>;

/// `Runtime` — see fields for the structure layout.
pub struct Runtime {
    /// `vars` field.
    pub vars: AwkMap<String, Value>,
    /// Post-`BEGIN` globals shared across parallel record workers (`Arc` clone is O(1)).
    /// Reads resolve `vars` first (per-record overlay), then this map. Not used in the main thread.
    pub global_readonly: Option<Arc<AwkMap<String, Value>>>,
    /// Owned field strings — only populated when a field is modified via `set_field`.
    pub fields: Vec<AwkStr>,
    /// Zero-copy field byte-ranges into `record`. Each `(start, end)` is a byte offset.
    pub field_ranges: Vec<(u32, u32)>,
    /// True when `set_field` has been called and `fields` vec is authoritative.
    pub fields_dirty: bool,
    /// True when record has been set but fields have not been split yet.
    pub fields_pending_split: bool,
    /// Cached FS for lazy field splitting.
    pub cached_fs: String,
    /// `record` field.
    pub record: AwkStr,
    /// True once `$0` has ever been given a value — by reading a record, by
    /// `getline`, or by assigning `$0` / `$n` / `NF`. Only `typeof($0)` reads
    /// it: gawk reports `"unassigned"` for `$0` in a `BEGIN` that has not
    /// touched the record yet and `"string"` afterwards, and merely *reading*
    /// `$0` does not flip it (`BEGIN { x = $0 ""; print typeof($0) }` is still
    /// `"unassigned"`). Every write goes through one of the four places that
    /// assign `self.record`, so the flag is set there rather than at each caller.
    pub record_assigned: bool,
    /// Whether `$0` is a POSIX *numeric string* (see [`Value::Str`] vs
    /// [`Value::StrLit`]). A record read from input is; a record **assigned** a
    /// plain string is not, so `$0 = "42"` makes `$0 < 7` a *string* compare —
    /// `"42" < "7"` — in gawk, mawk and one-true-awk alike, where awkrs used to
    /// re-derive numeric-string-ness from the text and answer numerically.
    /// Rebuilding `$0` from fields (`$n = …`, `NF = …`) also produces a plain
    /// string in gawk and one-true-awk; mawk keeps it a numeric string there and
    /// is the outlier, so this follows the two-reference majority.
    pub record_strnum: bool,
    /// Per-field counterpart of [`record_strnum`](Self::record_strnum), parallel
    /// to `fields` and only consulted while `fields_dirty` — fields produced by
    /// *splitting* a record are always numeric strings, whatever the record was.
    /// An index past the end reads as `true`, so leaving this empty preserves
    /// the split-derived default.
    pub field_strnum: Vec<bool>,
    /// Reusable buffer for input line reading (avoids per-line allocation).
    pub line_buf: Vec<u8>,
    /// Bytes read past the previous record's terminator that belong to the next
    /// record(s). Used by streaming regex/multi-char `RS` to avoid losing input
    /// that was over-consumed in chunked reads.
    pub read_leftover: Vec<u8>,
    /// `nr` field.
    pub nr: f64,
    /// `fnr` field.
    pub fnr: f64,
    /// `filename` field.
    pub filename: String,
    /// Set by `exit`; END rules run before process exit (POSIX).
    pub exit_pending: bool,
    /// `exit_code` field.
    pub exit_code: i32,
    /// Primary input stream for `getline` without `< file` (same as main record loop).
    pub input_reader: Option<SharedInputReader>,
    /// Set once the main record loop has run the primary stream to completion and
    /// detached it. A later plain `getline` (typically in `END`) is then at EOF and
    /// must return `0`, not raise "only valid during normal input" — gawk, mawk and
    /// one-true-awk all return `0` there.
    pub primary_input_done: bool,
    /// `GAWK_READ_TIMEOUT` once read — see [`Self::read_timeout_env_ms`].
    pub read_timeout_env: Cell<Option<i32>>,
    /// Memoised regex `FS`: `(pattern, IGNORECASE, engine)`, where a `None`
    /// engine records a pattern that does not compile. See [`FsRegex`] for why
    /// the record loop cannot afford to build this per record.
    pub fs_regex: Option<(String, bool, Option<BytesRegex>)>,
    /// Open files for `getline < path` / `close`.
    pub file_handles: HashMap<String, BufReader<File>>,
    /// [`Self::read_leftover`], per redirected `getline` stream.
    ///
    /// A regex or multi-character `RS` reads in chunks and can over-consume past
    /// a record's terminator, so each stream needs its own carry-over buffer.
    /// Keyed by the same path/command string as `file_handles`, `pipe_stdout`
    /// and `coproc_handles`, and dropped by `close`.
    pub getline_leftover: HashMap<String, Vec<u8>>,
    /// Directory iteration for `getline var < dir` (gawk **readdir** extension semantics).
    pub dir_read: HashMap<String, (Vec<String>, usize)>,
    /// Open files for `print … > path` / `print … >> path` / `fflush` / `close`.
    pub output_handles: HashMap<String, BufWriter<File>>,
    /// `print`/`printf` `| "cmd"` — stdin of `sh -c cmd` (key is the command string).
    pub pipe_stdin: HashMap<String, BufWriter<ChildStdin>>,
    /// `pipe_children` field.
    pub pipe_children: HashMap<String, Child>,
    /// `"cmd" | getline …` — stdout of `sh -c cmd`, kept open between calls so the
    /// command runs **once** per logical pipe and subsequent `getline`s advance
    /// through the same stream (matches gawk; pre-fix awkrs respawned every call).
    pub pipe_stdout: HashMap<String, BufReader<std::process::ChildStdout>>,
    /// `pipe_input_children` field.
    pub pipe_input_children: HashMap<String, Child>,
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
    /// `rand_seed` field.
    pub rand_seed: u64,
    /// Radix for `%f` / `%g` / etc. and `print` of numbers when `-N` / `--use-lc-numeric` is set (Unix).
    pub numeric_decimal: char,
    /// Thousands separator for gawk **`%'`** (`printf` / `sprintf` integer grouping), from `localeconv()` when available.
    pub numeric_thousands_sep: Option<char>,
    /// Indexed variable slots for the bytecode VM (fast Vec access instead of HashMap).
    pub slots: Vec<Value>,
    /// Parallel to [`Self::slots`]: has this slot ever been *used* — read as a
    /// value or written to? Slots are allocated at compile time and start
    /// [`Value::Uninit`], so without this bit a name the program never mentions
    /// outside `typeof` is indistinguishable from one that was read or assigned
    /// an uninitialized value. gawk separates the two: the first is
    /// **`"untyped"`**, the second **`"unassigned"`**. `typeof` itself does not
    /// set the bit — `BEGIN{ print typeof(z); print typeof(z) }` stays
    /// `untyped` twice in gawk.
    pub slot_touched: Vec<bool>,
    /// Compiled regex cache (case-sensitive) — avoids recompiling the same pattern every record.
    pub regex_cache_cs: AwkMap<Vec<u8>, BytesRegex>,
    /// Compiled regex cache when [`Self::ignore_case_flag`] is true.
    pub regex_cache_ci: AwkMap<Vec<u8>, BytesRegex>,
    /// Cached substring searchers for literal `sub`/`gsub` patterns — faster than `str::contains` per line.
    pub memmem_finder_cache: AwkMap<Vec<u8>, memmem::Finder<'static>>,
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
    /// gawk: `RS` longer than one character is a regex delimiter (cached here).
    pub rs_pattern_for_regex: String,
    /// `rs_regex_bytes` field.
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
    /// Cache of fusevm::Chunk built from awkrs Chunks via
    /// `fusevm_bridge::build_numeric_chunk`. Keyed by (chunk pointer, bignum
    /// flag): the bridge builder does an eligibility check + 2-pass op→fusevm
    /// translation that's identical for the same (chunk, bignum) combo across
    /// every record. Caching skips both passes on subsequent dispatches.
    /// `Some(None)` = checked, not eligible — short-circuit without rebuilding.
    /// The cache value is `Option<Arc<(Chunk, Vec<u16>)>>`. The second tuple
    /// element is the sorted-unique set of slot indices the chunk WRITES (via
    /// `fusevm::Op::SetSlot`), precomputed once so the per-record writeback
    /// only touches modified slots. `Arc`-wrapped so the per-record cache hit
    /// is a refcount bump rather than a deep clone of the `Vec<u16>`.
    /// fusevm's own native-code cache (op_hash-keyed TLS + on-disk) caches the
    /// JIT result; this cache catches the upstream translation work that
    /// previously rebuilt the chunk per record AND the per-record writeback
    /// over-iteration.
    pub fuse_chunk_cache: HashMap<(usize, bool), FuseChunkSlot>,
    /// Single-slot side-table that hoists the HashMap lookup out of the
    /// per-record dispatch path. Stores the cache key + Arc of the LAST
    /// chunk we looked up. For an awk program with a single record-rule
    /// (the common case), every record hits this and skips the
    /// `fuse_chunk_cache` HashMap lookup entirely; for N-rule programs the
    /// hit rate is N-1/N (each rule swaps the slot once per record). Updated
    /// only on HashMap miss; cleared in the Runtime constructors.
    pub fuse_last_chunk_key: (usize, bool),
    pub fuse_last_chunk_value: FuseChunkSlot,
    /// Same as [`fuse_chunk_cache`] but for `run_fusevm_region` which lowers
    /// just the eligible *prefix* of a chunk (when the whole chunk isn't
    /// eligible). Keyed by (slice base pointer, slice length, bignum) — the
    /// prefix length is determined by the chunk content so the same chunk
    /// always yields the same slice. Same `Arc<(Chunk, written-slots)>` value.
    pub fuse_prefix_chunk_cache: HashMap<(usize, usize, bool), FuseChunkSlot>,
    /// Recycled `fusevm::VM` instances. `try_fusevm_dispatch` / `run_fusevm_region`
    /// acquire a VM from the pool (which calls `VM::reset(chunk)` preserving
    /// internal Vec capacities — stack, frames, slot_buf, etc.) instead of
    /// allocating a fresh `fusevm::VM::new(chunk)` on every record. For an awk
    /// one-liner over millions of records, this skips per-record allocation of
    /// the VM's stack/frame storage. The pool is per-Runtime; parallel record
    /// workers get their own (fresh empty pool on Runtime clone).
    pub fuse_vm_pool: fusevm::VMPool,
    /// GNU MO catalogs loaded by `bindtextdomain` (domain → catalog).
    pub gettext_catalogs: AwkMap<String, Arc<Catalog>>,
    /// Copy of [`crate::bytecode::CompiledProgram::slot_map`] for SYMTAB / `array_keys` without VM context.
    /// Name -> slot index, with the fast hasher for the same reason
    /// [`crate::bytecode::CompiledProgram::slot_map`] uses it.
    pub symtab_slot_map: AwkMap<String, u16>,
    /// Decimal integer literals, parsed once, indexed by string-pool index.
    ///
    /// A literal is stored as its digits so `-M` can render it at full
    /// precision, but the ordinary path then parsed those digits to `f64` on
    /// every execution — a counted loop paid a full float parse per iteration
    /// for a constant. Filled on first use; empty under `-M`, which needs the
    /// digits themselves.
    pub decimal_lits: Vec<f64>,
    /// DAP debugger state. `Some` only under `awkrs --dap`; drives breakpoints,
    /// stepping, and variable inspection. The VM checks it on each
    /// [`crate::bytecode::Op::DebugLine`] marker.
    pub debugger: Option<crate::debugger::Debugger>,
    /// Active call stack for the debugger: `(function name, call-site line)`,
    /// innermost last. Only maintained when [`Self::debugger`] is set.
    pub debug_call_stack: Vec<(String, usize)>,
    /// Current source line, updated by `Op::DebugLine`. 0 when not debugging.
    pub cur_line: u32,
    /// `-p` / `--profile`: invocation count per **record** rule (index matches `CompiledProgram::record_rules`).
    pub profile_record_hits: Vec<u64>,
    /// One-shot: warn once when `PROCINFO["sorted_in"]` is set to an unsupported custom comparator name.
    pub sorted_in_warned: Cell<bool>,
    /// Last OS errno from [`Self::set_errno_io`] (gawk **`PROCINFO["errno"]`** numeric mirror).
    pub errno_code: i32,
    /// Registered AOP function-call intercepts (before/after/around advice).
    /// awkrs/zshrs-original extension; see [`crate::intercepts`]. Cloned into
    /// parallel record workers (cheap `Arc` refcount bumps) so advice fires
    /// regardless of which worker runs the intercepted call.
    pub intercepts: Vec<crate::intercepts::Intercept>,
    /// Live AOP call stack — pushed while an intercept set is firing so
    /// `intercept_proceed()` (from *around* advice) can reach the original
    /// function name + argument values. Fresh (empty) in every clone.
    pub intercept_call_stack: Vec<crate::intercepts::InterceptCall>,
    /// Unix: raw fd for [`libc::poll`] before reads on the primary input stream (stdin or first file).
    #[cfg(unix)]
    pub primary_input_poll_fd: Option<std::os::unix::io::RawFd>,
}

/// Translate an awk-style regex pattern to Rust regex syntax for the cases where
/// gawk's POSIX-ERE-with-extensions semantics diverge from Rust's regex engine:
///
///   - `\1`..`\9` (POSIX ERE has no backrefs; gawk treats as literal `\N`,
///     Rust regex parses as backreference and errors). Escape to `\\N`.
///   - `\d`, `\w`, `\s`, `\D`, `\W`, `\S` (Rust regex char classes; gawk emits
///     a warning and treats them as the literal trailing letter — e.g. `\d` ≡ `d`).
///     Strip the leading backslash so Rust sees a literal letter.
///
/// Escapes inside `[...]` bracket expressions are NOT translated — gawk's
/// bracket-interior semantics overlap enough with Rust's that the divergence
/// surface is small. The function tracks bracket-expression state by counting
/// unescaped `[` / `]` and only translates at top level.
/// One record from a redirected `getline` stream, honouring `RS`.
///
/// `getline < file`, `cmd | getline` and the coprocess differ only in which map
/// holds the reader, so the `RS` handling lives here. Returns the record with
/// its separator already removed — under the default `RS`, that means the
/// trailing newline and *only* the newline: a `\r` before it belongs to the
/// record, exactly as it does in the main loop. The old readers called
/// `BufRead::read_line` and the caller then trimmed `['\n', '\r']`, so a CRLF
/// file read through `getline` reported `length` one short of what the same file
/// read as ordinary input reported, and one short of all three references.
///
/// Returns the record and the separator text (for `RT`).
fn read_record_from_stream<R: BufRead>(
    reader: &mut R,
    rs: &str,
    regex_rs: Option<&BytesRegex>,
    leftover: &mut Vec<u8>,
) -> Result<Option<(String, Vec<u8>)>> {
    let mut buf = Vec::new();
    let mut sep = Vec::new();
    let got = crate::record_io::read_next_record_from(
        reader, rs, &mut buf, &mut sep, regex_rs, leftover,
    )?;
    if !got {
        return Ok(None);
    }
    // Only the newline path leaves its terminator in the record; every other
    // `RS` form hands the separator back through `sep` instead.
    let end = if rs == "\n" {
        crate::record_io::trim_end_record_bytes(&buf)
    } else {
        buf.len()
    };
    Ok(Some((
        String::from_utf8_lossy(&buf[..end]).into_owned(),
        sep,
    )))
}

/// [`translate_awk_re_to_rust`] for a pattern that is itself a byte string.
///
/// `regex::bytes::RegexBuilder` still takes its pattern as `&str`, so a byte
/// that no `&str` can name is written into the pattern source as `\xNN` — which,
/// with Unicode mode off, is exactly that one byte. Valid UTF-8 runs go through
/// the ordinary rewrite unchanged, so a pattern that is all text behaves exactly
/// as it did.
fn translate_awk_re_bytes_to_rust(pat: &[u8]) -> String {
    match std::str::from_utf8(pat) {
        Ok(text) => translate_awk_re_to_rust(text),
        Err(_) => {
            let mut src = String::with_capacity(pat.len() * 2);
            for chunk in pat.utf8_chunks() {
                src.push_str(&translate_awk_re_to_rust(chunk.valid()));
                for b in chunk.invalid() {
                    src.push_str(&format!("\\x{b:02x}"));
                }
            }
            src
        }
    }
}

fn translate_awk_re_to_rust(pat: &str) -> String {
    // Iterate characters, not bytes. Every rule below keys off an ASCII
    // character, but a byte loop that ends in `byte as char` latin-1-widens
    // each half of a multi-byte sequence: `é` (0xC3 0xA9) came out as `Ã©`,
    // which re-encodes to four bytes and can never match the two-byte `é` in
    // the subject. `"café" ~ /é/` was therefore false in every locale, while
    // `gsub(/é/, ...)` worked because a metacharacter-free pattern takes the
    // literal-substring fast path and never reaches this translator.
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::with_capacity(pat.len() + 4);
    let mut i = 0;
    let mut in_bracket = false;
    // Open capture groups, so an unmatched `)` can be recognised as a literal.
    let mut depth: i32 = 0;
    while i < chars.len() {
        let c = chars[i];
        // POSIX/gawk octal escape: a backslash followed by up to three octal
        // digits is the character with that code, and it is recognised inside a
        // bracket expression as well as outside one. gawk, mawk and
        // one-true-awk all agree — `/\141/` matches `a`, `/\12/` matches a
        // newline, `/\1411/` matches `a1`, and `/[\101]/` matches `A`.
        //
        // awkrs used to rewrite `\1`..`\9` into a *literal* backslash-digit,
        // which matches nothing any of those patterns are looking for, and left
        // the in-bracket spelling for Rust's regex parser, which rejected it
        // outright: `BEGIN { print ("A" ~ /[\101]/) }` died with "backreferences
        // are not supported" where every reference prints 1.
        if c == '\\' && i + 1 < chars.len() && chars[i + 1].is_digit(8) {
            let mut code = 0u32;
            let mut digits = 0;
            while digits < 3 {
                match chars.get(i + 1 + digits) {
                    Some(d) if d.is_digit(8) => {
                        code = code * 8 + d.to_digit(8).expect("checked octal digit");
                        digits += 1;
                    }
                    _ => break,
                }
            }
            // `\x{…}` is the one spelling Rust's regex parser accepts in both
            // positions, so the same rewrite serves inside and outside brackets.
            out.push_str(&format!("\\x{{{code:x}}}"));
            i += 1 + digits;
            continue;
        }
        if c == '\\' && in_bracket {
            // Inside a bracket expression the character escapes still mean their
            // character, but the class shorthands do not name a class — all
            // three references agree, and precisely: `[\t]` matches a tab and
            // `[\n]` a newline, while `[\w]` matches the letter `w` and neither
            // a backslash nor a digit. So a shorthand loses its backslash and
            // becomes the plain letter, exactly as an unknown escape does
            // outside a bracket. Rust reads `[\w]` as the word class, which
            // matched a digit.
            let Some(&next) = chars.get(i + 1) else {
                out.push_str("\\\\");
                i += 1;
                continue;
            };
            if matches!(next, 't' | 'n' | 'r' | 'f' | 'v' | 'a' | 'x')
                || !next.is_ascii_alphanumeric()
            {
                out.push('\\');
                out.push(next);
            } else {
                out.push(next);
            }
            i += 2;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() && !in_bracket {
            let next = chars[i + 1];
            if matches!(next, '8' | '9') {
                // Not an octal digit, so not an escape at all: gawk warns
                // "regexp escape sequence `\8' treated as plain `8'" and matches
                // the bare digit. mawk and one-true-awk do the same silently.
                out.push(next);
                i += 2;
                continue;
            }
            if matches!(next, 'd' | 'D') {
                // gawk doesn't support `\d`/`\D` (only `\w`/`\W`/`\s`/`\S`):
                // it emits "regexp escape sequence `\d' is not a known regexp
                // operator" and treats `\d` as the literal letter. Rust regex
                // would interpret as digit class — strip the `\` to literal.
                out.push(next);
                i += 2;
                continue;
            }
            if !rust_knows_escape(next) {
                // An escape Rust's parser does not know is a hard error there,
                // while gawk, mawk and one-true-awk all read it as the plain
                // character: `"QaE" ~ "\Qa\E"` matches in all three, and
                // awkrs died with "unrecognized escape sequence". Emit the
                // character, escaped when it is a metacharacter.
                if !next.is_ascii_alphanumeric() {
                    out.push('\\');
                }
                out.push(next);
                i += 2;
                continue;
            }
            // Other escapes (`\.`, `\(`, `\n`, `\t`, `\b`, `\B`, `\xHH`,
            // etc.) — pass through unchanged.
            out.push('\\');
            out.push(next);
            i += 2;
            continue;
        }
        if !in_bracket && c == ')' && depth == 0 {
            // An unmatched `)` is a literal in gawk, mawk and one-true-awk —
            // `"a))b" ~ "))"` matches in all three — and a hard error in Rust's
            // parser, which took awkrs's `~` down with it.
            out.push_str("\\)");
            i += 1;
            continue;
        }
        if !in_bracket && c == '(' {
            depth += 1;
        } else if !in_bracket && c == ')' {
            depth -= 1;
        }
        if !in_bracket && c == '{' && interval_end(&chars, i).is_none() {
            // A `{` that does not open a valid interval is a literal brace in
            // all three references (`"a{b" ~ "{"` matches); Rust rejects it.
            out.push_str("\\{");
            i += 1;
            continue;
        }
        if in_bracket
            && c == '-'
            && i > 0
            && i + 1 < chars.len()
            && chars[i + 1] != ']'
            && chars[i - 1] > chars[i + 1]
        {
            // A reversed range (`[z-a]`) is a hard error in Rust and in gawk,
            // but mawk and one-true-awk read the three characters literally —
            // `"z-a" ~ "[z-a]"` matches in both — so the majority is the literal
            // set. Escaping the `-` gives exactly that.
            out.push_str("\\-");
            i += 1;
            continue;
        }
        if c == '[' && !in_bracket {
            in_bracket = true;
        } else if c == ']' && in_bracket {
            in_bracket = false;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Whether Rust's regex parser gives `\<c>` a meaning.
///
/// Anything outside this set is a hard parse error there, while gawk, mawk and
/// one-true-awk all read the escape as the plain character. The list is the
/// escapes `regex-syntax` accepts: the C-style ones, the class shorthands, the
/// anchors, the numeric forms, and any punctuation (which escapes itself).
fn rust_knows_escape(c: char) -> bool {
    matches!(
        c,
        'a' | 'f' | 't' | 'n' | 'r' | 'v' | '0'
            | 'A' | 'z' | 'Z' | 'b' | 'B'
            | 's' | 'S' | 'w' | 'W'
            | 'x' | 'u' | 'U'
    ) || !c.is_ascii_alphanumeric()
}

// Verdict cache for `check_ere_separator`, keyed on the separator text.
//
// `FS` is re-read once per record and essentially never changes, so the check
// has to be free in the steady state; the walk itself only runs when the text
// differs from the last one seen.
thread_local! {
    static ERE_SEPARATOR_MEMO: std::cell::RefCell<Option<(String, Option<&'static str>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Fatal-check a separator that is about to be used as a regular expression —
/// `FS`, or the third argument of `split()`.
///
/// A one-character separator is a literal in every reference (POSIX: "if `FS`
/// is any other single character, that character is used as the separator"), so
/// `FS="["` splits on a literal `[` and is never a regex error. Only multi-
/// character separators are walked.
pub fn check_ere_separator(pat: &str) -> std::result::Result<(), String> {
    if pat.chars().nth(1).is_none() {
        return Ok(());
    }
    let verdict = ERE_SEPARATOR_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        match memo.as_ref() {
            Some((seen, verdict)) if seen == pat => *verdict,
            _ => {
                let verdict = ere_reject_reason(pat);
                *memo = Some((pat.to_string(), verdict));
                verdict
            }
        }
    });
    match verdict {
        Some(why) => Err(format!("invalid regexp: {why}: /{pat}/")),
        None => Ok(()),
    }
}

/// Reject the ERE spellings that gawk, mawk *and* one-true-awk all refuse to
/// compile, so `FS` and `split()` can be fatal on them the way the references
/// are instead of silently degrading to a literal split.
///
/// Deliberately narrower than "whatever Rust's regex crate accepts". The two
/// acceptance sets differ in both directions, and only the patterns all three
/// references reject may be made fatal here — anything the references disagree
/// about keeps the existing literal-split fallback. Observed on gawk 5.4.1,
/// mawk 1.3.4 and one-true-awk 20200816, splitting `"xayb"`:
///
/// | pattern    | gawk  | mawk  | b-awk | verdict           |
/// |------------|-------|-------|-------|-------------------|
/// | `[[`       | fatal | fatal | fatal | rejected here     |
/// | `[]`       | fatal | fatal | fatal | rejected here     |
/// | `[^]`      | fatal | fatal | fatal | rejected here     |
/// | `[a-`      | fatal | fatal | fatal | rejected here     |
/// | `[[:alpha:]` | fatal | fatal | fatal | rejected here   |
/// | `((`       | fatal | fatal | fatal | rejected here     |
/// | `a{2,1}`   | fatal | fatal | fatal | rejected here     |
/// | `{2}`      | fatal | fatal | fatal | rejected here     |
/// | `*x`       | fatal | fatal | fatal | rejected here     |
/// | `+x`       | fatal | fatal | fatal | rejected here     |
/// | `?x`       | fatal | fatal | fatal | rejected here     |
/// | `(?:a)`    | fatal | fatal | fatal | rejected here     |
/// | `))`       | ok    | ok    | ok    | accepted          |
/// | `{`, `a{`  | ok    | ok    | ok    | accepted          |
/// | `a{1,`     | fatal | ok    | fatal | accepted (2/3)    |
/// | `|x`       | ok    | fatal | fatal | accepted (2/3)    |
/// | `[z-a]`    | fatal | ok    | ok    | accepted (1/3)    |
/// | `[[:foo:]]`| fatal | fatal | ok    | accepted (2/3)    |
/// | `[a-b-c]`  | fatal | ok    | ok    | accepted (1/3)    |
/// | `()`,`(|)` | ok    | fatal | mixed | accepted          |
///
/// Returns the reason as `Err` so the caller can name the offending pattern.
fn ere_reject_reason(pat: &str) -> Option<&'static str> {
    let c: Vec<char> = pat.chars().collect();
    let n = c.len();
    let mut i = 0;
    let mut depth = 0i32;
    // True while the next token would be the first atom of a branch, i.e. at the
    // start of the pattern or straight after `(`. A quantifier there is what all
    // three references call "illegal primary" / "unbalanced" / "no preceding
    // regular expression". Not tracked after `|`: `|x` is fatal in only two of
    // the three, so it stays accepted.
    let mut need_atom = true;
    while i < n {
        match c[i] {
            '\\' => {
                // Any escape is one atom; a trailing backslash is accepted by
                // two of the three references, so it is not rejected here.
                i += if i + 1 < n { 2 } else { 1 };
                need_atom = false;
            }
            '[' => {
                match bracket_end(&c, i) {
                    Some(end) => i = end + 1,
                    None => return Some("unterminated bracket expression"),
                }
                need_atom = false;
            }
            '(' => {
                depth += 1;
                i += 1;
                need_atom = true;
            }
            ')' => {
                // An unmatched `)` is a literal in every reference, so it only
                // closes a group that is actually open.
                if depth > 0 {
                    depth -= 1;
                }
                i += 1;
                need_atom = false;
            }
            '*' | '+' | '?' => {
                if need_atom {
                    return Some("repetition operator with no preceding expression");
                }
                i += 1;
            }
            '{' => {
                match interval_end(&c, i) {
                    // A well-formed interval: it quantifies, so it needs an atom
                    // and its bounds have to be ordered.
                    Some((end, lo, hi)) => {
                        if need_atom {
                            return Some("repetition operator with no preceding expression");
                        }
                        if let (Some(lo), Some(hi)) = (lo, hi) {
                            if lo > hi {
                                return Some("invalid interval expression");
                            }
                        }
                        i = end + 1;
                    }
                    // Not an interval at all (`{`, `a{`, `a{1,`): a literal
                    // brace in mawk and one-true-awk, so it is an atom.
                    None => {
                        i += 1;
                        need_atom = false;
                    }
                }
            }
            _ => {
                i += 1;
                need_atom = false;
            }
        }
    }
    if depth > 0 {
        return Some("unbalanced (");
    }
    None
}

/// Index of the `]` that closes the bracket expression opening at `open`, or
/// `None` when the pattern runs out first.
///
/// POSIX bracket rules: a leading `]` (after an optional `^`) is a literal, and
/// `[:class:]`, `[.coll.]` and `[=equiv=]` swallow their own closing `]`.
fn bracket_end(c: &[char], open: usize) -> Option<usize> {
    let n = c.len();
    let mut i = open + 1;
    if i < n && c[i] == '^' {
        i += 1;
    }
    if i < n && c[i] == ']' {
        i += 1;
    }
    while i < n {
        if c[i] == '[' && i + 1 < n && matches!(c[i + 1], ':' | '.' | '=') {
            let kind = c[i + 1];
            let mut j = i + 2;
            while j + 1 < n && !(c[j] == kind && c[j + 1] == ']') {
                j += 1;
            }
            if j + 1 >= n {
                return None;
            }
            i = j + 2;
            continue;
        }
        if c[i] == ']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Parse `{n}`, `{n,}` or `{n,m}` starting at `open`, returning the index of the
/// closing brace and the bounds. `None` when the text is not a valid interval,
/// which every reference then treats as a literal `{`.
fn interval_end(c: &[char], open: usize) -> Option<(usize, Option<u64>, Option<u64>)> {
    let n = c.len();
    let mut i = open + 1;
    let lo_start = i;
    while i < n && c[i].is_ascii_digit() {
        i += 1;
    }
    if i == lo_start {
        return None;
    }
    let lo: u64 = c[lo_start..i].iter().collect::<String>().parse().ok()?;
    if i < n && c[i] == '}' {
        return Some((i, Some(lo), Some(lo)));
    }
    if i >= n || c[i] != ',' {
        return None;
    }
    i += 1;
    let hi_start = i;
    while i < n && c[i].is_ascii_digit() {
        i += 1;
    }
    if i >= n || c[i] != '}' {
        return None;
    }
    let hi = if i == hi_start {
        None
    } else {
        Some(c[hi_start..i].iter().collect::<String>().parse().ok()?)
    };
    Some((i, Some(lo), hi))
}

impl Runtime {
    /// `new` — see implementation for the contract.
    pub fn new() -> Self {
        // gawk parity: `printf "%'d"` consults the locale's `thousands_sep`
        // regardless of `-N`/`--use-lc-numeric`. Activate LC_NUMERIC from the
        // environment unconditionally so `localeconv()` returns the user's
        // grouping char (empty under `LC_ALL=C`, "," under `en_US.UTF-8`, etc.).
        // The decimal point is still kept as '.' unless `-N` is passed (gawk's
        // behavior).
        crate::locale_numeric::set_locale_numeric_from_env();
        let mut vars = AwkMap::default();
        // POSIX/gawk: FS defaults to " " (single space — special-cased to mean
        // "split on runs of whitespace"). awkrs's splitter behavior matches this
        // even when FS is "", but exposing FS as "" makes user code like
        // `if (FS == " ")` fail unnecessarily.
        vars.insert("FS".into(), Value::Str(" ".into()));
        vars.insert("OFS".into(), Value::Str(" ".into()));
        vars.insert("ORS".into(), Value::Str("\n".into()));
        vars.insert("OFMT".into(), Value::Str("%.6g".into()));
        // POSIX: number→string coercion (distinct from OFMT, which is for print).
        vars.insert("CONVFMT".into(), Value::Str("%.6g".into()));
        // POSIX record separator (default newline).
        vars.insert("RS".into(), Value::Str("\n".into()));
        // Text of the input record separator for the last record read (gawk).
        vars.insert("RT".into(), Value::Str(String::new().into()));
        vars.insert("ERRNO".into(), Value::Str(String::new().into()));
        vars.insert("ARGIND".into(), Value::Num(0.0));
        // Process environment (gawk associative array).
        let mut environ = AwkArray::new();
        for (k, v) in std::env::vars() {
            environ.insert(k, Value::Str(v.into()));
        }
        vars.insert("ENVIRON".into(), Value::Array(environ));
        // Stub gawk special arrays (full semantics not implemented).
        vars.insert("PROCINFO".into(), Value::Array(AwkArray::new()));
        vars.insert("SYMTAB".into(), Value::Array(AwkArray::new()));
        vars.insert("FUNCTAB".into(), Value::Array(AwkArray::new()));
        // POSIX octal \034 — multidimensional array subscript separator
        vars.insert("SUBSEP".into(), Value::Str("\x1c".into()));
        // Empty FPAT means use FS for field splitting (gawk).
        vars.insert("FPAT".into(), Value::Str(String::new().into()));
        vars.insert("FIELDWIDTHS".into(), Value::Str(String::new().into()));
        vars.insert("IGNORECASE".into(), Value::Num(0.0));
        vars.insert("BINMODE".into(), Value::Num(0.0));
        vars.insert("LINT".into(), Value::Num(0.0));
        vars.insert("TEXTDOMAIN".into(), Value::Str(String::new().into()));
        Self {
            vars,
            global_readonly: None,
            fields: Vec::new(),
            field_ranges: Vec::new(),
            fields_dirty: false,
            fields_pending_split: false,
            cached_fs: " ".into(),
            record: String::new().into(),
            record_assigned: false,
            record_strnum: true,
            field_strnum: Vec::new(),
            line_buf: Vec::with_capacity(256),
            read_leftover: Vec::new(),
            nr: 0.0,
            fnr: 0.0,
            filename: String::new(),
            exit_pending: false,
            exit_code: 0,
            input_reader: None,
            primary_input_done: false,
            inet_tcp_read: HashMap::new(),
            inet_tcp_write: HashMap::new(),
            inet_udp: HashMap::new(),
            gettext_dir: String::new(),
            bignum: false,
            read_timeout_env: Cell::new(None),
            fs_regex: None,
            file_handles: HashMap::new(),
            getline_leftover: HashMap::new(),
            dir_read: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            pipe_stdout: HashMap::new(),
            pipe_input_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed: 1,
            numeric_decimal: '.',
            // gawk parity: in the C locale, `localeconv` returns an empty
            // `thousands_sep` — `%'d` then prints WITHOUT grouping. Don't fall
            // back to `,`; preserve `None` so `printf "%'d", 1234567` matches
            // gawk under `LC_ALL=C`.
            numeric_thousands_sep: crate::locale_numeric::thousands_sep_from_locale(),
            slots: Vec::new(),
            slot_touched: Vec::new(),
            regex_cache_cs: AwkMap::default(),
            regex_cache_ci: AwkMap::default(),
            memmem_finder_cache: AwkMap::default(),
            print_buf: Vec::with_capacity(DEFAULT_PRINT_BUF_CAPACITY),
            ofs_bytes: b" ".to_vec(),
            ors_bytes: b"\n".to_vec(),
            vm_stack: Vec::with_capacity(64),
            csv_mode: false,
            rs_pattern_for_regex: String::new(),
            rs_regex_bytes: None,
            sandbox: false,
            characters_as_bytes: false,
            posix: false,
            traditional: false,
            jit_enabled: true,
            fuse_chunk_cache: HashMap::new(),
            fuse_last_chunk_key: (0, false),
            fuse_last_chunk_value: None,
            fuse_prefix_chunk_cache: HashMap::new(),
            fuse_vm_pool: fusevm::VMPool::new(),
            gettext_catalogs: AwkMap::default(),
            symtab_slot_map: AwkMap::default(),
            decimal_lits: Vec::new(),
            debugger: None,
            debug_call_stack: Vec::new(),
            cur_line: 0,
            profile_record_hits: Vec::new(),
            sorted_in_warned: Cell::new(false),
            errno_code: 0,
            intercepts: Vec::new(),
            intercept_call_stack: Vec::new(),
            #[cfg(unix)]
            primary_input_poll_fd: None,
        }
    }

    /// True when the **`LINT`** variable is set to a truthy value (after `BEGIN`, includes `-v LINT=1`).
    pub fn lint_runtime_active(&self) -> bool {
        self.get_global_var("LINT")
            .map(|v| v.truthy())
            .unwrap_or(false)
    }

    /// Emit a **`LINT`**-controlled warning to stderr (no-op when `LINT` is unset/false).
    pub fn lint_warn(&self, msg: &str) {
        if self.lint_runtime_active() {
            eprintln!("awkrs: warning: {msg}");
        }
    }

    /// gawk warns for **`log(x)`** / **`sqrt(x)`** when **`x < 0`**, even when **`LINT`** is off.
    pub fn warn_builtin_negative_arg(&self, name: &str, x: f64) {
        if x.is_nan() {
            return;
        }
        eprintln!("awkrs: warning: {name}: received negative argument {x}");
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

    /// gawk **`SUBSEP`** string used for **`PROCINFO[input, "READ_TIMEOUT"]`** composite keys.
    pub fn procinfo_subsep_string(&self) -> String {
        self.get_global_var("SUBSEP")
            .map(|v| v.as_str().to_string())
            .unwrap_or_else(|| "\x1c".into())
    }

    /// gawk: default **`PROCINFO["READ_TIMEOUT"]`** — explicit **`0`** disables; absent key uses **`GAWK_READ_TIMEOUT`**.
    pub fn global_read_timeout_ms(&self) -> i32 {
        match self.get_global_var("PROCINFO") {
            Some(Value::Array(m)) => match m.get("READ_TIMEOUT") {
                Some(v) => (v.as_number() as i32).max(0),
                None => self.read_timeout_env_ms(),
            },
            _ => self.read_timeout_env_ms(),
        }
    }

    /// `GAWK_READ_TIMEOUT`, read once per process rather than once per
    /// `getline`.
    ///
    /// The env lookup was 14% of a `while ((getline l < f) > 0)` loop in the
    /// profile — `std::env::var` walks the environment block on every call, and
    /// nothing in an awk program can change it. Memoised here rather than inside
    /// `gawk_read_timeout_env` so that function keeps re-reading, which is what
    /// its own tests check.
    fn read_timeout_env_ms(&self) -> i32 {
        match self.read_timeout_env.get() {
            Some(v) => v,
            None => {
                let v = crate::procinfo::gawk_read_timeout_env().max(0);
                self.read_timeout_env.set(Some(v));
                v
            }
        }
    }

    /// Per-input **`PROCINFO[input_name, "READ_TIMEOUT"]`** (gawk), else [`Self::global_read_timeout_ms`].
    pub fn procinfo_read_timeout_ms_for(&self, input_key: &str) -> i32 {
        let sep = self.procinfo_subsep_string();
        let composite = format!("{input_key}{sep}READ_TIMEOUT");
        if let Some(Value::Array(m)) = self.get_global_var("PROCINFO") {
            if let Some(v) = m.get(&composite) {
                return (v.as_number() as i32).max(0);
            }
        }
        self.global_read_timeout_ms()
    }

    /// gawk **`PROCINFO[input_name, "RETRY"]`**: when truthy, retryable I/O errors map to **`getline`** **`-2`**.
    pub fn procinfo_retry_enabled_for(&self, input_key: &str) -> bool {
        let sep = self.procinfo_subsep_string();
        let composite = format!("{input_key}{sep}RETRY");
        self.get_global_var("PROCINFO")
            .and_then(|v| match v {
                Value::Array(m) => m.get(&composite).map(|v| v.truthy()),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// gawk **`FILENAME`**-style key for primary-input timeouts (`"-"` when unset / stdin).
    pub fn primary_input_procinfo_key(&self) -> String {
        let f = self.filename.trim();
        if f.is_empty() {
            "-".into()
        } else {
            f.to_string()
        }
    }

    /// gawk **`getline`** return **`-2`** when **`PROCINFO[input, "RETRY"]`** is set and errno is retryable.
    pub fn getline_io_return_code(&self, e: &std::io::Error, input_key: &str) -> f64 {
        if !self.procinfo_retry_enabled_for(input_key) {
            return -1.0;
        }
        let retry = matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::Interrupted
        );
        if retry {
            -2.0
        } else {
            -1.0
        }
    }

    /// Map a **`getline`** I/O failure to **`-1`** / **`-2`** (sets **`ERRNO`**).
    pub fn getline_error_code_for_key(&mut self, err: &Error, input_key: &str) -> f64 {
        match err {
            Error::Io(e) => {
                self.set_errno_io(e);
                self.getline_io_return_code(e, input_key)
            }
            _ => {
                self.set_errno_str(err.to_string());
                -1.0
            }
        }
    }

    /// Refresh **`PROCINFO`**, **`FUNCTAB`**, and a **`SYMTAB`** mirror of globals (best-effort vs gawk introspection).
    pub fn refresh_special_arrays(&mut self, cp: &CompiledProgram, bin_name: &str) {
        self.procinfo_refresh(cp, bin_name);
        self.functab_refresh(cp);
        self.symtab_mirror_refresh(cp);
    }

    fn procinfo_refresh(&mut self, cp: &CompiledProgram, bin_name: &str) {
        let mut p = AwkArray::new();
        if let Some(Value::Array(old)) = self.vars.get("PROCINFO") {
            for (k, v) in old.iter() {
                p.insert_bytes(k.as_bytes(), v.clone());
            }
        }
        p.insert(
            "version".into(),
            Value::Str(env!("CARGO_PKG_VERSION").into()),
        );
        p.insert("api".into(), Value::Str("awkrs".into()));
        p.insert("api_major".into(), Value::Num(4.0));
        p.insert("api_minor".into(), Value::Num(1.0));
        p.insert("program".into(), Value::Str(bin_name.into()));
        // gawk: `posix` / `mingw` / `vms` — not Rust `std::env::consts::OS` (`macos`, `linux`, …).
        p.insert(
            "platform".into(),
            Value::Str(crate::procinfo::gawk_platform_string().into()),
        );
        if let Some(pma) = crate::procinfo::AWKRS_PMA_VERSION {
            p.insert("pma".into(), Value::Str(pma.into()));
        }
        p.insert("pid".into(), Value::Num(std::process::id() as f64));
        p.insert("errno".into(), Value::Num(self.errno_code as f64));
        #[cfg(unix)]
        {
            unsafe {
                p.insert("ppid".into(), Value::Num(libc::getppid() as f64));
                p.insert("uid".into(), Value::Num(libc::getuid() as f64));
                p.insert("euid".into(), Value::Num(libc::geteuid() as f64));
                p.insert("gid".into(), Value::Num(libc::getgid() as f64));
                p.insert("egid".into(), Value::Num(libc::getegid() as f64));
                p.insert("pgrpid".into(), Value::Num(libc::getpgrp() as f64));
            }
            for (k, v) in crate::procinfo::supplementary_group_entries() {
                p.insert(k, Value::Num(v));
            }
        }
        p.insert(
            "FS".into(),
            Value::Str(crate::procinfo::field_split_mode(self).into()),
        );
        // gawk parity: `PROCINFO["strftime"]` is the default format used when
        // `strftime()` is called with no arguments. Use gawk's actual default
        // (date(1)-equivalent) instead of `%c`.
        p.insert(
            "strftime".into(),
            Value::Str("%a %b %e %H:%M:%S %Z %Y".into()),
        );

        // `args_os`, not `args`: the latter panics on an argument that is not
        // valid UTF-8, and awk accepts one — a program written on the command
        // line may hold a byte inside a string or regex literal.
        let mut argv_proc = AwkArray::new();
        for (i, a) in std::env::args_os().enumerate() {
            argv_proc.insert(
                i.to_string(),
                Value::Str(AwkStr::from(&*crate::os_arg_bytes(&a))),
            );
        }
        p.insert("argv".into(), Value::Array(argv_proc));

        p.insert(
            "mb_cur_max".into(),
            Value::Num(crate::procinfo::mb_cur_max_value()),
        );

        if self.bignum && !p.contains_key("prec") {
            p.insert("prec".into(), Value::Num(MPFR_PREC as f64));
        }
        if !p.contains_key("roundmode") {
            p.insert("roundmode".into(), Value::Str("N".into()));
        }
        if !p.contains_key("READ_TIMEOUT") {
            let env_to = crate::procinfo::gawk_read_timeout_env();
            if env_to > 0 {
                p.insert("READ_TIMEOUT".into(), Value::Num(env_to as f64));
            }
        }
        if self.bignum {
            p.insert(
                "gmp_version".into(),
                Value::Str(crate::procinfo::gmp_version_string().into()),
            );
            p.insert(
                "mpfr_version".into(),
                Value::Str(crate::procinfo::mpfr_version_string().into()),
            );
            p.insert(
                "prec_min".into(),
                Value::Num(gmp_mpfr_sys::mpfr::PREC_MIN as f64),
            );
            p.insert(
                "prec_max".into(),
                Value::Num(gmp_mpfr_sys::mpfr::PREC_MAX as f64),
            );
        }
        let binmode = self
            .get_global_var("BINMODE")
            .map(|v| v.as_number())
            .unwrap_or(0.0);
        p.insert("awkrs_binmode".into(), Value::Num(binmode));

        // nproc: number of available CPUs
        p.or_insert(
            "nproc".into(),
            Value::Num(
                std::thread::available_parallelism()
                    .map(|n| n.get() as f64)
                    .unwrap_or(1.0),
            ),
        );

        // sorted_in: default sort order for for-in (gawk compat)
        p.or_insert("sorted_in".into(), Value::Str(String::new().into()));

        // PREC (default precision) when not in bignum mode
        if !self.bignum {
            p.or_insert("prec".into(), Value::Num(53.0));
        }

        crate::procinfo::merge_procinfo_identifiers(&mut p, cp);

        let sep = self
            .get_global_var("SUBSEP")
            .map(|v| v.as_str().to_string())
            .unwrap_or_else(|| "\x1c".into());
        let global_to = match p.get("READ_TIMEOUT") {
            Some(v) => v.as_number(),
            None => crate::procinfo::gawk_read_timeout_env() as f64,
        };
        let mut paths: Vec<String> = Vec::new();
        if let Some(Value::Array(argv)) = self.get_global_var("ARGV") {
            let argc = self
                .get_global_var("ARGC")
                .map(|v| v.as_number() as i64)
                .unwrap_or(0);
            for i in 1..argc {
                if let Some(v) = argv.get(&i.to_string()) {
                    paths.push(v.as_str().to_string());
                }
            }
        }
        paths.push("-".into());
        for path in paths {
            let k_rt = format!("{path}{sep}READ_TIMEOUT");
            p.or_insert(k_rt, Value::Num(global_to));
            let k_retry = format!("{path}{sep}RETRY");
            p.or_insert(k_retry, Value::Num(0.0));
        }

        self.vars.insert("PROCINFO".into(), Value::Array(p));
    }

    fn functab_refresh(&mut self, cp: &CompiledProgram) {
        let mut ft = AwkArray::new();
        // User-defined functions
        for (name, f) in &cp.functions {
            let mut meta = AwkArray::new();
            meta.insert("type".into(), Value::Str("user".into()));
            meta.insert("arity".into(), Value::Num(f.params.len() as f64));
            ft.insert(name.clone(), Value::Array(meta));
        }
        // Builtin functions (gawk includes these in FUNCTAB)
        for &name in crate::namespace::BUILTIN_NAMES {
            if !ft.contains_key(name) {
                let mut meta = AwkArray::new();
                meta.insert("type".into(), Value::Str("builtin".into()));
                ft.insert(name.into(), Value::Array(meta));
            }
        }
        self.vars.insert("FUNCTAB".into(), Value::Array(ft));
    }

    fn symtab_mirror_refresh(&mut self, cp: &CompiledProgram) {
        self.symtab_slot_map = cp.slot_map.clone();
        // SYMTAB subscripts resolve live via [`VmCtx`] / [`Runtime::symtab_elem_get`]; keep empty placeholder.
        self.vars
            .insert("SYMTAB".into(), Value::Array(AwkArray::new()));
    }

    ///
    /// `ARGV[0]` uses the basename of `argv[0]` (gawk convention — `"gawk"` rather than
    /// `"/opt/homebrew/bin/gawk"`); awk scripts that key off the interpreter name
    /// (`ARGV[0] == "awkrs"`) work regardless of how the binary was launched.
    ///
    /// In `--traditional` mode the full `argv[0]` is preserved verbatim, matching
    /// BSD `/usr/bin/awk` and bell-labs nawk semantics (no basename strip).
    pub fn init_argv(&mut self, files: &[std::path::PathBuf]) {
        use std::env;
        let raw = env::args_os()
            .next()
            .map(|a| String::from_utf8_lossy(&crate::os_arg_bytes(&a)).into_owned())
            .unwrap_or_else(|| "awkrs".to_string());
        let bin = if self.traditional {
            raw.clone()
        } else {
            std::path::Path::new(&raw)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| raw.clone())
        };
        let mut argv = vec![bin];
        for f in files {
            argv.push(f.to_string_lossy().into_owned());
        }
        let argc = argv.len();
        self.vars.insert("ARGC".into(), Value::Num(argc as f64));
        let mut map = AwkArray::new();
        for (i, s) in argv.iter().enumerate() {
            map.insert(i.to_string(), Value::Str(s.clone().into()));
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
            record: String::new().into(),
            record_assigned: false,
            record_strnum: true,
            field_strnum: Vec::new(),
            line_buf: Vec::new(),
            read_leftover: Vec::new(),
            nr: 0.0,
            fnr: 0.0,
            filename,
            exit_pending: false,
            exit_code: 0,
            input_reader: None,
            primary_input_done: false,
            inet_tcp_read: HashMap::new(),
            inet_tcp_write: HashMap::new(),
            inet_udp: HashMap::new(),
            gettext_dir: String::new(),
            bignum,
            read_timeout_env: Cell::new(None),
            fs_regex: None,
            file_handles: HashMap::new(),
            getline_leftover: HashMap::new(),
            dir_read: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            pipe_stdout: HashMap::new(),
            pipe_input_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed,
            numeric_decimal,
            numeric_thousands_sep,
            slots: Vec::new(),
            slot_touched: Vec::new(),
            regex_cache_cs: AwkMap::default(),
            regex_cache_ci: AwkMap::default(),
            memmem_finder_cache: AwkMap::default(),
            print_buf: Vec::new(),
            ofs_bytes: b" ".to_vec(),
            ors_bytes: b"\n".to_vec(),
            vm_stack: Vec::with_capacity(64),
            csv_mode,
            rs_pattern_for_regex: String::new(),
            rs_regex_bytes: None,
            sandbox,
            characters_as_bytes,
            posix,
            traditional,
            jit_enabled,
            fuse_chunk_cache: HashMap::new(),
            fuse_last_chunk_key: (0, false),
            fuse_last_chunk_value: None,
            fuse_prefix_chunk_cache: HashMap::new(),
            fuse_vm_pool: fusevm::VMPool::new(),
            gettext_catalogs,
            symtab_slot_map: AwkMap::default(),
            decimal_lits: Vec::new(),
            debugger: None,
            debug_call_stack: Vec::new(),
            cur_line: 0,
            profile_record_hits: Vec::new(),
            sorted_in_warned: Cell::new(false),
            errno_code: 0,
            intercepts: Vec::new(),
            intercept_call_stack: Vec::new(),
            #[cfg(unix)]
            primary_input_poll_fd: None,
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

    /// Fatal-check the `FS` the current record is about to be split with.
    ///
    /// gawk, mawk and one-true-awk all abort with a non-zero status once a
    /// record is read under an `FS` that is not a valid ERE; awkrs used to fall
    /// back to splitting on the separator text literally and carry on. The
    /// check is per record rather than per assignment because that is where the
    /// three agree: `awk 'BEGIN{FS="[["} END{print}' </dev/null` is fatal in
    /// gawk and mawk but exits 0 in one-true-awk, so an assignment no record
    /// ever reaches must stay silent.
    ///
    /// Memoised on the separator text, so the steady state is one string
    /// compare per record.
    pub fn check_fs_ere(&self) -> std::result::Result<(), String> {
        check_ere_separator(&self.cached_fs)
    }

    /// Ensure a regex is compiled and cached. Call before `regex_ref()`.
    /// [`Self::ensure_regex`] with a byte pattern.
    ///
    /// A pattern can itself hold a byte that is not part of valid UTF-8 —
    /// `$1 ~ $2` where the second field is one — and the cache is keyed on the
    /// bytes so two different patterns cannot collide on the same rendering.
    pub fn ensure_regex_bytes(&mut self, pat: &[u8]) -> std::result::Result<(), String> {
        let ic = self.ignore_case_flag();
        let cache = if ic {
            &mut self.regex_cache_ci
        } else {
            &mut self.regex_cache_cs
        };
        if cache.contains_key(pat) {
            return Ok(());
        }
        // The same ERE walk `FS` and `split()` use. Rust's parser accepts two
        // spellings ERE has no notion of — `(?:…)` and `(?i)…` — so `~` used to
        // honour them as a non-capturing group and an inline flag where gawk,
        // mawk and one-true-awk are all fatal ("illegal primary": a `?` with no
        // preceding expression, which is what the walk calls it too).
        if let Some(why) = ere_reject_reason(&String::from_utf8_lossy(pat)) {
            return Err(format!(
                "invalid regexp: {why}: /{}/",
                String::from_utf8_lossy(pat)
            ));
        }
        let translated = translate_awk_re_bytes_to_rust(pat);
        let mut b = regex::bytes::RegexBuilder::new(&translated);
        b.unicode(crate::locale_numeric::ctype_is_utf8());
        b.case_insensitive(ic);
        b.dot_matches_new_line(true);
        let re = b.build().map_err(|e| e.to_string())?;
        cache.insert(pat.to_vec(), re);
        Ok(())
    }

    /// [`Self::regex_ref`] with a byte pattern.
    pub fn regex_ref_bytes(&self, pat: &[u8]) -> &BytesRegex {
        if self.ignore_case_flag() {
            &self.regex_cache_ci[pat]
        } else {
            &self.regex_cache_cs[pat]
        }
    }

    pub fn ensure_regex(&mut self, pat: &str) -> std::result::Result<(), String> {
        self.ensure_regex_bytes(pat.as_bytes())
    }

    /// Get a cached regex (must call `ensure_regex` first).
    pub fn regex_ref(&self, pat: &str) -> &BytesRegex {
        self.regex_ref_bytes(pat.as_bytes())
    }

    /// gawk **`IGNORECASE`**: truthy value enables case-insensitive regex and string compares.
    #[inline]
    pub fn ignore_case_flag(&self) -> bool {
        self.get_global_var("IGNORECASE")
            .map(|v| v.truthy())
            .unwrap_or(false)
    }
    /// `clear_errno` — see implementation for the contract.
    pub fn clear_errno(&mut self) {
        self.errno_code = 0;
        self.vars.insert("ERRNO".into(), Value::Str(String::new().into()));
    }
    /// `set_errno_io` — see implementation for the contract.
    pub fn set_errno_io(&mut self, e: &std::io::Error) {
        self.errno_code = e.raw_os_error().unwrap_or(0);
        // gawk's ERRNO is the strerror string ("No such file or directory"),
        // not Rust's default Display which appends " (os error <code>)". Strip
        // that suffix so awkrs's ERRNO matches the gawk parity tests.
        let msg = e.to_string();
        let cleaned = match msg.rfind(" (os error ") {
            Some(pos) if msg.ends_with(')') => msg[..pos].to_string(),
            _ => msg,
        };
        self.vars.insert("ERRNO".into(), Value::Str(cleaned.into()));
    }
    /// `set_errno_str` — see implementation for the contract.
    pub fn set_errno_str(&mut self, msg: impl Into<String>) {
        self.errno_code = 0;
        self.vars.insert("ERRNO".into(), Value::Str(msg.into().into()));
    }
    /// `ensure_rs_regex_bytes` — see implementation for the contract.
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
    /// Publish `RT` — the separator text that ended the record just read.
    ///
    /// Runs once per record, so it writes through the existing entry rather than
    /// re-inserting. The straightforward `vars.insert("RT".into(), Value::Str(t))`
    /// allocated twice on every record — a fresh `String` key for a name already
    /// in the map, and a fresh value string — which is two million allocations
    /// across a million-record input, for a variable most programs never read.
    /// Reusing the stored `String`'s buffer makes the steady state allocation-free.
    pub fn set_rt_from_bytes(&mut self, sep: &[u8]) {
        if let Some(Value::Str(s)) = self.vars.get_mut("RT") {
            s.clear();
            match std::str::from_utf8(sep) {
                Ok(t) => s.push_str(t),
                Err(_) => s.push_str(&String::from_utf8_lossy(sep)),
            }
            return;
        }
        // First record, or a program that assigned a non-string to `RT`.
        let t = String::from_utf8_lossy(sep).into_owned();
        self.vars.insert("RT".into(), Value::Str(t.into()));
    }

    /// Cached [`memmem::Finder`] for a literal pattern string (non-empty).
    /// Used by literal `gsub`/`sub` to scan records with SIMD-friendly substring search.
    pub fn literal_substring_finder(&mut self, pat: &[u8]) -> &memmem::Finder<'static> {
        if !self.memmem_finder_cache.contains_key(pat) {
            let f = memmem::Finder::new(pat).into_owned();
            self.memmem_finder_cache.insert(pat.to_vec(), f);
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
    pub fn write_pipe_line(&mut self, cmd: &str, data: &[u8]) -> Result<()> {
        self.require_unsandboxed_io()?;
        if self.coproc_handles.contains_key(cmd) {
            return Err(Error::Runtime(format!(
                "one-way pipe `|` conflicts with two-way `|&` for `{cmd}`"
            )));
        }
        if !self.pipe_stdin.contains_key(cmd) {
            // The child inherits this process's stdout, so anything awk has
            // buffered and not yet written would appear *after* the child's
            // output even though the program printed it first. mawk and
            // one-true-awk both flush all output when they open a pipe, and
            // gawk flushes as well; awkrs flushed neither, so
            // `print "A"; print "B" | "cat"; close("cat")` emitted B before A
            // in all three references' disagreement-free case. Flushing at open
            // (rather than at close) is what mawk and one-true-awk do, and it
            // costs one flush per distinct command, not one per write.
            self.flush_stdout_before_child();
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
        w.write_all(data).map_err(Error::Io)?;
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
    pub fn write_coproc_line(&mut self, cmd: &str, data: &[u8]) -> Result<()> {
        self.ensure_coproc(cmd)?;
        let w = self.coproc_handles.get_mut(cmd).unwrap();
        w.stdin.write_all(data).map_err(Error::Io)?;
        Ok(())
    }

    /// `getline … <& "cmd"` — one line from the coprocess stdout.
    pub fn read_line_coproc(&mut self, cmd: &str) -> Result<Option<String>> {
        self.ensure_coproc(cmd)?;
        let to = self.procinfo_read_timeout_ms_for(cmd);
        #[cfg(unix)]
        if to > 0 {
            use std::os::unix::io::AsRawFd;
            let h = self.coproc_handles.get_mut(cmd).unwrap();
            let fd = h.stdout.get_ref().as_raw_fd();
            wait_fd_read_timeout(fd, to)?;
        }
        let (rs, re, mut leftover) = self.take_getline_rs_state(cmd)?;
        let h = self.coproc_handles.get_mut(cmd).unwrap();
        let read = read_record_from_stream(&mut h.stdout, &rs, re.as_ref(), &mut leftover);
        self.restore_getline_rs_state(cmd, re, leftover);
        self.finish_redirected_getline(read?)
    }

    /// `expr | getline` — one line from `sh -c expr` stdout.
    ///
    /// Spawns the subprocess on first call for `cmd` and caches the stdout handle so
    /// subsequent `getline`s advance through the **same** stream. `close(cmd)` tears
    /// the pipe down. Before this caching, every call respawned `cmd`, which made
    /// `while ((cmd | getline x) > 0)` loop forever on the first line of output.
    pub fn read_line_pipe(&mut self, cmd: &str) -> Result<Option<String>> {
        self.require_unsandboxed_io()?;
        if !self.pipe_stdout.contains_key(cmd) {
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
            self.pipe_stdout
                .insert(cmd.to_string(), BufReader::new(stdout));
            self.pipe_input_children.insert(cmd.to_string(), child);
        }
        let to = self.procinfo_read_timeout_ms_for(cmd);
        #[cfg(unix)]
        if to > 0 {
            use std::os::unix::io::AsRawFd;
            let fd = self
                .pipe_stdout
                .get(cmd)
                .expect("pipe_stdout entry exists; just inserted")
                .get_ref()
                .as_raw_fd();
            wait_fd_read_timeout(fd, to)?;
        }
        let (rs, re, mut leftover) = self.take_getline_rs_state(cmd)?;
        let reader = self
            .pipe_stdout
            .get_mut(cmd)
            .expect("pipe_stdout entry exists; just inserted");
        let read = read_record_from_stream(reader, &rs, re.as_ref(), &mut leftover);
        self.restore_getline_rs_state(cmd, re, leftover);
        self.finish_redirected_getline(read?)
    }

    /// Write one `print` line (including `ORS`) to `path`. First open uses truncate (`>`) or
    /// append (`>>`); later writes reuse the same handle until `close`.
    pub fn write_output_line(&mut self, path: &str, data: &[u8], append: bool) -> Result<()> {
        self.require_unsandboxed_io()?;
        if is_program_stdout(path) {
            // `>` and `>>` mean the same thing here: there is nothing to
            // truncate on a stream the process already holds open.
            let _ = append;
            self.print_buf.extend_from_slice(data);
            return Ok(());
        }
        if path.starts_with("/inet/udp/") {
            let _ = append;
            self.ensure_inet_udp(path)?;
            let s = self.inet_udp.get_mut(path).unwrap();
            s.send(data)
                .map_err(|e| Error::Runtime(format!("inet udp send `{path}`: {e}")))?;
            return Ok(());
        }
        if path.starts_with("/inet/tcp/") {
            let _ = append;
            self.ensure_inet_tcp_pair(path)?;
            let w = self.inet_tcp_write.get_mut(path).unwrap();
            w.write_all(data).map_err(Error::Io)?;
            return Ok(());
        }
        self.ensure_output_writer(path, append)?;
        let w = self.output_handles.get_mut(path).unwrap();
        w.write_all(data).map_err(Error::Io)?;
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
        if is_program_stdout(key) {
            crate::vm::flush_print_buf(&mut self.print_buf)?;
            std::io::stdout().flush().map_err(Error::Io)?;
            return Ok(());
        }
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
    /// `attach_input_reader` — see implementation for the contract.
    #[cfg_attr(unix, allow(dead_code))]
    pub fn attach_input_reader(&mut self, r: SharedInputReader) {
        self.attach_input_reader_with_poll_fd(r, None);
    }

    /// Attach primary input; **`poll_fd`** (Unix) is used with [`libc::poll`] for gawk-style **`READ_TIMEOUT`** on the record / primary-`getline` stream.
    pub fn attach_input_reader_with_poll_fd(
        &mut self,
        r: SharedInputReader,
        #[cfg(unix)] poll_fd: Option<std::os::unix::io::RawFd>,
        #[cfg(not(unix))] _poll_fd: Option<()>,
    ) {
        self.input_reader = Some(r);
        #[cfg(unix)]
        {
            self.primary_input_poll_fd = poll_fd;
        }
    }
    /// `detach_input_reader` — see implementation for the contract.
    pub fn detach_input_reader(&mut self) {
        self.input_reader = None;
        // The primary stream is now spent; a later plain `getline` is at EOF.
        self.primary_input_done = true;
        #[cfg(unix)]
        {
            self.primary_input_poll_fd = None;
        }
    }

    /// Unix: honor **`PROCINFO[FILENAME,"READ_TIMEOUT"]`** before each primary record read.
    #[cfg(unix)]
    pub fn poll_primary_read_timeout_if_needed(&self) -> Result<()> {
        let to = self.procinfo_read_timeout_ms_for(&self.primary_input_procinfo_key());
        if to > 0 {
            if let Some(fd) = self.primary_input_poll_fd {
                wait_fd_read_timeout(fd, to)?;
            }
        }
        Ok(())
    }

    /// Current [`RS`](https://www.gnu.org/software/gawk/manual/html_node/Built_002din-Variables.html) value.
    /// `RS == ""` — awk's paragraph mode. Records are separated by blank lines and
    /// <newline> is an additional field separator regardless of `FS`.
    pub fn paragraph_mode(&self) -> bool {
        match self.get_global_var("RS") {
            Some(v) => v.as_str_cow().is_empty(),
            None => false,
        }
    }

    pub fn rs_string(&self) -> String {
        match self.get_global_var("RS") {
            Some(Value::Str(s)) => s.clone().to_lossy_string(),
            Some(v) => v.as_str(),
            None => "\n".to_string(),
        }
    }

    /// Convert a [`Value`] to a string suitable for use as an array key.
    /// Numeric values pass through `CONVFMT` (POSIX-required for subscripts);
    /// all other values use their default string form. Integer-valued numbers
    /// bypass `CONVFMT` via the same heuristic as `as_str()` (e.g. `a[1]`
    /// stays as `"1"` regardless of CONVFMT).
    /// The array subscript `v` names, without allocating in either hot case.
    ///
    /// A string subscript IS the key, so it is borrowed straight from the value
    /// — `a[k]` inside `for (k in a)` used to clone the key once per element.
    /// An integral number is written into `buf` through the integer writer
    /// rather than the float formatter, and borrowed from there. Only the rare
    /// The subscript a value names, in bytes, borrowing when it already holds
    /// them.
    ///
    /// A rendered subscript cannot round-trip: `a[$1]` where the field holds a
    /// byte that is not part of valid UTF-8 would key on `U+FFFD`, and
    /// `for (k in a)` would hand that back instead of what the program stored.
    /// Numbers follow the same integral / `CONVFMT` rules as the rendered form
    /// — that text is ASCII, so the two agree for every numeric subscript.
    pub fn array_key_bytes_in<'a>(
        &self,
        v: &'a Value,
        buf: &'a mut KeyBuf,
    ) -> std::borrow::Cow<'a, [u8]> {
        use std::borrow::Cow;
        match v {
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => Cow::Borrowed(s.as_bytes()),
            Value::Uninit | Value::Array(_) => Cow::Borrowed(b""),
            // `a[1]` must key on "1", never "1.000000", so an integral value
            // never reaches CONVFMT. Every integral `f64` below 2^53 is exactly
            // an `i64`, so the cast cannot change a digit.
            Value::Num(n)
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 =>
            {
                Cow::Borrowed(buf.write_i64(*n as i64).as_bytes())
            }
            other => Cow::Owned(self.value_to_array_key(other).into_bytes()),
        }
    }

    pub fn value_to_array_key(&self, v: &Value) -> String {
        match v {
            Value::Num(n) => {
                // Integer-valued numbers must stay exact: a[1] keys on "1" not "1.000000".
                if n.is_finite() && n.fract() == 0.0 {
                    // Negative zero subscripts the same element as zero, as it
                    // does in gawk: `a[0] = 1; a[-0] = 2` leaves ONE element.
                    // `format!("{:.0}", -0.0)` writes "-0", which made it two.
                    if *n == 0.0 {
                        return "0".to_string();
                    }
                    if n.abs() < 9_007_199_254_740_992.0 {
                        // The integer writer, not the float one — the same
                        // digits, and this runs once per subscript stored.
                        return KeyBuf::new().write_i64(*n as i64).to_string();
                    }
                    format!("{:.0}", *n)
                } else {
                    self.num_to_string_convfmt(*n)
                }
            }
            Value::Mpfr(f) => {
                if f.is_finite() && f.is_integer() {
                    format!("{}", crate::bignum::float_trunc_integer(f))
                } else {
                    self.mpfr_to_string_convfmt(f)
                }
            }
            Value::Uninit => String::new(),
            Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => s.clone().to_lossy_string(),
            Value::Array(_) => String::new(),
        }
    }

    /// POSIX / gawk: format a number using **`CONVFMT`** (string coercion).
    /// Integer-valued numbers bypass CONVFMT and display in integer form —
    /// matches gawk where any integer-valued `n` prints exact via `%.0f`.
    /// Append `n`'s string form to `out` — [`Self::num_to_string_convfmt`]
    /// without the intermediate `String`.
    ///
    /// Concatenation appends a number in a loop, and the temporary was
    /// allocated, copied out of and dropped once per iteration. An integral
    /// value that fits an `i64` also formats through the integer writer rather
    /// than the float one, which is the same digits by a much shorter route:
    /// every integral `f64` below 2^53 is exactly representable as an `i64`, so
    /// the cast cannot lose one.
    pub fn push_num_convfmt(&self, out: &mut String, n: f64) {
        use std::fmt::Write as _;
        if n.is_finite() && n.fract() == 0.0 {
            // gawk parity: -0.0 prints as "0", not "-0".
            if n == 0.0 {
                out.push('0');
                return;
            }
            if n.abs() < 9_007_199_254_740_992.0 {
                let _ = write!(out, "{}", n as i64);
            } else {
                let _ = write!(out, "{n:.0}");
            }
            return;
        }
        out.push_str(&self.num_to_string_convfmt(n));
    }

    pub fn num_to_string_convfmt(&self, n: f64) -> String {
        if n.is_finite() && n.fract() == 0.0 {
            // gawk parity: -0.0 prints as "0", not "-0".
            if n == 0.0 {
                return "0".to_string();
            }
            return format!("{:.0}", n);
        }
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

    /// POSIX string coercion of an arbitrary scalar: the **one** conversion every
    /// user-visible "this value is used as a string" site must go through.
    ///
    /// Only `Num` / `Mpfr` are affected — they render through `CONVFMT` (integral
    /// values bypass it inside [`Self::num_to_string_convfmt`]). Everything else
    /// delegates to `as_str_cow`, which matters as much as the numeric case: a
    /// field is a `Value::Str` carrying the *original input text*, so `$1` of the
    /// record `1.23456` stays `"1.23456"` under `CONVFMT="%.2f"` in all three
    /// reference awks rather than being re-rendered to `"1.23"`.
    ///
    /// The conversion is deliberately performed **at the point of use**, not
    /// cached at assignment: `CONVFMT="%.2f"; x=1.23456; a=length(x);
    /// CONVFMT="%.4f"; b=length(x)` yields `4 6` in gawk, mawk and one-true-awk
    /// alike, so re-reading `CONVFMT` on every coercion is the observable
    /// behaviour, not an optimisation to hoist away.
    ///
    /// `Value::as_str` / `as_str_cow` render a number via `format_number` (full
    /// f64 precision) and are therefore **not** a substitute here; they remain
    /// correct only for values that are already strings, and for the internal
    /// bookkeeping that must not be reshaped by a user-settable format.
    #[inline]
    pub fn value_to_str_convfmt<'a>(&self, v: &'a Value) -> Cow<'a, str> {
        match v {
            Value::Num(n) => Cow::Owned(self.num_to_string_convfmt(*n)),
            Value::Mpfr(f) => Cow::Owned(self.mpfr_to_string_convfmt(f)),
            _ => v.as_str_cow(),
        }
    }

    /// [`Self::value_to_str_convfmt`] in bytes — the form for anything the awk
    /// program can observe.
    ///
    /// The `&str` version renders through `U+FFFD`, so a builtin that takes its
    /// argument that way can never hand back a byte it was given. `CONVFMT`
    /// output is ASCII, so the two agree for every numeric value.
    pub fn value_to_bytes_convfmt<'a>(&self, v: &'a Value) -> Cow<'a, [u8]> {
        match v {
            Value::Num(n) => Cow::Owned(self.num_to_string_convfmt(*n).into_bytes()),
            Value::Mpfr(f) => Cow::Owned(self.mpfr_to_string_convfmt(f).into_bytes()),
            _ => v.as_bytes_cow(),
        }
    }

    /// POSIX: `print` formats numbers with **`OFMT`** (distinct from [`Self::num_to_string_convfmt`]).
    /// Integer-valued numbers bypass OFMT and display in integer form so e.g.
    /// `print 999999999999` produces `"999999999999"` not `"1e+12"`. Large
    /// integers past `i64::MAX` still print via `%.0f`.
    pub fn num_to_string_ofmt(&self, n: f64) -> String {
        if n.is_finite() && n.fract() == 0.0 {
            // gawk parity: -0.0 prints as "0", not "-0".
            if n == 0.0 {
                return "0".to_string();
            }
            return format!("{:.0}", n);
        }
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
        // gawk parity: integer-valued bignums bypass CONVFMT so the full
        // precision is preserved (otherwise `%.6g` would truncate `2^100` to
        // "1.2677e+30" instead of the 31-digit exact value).
        if f.is_finite() && f.is_integer() {
            return format!("{}", crate::bignum::float_trunc_integer(f));
        }
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
        // Same integer fast path as the CONVFMT variant — see comment there.
        if f.is_finite() && f.is_integer() {
            return format!("{}", crate::bignum::float_trunc_integer(f));
        }
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
    pub fn set_field_from_mpfr(&mut self, i: i32, f: &Float) -> crate::error::Result<()> {
        let s = self.mpfr_to_string_convfmt(f);
        self.set_field(i, &s)
    }

    /// Next **record** from the primary input stream (respects `RS`), used by `getline` with no redirection.
    pub fn read_line_primary(&mut self) -> Result<Option<String>> {
        let Some(reader) = self.input_reader.clone() else {
            if self.primary_input_done {
                // Main input already ran to completion (we are in `END`, or after
                // an `exit`). POSIX makes this an ordinary end-of-file: `getline`
                // yields 0. gawk / mawk / one-true-awk all agree.
                return Ok(None);
            }
            return Err(Error::Runtime(
                "`getline` with no file is only valid during normal input".into(),
            ));
        };
        let to = self.procinfo_read_timeout_ms_for(&self.primary_input_procinfo_key());
        #[cfg(unix)]
        if to > 0 {
            if let Some(fd) = self.primary_input_poll_fd {
                wait_fd_read_timeout(fd, to)?;
            }
        }
        let rs = self.rs_string();
        self.ensure_rs_regex_bytes()?;
        let mut rt_sep = Vec::new();
        let mut line_buf = std::mem::take(&mut self.line_buf);
        let mut leftover = std::mem::take(&mut self.read_leftover);
        let read_ok = crate::record_io::read_next_record(
            &reader,
            &rs,
            &mut line_buf,
            &mut rt_sep,
            self.rs_regex_bytes.as_ref(),
            &mut leftover,
        )?;
        self.line_buf = line_buf;
        self.read_leftover = leftover;
        if !read_ok {
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

    /// `RS`, its compiled regex form, and this stream's carry-over buffer, all
    /// *moved out* of `self` — the reader is borrowed out of one of the handle
    /// maps for the read below, so nothing else may still be borrowed from
    /// `self` at that point. Every caller must hand all three back through
    /// [`Self::restore_getline_rs_state`], on the error path too.
    ///
    /// Moved rather than cloned. A cloned `regex` engine starts with an empty
    /// match-cache pool, so the lazy DFA it needs is rebuilt on the next match
    /// — a cache that hands out clones looks like it is working (pattern
    /// compilation vanishes from the profile) while the expensive half of the
    /// work still happens per call. Measured on a `while ((getline l < f) > 0)`
    /// loop over 200 000 records with `RS = "[0-9]"`: 1.98 s of CPU cloning,
    /// against gawk's 0.63 s.
    fn take_getline_rs_state(
        &mut self,
        key: &str,
    ) -> Result<(String, Option<BytesRegex>, Vec<u8>)> {
        let rs = self.rs_string();
        self.ensure_rs_regex_bytes()?;
        let re = self.rs_regex_bytes.take();
        // `get_mut` + `take` rather than `remove`: `remove` would be paired with
        // an `insert` that allocates the key string on every getline call, for a
        // key that is already in the map after the first one.
        let leftover = match self.getline_leftover.get_mut(key) {
            Some(buf) => std::mem::take(buf),
            None => Vec::new(),
        };
        Ok((rs, re, leftover))
    }

    /// The other half of [`Self::take_getline_rs_state`].
    fn restore_getline_rs_state(&mut self, key: &str, re: Option<BytesRegex>, leftover: Vec<u8>) {
        self.rs_regex_bytes = re;
        match self.getline_leftover.get_mut(key) {
            Some(buf) => *buf = leftover,
            None => {
                self.getline_leftover.insert(key.to_string(), leftover);
            }
        }
    }

    /// `getline var < filename` — one record from a kept-open file handle.
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
            // The caller no longer trims, so strip the terminator here.
            let keep = line.trim_end_matches('\n').len();
            line.truncate(keep);
            return Ok(Some(line));
        }
        if path.starts_with("/inet/") {
            return Err(Error::Runtime(format!(
                "unsupported inet path `{path}` (use /inet/tcp/... or /inet/udp/...)"
            )));
        }
        let p = Path::new(path);
        // `is_dir` is a `stat` syscall, and this runs once per `getline` — 66%
        // of a `while ((getline l < f) > 0)` loop in the profile, more than the
        // read itself. Only the *first* call for a path can discover that it is
        // a directory: after that the path is either already being iterated as
        // one, or has an open file handle. Both are answered from a map.
        let is_dir = self.dir_read.contains_key(path)
            || (!self.file_handles.contains_key(path) && p.is_dir());
        if is_dir {
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
            // Preserve the underlying `io::Error` so `getline_error_code_for_key`
            // can route through `set_errno_io`, giving ERRNO the clean OS error
            // message ("No such file or directory") that gawk produces, rather
            // than the noisier "open <path>: <full Rust display>" prefix.
            let f = File::open(getline_open_path(p)).map_err(Error::Io)?;
            self.file_handles
                .insert(path.to_string(), BufReader::new(f));
        }
        let to = self.procinfo_read_timeout_ms_for(path);
        #[cfg(unix)]
        if to > 0 {
            use std::os::unix::io::AsRawFd;
            let fd = self.file_handles[path].get_ref().as_raw_fd();
            wait_fd_read_timeout(fd, to)?;
        }
        let (rs, re, mut leftover) = self.take_getline_rs_state(path)?;
        let reader = self.file_handles.get_mut(path).unwrap();
        let read = read_record_from_stream(reader, &rs, re.as_ref(), &mut leftover);
        self.restore_getline_rs_state(path, re, leftover);
        self.finish_redirected_getline(read?)
    }

    /// Publish `RT` for a redirected `getline` and hand back the record.
    ///
    /// gawk sets `RT` from every `getline`, not just the main record loop:
    /// `BEGIN { RS="2"; getline l < "input.txt"; print RT }` prints `2`. mawk and
    /// one-true-awk have no `RT` at all, so there is nothing to disagree with.
    fn finish_redirected_getline(
        &mut self,
        read: Option<(String, Vec<u8>)>,
    ) -> Result<Option<String>> {
        match read {
            Some((record, sep)) => {
                self.set_rt_from_bytes(&sep);
                Ok(Some(record))
            }
            None => Ok(None),
        }
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
        let to = self.procinfo_read_timeout_ms_for(path);
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
        let to = self.procinfo_read_timeout_ms_for(path);
        if to > 0 {
            socket
                .set_read_timeout(Some(Duration::from_millis(to as u64)))
                .map_err(|e| Error::Runtime(format!("inet udp read timeout: {e}")))?;
        }
        self.inet_udp.insert(path.to_string(), socket);
        Ok(())
    }

    /// Flush every open output handle (files via `>` / `>>` and pipes via `|`),
    /// best-effort — used by `system()` to interleave subprocess output correctly.
    pub fn flush_all_output_handles(&mut self) {
        for w in self.output_handles.values_mut() {
            let _ = w.flush();
        }
        for w in self.pipe_stdin.values_mut() {
            let _ = w.flush();
        }
    }

    /// Push every byte awk still holds to the OS before handing a child process
    /// a descriptor it shares with us.
    ///
    /// A child inherits this process's stdout, so whatever is sitting in
    /// `print_buf` or in libc's stdout buffer would surface *after* the child's
    /// own writes and reorder output the program emitted first. `system()` and
    /// opening an output pipe both hand stdout to a child, and both need this.
    pub fn flush_stdout_before_child(&mut self) {
        let _ = crate::vm::flush_print_buf(&mut self.print_buf);
        let _ = std::io::stdout().flush();
        self.flush_all_output_handles();
    }
    /// `close_handle` — see implementation for the contract.
    pub fn close_handle(&mut self, path: &str) -> f64 {
        let mut exit_status: f64 = 0.0;
        let mut had_any = false;
        if is_program_stdout(path) {
            // The stream stays open — awk keeps printing after it — so this is
            // a flush that reports success, matching gawk's `close("/dev/stdout")`
            // answer of 0. Reporting -1 ("no such stream") would be wrong now
            // that writes to the name are accepted.
            let _ = crate::vm::flush_print_buf(&mut self.print_buf);
            let _ = std::io::stdout().flush();
            return 0.0;
        }
        // Any carry-over bytes belong to the stream being torn down; a reopen
        // under the same name must start clean.
        self.getline_leftover.remove(path);
        if let Some(h) = self.coproc_handles.remove(path) {
            had_any = true;
            let _ = shutdown_coproc(h);
        }
        if let Some(mut w) = self.output_handles.remove(path) {
            had_any = true;
            let _ = w.flush();
        }
        if let Some(mut w) = self.pipe_stdin.remove(path) {
            had_any = true;
            let _ = w.flush();
        }
        if let Some(mut ch) = self.pipe_children.remove(path) {
            had_any = true;
            exit_status = awk_process_status(ch.wait());
        }
        // `cmd | getline` reader/child — drop the reader (closes the read fd), then
        // reap the child so subsequent calls with the same key respawn cleanly.
        if self.pipe_stdout.remove(path).is_some() {
            had_any = true;
        }
        if let Some(mut ch) = self.pipe_input_children.remove(path) {
            had_any = true;
            exit_status = awk_process_status(ch.wait());
        }
        if self.file_handles.remove(path).is_some() {
            had_any = true;
        }
        if self.dir_read.remove(path).is_some() {
            had_any = true;
        }
        if self.inet_tcp_read.remove(path).is_some() {
            had_any = true;
        }
        if self.inet_tcp_write.remove(path).is_some() {
            had_any = true;
        }
        if self.inet_udp.remove(path).is_some() {
            had_any = true;
        }
        // gawk parity: `close()` on a name that does not correspond to an open handle
        // returns -1 (POSIX awk allows the return value to be implementation-defined,
        // but gawk's contract is "-1 for unknown name, 0 for a clean close of a file/pipe").
        if !had_any {
            return -1.0;
        }
        exit_status
    }
    /// `rand` — see implementation for the contract.
    pub fn rand(&mut self) -> f64 {
        self.rand_seed = self.rand_seed.wrapping_mul(1103515245).wrapping_add(12345);
        f64::from((self.rand_seed >> 16) as u32 & 0x7fff) / 32768.0
    }

    /// Seed PRNG; **`n`** is the full **`u64`** seed (POSIX/gawk-style **`srand(x)`** truncates **`x`** to an integer first).
    pub fn srand(&mut self, n: Option<u64>) -> f64 {
        let prev = self.rand_seed;
        self.rand_seed = n.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() ^ (d.subsec_nanos() as u64))
                .unwrap_or(1)
        });
        (prev & 0xffff_ffff) as f64
    }
    /// `set_field_sep_split` — see implementation for the contract.
    pub fn set_field_sep_split(&mut self, fs: &str, line: &[u8]) {
        self.reset_record(line);
        self.cached_fs.clear();
        self.cached_fs.push_str(fs);
    }

    /// [`set_field_sep_split`](Self::set_field_sep_split) minus the `FS` copy —
    /// for the record loop, where `FS` is usually the same as last record's.
    fn reset_record(&mut self, line: &[u8]) {
        self.record.clear();
        self.record.push_bytes(line);
        self.record_assigned = true;
        self.record_strnum = true;
        self.field_strnum.clear();
        self.fields_dirty = false;
        self.fields_pending_split = true;
        self.fields.clear();
        self.field_ranges.clear();
    }

    /// Start a new record, reading `FS` from the variable map.
    ///
    /// POSIX makes a rule's assignment to `FS` take effect on the *next* record,
    /// so the record loop has to re-read it every time and cannot hoist it. It
    /// used to do that through `Value::as_str()`, which clones — a `String`
    /// allocation per record for a value that changes in essentially no
    /// programs. Comparing against the copy `cached_fs` already holds keeps the
    /// steady state allocation-free while re-reading just as faithfully.
    pub fn set_record_with_current_fs(&mut self, line: &[u8]) {
        let unchanged = match self.vars.get("FS") {
            Some(Value::Str(s)) | Some(Value::StrLit(s)) | Some(Value::Regexp(s)) => {
                s == &self.cached_fs
            }
            // A numeric `FS`, or none at all, goes the long way — both are rare
            // and neither can be compared without building the string.
            _ => false,
        };
        if unchanged {
            self.reset_record(line);
            return;
        }
        let fs = self
            .vars
            .get("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.set_field_sep_split(&fs, line);
    }

    /// Like [`set_field_sep_split`](Self::set_field_sep_split) but takes an owned line (avoids extra
    /// copies when the caller already has a `String`, e.g. `gsub` replacing `$0`).
    pub fn set_field_sep_split_owned(&mut self, fs: &str, line: AwkStr) {
        self.record = line;
        self.record_assigned = true;
        self.record_strnum = true;
        self.field_strnum.clear();
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
            // Before `split_record_fields` borrows `self.record`: memoising the
            // compiled `FS` needs `&mut self`, and the split holds the record
            // and the field vector at once.
            self.sync_fs_regex();
            self.split_record_fields();
        }
    }

    /// Split `self.record` into `field_ranges` using current **`FPAT`** (if non-empty) or **`FS`**.
    /// Uses `cached_fs` when available (set by `set_field_sep_split`) to avoid per-record
    /// HashMap lookups and String allocations for the common case.
    /// Bring [`Self::fs_regex`] in step with the current `FS` and `IGNORECASE`.
    ///
    /// Runs before `self.record` is borrowed for the split, because it needs
    /// `&mut self`. The clone only happens on a miss — i.e. when `FS` or
    /// `IGNORECASE` actually changed, which is essentially never inside a record
    /// loop. A single-character, empty or default `FS` never reaches the regex
    /// engine, so nothing is compiled for it.
    fn sync_fs_regex(&mut self) {
        // Only a multi-character `FS` reaches the regex engine, so the common
        // separators return before even reading `IGNORECASE`.
        let fs_len = self.cached_fs.len();
        if fs_len <= 1 || self.cached_fs == " " {
            return;
        }
        let ignore_case = self.ignore_case_flag();
        if self
            .fs_regex
            .as_ref()
            .is_some_and(|(pat, ic, _)| *ic == ignore_case && pat.as_str() == self.cached_fs)
        {
            return;
        }
        let compiled = build_fs_regex(&self.cached_fs, ignore_case);
        self.fs_regex = Some((self.cached_fs.clone(), ignore_case, compiled));
    }

    fn split_record_fields(&mut self) {
        let record: &[u8] = self.record.as_bytes();
        if self.csv_mode {
            split_csv_gawk_fields(record, &mut self.field_ranges);
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                let raw = &record[s as usize..e as usize];
                // CSV doubled-quote escape: `""` → `"` inside a quoted field (gawk / RFC 4180).
                self.fields.push(if memchr::memmem::find(raw, b"\"\"").is_some() {
                    AwkStr::from_vec(byte_replace(raw, b"\"\"", b"\""))
                } else {
                    AwkStr::from(raw)
                });
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
        let fpat_trimmed: Option<String> = self.get_global_var("FPAT").and_then(|fv| {
            if !matches!(
                fv,
                Value::Str(ref s) | Value::StrLit(ref s) if !s.to_str_lossy().trim().is_empty()
            ) {
                return None;
            }
            let t = fv.as_str_cow();
            let tr = t.as_ref().trim();
            if tr.is_empty() {
                None
            } else {
                Some(tr.to_string())
            }
        });
        if let Some(ref fp_trimmed) = fpat_trimmed {
            if split_fields_fpat(record, fp_trimmed, &mut self.field_ranges) {
                return;
            }
        }
        // Use cached_fs (set by set_field_sep_split) to avoid HashMap lookup + String clone.
        let cab = self.characters_as_bytes;
        // Paragraph mode (`RS == ""`) makes <newline> an extra field separator for
        // a single-character FS. `FS == " "` already splits on newline and a regex
        // FS is left alone, so RS is only looked up in the one case that needs it —
        // the common paths keep their single global lookup.
        if !self.cached_fs.is_empty() {
            let para = self.cached_fs.len() == 1 && self.cached_fs != " " && self.paragraph_mode();
            let fs_re = memoised_fs_regex(&self.fs_regex, &self.cached_fs);
            split_fields_into(
                record,
                &self.cached_fs,
                &mut self.field_ranges,
                ic,
                cab,
                para,
                fs_re,
            );
        } else {
            match self.get_global_var("FS") {
                None => split_fields_into(
                    record,
                    " ",
                    &mut self.field_ranges,
                    ic,
                    cab,
                    false,
                    FsRegex::Unknown,
                ),
                Some(v) => {
                    let fs = v.as_str_cow().into_owned();
                    let para = fs.len() == 1 && fs != " " && self.paragraph_mode();
                    split_fields_into(
                        record,
                        &fs,
                        &mut self.field_ranges,
                        ic,
                        cab,
                        para,
                        FsRegex::Unknown,
                    );
                }
            }
        }
    }

    fn fieldwidths_vec(&self) -> Option<Vec<FieldwidthsSpec>> {
        let t = self.get_global_var("FIELDWIDTHS")?.as_str();
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        // gawk tokens:
        //   "N"      → take an N-byte field (skip 0)
        //   "M:N"    → skip M bytes, then take N bytes
        //   "*"      → take everything remaining (must be the last token)
        let mut specs = Vec::new();
        for tok in t.split_whitespace() {
            if tok == "*" {
                specs.push(FieldwidthsSpec {
                    skip: 0,
                    width: FIELDWIDTHS_REST,
                });
                continue;
            }
            if let Some((skip_s, width_s)) = tok.split_once(':') {
                let skip = skip_s.parse::<usize>().ok()?;
                if width_s == "*" {
                    specs.push(FieldwidthsSpec {
                        skip,
                        width: FIELDWIDTHS_REST,
                    });
                } else {
                    let width = width_s.parse::<usize>().ok()?;
                    if width == 0 {
                        continue;
                    }
                    specs.push(FieldwidthsSpec { skip, width });
                }
            } else if let Ok(width) = tok.parse::<usize>() {
                if width == 0 {
                    continue;
                }
                specs.push(FieldwidthsSpec { skip: 0, width });
            }
        }
        if specs.is_empty() {
            None
        } else {
            Some(specs)
        }
    }
    /// `field` — see implementation for the contract.
    pub fn field(&mut self, i: i32) -> crate::error::Result<Value> {
        if i < 0 {
            return Err(crate::error::Error::Runtime(
                "attempt to access field number -1".into(),
            ));
        }
        let idx = i as usize;
        if idx == 0 {
            let rec = self.record.clone();
            // A record assigned a plain string is not a numeric string, so it
            // must come back as `StrLit` — that is what makes `$0 < 7` compare
            // as text after `$0 = "42"`.
            return Ok(if self.record_strnum {
                Value::Str(rec.into())
            } else {
                Value::StrLit(rec.into())
            });
        }
        self.ensure_fields_split();
        if self.fields_dirty {
            // Same rule per field: only a field *assigned* a plain string loses
            // numeric-string status; everything the splitter produced keeps it.
            let strnum = self.field_strnum.get(idx - 1).copied().unwrap_or(true);
            let make = if strnum { Value::Str } else { Value::StrLit };
            Ok(self
                .fields
                .get(idx - 1)
                .map(|f| make(f.clone()))
                .unwrap_or_else(|| Value::Str(String::new().into())))
        } else {
            Ok(self
                .field_ranges
                .get(idx - 1)
                .map(|&(s, e)| Value::Str(AwkStr::from(&self.record[s as usize..e as usize])))
                .unwrap_or_else(|| Value::Str(String::new().into())))
        }
    }

    /// Get field value as f64 directly without allocating a String.
    #[inline]
    pub fn field_as_number(&mut self, i: i32) -> crate::error::Result<f64> {
        if i < 0 {
            return Err(crate::error::Error::Runtime(
                "attempt to access field number -1".into(),
            ));
        }
        let idx = i as usize;
        if idx == 0 {
            return Ok(parse_number(&&self.record.to_str_lossy()));
        }
        self.ensure_fields_split();
        if self.fields_dirty {
            Ok(self
                .fields
                .get(idx - 1)
                .map(|s| parse_number(&s.to_str_lossy()))
                .unwrap_or(0.0))
        } else {
            Ok(self
                .field_ranges
                .get(idx - 1)
                .map(|&(s, e)| parse_number(&String::from_utf8_lossy(&self.record[s as usize..e as usize])))
                .unwrap_or(0.0))
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
    pub fn field_str(&self, i: usize) -> &[u8] {
        if i == 0 {
            return self.record.as_bytes();
        }
        if self.fields_dirty {
            self.fields.get(i - 1).map(|s| s.as_bytes()).unwrap_or(b"")
        } else {
            self.field_ranges
                .get(i - 1)
                .map(|&(s, e)| &self.record[s as usize..e as usize])
                .unwrap_or(b"")
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

    /// Mark a slot as used (read as a value, or written). See
    /// [`Self::slot_touched`]. Grows the bit vector on demand so the many
    /// places that rebuild `slots` wholesale need no matching update — an
    /// absent bit means "never touched", which is the right default.
    #[inline]
    pub fn touch_slot(&mut self, slot: usize) {
        if self.slot_touched.len() <= slot {
            self.slot_touched.resize(slot + 1, false);
        }
        self.slot_touched[slot] = true;
    }

    /// Has this slot ever been read or written? See [`Self::touch_slot`].
    #[inline]
    pub fn slot_was_touched(&self, slot: usize) -> bool {
        self.slot_touched.get(slot).copied().unwrap_or(false)
    }

    /// True when `$i` is out of range for the current record (`i >= 1` and `i > NF`).
    #[inline]
    pub fn field_is_unassigned(&mut self, i: i32) -> bool {
        if i < 1 {
            return false;
        }
        (i as usize) > self.nf()
    }

    /// Assign `$0` — replace the record and re-split fields / `NF` per POSIX.
    ///
    /// The new record keeps numeric-string status, which is right for the
    /// callers that hand over an input-derived line. Assignments from awk source
    /// go through [`set_record_str_strnum`](Self::set_record_str_strnum) so a
    /// plain string like `$0 = "42"` stays a plain string.
    pub fn set_record_str(&mut self, val: &str) {
        self.set_record_str_strnum(val.as_bytes(), true);
    }

    /// [`set_record_str`](Self::set_record_str), stating whether the assigned
    /// text is a POSIX numeric string. Splitting always yields numeric-string
    /// fields, so only `$0` itself is affected.
    pub fn set_record_str_strnum(&mut self, val: &[u8], strnum: bool) {
        let fs = self
            .get_global_var("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.set_field_sep_split(&fs, val);
        self.ensure_fields_split();
        self.record_strnum = strnum;
        let nf = self.nf() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
    }

    /// Assign `NF` — truncate or extend fields and rebuild `$0` with `OFS`.
    pub fn set_nf(&mut self, n: i32) -> crate::error::Result<()> {
        if n < 0 {
            return Err(crate::error::Error::Runtime(
                "NF set to negative value".into(),
            ));
        }
        let nf = n as usize;
        self.ensure_fields_split();
        if !self.fields_dirty {
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                self.fields
                    .push(AwkStr::from(&self.record[s as usize..e as usize]));
            }
            self.fields_dirty = true;
        }
        if self.fields.len() > nf {
            self.fields.truncate(nf);
        } else {
            self.fields.resize(nf, String::new().into());
        }
        self.rebuild_record();
        self.vars.insert("NF".into(), Value::Num(nf as f64));
        Ok(())
    }
    /// `set_field` — see implementation for the contract.
    ///
    /// Treats `val` as a numeric string; [`set_field_strnum`](Self::set_field_strnum)
    /// is the form that says otherwise.
    pub fn set_field(&mut self, i: i32, val: &str) -> crate::error::Result<()> {
        self.set_field_strnum(i, val.as_bytes(), true)
    }

    /// [`set_field`](Self::set_field), stating whether the assigned text is a
    /// POSIX numeric string. `$1 = "42"` is not one, so `$1 < 7` compares as
    /// text in gawk, mawk and one-true-awk; `$1 = $2` and `$1 = 42` are.
    pub fn set_field_strnum(
        &mut self,
        i: i32,
        val: &[u8],
        strnum: bool,
    ) -> crate::error::Result<()> {
        if i == 0 {
            self.set_record_str_strnum(val, strnum);
            return Ok(());
        }
        if i < 1 {
            return Err(crate::error::Error::Runtime(
                "attempt to access field number -1".into(),
            ));
        }
        // Force the pending lazy split *before* materializing. Splitting is
        // deferred until a field is read, so a record whose first use is an
        // assignment still has an empty `field_ranges`: materializing from it
        // produced zero fields and `$1 = "Z"` silently destroyed every other
        // field and most of `$0`. Only the streaming input path split eagerly
        // enough to hide it, and the differential corpora feed every case on
        // stdin, so the file/mmap path never showed the loss.
        self.ensure_fields_split();
        // Materialize owned fields from ranges if needed
        if !self.fields_dirty {
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                self.fields
                    .push(AwkStr::from(&self.record[s as usize..e as usize]));
            }
            self.fields_dirty = true;
        }
        let idx = (i - 1) as usize;
        if self.fields.len() <= idx {
            self.fields.resize(idx + 1, String::new().into());
        }
        self.fields[idx] = AwkStr::from(val);
        // Absent entries read as `true`, so only a non-numeric-string assignment
        // has to be recorded — and the vec has to reach `idx` to record it.
        if !strnum || self.field_strnum.len() > idx {
            if self.field_strnum.len() <= idx {
                self.field_strnum.resize(idx + 1, true);
            }
            self.field_strnum[idx] = strnum;
        }
        self.rebuild_record();
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
        Ok(())
    }

    /// Set a field to a numeric value directly, formatting in-place without
    /// allocating a temporary `Value::Num` and round-tripping through `as_str()`.
    pub fn set_field_num(&mut self, i: i32, n: f64) -> crate::error::Result<()> {
        if i == 0 {
            let s = if n.is_finite() && n.fract() == 0.0 {
                format!("{:.0}", n)
            } else {
                format!("{n}")
            };
            self.set_record_str(&s);
            return Ok(());
        }
        if i < 1 {
            return Err(crate::error::Error::Runtime(
                "attempt to access field number -1".into(),
            ));
        }
        // Same pending-split hazard as [`set_field_strnum`](Self::set_field_strnum).
        self.ensure_fields_split();
        if !self.fields_dirty {
            self.fields.clear();
            for &(s, e) in &self.field_ranges {
                self.fields
                    .push(AwkStr::from(&self.record[s as usize..e as usize]));
            }
            self.fields_dirty = true;
        }
        let idx = (i - 1) as usize;
        if self.fields.len() <= idx {
            self.fields.resize(idx + 1, String::new().into());
        }
        // Format number into the existing String, reusing its allocation.
        self.fields[idx].clear();
        if n.is_finite() && n.fract() == 0.0 {
            use std::fmt::Write;
            let _ = write!(self.fields[idx], "{:.0}", n);
        } else {
            use std::fmt::Write;
            let _ = write!(self.fields[idx], "{n}");
        }
        self.rebuild_record();
        let nf = self.fields.len() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
        Ok(())
    }

    fn rebuild_record(&mut self) {
        let ofs = self
            .vars
            .get("OFS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.record = join_awkstrs(&self.fields, ofs.as_bytes());
        self.record_assigned = true;
        // A record joined back together from fields is a computed string. gawk
        // and one-true-awk both treat it that way (`$1 = $1` then `$0 < 7` is a
        // string compare on a record of `42`); mawk keeps it a numeric string
        // and is the outlier here.
        self.record_strnum = false;
    }
    /// `set_record_from_line` — see implementation for the contract.
    pub fn set_record_from_line(&mut self, line: &str) {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let fs = self
            .vars
            .get("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        self.set_field_sep_split(&fs, trimmed.as_bytes());
    }

    /// Parse the current `line_buf` as a record. Avoids the borrow-checker conflict
    /// of borrowing `line_buf` and calling `set_field_sep_split` simultaneously.
    pub fn set_record_from_line_buf(&mut self) {
        let rs = self.rs_string();
        let mut end = self.line_buf.len();
        if rs == "\n" {
            // gawk parity: only `\n` is the record terminator on Unix; a trailing
            // `\r` is part of the record (kept in `$0` / `length`). Older awkrs
            // also stripped `\r` here, breaking CRLF parity.
            while end > 0 && self.line_buf[end - 1] == b'\n' {
                end -= 1;
            }
        }
        // Copy the trimmed line into record (reuses allocation)
        self.record.clear();
        self.record_assigned = true;
        self.record_strnum = true;
        self.field_strnum.clear();
        // The record is the bytes that were read — no decode, no validation, no
        // substitution. This is the line that used to replace every byte that is
        // not part of valid UTF-8 with `U+FFFD`, which is where byte
        // transparency was lost for `$0`, every field cut from it, and every
        // `print` of either.
        self.record.push_bytes(&self.line_buf[..end]);
        // Sync cached_fs from vars (non-allocating check; only copies when changed).
        let fs_changed = match self.vars.get("FS") {
            Some(Value::Str(s)) | Some(Value::StrLit(s)) | Some(Value::Regexp(s)) => {
                s != &self.cached_fs
            }
            _ => false,
        };
        if fs_changed {
            if let Some(Value::Str(s)) | Some(Value::StrLit(s)) | Some(Value::Regexp(s)) =
                self.vars.get("FS")
            {
                self.cached_fs.clear();
                self.cached_fs.push_str(&s.to_str_lossy());
            }
        }
        // Split using current FPAT or FS
        self.fields_dirty = false;
        self.fields.clear();
        self.field_ranges.clear();
        // The streaming path splits eagerly rather than through
        // `ensure_fields_split`, so it memoises the compiled `FS` itself.
        self.sync_fs_regex();
        self.split_record_fields();
        let nf = self.nf() as f64;
        self.vars.insert("NF".into(), Value::Num(nf));
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
            "FILENAME" => Value::Str(self.filename.clone().into()),
            _ => Value::Uninit,
        }
    }

    /// Enumerate SYMTAB keys (globals, slot-backed names, special scalars).
    pub fn symtab_keys_reflect(&self) -> Vec<AwkStr> {
        use rustc_hash::FxHashSet;
        let mut seen = FxHashSet::default();
        for k in self.vars.keys() {
            if matches!(k.as_str(), "SYMTAB" | "FUNCTAB" | "PROCINFO") {
                continue;
            }
            seen.insert(AwkStr::from(k.as_str()));
        }
        if let Some(g) = &self.global_readonly {
            for k in g.keys() {
                if matches!(k.as_str(), "SYMTAB" | "FUNCTAB" | "PROCINFO") {
                    continue;
                }
                seen.insert(AwkStr::from(k.as_str()));
            }
        }
        for k in self.symtab_slot_map.keys() {
            seen.insert(AwkStr::from(k.as_str()));
        }
        for &s in crate::namespace::SPECIAL_GLOBAL_NAMES {
            seen.insert(AwkStr::from(s));
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
    /// `s = s expr` without reading `s` out first: append to the string the
    /// scalar already holds.
    ///
    /// The read-modify-write spelling copies the whole accumulator three times
    /// per iteration — once out of the symtab, once inside the concat, once
    /// back in on assignment — so an append loop is quadratic in the length it
    /// builds. gawk grows the value in place for the same reason, which is why
    /// it finishes a 100 K-iteration build in the time this took for a few
    /// thousand.
    ///
    /// Observably identical to `symtab_elem_set(key, Str(<value of key> +
    /// suffix))`, kind included: a concatenation yields a dynamic `Str`, so a
    /// later relational still sees a numeric string. `OFS`/`ORS` are excluded
    /// from the in-place path because their byte caches are refreshed by
    /// `symtab_elem_set`, which the fallback below still goes through.
    pub fn symtab_elem_append(&mut self, key: &str, suffix: &str) {
        if !matches!(key, "OFS" | "ORS") {
            let slot = self.symtab_slot_map.get(key).map(|&s| s as usize);
            let held = match slot {
                Some(i) => self.slots.get_mut(i),
                None => self.vars.get_mut(key),
            };
            if let Some(v) = held {
                if let Value::Str(s) | Value::StrLit(s) = v {
                    let mut owned = std::mem::take(s);
                    owned.push_str(suffix);
                    *v = Value::Str(owned);
                    return;
                }
            }
        }
        // A number, an unset name, `OFS`/`ORS`: no string to grow, so take the
        // same read/convert/store path the fused op replaced.
        let cur = self.symtab_elem_get(key);
        let mut s = self.value_to_str_convfmt(&cur).into_owned();
        s.push_str(suffix);
        self.symtab_elem_set(key, Value::Str(s.into()));
    }

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
    /// `array_get` — see implementation for the contract.
    #[inline]
    pub fn array_get(&self, name: &str, key: &str) -> Value {
        if name == "SYMTAB" {
            return self.symtab_elem_get(key);
        }
        match self.get_global_var(name) {
            Some(Value::Array(a)) => match a.get(key) {
                Some(Value::Num(n)) => Value::Num(*n),
                Some(v) => v.clone(),
                None => Value::Str(String::new().into()),
            },
            _ => Value::Str(String::new().into()),
        }
    }
    /// [`Self::array_get`] with a byte subscript.
    pub fn array_get_bytes(&self, name: &str, key: &[u8]) -> Value {
        if name == "SYMTAB" {
            return self.symtab_elem_get(&String::from_utf8_lossy(key));
        }
        match self.get_global_var(name) {
            Some(Value::Array(a)) => match a.get_bytes(key) {
                Some(Value::Num(n)) => Value::Num(*n),
                Some(v) => v.clone(),
                None => Value::Str(AwkStr::new()),
            },
            _ => Value::Str(AwkStr::new()),
        }
    }

    /// `array_set` — see implementation for the contract.
    /// The integer an array subscript names when it is one, so the VM can reach
    /// [`AwkArray`]'s integer half without rendering the number to a string for
    /// the array to parse straight back.
    pub fn subscript_int(v: &Value) -> Option<i64> {
        match v {
            Value::Num(n)
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 =>
            {
                // `-0` subscripts the same element as `0`; the cast already
                // gives 0 for both, which is what the string path renders too.
                Some(*n as i64)
            }
            _ => None,
        }
    }

    /// `a[k]` as an rvalue, creating the element POSIX says the read brings into
    /// existence. Byte subscript — see [`Self::array_key_bytes_in`].
    pub fn array_get_vivify_bytes(&mut self, name: &str, key: &[u8]) -> Value {
        if name == "SYMTAB" {
            return self.symtab_elem_get(&String::from_utf8_lossy(key));
        }
        if let Some(Value::Array(a)) = self.vars.get_mut(name) {
            if let Some(v) = a.get_bytes(key) {
                return match v {
                    Value::Num(n) => Value::Num(*n),
                    other => other.clone(),
                };
            }
            a.insert_bytes(key, Value::Uninit);
            return Value::Uninit;
        }
        // First touch of this name, or one still to be copied out of the
        // readonly globals: `array_set_bytes` handles both, once per array.
        self.array_set_bytes(name, key, Value::Uninit);
        Value::Uninit
    }

    /// `a[i]` with an integer subscript — the counted-loop shape, with no key
    /// rendered and none parsed.
    pub fn array_get_vivify_int(&mut self, name: &str, i: i64) -> Value {
        if name == "SYMTAB" {
            let mut b = KeyBuf::new();
            let k = b.write_i64(i).to_string();
            return self.symtab_elem_get(&k);
        }
        if let Some(Value::Array(a)) = self.vars.get_mut(name) {
            if let Some(v) = a.get_int(i) {
                return match v {
                    Value::Num(n) => Value::Num(*n),
                    other => other.clone(),
                };
            }
            a.insert_int(i, Value::Uninit);
            return Value::Uninit;
        }
        let mut b = KeyBuf::new();
        let k = b.write_i64(i).to_string();
        self.array_set(name, k, Value::Uninit);
        Value::Uninit
    }

    /// `a[i] = v` with an integer subscript.
    pub fn array_set_int(&mut self, name: &str, i: i64, val: Value) {
        if name == "SYMTAB" {
            let mut b = KeyBuf::new();
            let k = b.write_i64(i).to_string();
            self.symtab_elem_set(&k, val);
            return;
        }
        if let Some(Value::Array(a)) = self.vars.get_mut(name) {
            a.insert_int(i, val);
            return;
        }
        let mut b = KeyBuf::new();
        let k = b.write_i64(i).to_string();
        self.array_set(name, k, val);
    }

    /// `a[k]` read with POSIX auto-vivification, resolved in one hash lookup
    /// when the element is already there.
    ///
    /// Spelled out, the read was `array_has`, then `array_set(…, Uninit)` if
    /// missing, then `array_get` — three hashes of the key and an owned copy of
    /// it, on every element. Iterating a million-entry array paid all of that a
    /// million times over.
    ///
    /// A missing key is created as `Uninit` and read back as `Uninit`, which is
    /// what makes a later `k in a` true and `typeof(a[k])` `"untyped"` rather
    /// than the `"string"` a coerced `""` would report.

    /// [`array_set`](Self::array_set) with a borrowed subscript — see
    /// [`AwkArray::insert_str`] for why the borrow matters on this path.
    /// The caller is [`crate::vm::VmCtx::array_elem_set_str`], the store side
    /// of `a[$1] = v` / `a[$1] op= v`, which already holds the key text.
    /// [`Self::array_set_str`] with a byte subscript.
    pub fn array_set_bytes(&mut self, name: &str, key: &[u8], val: Value) {
        if name == "SYMTAB" {
            self.symtab_elem_set(&String::from_utf8_lossy(key), val);
            return;
        }
        if let Some(existing) = self.vars.get_mut(name) {
            match existing {
                Value::Array(a) => {
                    a.insert_bytes(key, val);
                }
                _ => {
                    let mut m = AwkArray::new();
                    m.insert_bytes(key, val);
                    *existing = Value::Array(m);
                }
            }
            return;
        }
        if let Some(Value::Array(a)) = self.global_readonly.as_ref().and_then(|g| g.get(name)) {
            let mut copy = a.clone();
            copy.insert_bytes(key, val);
            self.vars.insert(name.to_string(), Value::Array(copy));
        } else {
            let mut m = AwkArray::new();
            m.insert_bytes(key, val);
            self.vars.insert(name.to_string(), Value::Array(m));
        }
    }

    pub fn array_set_str(&mut self, name: &str, key: &str, val: Value) {
        if name == "SYMTAB" {
            self.symtab_elem_set(key, val);
            return;
        }
        if let Some(existing) = self.vars.get_mut(name) {
            match existing {
                Value::Array(a) => {
                    a.insert_str(key, val);
                }
                _ => {
                    let mut m = AwkArray::new();
                    m.insert_str(key, val);
                    *existing = Value::Array(m);
                }
            }
            return;
        }
        // First access: seed from the read-only `BEGIN` snapshot if it has one.
        if let Some(Value::Array(a)) = self.global_readonly.as_ref().and_then(|g| g.get(name)) {
            let mut copy = a.clone();
            copy.insert_str(key, val);
            self.vars.insert(name.to_string(), Value::Array(copy));
        } else {
            let mut m = AwkArray::new();
            m.insert_str(key, val);
            self.vars.insert(name.to_string(), Value::Array(m));
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
                    let mut m = AwkArray::new();
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
            let mut m = AwkArray::new();
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
    ///
    /// Two shapes leave the `f64` fast path because it cannot express them:
    /// `$0` as the subscript (the key is the whole record, not a field), and
    /// `-M`, where the sum has to be an [`Value::Mpfr`] built at the requested
    /// precision. Both used to be answered with the fast path anyway — `$0`
    /// keyed every record under `""`, collapsing the array to a single element,
    /// and `-M` silently did the arithmetic in `f64`.
    pub fn array_field_add_delta(&mut self, name: &str, field: i32, delta: f64) {
        self.ensure_fields_split();
        if field == 0 || self.bignum {
            let key = match field {
                0 => self.record.clone(),
                n => self.field(n).map(|v| v.as_str()).unwrap_or_default().into(),
            };
            if self.bignum {
                let prec = self.mpfr_prec_bits();
                let round = self.mpfr_round();
                let old = value_to_mpfr(&self.array_get(name, &key.to_str_lossy()), prec, round);
                // The delta is a *literal*, so it goes through the same
                // decimal recovery `Op::PushNum` uses — widening the `f64`
                // would make `a[$1] += 0.1` accumulate the double rather than
                // one tenth.
                let d = crate::bignum::literal_f64_to_mpfr(delta, self);
                let sum = Float::with_val_round(prec, old + d, round).0;
                self.array_set(name, key.to_lossy_string(), Value::Mpfr(sum));
            } else {
                Self::apply_array_numeric_delta(
                    &mut self.vars,
                    &self.global_readonly,
                    name,
                    &key.to_str_lossy(),
                    delta,
                );
            }
            return;
        }
        if field < 0 {
            Self::apply_array_numeric_delta(&mut self.vars, &self.global_readonly, name, "", delta);
            return;
        }
        let idx = (field - 1) as usize;
        if self.fields_dirty {
            let key = self.fields.get(idx).map(|s| s.to_str_lossy()).unwrap_or_default();
            Self::apply_array_numeric_delta(
                &mut self.vars,
                &self.global_readonly,
                name,
                &key,
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
        let key = String::from_utf8_lossy(&self.record[s..e]);
        Self::apply_array_numeric_delta(&mut self.vars, &self.global_readonly, name, &key, delta);
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
                    let mut m = AwkArray::new();
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
            let mut m = AwkArray::new();
            m.insert(key.to_string(), Value::Num(delta));
            vars.insert(name.to_string(), Value::Array(m));
        }
    }
    /// `array_delete` — see implementation for the contract.
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
            // `delete arr` empties the array; it does not untype the name.
            // gawk keeps reporting `typeof(arr) == "array"` afterwards and
            // still rejects `arr = 5` as "attempt to use array `arr' in a
            // scalar context", and mawk and one-true-awk agree. Removing the
            // entry outright — what awkrs used to do — silently turned the
            // name back into a fresh untyped variable.
            match self.vars.get_mut(name) {
                Some(Value::Array(a)) => a.clear(),
                // Anything else — absent, or present but unassigned — becomes
                // an empty array. `delete a` on a fresh name is how gawk types
                // it: `BEGIN { delete a; print typeof(a) }` answers `array`.
                _ => {
                    self.vars
                        .insert(name.to_string(), Value::Array(AwkArray::new()));
                }
            }
        }
    }

    /// Keys for `for (k in arr)` / `SYMTAB` in **sorted** order. When `PROCINFO["sorted_in"]` names a
    /// **user function**, sorting requires VM context — use [`crate::vm::VmCtx::for_in_keys`];
    /// this method returns **unsorted** hash iteration order in that case.
    pub fn array_keys(&self, name: &str) -> Vec<AwkStr> {
        if name == "SYMTAB" {
            let mut keys = self.symtab_keys_reflect();
            if self.posix {
                return keys;
            }
            let mode = sorted_in_mode(self);
            if matches!(mode, SortedInMode::CustomFn(_)) {
                return keys;
            }
            let mut tmp = AwkArray::new();
            for k in &keys {
                tmp.insert_bytes(k.as_bytes(), self.symtab_elem_get(&k.to_str_lossy()));
            }
            sort_for_in_keys(&mut keys, &tmp, mode);
            return keys;
        }
        let Some(Value::Array(a)) = self.get_global_var(name) else {
            return Vec::new();
        };
        let mut keys: Vec<AwkStr> = a.keys();
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
    /// [`Self::array_has`] with a byte subscript.
    pub fn array_has_bytes(&self, name: &str, key: &[u8]) -> bool {
        match self.get_global_var(name) {
            Some(Value::Array(a)) => a.contains_key_bytes(key),
            _ => false,
        }
    }

    pub fn array_has(&self, name: &str, key: &str) -> bool {
        if name == "SYMTAB" {
            return self.symtab_has_key(key);
        }
        match self.get_global_var(name) {
            Some(Value::Array(a)) => a.contains_key(key),
            _ => false,
        }
    }
    /// `split_into_array` — see implementation for the contract.
    pub fn split_into_array(&mut self, arr_name: &str, parts: &[AwkStr]) {
        self.array_delete(arr_name, None);
        // `split()` makes the target an array even when it produces no fields:
        // gawk reports `typeof(z)` as `"array"` after `split("", z)`. Without
        // this the name stays absent and reads back as `untyped`.
        self.vars
            .entry(arr_name.to_string())
            .or_insert_with(|| Value::Array(AwkArray::new()));
        for (i, p) in parts.iter().enumerate() {
            self.array_set(arr_name, format!("{}", i + 1), Value::Str(p.clone()));
        }
    }
}

/// Field-splitting for `split(s, a [, fs])` — same algorithm as [`crate::bytecode::Op::Split`].
///
/// Thin wrapper around `split_string_with_seps`; unused outside this module's
/// test suite, kept for direct splitter testing without going through `Op::Split`.
#[allow(dead_code)]
pub fn split_string_by_field_separator(s: &[u8], fs: &str, ignore_case: bool) -> Vec<AwkStr> {
    split_string_with_seps(s, fs, ignore_case).0
}

/// Field-splitting for `split(s, a, fs, seps)` — returns both the fields and the
/// separator strings between consecutive fields (gawk 4-arg extension).
/// `seps.len() == fields.len().saturating_sub(1)` on a non-empty record.
pub fn split_string_with_seps(s: &[u8], fs: &str, ignore_case: bool) -> (Vec<AwkStr>, Vec<AwkStr>) {
    split_string_impl(s, fs, ignore_case, false)
}

/// `split(s, a, /re/ [, seps])` — the separator came from a **regex literal**,
/// so the FS shorthands never apply: `/ /` splits on one literal space (not on
/// runs of whitespace with leading blanks stripped), and `/./` is the
/// any-character regex (not a literal dot). gawk, mawk and one-true-awk all
/// take the regex path for a literal; awkrs used to stringify it and re-enter
/// the FS rules, so `split("  a  b  ", A, / /)` answered 2 instead of 7.
pub fn split_string_with_seps_regex(
    s: &[u8],
    re: &str,
    ignore_case: bool,
) -> (Vec<AwkStr>, Vec<AwkStr>) {
    split_string_impl(s, re, ignore_case, true)
}

fn split_string_impl(
    s: &[u8],
    fs: &str,
    ignore_case: bool,
    fs_is_regex: bool,
) -> (Vec<AwkStr>, Vec<AwkStr>) {
    if s.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // An empty separator splits into characters whether it was written `""` or
    // `//`: gawk, mawk and one-true-awk all return 3 for
    // `split("abc", a, //)`. Only the `" "` and single-character shorthands are
    // string-FS rules that a regex literal must bypass.
    if fs.is_empty() {
        // Empty FS: each character becomes a field; separators between them are
        // empty. A byte that does not begin a valid UTF-8 character is one
        // character, the same rule the record splitter uses.
        let mut parts: Vec<AwkStr> = Vec::new();
        let mut i = 0usize;
        while i < s.len() {
            let n = utf8_char_len(&s[i..]);
            parts.push(AwkStr::from(&s[i..i + n]));
            i += n;
        }
        let seps = vec![AwkStr::new(); parts.len().saturating_sub(1)];
        return (parts, seps);
    }
    if !fs_is_regex && fs == " " {
        // Default whitespace: leading whitespace is stripped (no leading empty field).
        let mut parts: Vec<AwkStr> = Vec::new();
        let mut seps: Vec<AwkStr> = Vec::new();
        let bytes = s;
        let mut i = 0usize;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n') {
            i += 1;
        }
        while i < bytes.len() {
            let start = i;
            while i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'\n' {
                i += 1;
            }
            parts.push(AwkStr::from(&s[start..i]));
            let ws_start = i;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n') {
                i += 1;
            }
            if i < bytes.len() {
                seps.push(AwkStr::from(&s[ws_start..i]));
            }
        }
        return (parts, seps);
    }
    // Single-char FS — awk POSIX semantics treat it as a literal character,
    // never a regex metachar. gawk additionally documents that **IGNORECASE
    // does not apply** to a single-char string FS (only multi-char regex FS
    // honors it), so the literal split is always correct here regardless of
    // `ignore_case`.
    if !fs_is_regex && fs.chars().count() == 1 {
        let parts: Vec<AwkStr> = byte_split(s, fs.as_bytes())
            .into_iter()
            .map(AwkStr::from)
            .collect();
        let seps = vec![AwkStr::from(fs); parts.len().saturating_sub(1)];
        return (parts, seps);
    }
    // Regex FS (or multi-char / case-insensitive): use the regex engine and
    // capture each match for the seps array. The engine comes from the memo —
    // `split()` is routinely called once per record, and compiling the
    // separator per call is what made `{ n += split($0, a, "[ ,]+") }` cost
    // 3.18 s of CPU over 300 000 records where mawk needs 0.04 s.
    with_split_regex(fs, ignore_case, |re| {
        let Some(re) = re else {
            let parts: Vec<AwkStr> = byte_split(s, fs.as_bytes())
                .into_iter()
                .map(AwkStr::from)
                .collect();
            let seps = vec![AwkStr::from(fs); parts.len().saturating_sub(1)];
            return (parts, seps);
        };
        split_on_regex_bytes(s, re)
    })
}

/// The separator-capturing split, once the engine is in hand.
///
/// The pieces are cut from the subject's own bytes, so an element of the target
/// array holds exactly what the record held there.
fn split_on_regex_bytes(hay: &[u8], re: &BytesRegex) -> (Vec<AwkStr>, Vec<AwkStr>) {
    let mut parts: Vec<AwkStr> = Vec::new();
    let mut seps: Vec<AwkStr> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(hay) {
        // gawk parity: zero-width matches are ignored during split. Without
        // this, `split("abc", a, /x*/)` would emit one split between every
        // character because `/x*/` matches the empty string everywhere; gawk
        // returns 1 field ("abc"). (Regexes that match `""` AND a real
        // substring — e.g. `/a*/` on "aaab" — still emit splits at the
        // non-empty matches because those have `m.start() < m.end()`.)
        if m.start() == m.end() {
            continue;
        }
        parts.push(AwkStr::from(&hay[last..m.start()]));
        seps.push(AwkStr::from(m.as_bytes()));
        last = m.end();
    }
    parts.push(AwkStr::from(&hay[last..]));
    (parts, seps)
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
            record_assigned: self.record_assigned,
            record_strnum: self.record_strnum,
            field_strnum: self.field_strnum.clone(),
            line_buf: Vec::new(),
            read_leftover: Vec::new(),
            nr: self.nr,
            fnr: self.fnr,
            filename: self.filename.clone(),
            exit_pending: self.exit_pending,
            exit_code: self.exit_code,
            input_reader: None,
            primary_input_done: false,
            inet_tcp_read: HashMap::new(),
            inet_tcp_write: HashMap::new(),
            inet_udp: HashMap::new(),
            gettext_dir: self.gettext_dir.clone(),
            bignum: self.bignum,
            read_timeout_env: Cell::new(None),
            fs_regex: None,
            file_handles: HashMap::new(),
            getline_leftover: HashMap::new(),
            dir_read: HashMap::new(),
            output_handles: HashMap::new(),
            pipe_stdin: HashMap::new(),
            pipe_children: HashMap::new(),
            pipe_stdout: HashMap::new(),
            pipe_input_children: HashMap::new(),
            coproc_handles: HashMap::new(),
            rand_seed: self.rand_seed,
            numeric_decimal: self.numeric_decimal,
            numeric_thousands_sep: self.numeric_thousands_sep,
            slots: self.slots.clone(),
            slot_touched: self.slot_touched.clone(),
            regex_cache_cs: self.regex_cache_cs.clone(),
            regex_cache_ci: self.regex_cache_ci.clone(),
            memmem_finder_cache: self.memmem_finder_cache.clone(),
            print_buf: Vec::new(),
            ofs_bytes: self.ofs_bytes.clone(),
            ors_bytes: self.ors_bytes.clone(),
            vm_stack: Vec::with_capacity(64),
            csv_mode: self.csv_mode,
            rs_pattern_for_regex: self.rs_pattern_for_regex.clone(),
            rs_regex_bytes: self.rs_regex_bytes.clone(),
            sandbox: self.sandbox,
            characters_as_bytes: self.characters_as_bytes,
            posix: self.posix,
            traditional: self.traditional,
            jit_enabled: self.jit_enabled,
            // Parallel record worker — don't share cache; rebuild lazily per
            // worker (each has its own thread-local fusevm state too).
            fuse_chunk_cache: HashMap::new(),
            fuse_last_chunk_key: (0, false),
            fuse_last_chunk_value: None,
            fuse_prefix_chunk_cache: HashMap::new(),
            fuse_vm_pool: fusevm::VMPool::new(),
            gettext_catalogs: self.gettext_catalogs.clone(),
            symtab_slot_map: self.symtab_slot_map.clone(),
            decimal_lits: Vec::new(),
            // The debugger lives only on the main thread's runtime; parallel
            // worker clones never debug.
            debugger: None,
            debug_call_stack: Vec::new(),
            cur_line: 0,
            profile_record_hits: Vec::new(),
            sorted_in_warned: Cell::new(self.sorted_in_warned.get()),
            errno_code: self.errno_code,
            // Advice set carries into parallel workers (cheap Arc clones); the
            // live AOP call stack is per-execution, so start each clone empty.
            intercepts: self.intercepts.clone(),
            intercept_call_stack: Vec::new(),
            #[cfg(unix)]
            primary_input_poll_fd: self.primary_input_poll_fd,
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
        self.pipe_stdout.clear();
        for (_, mut ch) in self.pipe_input_children.drain() {
            let _ = ch.wait();
        }
    }
}

#[cfg(test)]
mod regex_translator_tests {
    use super::translate_awk_re_to_rust;

    #[test]
    fn passes_through_plain_patterns_unchanged() {
        assert_eq!(translate_awk_re_to_rust("abc"), "abc");
        assert_eq!(translate_awk_re_to_rust("[a-z]+"), "[a-z]+");
        // (`\d\w\s` is no longer pass-through — see `strips_backslash_from_unsupported_class_escapes`)
    }

    /// `\N` is an **octal escape**, not a backreference and not a literal
    /// backslash-digit. These three tests used to pin the literal reading, which
    /// no reference awk produces — verified against all three under `LC_ALL=C`:
    ///
    /// ```text
    /// BEGIN { print ("aa" ~ /(.)\1/), ("a\\1" ~ /(.)\1/), ("a\001" ~ /(.)\1/),
    ///               ("a\002b" ~ /a\2b/), ("\011" ~ /\9/), ("9" ~ /\9/) }
    /// gawk 5.4.1        → 0 0 1 1 0 1
    /// one-true-awk      → 0 0 1 1 0 1
    /// mawk 1.3.4        → 0 0 1 1 0 1
    /// ```
    ///
    /// So `\1` matches the byte 0x01 (not `aa`, and not the two characters
    /// `\1`), and `\9` — `9` is not an octal digit — is the plain digit.
    #[test]
    fn numeric_escape_is_octal_not_a_backreference() {
        assert_eq!(translate_awk_re_to_rust(r"(.)\1"), r"(.)\x{1}");
        assert_eq!(translate_awk_re_to_rust(r"a\2b"), r"a\x{2}b");
        assert_eq!(translate_awk_re_to_rust(r"\9"), "9");
        // Up to three octal digits, greedily: `\141` is `a`, and a fourth digit
        // is an ordinary character after it.
        assert_eq!(translate_awk_re_to_rust(r"\141"), r"\x{61}");
        assert_eq!(translate_awk_re_to_rust(r"\1411"), r"\x{61}1");
    }

    #[test]
    fn handles_multiple_octal_escapes_in_one_pattern() {
        assert_eq!(translate_awk_re_to_rust(r"(.)(.)\1\2"), r"(.)(.)\x{1}\x{2}");
    }

    /// A bracket expression is not an exception: `("\001" ~ /[\1\2]/)` and
    /// `("\002" ~ /[\1\2]/)` are both 1 in gawk, mawk and one-true-awk, while
    /// `("1" ~ /[\1\2]/)` is 0 — so the digits are octal codes there too, not
    /// literal members of the set. Left untranslated, Rust's parser rejected the
    /// whole pattern as an unsupported backreference.
    #[test]
    fn octal_escape_is_recognised_inside_a_character_class() {
        assert_eq!(translate_awk_re_to_rust(r"[\1\2]"), r"[\x{1}\x{2}]");
        assert_eq!(translate_awk_re_to_rust(r"[\101]"), r"[\x{41}]");
    }

    #[test]
    fn strips_backslash_from_d_class_escape() {
        // gawk supports `\w`/`\W`/`\s`/`\S` as char classes but NOT `\d`/`\D`.
        // It emits a warning for `\d` and treats it as literal `d`. Rust regex
        // would interpret `\d` as digit class — strip the `\` to make literal.
        assert_eq!(translate_awk_re_to_rust(r"\d+"), "d+");
        assert_eq!(translate_awk_re_to_rust(r"\D+"), "D+");
        assert_eq!(translate_awk_re_to_rust(r"\d\D"), "dD");
    }

    #[test]
    fn preserves_supported_class_escapes() {
        // `\w`/`\W`/`\s`/`\S` are gawk extensions and match Rust regex semantics —
        // pass through. POSIX escapes (`\.`, `\(`, `\n`, etc.) too.
        assert_eq!(translate_awk_re_to_rust(r"\w+"), r"\w+");
        assert_eq!(translate_awk_re_to_rust(r"\W"), r"\W");
        assert_eq!(translate_awk_re_to_rust(r"\s"), r"\s");
        assert_eq!(translate_awk_re_to_rust(r"\S+"), r"\S+");
        assert_eq!(translate_awk_re_to_rust(r"\."), r"\.");
        assert_eq!(translate_awk_re_to_rust(r"\("), r"\(");
        assert_eq!(translate_awk_re_to_rust(r"\n\t\r"), r"\n\t\r");
        assert_eq!(translate_awk_re_to_rust(r"\b"), r"\b");
    }

    #[test]
    fn octal_escape_compiles_after_translation() {
        // End-to-end: a pattern Rust's parser rejects outright now compiles,
        // and matches what the three references match — the byte 0x01, not a
        // repeated character and not the two characters `\` and `1`.
        use regex::Regex;
        let translated = translate_awk_re_to_rust(r"(.)\1");
        let re = Regex::new(&translated).expect("should compile after translation");
        assert!(
            !re.is_match("aa"),
            "repeated-char should NOT match (no real backref)"
        );
        assert!(
            !re.is_match("a\\1"),
            "the two characters `\\` `1` are not what `\\1` means"
        );
        assert!(re.is_match("a\u{1}"), "octal 1 SHOULD match");
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
    fn value_as_number_regexp_uses_pattern_text_as_numeric_string() {
        assert_eq!(Value::Regexp("3.5".into()).as_number(), 3.5);
        assert_eq!(Value::Regexp("notnum".into()).as_number(), 0.0);
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
    fn value_truthy_cond_rejects_whole_array() {
        let mut m = super::AwkArray::new();
        m.insert("k".into(), Value::Num(1.0));
        let v = Value::Array(m);
        assert!(v.truthy_cond().is_err());
        assert!(v.truthy());
    }

    #[test]
    fn value_is_numeric_str_detects_decimal() {
        assert!(Value::Str("3.14".into()).is_numeric_str());
        assert!(!Value::Str("x".into()).is_numeric_str());
    }

    #[test]
    fn str_lit_not_numeric_string_for_relops() {
        assert!(!Value::StrLit("10".into()).is_numeric_str());
        assert!(Value::Uninit.is_numeric_str());
    }

    #[test]
    fn as_number_longest_numeric_prefix() {
        assert_eq!(Value::StrLit("42trailing".into()).as_number(), 42.0);
    }

    #[test]
    fn split_empty_source_zero_fields() {
        let v = super::split_string_by_field_separator(b"", ",", false);
        assert!(v.is_empty());
    }

    #[test]
    fn set_nf_truncates_and_rebuilds_record() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(" ", b"a b c d e");
        rt.ensure_fields_split();
        rt.set_nf(3).unwrap();
        assert_eq!(rt.record, "a b c");
        assert_eq!(rt.nf(), 3);
    }

    #[test]
    fn set_record_str_resplits_nf() {
        let mut rt = super::Runtime::new();
        rt.vars.insert("FS".into(), Value::Str(" ".into()));
        rt.set_field_sep_split(" ", b"a b c");
        rt.ensure_fields_split();
        rt.set_record_str("x y");
        assert_eq!(rt.nf(), 2);
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
        let m = super::AwkArray::new();
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
    fn value_as_number_hex_only_when_non_decimal_parse_mode() {
        super::set_numeric_parse_mode(false);
        assert_eq!(Value::Str("0x10".into()).as_number(), 0.0);
        super::set_numeric_parse_mode(true);
        assert_eq!(Value::Str("0x10".into()).as_number(), 16.0);
        super::set_numeric_parse_mode(false);
    }

    #[test]
    fn value_as_number_leading_zero_octal_only_in_non_decimal_mode() {
        super::set_numeric_parse_mode(false);
        assert_eq!(Value::Str("010".into()).as_number(), 10.0);
        super::set_numeric_parse_mode(true);
        assert_eq!(Value::Str("010".into()).as_number(), 8.0);
        super::set_numeric_parse_mode(false);
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
        rt.set_field_sep_split(",", r#"a,"b,c",d"#.as_bytes());
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
        assert_eq!(rt.field(1).unwrap().as_str(), "a");
        assert_eq!(rt.field(2).unwrap().as_str(), "b,c");
        assert_eq!(rt.field(3).unwrap().as_str(), "d");
    }

    #[test]
    fn csv_mode_escape_double_quote_in_field() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b"\"a\"\"b\"");
        rt.ensure_fields_split();
        assert_eq!(rt.field(1).unwrap().as_str(), "a\"b");
    }

    #[test]
    fn csv_mode_trailing_comma_empty_field() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b"a,");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 2);
        assert_eq!(rt.field(1).unwrap().as_str(), "a");
        assert_eq!(rt.field(2).unwrap().as_str(), "");
    }

    // ── More CSV (RFC 4180) edge cases ───────────────────────────────────────

    #[test]
    fn csv_mode_leading_empty_field() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b",a,b");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
        assert_eq!(rt.field(1).unwrap().as_str(), "");
        assert_eq!(rt.field(2).unwrap().as_str(), "a");
        assert_eq!(rt.field(3).unwrap().as_str(), "b");
    }

    #[test]
    fn csv_mode_all_empty_fields() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b",,,");
        rt.ensure_fields_split();
        // 3 commas = 4 fields, all empty
        assert_eq!(rt.nf(), 4);
        for i in 1..=4 {
            assert_eq!(rt.field(i).unwrap().as_str(), "");
        }
    }

    #[test]
    fn csv_mode_mixed_quoted_and_unquoted() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", r#"a,"b,c",d,"e"#.as_bytes());
        rt.ensure_fields_split();
        // "e is unterminated; awkrs should accept (RFC 4180 strict would reject,
        // gawk is lenient). Pin actual behavior: 4 fields including the
        // unterminated one.
        assert!(rt.nf() >= 3, "got NF={}", rt.nf());
        assert_eq!(rt.field(1).unwrap().as_str(), "a");
        assert_eq!(rt.field(2).unwrap().as_str(), "b,c");
        assert_eq!(rt.field(3).unwrap().as_str(), "d");
    }

    #[test]
    fn csv_mode_double_quote_escape_in_middle() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", r#""he said ""hi""","next""#.as_bytes());
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 2);
        assert_eq!(rt.field(1).unwrap().as_str(), r#"he said "hi""#);
    }

    #[test]
    fn csv_mode_single_comma_two_empty_fields() {
        // `,` is one separator → 2 empty fields. Previously broken (returned 1).
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b",");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 2);
        assert_eq!(rt.field(1).unwrap().as_str(), "");
        assert_eq!(rt.field(2).unwrap().as_str(), "");
    }

    #[test]
    fn csv_mode_leading_comma_with_trailing_value() {
        // `,a,b` → 3 fields. Previously broken — leading-comma path was a
        // separate branch that didn't account for the implicit empty start.
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b",a,b");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
        assert_eq!(rt.field(1).unwrap().as_str(), "");
        assert_eq!(rt.field(2).unwrap().as_str(), "a");
        assert_eq!(rt.field(3).unwrap().as_str(), "b");
    }

    #[test]
    fn csv_mode_empty_record_zero_fields() {
        let mut rt = super::Runtime::new();
        rt.csv_mode = true;
        rt.set_field_sep_split(",", b"");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 0);
    }

    #[test]
    fn csv_mode_disabled_treats_comma_normally() {
        // Without csv_mode the quotes are part of the field text.
        let mut rt = super::Runtime::new();
        rt.csv_mode = false;
        rt.set_field_sep_split(",", r#"a,"b,c",d"#.as_bytes());
        rt.ensure_fields_split();
        // Without CSV mode: comma-only splitting → 4 fields.
        assert_eq!(rt.nf(), 4);
        assert_eq!(rt.field(1).unwrap().as_str(), "a");
        assert_eq!(rt.field(2).unwrap().as_str(), "\"b");
        assert_eq!(rt.field(3).unwrap().as_str(), "c\"");
        assert_eq!(rt.field(4).unwrap().as_str(), "d");
    }

    #[test]
    fn ignore_case_false_when_unset_or_zero() {
        let rt = super::Runtime::new();
        assert!(!rt.ignore_case_flag());
        let mut rt0 = super::Runtime::new();
        rt0.vars.insert("IGNORECASE".into(), Value::Num(0.0));
        assert!(!rt0.ignore_case_flag());
    }

    #[test]
    fn ignore_case_true_for_numeric_one() {
        let mut rt = super::Runtime::new();
        rt.vars.insert("IGNORECASE".into(), Value::Num(1.0));
        assert!(rt.ignore_case_flag());
    }

    #[test]
    fn ignore_case_true_for_non_numeric_string() {
        let mut rt = super::Runtime::new();
        rt.vars
            .insert("IGNORECASE".into(), Value::Str("yes".into()));
        assert!(rt.ignore_case_flag());
    }

    #[test]
    fn num_to_string_convfmt_uses_convfmt_global() {
        let mut rt = super::Runtime::new();
        rt.vars.insert("CONVFMT".into(), Value::Str("%.0f".into()));
        assert_eq!(rt.num_to_string_convfmt(3.2), "3");
    }

    #[test]
    fn num_to_string_ofmt_uses_ofmt_global() {
        let mut rt = super::Runtime::new();
        rt.vars.insert("OFMT".into(), Value::Str("%.2f".into()));
        assert_eq!(rt.num_to_string_ofmt(1.2), "1.20");
    }
}

#[cfg(test)]
mod longest_prefix_and_sorted_in_tests {
    use super::{
        longest_f64_prefix, sort_for_in_keys, sorted_in_mode, Runtime, SortedInMode, Value,
    };

    #[test]
    fn longest_f64_prefix_empty_none() {
        assert_eq!(longest_f64_prefix(""), None);
    }

    #[test]
    fn longest_f64_prefix_multibyte_first_char_returns_none() {
        // Regression: vigenere.awk (examples/) round-trips bytes 0..255 through
        // `sprintf("%c", i)` and uses the result as an array key — strings like
        // "ÿ" reach `longest_f64_prefix`. The old loop sliced bytewise at
        // `&s[..end]`, panicking with "end byte index 1 is not a char boundary;
        // it is inside 'ÿ' (bytes 0..2 of string)". The fix bounds the loop to
        // the leading ASCII run (numeric prefixes are always ASCII).
        assert_eq!(longest_f64_prefix("\u{ff}"), None);
        assert_eq!(longest_f64_prefix("é"), None);
        assert_eq!(longest_f64_prefix("é42"), None); // leading non-ASCII → no prefix
    }

    #[test]
    fn longest_f64_prefix_ascii_then_multibyte() {
        // Leading ASCII digits, then a multibyte char — should pick the ASCII run.
        assert_eq!(longest_f64_prefix("42é"), Some("42"));
        assert_eq!(longest_f64_prefix("1.5ÿ"), Some("1.5"));
    }

    #[test]
    fn longest_f64_prefix_scientific_ok() {
        assert_eq!(longest_f64_prefix("1e2"), Some("1e2"));
    }

    #[test]
    fn longest_f64_prefix_non_monotonic_stops_at_last_valid() {
        assert_eq!(longest_f64_prefix("1ex"), Some("1"));
    }

    #[test]
    fn longest_f64_prefix_trailing_non_numeric() {
        assert_eq!(longest_f64_prefix("3.5abc"), Some("3.5"));
    }

    #[test]
    fn sorted_in_posix_forces_unsorted() {
        let mut rt = Runtime::new();
        rt.posix = true;
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("@ind_str_desc".into()));
        rt.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt), SortedInMode::Unsorted);
    }

    #[test]
    fn sorted_in_reads_at_tokens_with_trim() {
        let mut rt = Runtime::new();
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("  @val_num_asc  ".into()));
        rt.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt), SortedInMode::ValNumAsc);
    }

    #[test]
    fn sorted_in_user_function_name() {
        let mut rt = Runtime::new();
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("my_cmp".into()));
        rt.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt), SortedInMode::CustomFn("my_cmp".into()));
    }

    #[test]
    fn sorted_in_missing_or_empty_is_unsorted() {
        let rt = Runtime::new();
        assert_eq!(sorted_in_mode(&rt), SortedInMode::Unsorted);

        let mut rt2 = Runtime::new();
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("  ".into()));
        rt2.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt2), SortedInMode::Unsorted);

        let mut rt3 = Runtime::new();
        let pi = super::AwkArray::new();
        rt3.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt3), SortedInMode::Unsorted);
    }

    #[test]
    fn sort_for_in_ind_num_asc_numeric_not_lexicographic() {
        let mut keys = vec!["10".into(), "2".into(), "1".into()];
        let arr = super::AwkArray::new();
        sort_for_in_keys(&mut keys, &arr, SortedInMode::IndNumAsc);
        assert_eq!(keys, vec!["1", "2", "10"]);
    }

    #[test]
    fn sort_for_in_val_num_desc_by_values() {
        let mut keys = vec!["a".into(), "b".into()];
        let mut arr = super::AwkArray::new();
        arr.insert("a".into(), Value::Num(1.0));
        arr.insert("b".into(), Value::Num(10.0));
        sort_for_in_keys(&mut keys, &arr, SortedInMode::ValNumDesc);
        assert_eq!(keys, vec!["b", "a"]);
    }

    #[test]
    fn sort_for_in_val_str_asc_by_string_values() {
        let mut keys = vec!["a".into(), "b".into()];
        let mut arr = super::AwkArray::new();
        arr.insert("a".into(), Value::Str("z".into()));
        arr.insert("b".into(), Value::Str("a".into()));
        sort_for_in_keys(&mut keys, &arr, SortedInMode::ValStrAsc);
        assert_eq!(keys, vec!["b", "a"]);
    }

    #[test]
    fn sorted_in_mode_ind_str_asc_token() {
        let mut rt = Runtime::new();
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("@ind_str_asc".into()));
        rt.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt), SortedInMode::IndStrAsc);
    }

    #[test]
    fn sorted_in_mode_val_type_asc_token() {
        let mut rt = Runtime::new();
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("@val_type_asc".into()));
        rt.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt), SortedInMode::ValTypeAsc);
    }

    #[test]
    fn sorted_in_mode_val_type_desc_token() {
        let mut rt = Runtime::new();
        let mut pi = crate::runtime::AwkArray::new();
        pi.insert("sorted_in".into(), Value::Str("@val_type_desc".into()));
        rt.vars.insert("PROCINFO".into(), Value::Array(pi));
        assert_eq!(sorted_in_mode(&rt), SortedInMode::ValTypeDesc);
    }

    #[test]
    fn sort_for_in_val_type_asc_orders_type_rank_then_value_string() {
        let mut keys = vec!["str".into(), "num".into(), "absent".into()];
        let mut arr = super::AwkArray::new();
        arr.insert("num".into(), Value::Num(1.0));
        arr.insert("str".into(), Value::Str("z".into()));
        sort_for_in_keys(&mut keys, &arr, SortedInMode::ValTypeAsc);
        assert_eq!(keys, vec!["absent", "num", "str"]);
    }

    #[test]
    fn sort_for_in_val_type_desc_reverses_type_rank() {
        let mut keys = vec!["absent".into(), "num".into(), "str".into()];
        let mut arr = super::AwkArray::new();
        arr.insert("num".into(), Value::Num(1.0));
        arr.insert("str".into(), Value::Str("z".into()));
        sort_for_in_keys(&mut keys, &arr, SortedInMode::ValTypeDesc);
        assert_eq!(keys, vec!["str", "num", "absent"]);
    }

    #[test]
    fn sort_for_in_ind_str_desc() {
        let mut keys = vec!["a".into(), "c".into(), "b".into()];
        let arr = super::AwkArray::new();
        sort_for_in_keys(&mut keys, &arr, SortedInMode::IndStrDesc);
        assert_eq!(keys, vec!["c", "b", "a"]);
    }

    #[test]
    fn sort_for_in_unsorted_no_op() {
        let mut keys = vec!["z".into(), "a".into()];
        let arr = super::AwkArray::new();
        sort_for_in_keys(&mut keys, &arr, SortedInMode::Unsorted);
        assert_eq!(keys, vec!["z", "a"]);
    }

    #[test]
    fn sort_for_in_custom_fn_no_op_in_runtime_helper() {
        let mut keys = vec!["b".into(), "a".into()];
        let arr = super::AwkArray::new();
        sort_for_in_keys(&mut keys, &arr, SortedInMode::CustomFn("cmp".into()));
        assert_eq!(keys, vec!["b", "a"]);
    }
}

#[cfg(test)]
mod init_argv_tests {
    use super::{Runtime, Value};
    use std::path::PathBuf;

    #[test]
    fn init_argv_sets_argc_and_numeric_string_keys() {
        let mut rt = Runtime::new();
        rt.init_argv(&[
            PathBuf::from("/data/one.txt"),
            PathBuf::from("/data/two.txt"),
        ]);
        assert_eq!(rt.vars.get("ARGC").unwrap().as_number(), 3.0);
        let Value::Array(argv) = rt.vars.get("ARGV").expect("ARGV") else {
            panic!("ARGV not array");
        };
        assert!(!argv.get("0").unwrap().as_str().is_empty());
        assert_eq!(argv.get("1").unwrap().as_str(), "/data/one.txt");
        assert_eq!(argv.get("2").unwrap().as_str(), "/data/two.txt");
    }

    #[test]
    fn init_argv_empty_file_list_leaves_only_program_name() {
        let mut rt = Runtime::new();
        rt.init_argv(&[]);
        assert_eq!(rt.vars.get("ARGC").unwrap().as_number(), 1.0);
        let Value::Array(argv) = rt.vars.get("ARGV").expect("ARGV") else {
            panic!("ARGV not array");
        };
        assert_eq!(argv.len(), 1);
        assert!(argv.get("0").is_some());
    }
}

// ── awk_binop_values: pin POSIX arithmetic-coercion semantics ────────────────
//
// Every `+= -= *= /= %= ^=` flows through here. These tests pin the contracts:
//   - string ↔ number coercion follows longest-numeric-prefix rule
//   - uninitialized values arithmetic to 0
//   - division by zero returns Err
//   - array-as-scalar is a fatal runtime error
//   - compound-only ops (Pow / Mod) return Num in non-MPFR mode
// Any of these getting "fixed" silently breaks awk programs in user-invisible
// ways, so each is a named test that has to be acknowledged.

#[cfg(test)]
mod awk_binop_values_pinning {
    use super::{awk_binop_values, Runtime, Value};
    use crate::ast::BinOp;

    fn rt() -> Runtime {
        Runtime::new()
    }

    fn binop_num(op: BinOp, a: Value, b: Value) -> f64 {
        let rt = rt();
        match awk_binop_values(op, &a, &b, false, &rt).unwrap() {
            Value::Num(n) => n,
            v => panic!("expected Num, got {v:?}"),
        }
    }

    #[test]
    fn binop_add_numbers() {
        assert_eq!(binop_num(BinOp::Add, Value::Num(2.0), Value::Num(3.0)), 5.0);
    }

    #[test]
    fn binop_sub_string_to_number_coercion() {
        // "10" - "4" coerces both via longest-numeric-prefix.
        assert_eq!(
            binop_num(BinOp::Sub, Value::Str("10".into()), Value::Str("4".into())),
            6.0
        );
    }

    #[test]
    fn binop_mul_with_alpha_suffix_coerces_to_prefix() {
        // POSIX: "2.5abc" + 0 → 2.5. Multiplication should see 2.5 on the LHS.
        // (Use 2.5 instead of 3.14 to avoid clippy::approx_constant flagging the
        // intermediate 6.28 as approximating TAU.)
        let n = binop_num(BinOp::Mul, Value::Str("2.5abc".into()), Value::Num(4.0));
        assert!((n - 10.0).abs() < 1e-9, "expected 10.0, got {n}");
    }

    #[test]
    fn binop_div_by_zero_returns_error() {
        let rt = rt();
        let err = awk_binop_values(BinOp::Div, &Value::Num(1.0), &Value::Num(0.0), false, &rt)
            .unwrap_err();
        assert!(
            format!("{err}").contains("division by zero"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn binop_mod_returns_remainder() {
        assert_eq!(
            binop_num(BinOp::Mod, Value::Num(10.0), Value::Num(3.0)),
            1.0
        );
    }

    #[test]
    fn binop_pow_returns_exponent() {
        assert_eq!(
            binop_num(BinOp::Pow, Value::Num(2.0), Value::Num(10.0)),
            1024.0
        );
    }

    #[test]
    fn binop_uninit_treated_as_zero() {
        // POSIX: uninitialized variable in arithmetic context is 0.
        assert_eq!(binop_num(BinOp::Add, Value::Uninit, Value::Num(7.0)), 7.0);
        assert_eq!(binop_num(BinOp::Mul, Value::Uninit, Value::Num(99.0)), 0.0);
    }

    #[test]
    fn binop_array_as_scalar_is_fatal() {
        // gawk-style: using an array name in scalar context is a fatal error,
        // not a silent empty-string coercion.
        let rt = rt();
        let arr = Value::Array(super::AwkArray::new());
        let err = awk_binop_values(BinOp::Add, &arr, &Value::Num(1.0), false, &rt).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("array") || msg.contains("scalar"),
            "expected array-as-scalar error, got: {msg}"
        );
    }

    #[test]
    fn binop_non_arithmetic_op_returns_error() {
        // Comparison / logical ops aren't valid compound-assignment ops; the
        // dispatch must reject them with a clear error message instead of
        // silently doing the wrong thing.
        let rt = rt();
        let err = awk_binop_values(BinOp::Eq, &Value::Num(1.0), &Value::Num(1.0), false, &rt)
            .unwrap_err();
        assert!(
            format!("{err}").contains("invalid compound assignment"),
            "expected reject message, got: {err}"
        );
    }
}

// ── parse_inet_tcp / parse_inet_udp: pin gawk /inet path grammar ─────────────
#[cfg(test)]
mod parse_inet_pinning {
    use super::{parse_inet_tcp, parse_inet_udp};

    #[test]
    fn inet_tcp_valid_three_components() {
        assert_eq!(
            parse_inet_tcp("/inet/tcp/0/example.com/443"),
            Some((0, "example.com".to_string(), 443))
        );
    }

    #[test]
    fn inet_tcp_with_explicit_local_port() {
        assert_eq!(
            parse_inet_tcp("/inet/tcp/8080/localhost/9000"),
            Some((8080, "localhost".to_string(), 9000))
        );
    }

    #[test]
    fn inet_udp_valid() {
        assert_eq!(
            parse_inet_udp("/inet/udp/0/dns.example/53"),
            Some((0, "dns.example".to_string(), 53))
        );
    }

    #[test]
    fn inet_wrong_prefix_returns_none() {
        // tcp parser must reject udp paths and vice versa.
        assert!(parse_inet_tcp("/inet/udp/0/h/53").is_none());
        assert!(parse_inet_udp("/inet/tcp/0/h/443").is_none());
        assert!(parse_inet_tcp("/inet/raw/0/h/1").is_none());
    }

    #[test]
    fn inet_missing_rport_returns_none() {
        assert!(parse_inet_tcp("/inet/tcp/0/host").is_none());
    }

    #[test]
    fn inet_extra_trailing_component_returns_none() {
        // Trailing slash + extra component must reject (no silent acceptance).
        assert!(parse_inet_tcp("/inet/tcp/0/host/443/extra").is_none());
    }

    #[test]
    fn inet_non_numeric_port_returns_none() {
        assert!(parse_inet_tcp("/inet/tcp/abc/host/443").is_none());
        assert!(parse_inet_tcp("/inet/tcp/0/host/xyz").is_none());
    }

    #[test]
    fn inet_port_out_of_u16_range_returns_none() {
        // 70000 doesn't fit in u16 — must reject, not silently truncate.
        assert!(parse_inet_tcp("/inet/tcp/0/host/70000").is_none());
    }
}

// ── awk_locale_str_cmp: pin string ordering contract ─────────────────────────
#[cfg(test)]
mod awk_locale_str_cmp_pinning {
    use super::awk_locale_str_cmp;
    use std::cmp::Ordering;

    #[test]
    fn equal_strings_compare_equal() {
        assert_eq!(awk_locale_str_cmp("abc", "abc"), Ordering::Equal);
    }

    #[test]
    fn empty_string_orders_first() {
        assert_eq!(awk_locale_str_cmp("", "a"), Ordering::Less);
        assert_eq!(awk_locale_str_cmp("a", ""), Ordering::Greater);
        assert_eq!(awk_locale_str_cmp("", ""), Ordering::Equal);
    }

    #[test]
    fn null_byte_falls_back_to_byte_compare() {
        // CString::new fails on embedded NUL — the function must fall back to
        // a.cmp(b) instead of panicking or returning Equal incorrectly.
        let with_nul = "a\0b";
        let without = "ab";
        let o = awk_locale_str_cmp(with_nul, without);
        // Should not panic; the relative ordering is implementation-defined but
        // must be one of Less/Greater (not erroneously Equal).
        assert_ne!(
            o,
            Ordering::Equal,
            "NUL-containing strings shouldn't compare equal to clean strings"
        );
    }

    #[test]
    fn value_to_number_conversions() {
        use crate::runtime::Value;
        assert_eq!(Value::Str("123".into()).as_number(), 123.0);
        assert_eq!(Value::Str("12.3".into()).as_number(), 12.3);
        assert_eq!(Value::Str("abc".into()).as_number(), 0.0);
    }

    #[test]
    fn runtime_array_operations() {
        use crate::runtime::{Runtime, Value};
        let mut rt = Runtime::new();
        rt.array_set("a", "k1".to_string(), Value::Num(42.0));
        assert!(rt.array_has("a", "k1"));
        assert!(!rt.array_has("a", "k2"));
        rt.array_delete("a", Some("k1"));
        assert!(!rt.array_has("a", "k1"));
    }

    #[test]
    fn runtime_global_var_ops() {
        use crate::runtime::{Runtime, Value};
        let mut rt = Runtime::new();
        rt.vars.insert("x".into(), Value::Num(100.0));
        assert_eq!(rt.get_global_var("x").unwrap().as_number(), 100.0);
    }
}

#[cfg(test)]
mod extra_runtime_tests {
    use super::*;

    #[test]
    fn value_truthiness() {
        assert!(!Value::Uninit.truthy());
        assert!(Value::Num(1.0).truthy());
        assert!(!Value::Num(0.0).truthy());
        assert!(Value::StrLit("0".into()).truthy()); // "0" as literal is truthy
        assert!(!Value::Str("0".into()).truthy()); // "0" as dynamic string is numeric 0 -> falsy
        assert!(!Value::Str("00".into()).truthy()); // "00" is numeric 0 -> falsy
                                                    // regex literals are truthy if non-empty
        assert!(Value::Regexp(".".into()).truthy());
        assert!(!Value::Regexp("".into()).truthy());
    }

    #[test]
    fn value_numeric_conversions() {
        assert_eq!(Value::Uninit.as_number(), 0.0);
        assert_eq!(Value::Str("  123.45  ".into()).as_number(), 123.45);
        assert_eq!(Value::Str("1e2".into()).as_number(), 100.0);
        // gawk parity: bare "inf"/"nan" coerce to 0 (no sign prefix → not a
        // numeric prefix in gawk). Signed three-letter forms are accepted.
        assert_eq!(Value::Str("inf".into()).as_number(), 0.0);
        assert_eq!(Value::Str("nan".into()).as_number(), 0.0);
        assert_eq!(Value::Str("+inf".into()).as_number(), f64::INFINITY);
        assert!(Value::Str("+nan".into()).as_number().is_nan());
        // large integers
        assert_eq!(
            Value::Str("9223372036854775807".into()).as_number(),
            9223372036854775807.0
        );
    }

    #[test]
    fn value_is_numeric_str() {
        assert!(Value::Uninit.is_numeric_str());
        assert!(Value::Num(1.0).is_numeric_str());
        assert!(Value::Str("123".into()).is_numeric_str());
        assert!(Value::Str(" -12.3e-1 ".into()).is_numeric_str());
        assert!(!Value::Str("abc".into()).is_numeric_str());
        assert!(!Value::StrLit("123".into()).is_numeric_str());
        // extreme scientific
        assert!(Value::Str("1.234567e+10".into()).is_numeric_str());
        // gawk parity: bare "inf"/"nan" are NOT numeric strings (no sign prefix
        // → not a numeric prefix in gawk). Only signed forms count.
        assert!(!Value::Str("inf".into()).is_numeric_str());
        assert!(!Value::Str("nan".into()).is_numeric_str());
        assert!(Value::Str("+inf".into()).is_numeric_str());
        assert!(Value::Str("-nan".into()).is_numeric_str());
    }

    #[test]
    fn parse_ascii_integer_logic() {
        assert_eq!(parse_ascii_integer("123"), Some(123));
        assert_eq!(parse_ascii_integer("-456"), Some(-456));
        assert_eq!(parse_ascii_integer("+789"), Some(789));
        assert_eq!(parse_ascii_integer("12.3"), None);
        assert_eq!(parse_ascii_integer("abc"), None);
    }

    #[test]
    fn longest_f64_prefix_logic() {
        assert_eq!(longest_f64_prefix("123abc"), Some("123"));
        assert_eq!(longest_f64_prefix("1.2.3"), Some("1.2"));
        assert_eq!(longest_f64_prefix("1e2e3"), Some("1e2"));
        assert_eq!(longest_f64_prefix("abc"), None);
        // scientific notation with + sign
        assert_eq!(longest_f64_prefix("1.2E+10x"), Some("1.2E+10"));
        // negative sign (handled by as_number, but prefix should include it)
        assert_eq!(longest_f64_prefix("-42.5z"), Some("-42.5"));
        // lowercase e
        assert_eq!(longest_f64_prefix("3.14e-5y"), Some("3.14e-5"));
    }

    #[test]
    fn parse_ascii_integer_edge_cases() {
        assert_eq!(parse_ascii_integer("0"), Some(0));
        assert_eq!(parse_ascii_integer("-0"), Some(0));
        assert_eq!(parse_ascii_integer("+0"), Some(0));
        assert_eq!(parse_ascii_integer("007"), Some(7));
        // Overflows/underflows? i64::MAX is large, but let's test a large one.
        assert_eq!(
            parse_ascii_integer("9223372036854775807"),
            Some(9223372036854775807)
        );
    }

    #[test]
    fn split_fields_whitespace() {
        let mut ranges = Vec::new();
        split_fields_into(
            b"  a  b   c  ",
            " ",
            &mut ranges,
            false,
            false,
            false,
            FsRegex::Unknown,
        );
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (2, 3)); // "a"
        assert_eq!(ranges[1], (5, 6)); // "b"
        assert_eq!(ranges[2], (9, 10)); // "c"
    }

    #[test]
    fn split_fields_comma() {
        let mut ranges = Vec::new();
        split_fields_into(
            b"a,b,,c",
            ",",
            &mut ranges,
            false,
            false,
            false,
            FsRegex::Unknown,
        );
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0], (0, 1)); // "a"
        assert_eq!(ranges[1], (2, 3)); // "b"
        assert_eq!(ranges[2], (4, 4)); // ""
        assert_eq!(ranges[3], (5, 6)); // "c"
    }

    #[test]
    fn split_fields_regex() {
        let mut ranges = Vec::new();
        split_fields_into(
            b"a1b22c",
            "[0-9]+",
            &mut ranges,
            false,
            false,
            false,
            FsRegex::Unknown,
        );
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 1)); // "a"
        assert_eq!(ranges[1], (2, 3)); // "b"
        assert_eq!(ranges[2], (5, 6)); // "c"
    }

    #[test]
    fn split_csv_gawk_rfc4180() {
        let mut ranges = Vec::new();
        split_csv_gawk_fields(b"a,\"b,c\",d", &mut ranges);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 1)); // "a"
        assert_eq!(ranges[1], (3, 6)); // "b,c"
        assert_eq!(ranges[2], (8, 9)); // "d"

        split_csv_gawk_fields(b"\"\"\"\"", &mut ranges); // escaped quote
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], (1, 3)); // ""
    }

    #[test]
    fn runtime_variable_overlay() {
        let mut globals = AwkMap::default();
        globals.insert("x".into(), Value::Num(1.0));
        globals.insert("y".into(), Value::Num(2.0));

        let shared = Arc::new(globals);
        let mut rt = Runtime::new();
        rt.global_readonly = Some(shared);

        rt.vars.insert("x".into(), Value::Num(10.0)); // overlay

        assert_eq!(rt.get_global_var("x").unwrap().as_number(), 10.0);
        assert_eq!(rt.get_global_var("y").unwrap().as_number(), 2.0);
        assert!(rt.get_global_var("z").is_none());
    }

    #[test]
    fn value_as_number_hex_and_octal_strings() {
        // as_number() on Value::Str does NOT automatically parse hex/octal.
        // It uses standard f64 parsing.
        assert_eq!(Value::Str("0x10".into()).as_number(), 0.0);
        assert_eq!(Value::Str("010".into()).as_number(), 10.0);
    }

    #[test]
    fn value_str_coercion_from_float() {
        let rt = Runtime::new();
        // CONVFMT is "%.6g" by default
        let v = Value::Num(1.23456789);
        assert_eq!(rt.value_to_array_key(&v), "1.23457");
    }

    #[test]
    fn value_str_coercion_from_large_integer() {
        let rt = Runtime::new();
        // Large integers should use exact decimal form
        let v = Value::Num(1e12);
        assert_eq!(rt.value_to_array_key(&v), "1000000000000");
    }

    #[test]
    fn awk_map_nested_keys() {
        let mut m: AwkMap<String, Value> = AwkMap::default();
        m.insert("1\x1c2".into(), Value::Num(42.0)); // SUBSEP is \x1c
        assert_eq!(m.get("1\x1c2").unwrap().as_number(), 42.0);
    }

    #[test]
    fn awk_map_delete_non_existent() {
        let mut m: AwkMap<String, Value> = AwkMap::default();
        m.remove("x"); // should not panic
    }

    #[test]
    fn awk_map_iteration_order() {
        let mut m: AwkMap<String, Value> = AwkMap::default();
        m.insert("a".into(), Value::Num(1.0));
        m.insert("b".into(), Value::Num(2.0));
        // Order is not guaranteed, but we can verify we get both.
        let keys: Vec<_> = m.keys().collect();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn runtime_set_field_reconstructs_record() {
        let mut rt = Runtime::new();
        rt.set_record_str("a b c");
        rt.set_field(2, "X").unwrap();
        // $0 should be "a X c" (using current OFS, which is " " by default)
        assert_eq!(rt.record, "a X c");
    }

    #[test]
    fn runtime_set_field_beyond_nf() {
        let mut rt = Runtime::new();
        rt.set_record_str("a");
        rt.set_field(3, "c").unwrap();
        // $0 should be "a  c" (OFS is " ")
        assert_eq!(rt.record, "a  c");
        assert_eq!(rt.nf(), 3);

        // Custom OFS
        rt.vars.insert("OFS".into(), Value::Str(",".into()));
        rt.set_field(2, "b").unwrap();
        assert_eq!(rt.record, "a,b,c");
    }

    #[test]
    fn runtime_set_record_updates_nf() {
        let mut rt = Runtime::new();
        rt.vars.insert("FS".into(), Value::Str(",".into()));
        rt.set_record_str("1,2,3");
        // NF should be 3
        assert_eq!(rt.nf(), 3);
    }

    #[test]
    fn split_string_by_fs_character_mode() {
        // Empty FS -> split into individual characters
        let res = split_string_by_field_separator(b"abc", "", false);
        assert_eq!(res, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn split_string_by_fs_whitespace_mode() {
        // Space FS -> split by any whitespace, stripping leading/trailing
        let res = split_string_by_field_separator(b"  x  y   z  ", " ", false);
        assert_eq!(res, vec!["x".to_string(), "y".to_string(), "z".to_string()]);
    }

    #[test]
    fn split_string_by_fs_regex_case_insensitive() {
        let res = split_string_by_field_separator(b"aXbYc", "[xy]", true);
        assert_eq!(res, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn runtime_vars_initial_state() {
        let rt = Runtime::new();
        // RS and OFS are initialized in vars map.
        assert_eq!(rt.vars.get("RS").unwrap().as_str(), "\n");
        assert_eq!(rt.vars.get("OFS").unwrap().as_str(), " ");
        // FS might be missing from vars map (uses cached_fs until first split/assignment).
        assert_eq!(rt.vars.get("OFMT").unwrap().as_str(), "%.6g");
    }

    // ── split_string_with_seps (gawk 4-arg split) ───────────────────────────

    #[test]
    fn split_with_seps_regex_captures_each_matched_run() {
        // Regression: `split(s, a, /[0-9]+/, seps)` must populate seps with the
        // actual matched separator strings, not leave them empty.
        let (parts, seps) = split_string_with_seps(b"a1b22c333d", "[0-9]+", false);
        assert_eq!(parts, vec!["a", "b", "c", "d"]);
        assert_eq!(seps, vec!["1", "22", "333"]);
    }

    #[test]
    fn split_with_seps_single_char_fs_separator_is_the_char() {
        let (parts, seps) = split_string_with_seps(b"a-b-c", "-", false);
        assert_eq!(parts, vec!["a", "b", "c"]);
        assert_eq!(seps, vec!["-", "-"]);
    }

    #[test]
    fn split_with_seps_single_char_fs_is_literal_not_regex_dot() {
        // POSIX awk: single-char FS is always literal. `"."` splits on the dot character,
        // never as "any byte" (the regex metachar meaning).
        let (parts, seps) = split_string_with_seps(b"a.b.c", ".", false);
        assert_eq!(parts, vec!["a", "b", "c"]);
        assert_eq!(seps, vec![".", "."]);
    }

    #[test]
    fn split_with_seps_empty_fs_yields_empty_separators_between_chars() {
        let (parts, seps) = split_string_with_seps(b"abc", "", false);
        assert_eq!(parts, vec!["a", "b", "c"]);
        assert_eq!(seps, vec!["", ""]);
    }

    #[test]
    fn split_with_seps_whitespace_fs_captures_actual_whitespace_run() {
        // Default-whitespace FS (`" "`): leading whitespace is dropped (no leading empty
        // field), and each captured separator is the literal whitespace run found.
        let (parts, seps) = split_string_with_seps(b"  a   b\tc", " ", false);
        assert_eq!(parts, vec!["a", "b", "c"]);
        assert_eq!(seps, vec!["   ", "\t"]);
    }

    #[test]
    fn split_with_seps_no_match_returns_single_field_no_seps() {
        let (parts, seps) = split_string_with_seps(b"zzz", "[0-9]+", false);
        assert_eq!(parts, vec!["zzz"]);
        assert!(seps.is_empty());
    }

    #[test]
    fn split_with_seps_empty_input_returns_empty_vec() {
        let (parts, seps) = split_string_with_seps(b"", ",", false);
        assert!(parts.is_empty());
        assert!(seps.is_empty());
    }

    #[test]
    fn split_with_seps_trailing_separator_keeps_empty_tail_field() {
        // Single-char FS uses `str::split`, which yields an empty trailing field
        // when the input ends in the separator. seps has one entry (the trailing match).
        let (parts, seps) = split_string_with_seps(b"a,b,", ",", false);
        assert_eq!(parts, vec!["a", "b", ""]);
        assert_eq!(seps, vec![",", ","]);
    }

    #[test]
    fn awk_map_insertion_lookup_deletion_v3() {
        let mut m: super::AwkMap<String, Value> = super::AwkMap::default();
        m.insert(String::from("key1"), Value::Num(100.0));
        m.insert(String::from("key2"), Value::Str("val2".into()));

        assert_eq!(m.get(&String::from("key1")).unwrap().as_number(), 100.0);
        assert_eq!(m.get(&String::from("key2")).unwrap().as_str(), "val2");

        m.remove(&String::from("key1"));
        assert!(!m.contains_key(&String::from("key1")));
    }

    #[test]
    fn runtime_set_field_edge_cases_v3() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(" ", b"a b c");
        rt.ensure_fields_split();
        rt.set_field(5, "e").unwrap();
        assert_eq!(rt.nf(), 5);
        assert_eq!(rt.field(5).unwrap().as_str(), "e");
        rt.set_field(0, "x y").unwrap();
        assert_eq!(rt.record, "x y");
        assert_eq!(rt.nf(), 2);
    }

    #[test]
    fn value_truthiness_v2() {
        assert!(!Value::Uninit.truthy());
        assert!(Value::Num(1.0).truthy());
        assert!(!Value::Num(0.0).truthy());
        // Value::Str parses as number if possible; "0" -> 0.0 -> false
        assert!(!Value::Str("0".into()).truthy());
        assert!(Value::StrLit("0".into()).truthy()); // Literals are truthy if non-empty
        assert!(!Value::Str("".into()).truthy());
    }

    #[test]
    fn value_numeric_conversions_v2() {
        assert_eq!(Value::Uninit.as_number(), 0.0);
        assert_eq!(Value::Str("123.45".into()).as_number(), 123.45);
        assert_eq!(Value::Str("abc".into()).as_number(), 0.0);
    }

    #[test]
    fn runtime_variable_overlay_v2() {
        let mut rt = super::Runtime::new();
        rt.vars.insert("x".into(), Value::Num(10.0));
        assert_eq!(rt.vars.get("x").unwrap().as_number(), 10.0);
    }

    #[test]
    fn runtime_nf_truncation_v2() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(":", b"a:b:c:d");
        rt.ensure_fields_split();
        let _ = rt.set_nf(2);
        assert_eq!(rt.nf(), 2);
        // Default OFS is " "
        assert_eq!(rt.record, "a b");
    }

    #[test]
    fn runtime_record_reconstruction_with_ofs_v2() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(",", b"a,b");
        rt.ensure_fields_split();
        rt.vars.insert("OFS".into(), Value::Str("|".into()));
        rt.set_field(1, "x").unwrap();
        // Changing a field should rebuild the record using OFS
        assert_eq!(rt.record, "x|b");
    }

    #[test]
    fn value_num_v9() {
        let _ = Value::Num(0.0).clone();
    }
    #[test]
    fn value_str_v9() {
        let _ = Value::Str("".into()).clone();
    }
    #[test]
    fn value_uninit_v9() {
        let _ = Value::Uninit.clone();
    }
    #[test]
    fn value_as_number_v9() {
        assert_eq!(Value::Num(1.2).as_number(), 1.2);
    }
    #[test]
    fn value_as_str_v9() {
        assert_eq!(Value::Str("abc".into()).as_str(), "abc");
    }
    #[test]
    fn runtime_nf_initial_v9() {
        assert_eq!(super::Runtime::new().nf(), 0);
    }
    #[test]
    fn runtime_nr_initial_v9() {
        assert_eq!(super::Runtime::new().nr, 0.0);
    }
    #[test]
    fn runtime_fnr_initial_v9() {
        assert_eq!(super::Runtime::new().fnr, 0.0);
    }
    #[test]
    fn runtime_fpat_initial_v9() {
        assert_eq!(super::Runtime::new().vars.get("FPAT").unwrap().as_str(), "");
    }
    #[test]
    fn runtime_fs_initial_v9() {
        // POSIX/gawk default: FS = " " (single space, special-cased to mean
        // "split on whitespace runs").
        let rt = super::Runtime::new();
        let fs = rt
            .vars
            .get("FS")
            .expect("FS must be initialized to gawk default");
        assert_eq!(fs.as_str(), " ", "FS default should be single space");
    }

    #[test]
    fn runtime_ors_initial_v56() {
        assert_eq!(
            super::Runtime::new().vars.get("ORS").unwrap().as_str(),
            "\n"
        );
    }
    #[test]
    fn runtime_ofs_initial_v56() {
        assert_eq!(super::Runtime::new().vars.get("OFS").unwrap().as_str(), " ");
    }
    #[test]
    fn runtime_ofmt_initial_v56() {
        assert_eq!(
            super::Runtime::new().vars.get("OFMT").unwrap().as_str(),
            "%.6g"
        );
    }
    #[test]
    fn runtime_convfmt_initial_v56() {
        assert_eq!(
            super::Runtime::new().vars.get("CONVFMT").unwrap().as_str(),
            "%.6g"
        );
    }
    #[test]
    fn runtime_subsep_initial_v56() {
        assert_eq!(
            super::Runtime::new().vars.get("SUBSEP").unwrap().as_str(),
            "\x1c"
        );
    }

    #[test]
    fn runtime_field_sep_v56_0() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(":", b"a:b:c");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
    }
    #[test]
    fn runtime_field_sep_v56_1() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(",", b"a,b,c");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
    }
    #[test]
    fn runtime_field_sep_v56_2() {
        let mut rt = super::Runtime::new();
        rt.set_field_sep_split(" ", b"a b c");
        rt.ensure_fields_split();
        assert_eq!(rt.nf(), 3);
    }
}

/// An awk array.
///
/// Subscripts are strings by definition, but one that IS the canonical decimal
/// spelling of an integer is stored as that integer. `a[1]` and `a["1"]` name
/// the same element, so both spellings must normalize to one key — and once
/// they do, the `a[i]` of a counted loop neither renders a string nor
/// allocates one.
///
/// gawk draws the same distinction, and it is most of the difference between
/// them: denying gawk its integer representation (by subscripting with
/// `"k" i`) costs it 2.5x on a million-element build, while awkrs, which had
/// only the string map, barely moves.
///
/// Iteration yields the integer-subscripted elements first. awk leaves
/// `for (k in a)` order unspecified and every implementation answers
/// differently, so this is a legal order — just not the one the single hash
/// map happened to produce.
#[derive(Debug, Clone, Default)]
pub struct AwkArray {
    ints: AwkMap<i64, Value>,
    strs: AwkMap<Box<[u8]>, Value>,
}

/// The integer a subscript names, when the subscript is exactly that integer's
/// decimal spelling.
///
/// `"1"` is `1`. `"01"`, `"+1"`, `" 1"`, `"1.0"`, `"-0"` and `"9223372036854775808"`
/// are not: awk keeps each as its own element, so each has to stay a string
/// key. Checking by rendering the parse back is what makes that exact.
fn canonical_int(b: &[u8]) -> Option<i64> {
    let (neg, d) = match b.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, b),
    };
    // 19 digits is the most an `i64` can hold without overflow checks per digit;
    // longer subscripts stay strings, as does anything with a leading zero, a
    // sign it does not render, or a non-digit. `-0` is not canonical either:
    // zero renders as "0".
    if d.is_empty() || d.len() > 19 || (d[0] == b'0' && d.len() > 1) || (neg && d == b"0") {
        return None;
    }
    let mut acc: i64 = 0;
    for &c in d {
        if !c.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((c - b'0') as i64)?;
    }
    Some(if neg { -acc } else { acc })
}

impl AwkArray {
    pub fn new() -> Self {
        Self::default()
    }

    /// The element `key` names, whichever half holds it.
    ///
    /// Subscripts are byte strings: `a[$1]` where the field holds a byte that is
    /// not part of valid UTF-8 has to name an entry, and `for (k in a)` has to
    /// hand that subscript back unchanged. The `&str` spellings below are thin
    /// wrappers for the many call sites whose key is text by construction.
    pub fn get_bytes(&self, key: &[u8]) -> Option<&Value> {
        match canonical_int(key) {
            Some(i) => self.ints.get(&i),
            None => self.strs.get(key),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.get_bytes(key.as_bytes())
    }

    /// `a[i]` where the subscript is already an integer — the hot path, with no
    /// rendering and no parse.
    pub fn get_int(&self, i: i64) -> Option<&Value> {
        self.ints.get(&i)
    }

    pub fn get_mut_bytes(&mut self, key: &[u8]) -> Option<&mut Value> {
        match canonical_int(key) {
            Some(i) => self.ints.get_mut(&i),
            None => self.strs.get_mut(key),
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.get_mut_bytes(key.as_bytes())
    }

    pub fn insert(&mut self, key: String, val: Value) -> Option<Value> {
        self.insert_bytes(key.as_bytes(), val)
    }

    /// `a[k] = v` with a borrowed subscript.
    ///
    /// The owned form has to be handed a `String` even when the subscript names
    /// an integer — the overwhelmingly common case for `a[$1]`, `a[NR]` and
    /// `a[i]` — and then drops it unread, because an integer subscript is stored
    /// in the `ints` half. That is one wasted allocation per element stored.
    /// Taking the key by reference defers the allocation to the `strs` half,
    /// which is the only half that needs to own it.
    pub fn insert_bytes(&mut self, key: &[u8], val: Value) -> Option<Value> {
        match canonical_int(key) {
            Some(i) => self.ints.insert(i, val),
            None => self.strs.insert(key.into(), val),
        }
    }

    pub fn insert_str(&mut self, key: &str, val: Value) -> Option<Value> {
        self.insert_bytes(key.as_bytes(), val)
    }

    /// `a[i] = v` with an integer subscript — stores without building a key.
    pub fn insert_int(&mut self, i: i64, val: Value) -> Option<Value> {
        self.ints.insert(i, val)
    }

    pub fn remove_bytes(&mut self, key: &[u8]) -> Option<Value> {
        match canonical_int(key) {
            Some(i) => self.ints.remove(&i),
            None => self.strs.remove(key),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.remove_bytes(key.as_bytes())
    }

    pub fn contains_key_bytes(&self, key: &[u8]) -> bool {
        match canonical_int(key) {
            Some(i) => self.ints.contains_key(&i),
            None => self.strs.contains_key(key),
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.contains_key_bytes(key.as_bytes())
    }

    pub fn len(&self) -> usize {
        self.ints.len() + self.strs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ints.is_empty() && self.strs.is_empty()
    }

    pub fn clear(&mut self) {
        self.ints.clear();
        self.strs.clear();
    }

    /// Every subscript, rendered. Owned rather than borrowed because an integer
    /// subscript has no stored string to lend.
    pub fn keys(&self) -> Vec<AwkStr> {
        let mut out = Vec::with_capacity(self.len());
        let mut b = KeyBuf::new();
        for i in self.ints.keys() {
            out.push(AwkStr::from(b.write_i64(*i)));
        }
        out.extend(self.strs.keys().map(|k| AwkStr::from(&k[..])));
        out
    }

    pub fn iter(&self) -> impl Iterator<Item = (AwkStr, &Value)> {
        let ints = self.ints.iter().map(|(i, v)| {
            let mut b = KeyBuf::new();
            (AwkStr::from(b.write_i64(*i)), v)
        });
        ints.chain(self.strs.iter().map(|(k, v)| (AwkStr::from(&k[..]), v)))
    }

    /// `entry(k).or_insert(v)` in one call — store `val` only when the
    /// subscript is absent, and hand back what the subscript now names.
    pub fn or_insert(&mut self, key: String, val: Value) -> &mut Value {
        match canonical_int(key.as_bytes()) {
            Some(i) => self.ints.entry(i).or_insert(val),
            None => self.strs.entry(key.into_bytes().into_boxed_slice()).or_insert(val),
        }
    }

    /// [`AwkArray::or_insert`] with the value built only when it is needed.
    pub fn or_insert_with(&mut self, key: String, f: impl FnOnce() -> Value) -> &mut Value {
        match canonical_int(key.as_bytes()) {
            Some(i) => self.ints.entry(i).or_insert_with(f),
            None => self
                .strs
                .entry(key.into_bytes().into_boxed_slice())
                .or_insert_with(f),
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.ints.values().chain(self.strs.values())
    }
}

impl FromIterator<(String, Value)> for AwkArray {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(it: T) -> Self {
        let mut a = AwkArray::new();
        for (k, v) in it {
            a.insert(k, v);
        }
        a
    }
}

/// Scratch for rendering an integral array subscript in place.
///
/// Wide enough for any `i64` in decimal with its sign, so writing one can never
/// need to grow or allocate.
pub struct KeyBuf([u8; 20]);

impl Default for KeyBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyBuf {
    pub fn new() -> Self {
        KeyBuf([0; 20])
    }

    /// `n` in decimal, written back-to-front, returned as a borrow of this
    /// buffer — the integer writer without the formatting machinery, on a path
    /// that runs once per array subscript.
    pub fn write_i64(&mut self, n: i64) -> &str {
        let neg = n < 0;
        // `-i64::MIN` overflows, so accumulate the magnitude as `u64`.
        let mut m = if neg {
            (n as i128).unsigned_abs() as u64
        } else {
            n as u64
        };
        let mut i = self.0.len();
        loop {
            i -= 1;
            self.0[i] = b'0' + (m % 10) as u8;
            m /= 10;
            if m == 0 {
                break;
            }
        }
        if neg {
            i -= 1;
            self.0[i] = b'-';
        }
        // Every byte written is an ASCII digit or '-', so this is UTF-8 by
        // construction.
        std::str::from_utf8(&self.0[i..]).expect("ascii digits")
    }
}
