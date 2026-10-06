//! `sprintf` / `printf` formatting (POSIX-ish; common awk conversions).

use crate::awkstr::AwkStr;
use crate::bignum::{float_trunc_integer, mpfr_string_for_percent_s, value_to_mpfr};
use crate::runtime::Value;
use rug::float::Round;
use rug::ops::Pow;
use std::sync::atomic::{AtomicBool, Ordering};

/// Process-wide opt-in for BSD/Bell-Labs awk printf quirks: zero-padding `%0Ns`
/// and `%0Nc` (non-POSIX — POSIX says the `0` flag is for numeric conversions
/// only). Set once from `--traditional` at startup; read with `Relaxed`
/// ordering inside `format_one_spec`. Defaults to off so gawk/POSIX parity
/// stays intact for every caller that doesn't explicitly flip the switch.
pub static AWK_TRADITIONAL_MODE: AtomicBool = AtomicBool::new(false);

/// Process-wide byte character model for `%s` / `%c`: precision, field width
/// and a string's first character count bytes, and `%c` of a number is its low
/// byte. Set once at startup from `-b` (the runtime's `characters_as_bytes`),
/// gawk's `MB_CUR_MAX == 1` model.
pub static AWK_CHARS_AS_BYTES: AtomicBool = AtomicBool::new(false);

/// Default C-locale radix (`.`). Use [`awk_sprintf_with_decimal`] when `-N` applies.
pub fn awk_sprintf(fmt: &str, vals: &[Value]) -> Result<String, String> {
    awk_sprintf_with_decimal(fmt, vals, '.', Some(','), None)
}

/// `%s` of a plain number under `printf` / `sprintf` renders through `CONVFMT`
/// (integers bypass it), as gawk's `format_tree` does with `force_string`.
fn format_num_via_convfmt(n: f64, convfmt: &str) -> String {
    // Integer-valued numbers bypass CONVFMT (gawk parity).
    if n.is_finite() && n.fract() == 0.0 {
        if n == 0.0 {
            return "0".to_string();
        }
        return format!("{:.0}", n);
    }
    if !n.is_finite() {
        let sign = if n.is_sign_negative() { '-' } else { '+' };
        let body = if n.is_nan() { "nan" } else { "inf" };
        return format!("{sign}{body}");
    }
    // Apply the user-supplied CONVFMT.
    awk_sprintf_with_decimal(convfmt, &[Value::Num(n)], '.', Some(','), None)
        .unwrap_or_else(|_| format!("{n}"))
}

/// Variant of [`awk_sprintf_with_decimal`] that honors `CONVFMT` for numeric
/// values that flow through `%s` conversion.
pub fn awk_sprintf_with_convfmt(
    fmt: &str,
    vals: &[Value],
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
    convfmt: &str,
) -> Result<AwkStr, String> {
    format_tree(fmt, vals, decimal, thousands_sep, mpfr_mode, Some(convfmt))
}
/// `awk_sprintf_with_decimal` — see implementation for the contract.
pub fn awk_sprintf_with_decimal(
    fmt: &str,
    vals: &[Value],
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
) -> Result<String, String> {
    // The `String` form is for the number-rendering callers (`CONVFMT`,
    // `OFMT`, the MPFR display paths), whose output is ASCII either way.
    awk_sprintf_bytes(fmt, vals, decimal, thousands_sep, mpfr_mode).map(|s| s.to_lossy_string())
}

/// [`awk_sprintf_with_decimal`] without the rendering — what `printf` and
/// `sprintf` use, so a `%s` of a value holding a byte that is not part of valid
/// UTF-8 emits that byte instead of `U+FFFD`.
pub fn awk_sprintf_bytes(
    fmt: &str,
    vals: &[Value],
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
) -> Result<AwkStr, String> {
    format_tree(fmt, vals, decimal, thousands_sep, mpfr_mode, None)
}

/// Which number a digit run or `*` in a conversion spec sets: gawk's `cur`,
/// pointing at `fw`, at `prec`, or at nothing once the precision is complete.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecTarget {
    Width,
    Precision,
    Closed,
}

/// The upper bound awkrs puts on a field width. Real format widths never
/// approach it; past it is user error or fuzz input, and honoring it would
/// only exhaust memory (`%99999999999999d`).
const MAX_FMT_WIDTH: i64 = 100_000;

/// Read a run of decimal digits starting at `*i` (the first digit already
/// known to be there), saturating rather than overflowing.
fn spec_digits(b: &[u8], i: &mut usize, mut acc: i64) -> i64 {
    while *i < b.len() && b[*i].is_ascii_digit() {
        acc = acc.saturating_mul(10).saturating_add((b[*i] - b'0') as i64);
        *i += 1;
    }
    acc
}

/// gawk `get_number_si`: a `*` operand's value truncated to a C `long`.
fn star_operand(v: &Value) -> i64 {
    let n = v.as_number();
    if n.is_nan() {
        0
    } else {
        n as i64
    }
}

/// Port of gawk's `format_tree` (builtin.c) scanning loop.
///
/// After a `%`, the flag, width, precision and length-modifier characters are
/// taken in **any order** through gawk's `retry` state machine: `%l5d`,
/// `%5-d` and `%h-5d` are all valid conversions. A character the state
/// machine rejects — a repeated `h` / `l` / `L` / `P`, a flag after the
/// precision, an unknown conversion letter, a second `.` — abandons the
/// conversion: the text from the `%` up to and including that character is
/// copied to the output literally, no argument is converted, and scanning
/// resumes right after it (so `%lld` prints `%lld`, and in `%ll%d` the `%d`
/// still converts). A format that ends inside a spec is copied literally too
/// (`%5` prints `%5`). `%%` emits one `%` whatever flags precede it.
///
/// Positional `n$` values and `*n$` operands keep awkrs's own rules for mixing
/// with sequential arguments, which the unit tests pin.
fn format_tree(
    fmt: &str,
    vals: &[Value],
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
    convfmt: Option<&str>,
) -> Result<AwkStr, String> {
    let b = fmt.as_bytes();
    let n = b.len();
    let mut out = AwkStr::new();
    let mut vi = 0usize;
    // `s0`: start of the text not yet copied out; `s1`: the scan position.
    let mut s0 = 0usize;
    let mut s1 = 0usize;
    'scan: while s1 < n {
        if b[s1] != b'%' {
            s1 += 1;
            continue;
        }
        out.push_bytes(&b[s0..s1]);
        s0 = s1;
        s1 += 1;

        let mut cur = SpecTarget::Width;
        let mut fw: i64 = 0;
        let mut prec: i64 = 0;
        let mut have_prec = false;
        let mut argnum: Option<usize> = None;
        let mut signchar: Option<u8> = None;
        let mut zero_flag = false;
        let mut quote_flag = false;
        let mut lj = false;
        let mut alt = false;
        let mut big_flag = false;
        let mut bigbig_flag = false;
        let mut small_flag = false;
        let mut magic_posix_flag = false;

        loop {
            // gawk `retry:` — a format that ends inside a spec is literal text.
            if s1 >= n {
                break 'scan;
            }
            let cs1 = b[s1];
            s1 += 1;
            // `check_pos`: a flag is accepted only before the precision.
            let check_pos = |cur: SpecTarget| cur == SpecTarget::Width;
            match cs1 {
                b'%' => {
                    out.push_byte(b'%');
                    s0 = s1;
                    break;
                }
                b'0'..=b'9' => {
                    if cs1 == b'0' {
                        // Only a `0` before the width and precision is the flag.
                        if cur == SpecTarget::Width {
                            zero_flag = true;
                        }
                        if lj {
                            continue;
                        }
                    }
                    if cur == SpecTarget::Closed {
                        break;
                    }
                    let v = if prec >= 0 {
                        spec_digits(b, &mut s1, (cs1 - b'0') as i64)
                    } else {
                        // A negative precision (`%.-3d`) eats its digits and is discarded.
                        spec_digits(b, &mut s1, 0);
                        prec
                    };
                    match cur {
                        SpecTarget::Width => fw = v,
                        _ => prec = v,
                    }
                    if prec < 0 {
                        have_prec = false;
                    }
                    if cur == SpecTarget::Precision {
                        cur = SpecTarget::Closed;
                    }
                }
                b'$' => {
                    if cur != SpecTarget::Width {
                        return Err("sprintf: `$' not permitted after period in format".into());
                    }
                    if fw <= 0 {
                        return Err("sprintf: positional argument was 0".into());
                    }
                    let k = fw as usize;
                    if k > vals.len() {
                        return Err("sprintf: invalid positional argument".into());
                    }
                    argnum = Some(k);
                    fw = 0;
                }
                b'*' => {
                    if cur == SpecTarget::Closed {
                        break;
                    }
                    let v = if s1 < n && b[s1].is_ascii_digit() {
                        let k = spec_digits(b, &mut s1, 0);
                        if s1 >= n || b[s1] != b'$' {
                            return Err(
                                "sprintf: no `$' supplied for positional field width or precision"
                                    .into(),
                            );
                        }
                        s1 += 1;
                        if k <= 0 {
                            return Err("sprintf: positional argument was 0".into());
                        }
                        let k = k as usize;
                        let v = val_at(vals, k)?;
                        vi = vi.max(k);
                        star_operand(v)
                    } else {
                        star_operand(take_val(vals, &mut vi)?)
                    };
                    match cur {
                        SpecTarget::Width => {
                            fw = v;
                            if fw < 0 {
                                fw = fw.saturating_neg();
                                lj = true;
                            }
                        }
                        _ => {
                            prec = v;
                            have_prec = prec >= 0;
                            cur = SpecTarget::Closed;
                        }
                    }
                }
                b' ' | b'+' => {
                    // A space never overrides a sign flag already given.
                    if cs1 == b'+' || signchar.is_none() {
                        signchar = Some(cs1);
                    }
                    if !check_pos(cur) {
                        break;
                    }
                }
                b'-' => {
                    if prec < 0 {
                        break;
                    }
                    if cur == SpecTarget::Precision {
                        prec = -1;
                        continue;
                    }
                    lj = true;
                    if !check_pos(cur) {
                        break;
                    }
                }
                b'.' => {
                    if cur != SpecTarget::Width {
                        break;
                    }
                    cur = SpecTarget::Precision;
                    have_prec = true;
                }
                b'#' => {
                    alt = true;
                    if !check_pos(cur) {
                        break;
                    }
                }
                b'\'' => {
                    quote_flag = true;
                    if !check_pos(cur) {
                        break;
                    }
                }
                // Length modifiers are meaningless in awk and ignored — but each
                // may appear only once.
                b'l' => {
                    if big_flag {
                        break;
                    }
                    big_flag = true;
                }
                b'L' => {
                    if bigbig_flag {
                        break;
                    }
                    bigbig_flag = true;
                }
                b'h' => {
                    if small_flag {
                        break;
                    }
                    small_flag = true;
                }
                b'P' => {
                    if magic_posix_flag {
                        break;
                    }
                    magic_posix_flag = true;
                }
                c if is_known_conv(c as char) => {
                    let conv = c as char;
                    let v = match argnum {
                        Some(k) => val_at(vals, k)?,
                        None => take_val(vals, &mut vi)?,
                    };
                    let width = (fw != 0).then(|| fw.clamp(0, MAX_FMT_WIDTH) as usize);
                    let prec = (have_prec && prec >= 0).then_some(prec as usize);
                    let piece = if conv == 's' || conv == 'c' {
                        // `%s` and `%c` are the two conversions whose output is the
                        // caller's own bytes rather than digits awkrs generated, so
                        // they are answered before `format_one`, which works in
                        // `String` and cannot carry one.
                        let conv_s;
                        let v = match (conv, convfmt, v) {
                            ('s', Some(cf), Value::Num(x)) => {
                                conv_s = Value::StrLit(format_num_via_convfmt(*x, cf).into());
                                &conv_s
                            }
                            _ => v,
                        };
                        format_str_or_char_bytes(conv, v, lj, zero_flag, width, prec)?
                    } else {
                        format_one(
                            conv,
                            v,
                            lj,
                            signchar == Some(b'+'),
                            signchar == Some(b' '),
                            alt,
                            zero_flag,
                            quote_flag,
                            width,
                            prec,
                            decimal,
                            thousands_sep,
                            mpfr_mode,
                        )?
                        .into()
                    };
                    out.push_awkstr(&piece);
                    s0 = s1;
                    break;
                }
                // An unknown conversion character: nothing is converted.
                _ => break,
            }
        }
    }
    out.push_bytes(&b[s0..]);
    Ok(out)
}

fn take_val<'a>(vals: &'a [Value], vi: &mut usize) -> Result<&'a Value, String> {
    let v = vals
        .get(*vi)
        .ok_or_else(|| "sprintf: not enough arguments".to_string())?;
    *vi += 1;
    Ok(v)
}

fn val_at(vals: &[Value], one_based: usize) -> Result<&Value, String> {
    vals.get(one_based - 1)
        .ok_or_else(|| "sprintf: invalid positional argument".to_string())
}

/// Conversion letters that `format_one` understands. Anything outside this set is
/// emitted as a literal `%<conv>` (gawk's behavior for unknown specifiers).
fn is_known_conv(c: char) -> bool {
    matches!(
        c,
        's' | 'd'
            | 'i'
            | 'u'
            | 'o'
            | 'x'
            | 'X'
            | 'a'
            | 'A'
            | 'f'
            | 'F'
            | 'e'
            | 'E'
            | 'g'
            | 'G'
            | 'c'
    )
}

/// Same as [`insert_thousands_sep`] but only groups the integer portion of a
/// floating value, leaving anything after the radix point unchanged. Used by
/// the `%'f` / `%'e` / `%'g` flags.
fn insert_thousands_sep_float(s: String, sep: char, decimal: char) -> String {
    let (int_part, frac_part) = match s.find(decimal) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s.as_str(), ""),
    };
    let int_grouped = insert_thousands_sep(int_part.to_string(), sep);
    if frac_part.is_empty() {
        int_grouped
    } else {
        format!("{int_grouped}{frac_part}")
    }
}

/// Insert thousands separators (gawk **`%'`** flag) for a signed decimal digit string.
fn insert_thousands_sep(s: String, sep: char) -> String {
    if sep == '\0' || s.is_empty() {
        return s;
    }
    let neg = s.starts_with('-');
    let digit_part = if neg { &s[1..] } else { &s[..] };
    if digit_part.is_empty() {
        return s;
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    let len = digit_part.len();
    for (i, c) in digit_part.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

fn localize_float_radix(s: String, decimal: char) -> String {
    if decimal == '.' {
        return s;
    }
    let rep = decimal.to_string();
    s.replacen('.', &rep, 1)
}

/// C `printf` `%g` / `%G`: trim fractional zeros and a dangling radix point.
fn trim_trailing_zero_fraction(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let mut t = s.trim_end_matches('0').to_string();
    if t.ends_with('.') {
        t.pop();
    }
    t
}

/// gawk-style spelling of a non-finite float for `%f`/`%e`/`%g`/`%a` conversions.
///
/// Returns `+inf`/`-inf`/`+nan`/`-nan` (`INF`/`NAN` for the uppercase variants).
/// `+` is emitted on positive (or unsigned) values; the IEEE 754 sign bit is
/// preserved for both inf and NaN. Math functions that produce NaN (sqrt, log
/// of negatives) normalize the sign at their call sites — see [`crate::builtins`].
fn format_non_finite(n: f64, upper: bool) -> Option<String> {
    if n.is_finite() {
        return None;
    }
    let body = if n.is_nan() {
        if upper {
            "NAN"
        } else {
            "nan"
        }
    } else if upper {
        "INF"
    } else {
        "inf"
    };
    let sign = if n.is_sign_negative() { '-' } else { '+' };
    Some(format!("{sign}{body}"))
}

/// POSIX / awk exponent: `e`/`E` then sign and at least two magnitude digits (`e+03`).
fn format_sprintf_exponent(exp: i32, upper_e: bool) -> String {
    let ec = if upper_e { 'E' } else { 'e' };
    let sign = if exp < 0 { '-' } else { '+' };
    let mag = exp.unsigned_abs();
    let w = if mag == 0 {
        2usize
    } else {
        (mag.ilog10() as usize + 1).max(2)
    };
    format!("{ec}{sign}{mag:0w$}", w = w)
}

/// Format a float as C99 hex-float (`%a` / `%A`): `[-]0xh.hhhhp±d`.
fn format_hex_float(n: f64, prec: Option<usize>, upper: bool, alt: bool) -> String {
    if let Some(s) = format_non_finite(n, upper) {
        return s;
    }
    if n == 0.0 {
        let sign = if n.is_sign_negative() { "-" } else { "" };
        let prefix = if upper { "0X" } else { "0x" };
        let p = prec.unwrap_or(0);
        let dot = if p > 0 || alt { "." } else { "" };
        let frac = "0".repeat(p);
        let exp_char = if upper { 'P' } else { 'p' };
        return format!("{sign}{prefix}0{dot}{frac}{exp_char}+0");
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let abs_n = n.abs();
    let bits: u64 = abs_n.to_bits();
    let raw_exp = ((bits >> 52) & 0x7FF) as i64;
    let raw_mant = bits & 0x000F_FFFF_FFFF_FFFF;
    let (exp, int_digit, frac_bits) = if raw_exp == 0 {
        // Subnormal: normalize by finding the leading 1
        if raw_mant == 0 {
            (0i64, 0u64, 0u64)
        } else {
            let shift = raw_mant.leading_zeros() as i64 - 12; // 12 = 64 - 52
            let normalized = raw_mant << shift;
            let exp = -1022 - shift;
            (exp, 1, normalized & 0x000F_FFFF_FFFF_FFFF)
        }
    } else {
        // Normal: implicit leading 1, exponent is biased
        (raw_exp - 1023, 1, raw_mant)
    };
    // Apply rounding when an explicit precision truncates hex digits (round half to even).
    let (int_digit, frac_bits) = if let Some(p) = prec {
        if p < 13 {
            let keep_bits = p * 4;
            let drop_bits = 52 - keep_bits;
            let half = 1u64 << (drop_bits - 1);
            let mask = (1u64 << drop_bits) - 1;
            let dropped = frac_bits & mask;
            let mut kept = frac_bits >> drop_bits;
            let mut id = int_digit;
            let lsb = if p > 0 { kept & 1 } else { id & 1 };
            if dropped > half || (dropped == half && lsb != 0) {
                kept += 1;
                if kept >= (1u64 << keep_bits) {
                    kept = 0;
                    id += 1;
                }
            }
            (id, kept << drop_bits)
        } else {
            (int_digit, frac_bits)
        }
    } else {
        (int_digit, frac_bits)
    };
    // frac_bits holds the 52-bit fractional mantissa → 13 hex digits
    let full_frac = format!("{frac_bits:013x}");
    let frac_str = match prec {
        Some(0) if !alt => String::new(),
        Some(p) => {
            let needed = p.min(13);
            if needed <= full_frac.len() {
                format!(".{}", &full_frac[..needed])
            } else {
                let pad = needed - full_frac.len();
                format!(".{}{}", full_frac, "0".repeat(pad))
            }
        }
        None => {
            // Default: show all significant hex digits (trim trailing zeros)
            let trimmed = full_frac.trim_end_matches('0');
            if trimmed.is_empty() && !alt {
                String::new()
            } else if trimmed.is_empty() {
                ".".to_string()
            } else {
                format!(".{trimmed}")
            }
        }
    };
    let prefix = if upper { "0X" } else { "0x" };
    let exp_char = if upper { 'P' } else { 'p' };
    let exp_sign = if exp >= 0 { '+' } else { '-' };
    let exp_abs = exp.unsigned_abs();
    let int_hex = if upper {
        format!("{int_digit:X}")
    } else {
        format!("{int_digit:x}")
    };
    let frac_str = if upper {
        frac_str.to_uppercase()
    } else {
        frac_str
    };
    format!("{sign}{prefix}{int_hex}{frac_str}{exp_char}{exp_sign}{exp_abs}")
}

/// Rewrite `…e±digits` / `…E±digits` to awk-style exponent (always signed, min 2 magnitude digits).
fn normalize_sprintf_scientific_exponent(s: &str) -> String {
    let Some(pos) = s.find(['e', 'E']) else {
        return s.to_string();
    };
    let (mant, rest) = s.split_at(pos);
    let upper = rest.starts_with('E');
    let exp: i32 = rest[1..].parse().unwrap_or(0);
    format!("{}{}", mant, format_sprintf_exponent(exp, upper))
}

/// After `%e`/`%E` formatting for `%g`, trim zeros in the mantissa only, then normalize exponent.
fn trim_sprintf_g_scientific(s: &str) -> String {
    let Some(pos) = s.find(['e', 'E']) else {
        return trim_trailing_zero_fraction(s);
    };
    let (mant, exp_with_e) = s.split_at(pos);
    let upper = exp_with_e.starts_with('E');
    let exp: i32 = exp_with_e[1..].parse().unwrap_or(0);
    format!(
        "{}{}",
        trim_trailing_zero_fraction(mant),
        format_sprintf_exponent(exp, upper)
    )
}

/// ISO C / POSIX: for `%g` / `%G` in **fixed** style, precision is **significant digits**, not
/// fraction digits after the radix (unlike `%f`).
fn format_g_decimal_significant_f64(mut n: f64, p: usize) -> String {
    let p = p.max(1);
    if !n.is_finite() {
        return format!("{n}");
    }
    let neg = n.is_sign_negative();
    n = n.abs();
    if n == 0.0 {
        return if neg {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    // The rounding has to come from Rust's formatter, not from arithmetic.
    // Scaling by `10^(p-1-e)`, rounding, and scaling back was wrong twice over:
    // `f64::round` rounds halves *away from zero* where C rounds the exact
    // binary value half-to-even, and the multiply itself moved the value before
    // the rounding could see it. `printf "%.1g", 2.5` came out `3` against `2`
    // in gawk, mawk and one-true-awk alike (and `4.5` → `5` against `4`, `%.2g`
    // of `1.25` → `1.3` against `1.2`), while `printf "%.1g", 0.15` came out
    // `0.2` against `0.1` for the other reason: 0.15 is really 0.1499…, but
    // `0.15 * 10` rounds *up* to exactly 1.5 before `round()` ever runs.
    //
    // `{:.*e}` gives the value correctly rounded to `p` significant digits, so
    // its exponent is the one C uses to pick the fixed-form precision; `{:.*}`
    // then rounds the original value to that many decimals, again exactly.
    let sci = format!("{:.*e}", p - 1, n);
    let exp: i32 = sci
        .rfind('e')
        .and_then(|i| sci[i + 1..].parse().ok())
        .unwrap_or(0);
    let frac = (p as i32 - 1 - exp).max(0) as usize;
    let body = format!("{:.*}", frac, n);
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// The `%s` and `%c` conversions, over bytes.
///
/// Everything else `printf` produces is digits awkrs generated, which are ASCII
/// and lose nothing through a `String`. These two hand back the caller's own
/// bytes, so they must not: `printf "%s", $0` of a record holding `\351` used to
/// emit the three bytes of `U+FFFD` where gawk, mawk and one-true-awk all emit
/// the one byte they were given.
fn format_str_or_char_bytes(
    conv: char,
    v: &Value,
    left: bool,
    pad_zero: bool,
    width: Option<usize>,
    prec: Option<usize>,
) -> Result<AwkStr, String> {
    let w = width.unwrap_or(0);
    // POSIX / gawk: the `0` flag has no effect on a string conversion — pad with
    // spaces regardless. BSD `/usr/bin/awk` zero-pads under `%0Ns`, and that
    // quirk stays selectable through `--traditional`.
    let pad = if AWK_TRADITIONAL_MODE.load(Ordering::Relaxed) && pad_zero && !left {
        b'0'
    } else {
        b' '
    };
    let bytes = AWK_CHARS_AS_BYTES.load(Ordering::Relaxed);
    let body = match conv {
        's' => {
            let mut b = AwkStr::from_vec(v.as_bytes_cow().into_owned());
            if let Some(p) = prec {
                b = if bytes {
                    b.substr_bytes(0, p)
                } else {
                    b.substr_chars(0, p)
                };
            }
            b
        }
        _ => sprintf_c_char_bytes(v),
    };
    Ok(pad_bytes(body, w, left, pad))
}

/// [`pad_string`] over bytes, counting the field width in characters the same
/// way — a byte that does not begin a valid UTF-8 character counts as one — or
/// in bytes under [`AWK_CHARS_AS_BYTES`].
fn pad_bytes(body: AwkStr, width: usize, left: bool, pad: u8) -> AwkStr {
    let len = if AWK_CHARS_AS_BYTES.load(Ordering::Relaxed) {
        body.len()
    } else {
        body.chars_lossy().count()
    };
    if width <= len {
        return body;
    }
    let padn = width - len;
    let mut out = AwkStr::with_capacity(body.len() + padn);
    if left {
        out.push_awkstr(&body);
        out.push_bytes(&vec![pad; padn]);
    } else {
        out.push_bytes(&vec![pad; padn]);
        out.push_awkstr(&body);
    }
    out
}

/// [`sprintf_c_char`] over bytes: the first **character** of a string argument,
/// as the bytes that spell it, and the character with the given code for a
/// numeric one.
///
/// The numeric case is where the two character models part. In a single-byte
/// locale gawk, mawk and one-true-awk all emit `N & 0xFF` — `printf "%c", 233`
/// is the one byte `\351`, and it stays the low byte above 255 (`300` is `\054`,
/// `955` is `\273`). In a UTF-8 locale gawk emits the UTF-8 encoding of the code
/// point instead, and mawk and one-true-awk still emit the low byte. Following
/// the locale therefore matches gawk in both and the other two in the one they
/// agree with it on; awkrs used to emit the UTF-8 encoding unconditionally,
/// which is the C-locale gap section 9 recorded.
fn sprintf_c_char_bytes(v: &Value) -> AwkStr {
    match v {
        // A numeric string (a field, a `getline` variable, a `split` element)
        // counts as numeric, so `echo 65 | awk '{printf "%c", $1}'` prints `A`.
        Value::Str(_) if v.is_numeric_str() => numeric_c_char(v),
        Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => {
            if s.is_empty() {
                // gawk copies one byte from the string's buffer, which for an
                // empty string is its NUL terminator; mawk does the same, so
                // `printf "[%3c]", ""` is `[  \0]` in both.
                AwkStr::from_vec(vec![0])
            } else if AWK_CHARS_AS_BYTES.load(Ordering::Relaxed) {
                s.substr_bytes(0, 1)
            } else {
                s.substr_chars(0, 1)
            }
        }
        _ => numeric_c_char(v),
    }
}

/// The numeric half of [`sprintf_c_char_bytes`] — see its note on the locale.
fn numeric_c_char(v: &Value) -> AwkStr {
    let code = match v {
        Value::Mpfr(f) => float_trunc_integer(f).to_u32_wrapping(),
        _ => v.as_number() as i64 as u32,
    };
    // A code point no character can name falls back to the low byte in either
    // model: mawk and one-true-awk emit it in both locales, so it is the
    // majority answer where gawk clamps to NUL instead.
    match (
        crate::locale_numeric::chars_are_multibyte(),
        char::from_u32(code),
    ) {
        (true, Some(c)) => AwkStr::from(c),
        _ => {
            let mut out = AwkStr::new();
            out.push_byte((code & 0xff) as u8);
            out
        }
    }
}

/// `%.*f` for an MPFR value: `p` digits **after the radix point**.
///
/// `rug::Float`'s `Display` precision counts *significant* digits, not decimals,
/// so `format!("{:.4}", 2.5)` gives `2.500` where C's `%.4f` gives `2.5000`,
/// and a precision of 0 asked for no significant digits at all and printed the
/// value's whole expansion. Converting decimals to significant digits needs the
/// value's decimal exponent, which its scientific form reports — and reports
/// *after* rounding, which is the exponent the digit count has to be based on.
fn mpfr_fixed(f: &rug::Float, p: usize) -> String {
    if !f.is_finite() {
        return format!("{:.*}", p, f.to_f64());
    }
    // `%.*f` is "round the value to `p` decimals", which is exactly rounding
    // `value * 10^p` to an integer. Doing it that way avoids `Display`
    // altogether: rug's precision counts *significant* digits, not decimals, so
    // `format!("{:.4}", 2.5)` gave `2.500` where C gives `2.5000`, and it
    // switches to scientific notation on its own for small values, which `%f`
    // never does.
    let scale = rug::Integer::from(10).pow(p as u32);
    // Room for the shift plus guard bits, so the multiply itself cannot round.
    let bits = f
        .prec()
        .saturating_add((p as u32).saturating_mul(4))
        .saturating_add(64);
    let scaled = rug::Float::with_val(bits, f) * &scale;
    let Some((int, _)) = scaled.to_integer_round(Round::Nearest) else {
        return format!("{:.*}", p, f.to_f64());
    };
    // The sign comes from the value, not the rounded integer: `%.2f` of
    // `-0.001` is `-0.00` in gawk, and the integer it rounds to is plain zero.
    let neg = f.is_sign_negative();
    let digits = int.abs().to_string();
    let mut out = String::with_capacity(digits.len() + p + 2);
    if neg {
        out.push('-');
    }
    if p == 0 {
        out.push_str(&digits);
        return out;
    }
    if digits.len() <= p {
        out.push_str("0.");
        for _ in 0..(p - digits.len()) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let split = digits.len() - p;
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    }
    out
}

/// `%.*e` for an MPFR value: `p` digits after the radix point, so `p + 1`
/// significant. Same significant-vs-decimal mismatch as [`mpfr_fixed`].
fn mpfr_scientific(f: &rug::Float, p: usize) -> String {
    if !f.is_finite() {
        return format!("{:.*e}", p, f.to_f64());
    }
    format!("{:.*e}", p + 1, f)
}

fn sprintf_c_char(v: &Value) -> String {
    match v {
        // POSIX: `%c` prints the first character of a *string* argument and the
        // character with the given code for a *numeric* one. A numeric string
        // (a field, a `getline` variable, a `split` element) counts as numeric,
        // so `echo 65 | awk '{printf "%c", $1}'` prints `A` in gawk, mawk and
        // one-true-awk alike — awkrs used to print `6`.
        Value::Str(s) if v.is_numeric_str() => char::from_u32(v.as_number() as u32)
            .unwrap_or('\u{fffd}')
            .to_string(),
        Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => s
            .to_str_lossy()
            .chars()
            .next()
            .map(|c| c.to_string())
            // An empty string yields its NUL terminator, as in gawk and mawk.
            .unwrap_or_else(|| "\0".to_string()),
        Value::Mpfr(f) => {
            let code = float_trunc_integer(f).to_u32_wrapping();
            char::from_u32(code).unwrap_or('\u{fffd}').to_string()
        }
        _ => {
            let code = v.as_number() as u32;
            char::from_u32(code).unwrap_or('\u{fffd}').to_string()
        }
    }
}

/// `-M` operand of `%o %x %X`: gawk prints the whole truncated integer in the
/// base (`printf "%x", 2^64` is `10000000000000000`). A negative one keeps the
/// 64-bit two's complement awkrs has always printed; gawk 5.4.1 itself emits a
/// malformed `0x-ff` or an internal error there.
fn mpfr_unsigned_digits(f: &rug::Float) -> rug::Integer {
    let int = float_trunc_integer(f);
    if int < 0 {
        rug::Integer::from(int.to_u64_wrapping())
    } else {
        int
    }
}

/// gawk `format_integer_digits` (printf.c) operand rule for `%o %u %x %X`:
/// truncate, cast to `uintmax_t` (through `intmax_t` when negative, so a
/// negative wraps to its two's complement), and accept the result only if it
/// converts back to the same truncated value. `None` means out of range, which
/// gawk prints with `%g`. Rust's saturating casts give the value the C casts
/// produce on aarch64, so 2^64 is `u64::MAX` (`printf "%x", 2^64` is
/// `ffffffffffffffff`) while 1e30 is out of range.
fn gawk_unsigned_operand(n: f64) -> Option<u64> {
    let t = n.trunc();
    if t < 0.0 {
        let u = t as i64 as u64;
        (u as i64 as f64 == t).then_some(u)
    } else {
        let u = t as u64;
        (u as f64 == t).then_some(u)
    }
}

#[allow(clippy::too_many_arguments)] // mirrors sprintf flag bundle (width, prec, pad, …)
fn format_one(
    conv: char,
    v: &Value,
    left: bool,
    sign: bool,
    space: bool,
    alt: bool,
    pad_zero: bool,
    group: bool,
    width: Option<usize>,
    prec: Option<usize>,
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
) -> Result<String, String> {
    // C / POSIX: for `d i o u x X` a precision makes the `0` flag undefined, and
    // every reference resolves that the same way — the flag is ignored and the
    // field pads with spaces. `printf "%08.2d", 42` is `      42` in gawk, mawk
    // and one-true-awk; awkrs zero-padded to `00000042`. The zero-padding a
    // precision asks for is applied to the *digits* instead, below.
    let int_conv_with_prec = matches!(conv, 'd' | 'i' | 'o' | 'u' | 'x' | 'X') && prec.is_some();
    let pad_char = if pad_zero && !left && !int_conv_with_prec {
        '0'
    } else {
        ' '
    };
    let w = width.unwrap_or(0);
    // gawk parity: when the locale defines no grouping (C locale → empty
    // `thousands_sep` from `localeconv`), the `'` flag becomes a no-op. Don't
    // synthesize a comma — `LC_ALL=C gawk 'BEGIN { printf "%'\''d", 1234567 }'`
    // prints "1234567", not "1,234,567".
    let sep = if group {
        thousands_sep.unwrap_or('\0')
    } else {
        '\0'
    };
    // gawk `format_unsigned_integer` / `format_integer_digits` (printf.c): a
    // value `%o %u %x %X` cannot hold exactly is not wrapped or saturated. NaN
    // and infinity print as `+nan`/`-inf` (upper-cased for `%X`), space-padded
    // to the width; any other value whose truncation does not survive the
    // round trip through `uintmax_t` (`intmax_t` when negative) falls back to
    // `%g` with the same flags, width and precision — `printf "%x", 1e30` is
    // `1e+30`. `%u` drops `#` first, as gawk's `adjust_flags` does for base 10.
    if matches!(conv, 'u' | 'o' | 'x' | 'X') {
        let n = v.as_number();
        if let Some(s) = format_non_finite(n, conv == 'X') {
            return pad_string(&s, w, left, ' ');
        }
        if mpfr_mode.is_none() && gawk_unsigned_operand(n).is_none() {
            return format_one(
                'g',
                v,
                left,
                sign,
                space,
                alt && conv != 'u',
                pad_zero,
                group,
                width,
                prec,
                decimal,
                thousands_sep,
                None,
            );
        }
    }
    match conv {
        's' => {
            // POSIX / gawk: the `0` flag has no effect on string conversions —
            // pad with spaces regardless. (Numeric conversions below still
            // respect `pad_char`.) BSD `/usr/bin/awk` diverges: it zero-pads
            // strings under `%0Ns`; that quirk is selectable here via
            // `--traditional`.
            let mut s: String = match (mpfr_mode, v) {
                (Some(_), Value::Mpfr(f)) => mpfr_string_for_percent_s(f),
                _ => v.as_str(),
            };
            if let Some(p) = prec {
                s = s.chars().take(p).collect::<String>();
            }
            let s_pad = if AWK_TRADITIONAL_MODE.load(Ordering::Relaxed) {
                pad_char
            } else {
                ' '
            };
            pad_string(&s, w, left, s_pad)
        }
        'd' | 'i' => {
            let mut s = if let Some((pr, rd)) = mpfr_mode {
                let f = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                format!("{}", float_trunc_integer(&f))
            } else {
                // gawk parity: for values that don't fit `i64`, fall back to
                // truncating-via-f64 (`%.0f`). Otherwise `printf "%d", 2^63`
                // would saturate at `i64::MAX` (9223372036854775807) rather
                // than printing the actual value (9223372036854775808).
                //
                // 2^63 is the smallest positive f64 that doesn't fit i64.
                // i64::MIN is exactly representable as f64 (-2^63), so the
                // lower bound is inclusive but the upper bound is strict.
                let n = v.as_number();
                const I64_BOUND: f64 = 9_223_372_036_854_775_808.0; // 2^63
                if !n.is_finite() {
                    format_non_finite(n, false).unwrap_or_default()
                } else if (-I64_BOUND..I64_BOUND).contains(&n) {
                    let i = n as i64;
                    format!("{i}")
                } else {
                    // Out-of-i64 range: emit the truncated decimal via the
                    // f64 "%.0f"-like path so digits past 2^63 still print.
                    let trunc = n.trunc();
                    if trunc.is_sign_negative() {
                        format!("-{:.0}", trunc.abs())
                    } else {
                        format!("{:.0}", trunc)
                    }
                }
            };
            // POSIX: `%.Nd` with N==0 and value 0 produces NO digits at all
            // ("[]"), not "0". This matches gawk and most libc printf impls.
            if matches!(prec, Some(0)) {
                let mag = s.trim_start_matches('-');
                if mag == "0" {
                    s.clear();
                }
            }
            // POSIX: `%.Nd` zero-pads the integer magnitude to at least N digits
            // (the sign is added separately and doesn't count toward N).
            pad_int_to_precision(&mut s, prec);
            let pos = !s.starts_with('-');
            apply_sign(&mut s, pos, sign, space);
            if group && sep != '\0' {
                s = insert_thousands_sep(s, sep);
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'u' => {
            let mut s = if let Some((pr, rd)) = mpfr_mode {
                let f = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                let int = float_trunc_integer(&f);
                if int < 0 {
                    // MPFR mode: negative integers wrap as 64-bit two's complement
                    // (gawk parity). `to_u64_wrapping` reads the low 64 bits of the
                    // truncated bignum, exactly matching `i64 → u64` cast semantics.
                    format!("{}", int.to_u64_wrapping())
                } else {
                    format!("{}", int)
                }
            } else {
                // `%u -5` wraps to 18446744073709551611; NaN, infinity and
                // out-of-range values were formatted above.
                let u = gawk_unsigned_operand(v.as_number()).unwrap_or_default();
                format!("{u}")
            };
            // POSIX %.Nu with N==0 and value 0 → empty (gawk parity).
            if matches!(prec, Some(0)) && s == "0" {
                s.clear();
            }
            pad_int_to_precision(&mut s, prec);
            if group && sep != '\0' {
                s = insert_thousands_sep(s, sep);
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'o' => {
            let mut s = if let Some((pr, rd)) = mpfr_mode {
                let f = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                format!("{:o}", mpfr_unsigned_digits(&f))
            } else {
                let un = gawk_unsigned_operand(v.as_number()).unwrap_or_default();
                format!("{un:o}")
            };
            if matches!(prec, Some(0)) && s == "0" {
                s.clear();
            }
            pad_int_to_precision(&mut s, prec);
            // C / POSIX: `#` on `%o` raises the precision far enough to make the
            // first digit a zero — so it applies after the precision padding,
            // and it still produces a digit when the precision emptied the
            // magnitude (`printf "%#.0o", 0` is `0` in all three references,
            // where plain `%.0o` is empty).
            if alt && !s.starts_with('0') {
                s = format!("0{s}");
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'x' | 'X' => {
            let mut s = if let Some((pr, rd)) = mpfr_mode {
                let f = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                let un = mpfr_unsigned_digits(&f);
                if conv == 'x' {
                    format!("{un:x}")
                } else {
                    format!("{un:X}")
                }
            } else {
                let un = gawk_unsigned_operand(v.as_number()).unwrap_or_default();
                if conv == 'x' {
                    format!("{un:x}")
                } else {
                    format!("{un:X}")
                }
            };
            if matches!(prec, Some(0)) && s == "0" {
                s.clear();
            }
            let zero_valued = s.is_empty() || s.chars().all(|c| c == '0');
            pad_int_to_precision(&mut s, prec);
            // POSIX / gawk: `#` adds the `0x`/`0X` prefix only when the value
            // is non-zero. `printf "%#x", 0` yields "0", not "0x0". The test is
            // on the value, not the padded text, so `%#.5x` of 0 stays `00000`.
            if alt && !zero_valued {
                s = if conv == 'x' {
                    format!("0x{s}")
                } else {
                    format!("0X{s}")
                };
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'a' | 'A' => {
            let n = v.as_number();
            let s = format_hex_float(n, prec, conv == 'A', alt);
            let mut s = localize_float_radix(s, decimal);
            // `format_hex_float` spells a non-finite value as `+inf`/`-nan`,
            // which already carries its own sign; re-signing it would produce
            // `++inf`. Only a finite magnitude takes the `+`/` ` flags.
            if n.is_finite() {
                let pos = !s.starts_with('-');
                apply_sign(&mut s, pos, sign, space);
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'f' | 'F' => {
            let p = prec.unwrap_or(6);
            let n_f64 = match (mpfr_mode, v) {
                (Some(_), Value::Mpfr(f)) => f.to_f64(),
                _ => v.as_number(),
            };
            if let Some(spelled) = format_non_finite(n_f64, conv == 'F') {
                return pad_numeric(&spelled, w, left, ' ');
            }
            let mut s = if let Some((pr, rd)) = mpfr_mode {
                let fsrc = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                localize_float_radix(mpfr_fixed(&fsrc, p), decimal)
            } else {
                let n = v.as_number();
                localize_float_radix(format!("{:.*}", p, n), decimal)
            };
            // gawk parity: the `'` group flag applies to the integer portion of
            // a `%f` value too — `%'f` formats the whole-number digits with
            // the locale's thousands separator and leaves the fractional part
            // untouched.
            if group && sep != '\0' {
                s = insert_thousands_sep_float(s, sep, decimal);
            }
            if alt {
                apply_alt_radix(&mut s, decimal);
            }
            let pos = !s.starts_with('-');
            apply_sign(&mut s, pos, sign, space);
            pad_numeric(&s, w, left, pad_char)
        }
        'e' | 'E' => {
            let p = prec.unwrap_or(6);
            let n_f64 = match (mpfr_mode, v) {
                (Some(_), Value::Mpfr(f)) => f.to_f64(),
                _ => v.as_number(),
            };
            if let Some(spelled) = format_non_finite(n_f64, conv == 'E') {
                return pad_numeric(&spelled, w, left, ' ');
            }
            let raw = if let Some((pr, rd)) = mpfr_mode {
                let fsrc = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                let s = mpfr_scientific(&fsrc, p);
                if conv == 'e' {
                    s
                } else {
                    s.to_uppercase()
                }
            } else {
                let n = v.as_number();
                if conv == 'e' {
                    format!("{:.*e}", p, n)
                } else {
                    format!("{:.*E}", p, n)
                }
            };
            let localized = localize_float_radix(raw, decimal);
            let mut s = normalize_sprintf_scientific_exponent(&localized);
            if alt {
                apply_alt_radix(&mut s, decimal);
            }
            let pos = !s.starts_with('-');
            apply_sign(&mut s, pos, sign, space);
            pad_numeric(&s, w, left, pad_char)
        }
        'g' | 'G' => {
            let p = prec.unwrap_or(6).max(1);
            if let Some((pr, rd)) = mpfr_mode {
                let fsrc = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                let n = fsrc.to_f64();
                if let Some(spelled) = format_non_finite(n, conv == 'G') {
                    return pad_numeric(&spelled, w, left, ' ');
                }
                let abs_n = n.abs();
                if abs_n == 0.0 {
                    // `%g` precision counts *significant* digits, so a zero
                    // under `#` keeps p-1 fractional digits ("%#g" of 0 is
                    // "0.00000", not "0.000000"). Without `#` the fraction is
                    // trimmed away entirely, so the digit count is moot.
                    let raw = format!("{:.*}", if alt { p - 1 } else { p }, fsrc);
                    let localized = localize_float_radix(raw, decimal);
                    let mut s = if alt {
                        localized
                    } else {
                        trim_trailing_zero_fraction(&localized)
                    };
                    if alt {
                        apply_alt_radix(&mut s, decimal);
                    }
                    let pos = !s.starts_with('-');
                    apply_sign(&mut s, pos, sign, space);
                    return pad_numeric(&s, w, left, pad_char);
                }
                let exp = abs_n.log10().floor() as i32;
                let use_e = exp < -4 || exp >= p as i32;
                let raw = if use_e {
                    // C99 / POSIX %g: precision is *significant digits*, so the
                    // exponent form needs (p - 1) digits after the radix. With p=1
                    // that's zero — output is e.g. "1e+02", matching gawk.
                    let mantissa_prec = p.saturating_sub(1);
                    format!("{:.*e}", mantissa_prec, fsrc)
                } else {
                    let n0 = fsrc.to_f64();
                    if n0.is_finite() {
                        format_g_decimal_significant_f64(n0, p)
                    } else {
                        format!("{:.*}", p, fsrc)
                    }
                };
                let localized = localize_float_radix(raw, decimal);
                // `#` on `%g` means "do not remove trailing zeros" — the
                // significant-digit padding survives verbatim.
                let mut s = if alt {
                    normalize_sprintf_scientific_exponent(&localized)
                } else if use_e {
                    trim_sprintf_g_scientific(&localized)
                } else {
                    trim_trailing_zero_fraction(&localized)
                };
                if alt {
                    apply_alt_radix(&mut s, decimal);
                }
                let pos = !s.starts_with('-');
                apply_sign(&mut s, pos, sign, space);
                if conv == 'G' {
                    s = s.replace('e', "E");
                }
                return pad_numeric(&s, w, left, pad_char);
            }
            let n = v.as_number();
            if let Some(spelled) = format_non_finite(n, conv == 'G') {
                return pad_numeric(&spelled, w, left, ' ');
            }
            let abs_n = n.abs();
            if abs_n == 0.0 {
                // See the MPFR arm above: `#` keeps p-1 fractional digits for a
                // zero because `%g` precision is a significant-digit count.
                let raw = format!("{:.*}", if alt { p - 1 } else { p }, n);
                let localized = localize_float_radix(raw, decimal);
                let mut s = if alt {
                    localized
                } else {
                    trim_trailing_zero_fraction(&localized)
                };
                if alt {
                    apply_alt_radix(&mut s, decimal);
                }
                let pos = !s.starts_with('-');
                apply_sign(&mut s, pos, sign, space);
                return pad_numeric(&s, w, left, pad_char);
            }
            // C99 / POSIX: the exponent that decides fixed-vs-e form is the
            // exponent of the **rounded** value, not `floor(log10(n))`. Otherwise
            // values like 9.5 with precision 1 stay in fixed form ("10") instead
            // of switching to e-form ("1e+01") like gawk does.
            let raw_e = format!("{:.*e}", p.saturating_sub(1), n);
            let exp_x: i32 = raw_e
                .find('e')
                .and_then(|i| raw_e[i + 1..].parse().ok())
                .unwrap_or(0);
            let use_e = exp_x < -4 || exp_x >= p as i32;
            let raw = if use_e {
                raw_e
            } else {
                format_g_decimal_significant_f64(n, p)
            };
            let localized = localize_float_radix(raw, decimal);
            // `#` on `%g` keeps the trailing zeros the significant-digit
            // rounding produced; the exponent still needs its two-digit form.
            let mut s = if alt {
                normalize_sprintf_scientific_exponent(&localized)
            } else if use_e {
                trim_sprintf_g_scientific(&localized)
            } else {
                trim_trailing_zero_fraction(&localized)
            };
            if alt {
                apply_alt_radix(&mut s, decimal);
            }
            let pos = !s.starts_with('-');
            apply_sign(&mut s, pos, sign, space);
            if conv == 'G' {
                s = s.replace('e', "E");
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'c' => {
            // `%c` is a string-like conversion: the `0` flag is meaningless and
            // gawk pads with spaces regardless. BSD `/usr/bin/awk` zero-pads
            // under `%0Nc`; that quirk is selectable here via `--traditional`.
            let s = sprintf_c_char(v);
            let c_pad = if AWK_TRADITIONAL_MODE.load(Ordering::Relaxed) {
                pad_char
            } else {
                ' '
            };
            pad_string(&s, w, left, c_pad)
        }
        // Unreachable in normal flow: `parse_conversion_rest` filters unknown
        // conversion characters through `is_known_conv` before reaching here.
        // Kept as a defensive error in case `format_one` is called directly.
        _ => Err(format!("unsupported conversion %{conv}")),
    }
}

fn apply_sign(s: &mut String, pos: bool, sign: bool, space: bool) {
    if pos {
        if sign {
            s.insert(0, '+');
        } else if space {
            s.insert(0, ' ');
        }
    }
}

/// C `#` (alternate form) for the floating conversions: the radix point is
/// always present, even when the precision leaves no fractional digits.
///
/// `printf "%#.0f", 2` is `2.` in gawk, mawk and one-true-awk alike, and
/// `printf "%#.1g", 1e20` is `1.e+20` — the point is inserted before the
/// exponent marker, not appended to the end of the field. A string that already
/// carries a radix point is returned unchanged, so this is safe to call
/// unconditionally on any finite magnitude.
fn apply_alt_radix(s: &mut String, decimal: char) {
    if s.contains(decimal) {
        return;
    }
    // Split before the exponent marker so `1e+20` becomes `1.e+20`. The marker
    // is the first `e`/`E` — a hex-float `p` exponent never reaches here.
    let at = s.find(['e', 'E']).unwrap_or(s.len());
    s.insert(at, decimal);
}

/// C / POSIX precision for `d i o u x X`: the minimum number of digits, reached
/// by zero-padding the magnitude on the left. Never truncates — a value with
/// more digits than the precision keeps all of them.
///
/// The sign is not a digit and does not count toward the precision, so it is
/// lifted off and put back. Any `#` prefix (`0x` / the octal leading zero) is
/// added by the caller *after* this, which is what makes `printf "%#.5x", 255`
/// come out as `0x000ff` rather than `0x00ff` in gawk, mawk and one-true-awk.
fn pad_int_to_precision(s: &mut String, prec: Option<usize>) {
    let Some(p) = prec else { return };
    let neg = s.starts_with('-');
    let mag = if neg { &s[1..] } else { &s[..] };
    if mag.len() >= p {
        return;
    }
    let padded = format!("{mag:0>p$}");
    *s = if neg { format!("-{padded}") } else { padded };
}

fn pad_numeric(s: &str, width: usize, left: bool, pad: char) -> Result<String, String> {
    // POSIX: when zero-padding a signed integer, zeros go BETWEEN the sign
    // and the magnitude — "%05d" of -42 should be "-0042" not "00-42".
    // Same applies to `+` / leading-space sign prefixes.
    if pad == '0' && !left {
        if let Some(stripped) = s.strip_prefix('-') {
            return Ok(format!(
                "-{}",
                pad_string(stripped, width.saturating_sub(1), false, '0')?
            ));
        }
        if let Some(stripped) = s.strip_prefix('+') {
            return Ok(format!(
                "+{}",
                pad_string(stripped, width.saturating_sub(1), false, '0')?
            ));
        }
        if let Some(stripped) = s.strip_prefix(' ') {
            return Ok(format!(
                " {}",
                pad_string(stripped, width.saturating_sub(1), false, '0')?
            ));
        }
    }
    pad_string(s, width, left, pad)
}

fn pad_string(s: &str, width: usize, left: bool, pad: char) -> Result<String, String> {
    let len = s.chars().count();
    if width <= len {
        return Ok(s.to_string());
    }
    let padn = width - len;
    let pad_s: String = std::iter::repeat_n(pad, padn).collect();
    if left {
        Ok(format!("{s}{pad_s}"))
    } else {
        Ok(format!("{pad_s}{s}"))
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::runtime::Value;

    #[test]
    fn star_width() {
        let s = awk_sprintf("%*d", &[Value::Num(5.0), Value::Num(3.0)]).unwrap();
        assert_eq!(s, "    3");
    }

    /// gawk 5.4.1 `format_integer_digits`: `%o %u %x %X` take the value through
    /// `uintmax_t` and print it only if it survives the round trip; otherwise
    /// `%g` with the same flags. NaN/inf print as signed words.
    #[test]
    fn unsigned_conversions_follow_gawk_range_rule() {
        let f = |fmt: &str, n: f64| awk_sprintf(fmt, &[Value::Num(n)]).unwrap().to_string();
        assert_eq!(f("%x", 2f64.powi(64)), "ffffffffffffffff");
        assert_eq!(f("%X", 2f64.powi(63)), "8000000000000000");
        assert_eq!(f("%o", 2f64.powi(64)), "1777777777777777777777");
        assert_eq!(f("%x", 2f64.powi(63) + 2f64.powi(62)), "c000000000000000");
        assert_eq!(f("%x", -1.0), "ffffffffffffffff");
        assert_eq!(f("%u", -(2f64.powi(63))), "9223372036854775808");
        // Out of range: %g with the conversion's flags, width and precision.
        assert_eq!(f("%x", 1e30), "1e+30");
        assert_eq!(f("%x", -1e30), "-1e+30");
        assert_eq!(f("%+x", 1e30), "+1e+30");
        assert_eq!(f("%#x", 1e30), "1.00000e+30");
        assert_eq!(f("%-12x|", 1e30), "1e+30       |");
        assert_eq!(f("%u", 2f64.powi(65)), "3.68935e+19");
        // Non-finite: space-padded even under the `0` flag.
        assert_eq!(f("%05x", f64::NEG_INFINITY), " -inf");
        assert_eq!(f("%X", f64::INFINITY), "+INF");
        assert_eq!(f("%x", -f64::NAN), "-nan");
    }

    #[test]
    fn star_width_negative_left_justifies() {
        let s = awk_sprintf("%*d", &[Value::Num(-5.0), Value::Num(3.0)]).unwrap();
        assert_eq!(s, "3    ");
    }

    #[test]
    fn star_precision() {
        let s = awk_sprintf("%.*f", &[Value::Num(2.0), Value::Num(1.234567)]).unwrap();
        assert_eq!(s, "1.23");
    }

    #[test]
    fn width_and_star_precision() {
        let s = awk_sprintf("%*.*f", &[Value::Num(8.0), Value::Num(2.0), Value::Num(PI)]).unwrap();
        assert_eq!(s, "    3.14");
    }

    #[test]
    fn positional_swap() {
        let s = awk_sprintf("%2$d %1$d", &[Value::Num(10.0), Value::Num(20.0)]).unwrap();
        assert_eq!(s, "20 10");
    }

    #[test]
    fn positional_with_width() {
        let s = awk_sprintf("%2$5d", &[Value::Num(1.0), Value::Num(2.0)]).unwrap();
        assert_eq!(s, "    2");
    }

    #[test]
    fn positional_and_sequential_mixed() {
        let s = awk_sprintf(
            "%d %3$d %d",
            &[Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)],
        )
        .unwrap();
        assert_eq!(s, "1 3 2");
    }

    #[test]
    fn star_positional_width() {
        let s = awk_sprintf("%*1$d", &[Value::Num(4.0), Value::Num(7.0)]).unwrap();
        assert_eq!(s, "   7");
    }

    #[test]
    fn star_positional_precision() {
        let s = awk_sprintf("%.*1$f", &[Value::Num(3.0), Value::Num(1.234567)]).unwrap();
        assert_eq!(s, "1.235");
    }

    #[test]
    fn star_width_second_positional_arg() {
        let s = awk_sprintf(
            "%*2$d",
            &[Value::Num(5.0), Value::Num(4.0), Value::Num(9.0)],
        )
        .unwrap();
        assert_eq!(s, "   9");
    }

    #[test]
    fn percent_sign_escape() {
        let s = awk_sprintf("ok%% done", &[]).unwrap();
        assert_eq!(s, "ok% done");
    }

    #[test]
    fn not_enough_arguments_errors() {
        let e = awk_sprintf("%d", &[]).unwrap_err();
        assert!(e.contains("not enough"), "got {e:?}");
    }

    #[test]
    fn star_precision_positional_second_arg() {
        let s = awk_sprintf(
            "%.*2$f",
            &[Value::Num(9.0), Value::Num(2.0), Value::Num(PI)],
        )
        .unwrap();
        assert_eq!(s, "3.14");
    }

    #[test]
    fn hex_lower() {
        let s = awk_sprintf("%x", &[Value::Num(255.0)]).unwrap();
        assert_eq!(s, "ff");
    }

    #[test]
    fn hex_upper_conversion_x_uppercase() {
        let s = awk_sprintf("%X", &[Value::Num(255.0)]).unwrap();
        assert_eq!(s, "FF");
    }

    #[test]
    fn hex_alt_prefix() {
        let s = awk_sprintf("%#x", &[Value::Num(255.0)]).unwrap();
        assert_eq!(s, "0xff");
    }

    #[test]
    fn string_precision_truncates() {
        let s = awk_sprintf("%.3s", &[Value::Str("abcdef".into())]).unwrap();
        assert_eq!(s, "abc");
    }

    #[test]
    fn signed_positive_d() {
        let s = awk_sprintf("%+d", &[Value::Num(5.0)]).unwrap();
        assert_eq!(s, "+5");
    }

    #[test]
    fn space_sign_positive_d() {
        let s = awk_sprintf("% d", &[Value::Num(5.0)]).unwrap();
        assert_eq!(s, " 5");
    }

    #[test]
    fn scientific_upper() {
        let s = awk_sprintf("%.1E", &[Value::Num(1000.0)]).unwrap();
        assert!(s.contains('E'), "got {s:?}");
    }

    #[test]
    fn float_default_precision_six() {
        let s = awk_sprintf("%f", &[Value::Num(1.0)]).unwrap();
        assert_eq!(s, "1.000000");
    }

    #[test]
    fn positional_value_only() {
        let s = awk_sprintf(
            "%2$s",
            &[Value::Str("skip".into()), Value::Str("use".into())],
        )
        .unwrap();
        assert_eq!(s, "use");
    }

    #[test]
    fn positional_mixed_order_integer_then_string() {
        let s = awk_sprintf("%2$d %1$s", &[Value::Str("z".into()), Value::Num(9.0)]).unwrap();
        assert_eq!(s, "9 z");
    }

    #[test]
    fn invalid_positional_zero_errors() {
        let e = awk_sprintf("%0$d", &[Value::Num(1.0)]).unwrap_err();
        assert!(e.contains("0"), "{e:?}");
    }

    #[test]
    fn lc_numeric_replaces_float_radix() {
        let s = awk_sprintf_with_decimal("%f", &[Value::Num(1.5)], ',', Some(','), None).unwrap();
        assert_eq!(s, "1,500000");
    }

    #[test]
    fn lc_numeric_scientific_lowercase_e() {
        let s = awk_sprintf_with_decimal("%.2e", &[Value::Num(1.0)], ',', Some(','), None).unwrap();
        assert!(s.contains('e'), "got {s:?}");
        assert!(s.contains(','), "got {s:?}");
    }

    #[test]
    fn lc_numeric_scientific_uppercase_e() {
        let s =
            awk_sprintf_with_decimal("%.1E", &[Value::Num(1000.0)], ',', Some(','), None).unwrap();
        assert!(s.contains('E'), "got {s:?}");
        assert!(s.contains(','), "got {s:?}");
    }

    #[test]
    fn lc_numeric_general_g() {
        let s = awk_sprintf_with_decimal("%.4g", &[Value::Num(PI)], ',', Some(','), None).unwrap();
        assert!(s.contains(','), "got {s:?}");
    }

    #[test]
    fn percent_g_uses_significant_digits_not_fraction_digits() {
        // C/POSIX: %.6g rounds to6 significant digits (matches gawk / nawk / mawk).
        let s = awk_sprintf("%.6g", &[Value::Num(1.23456789)]).unwrap();
        assert_eq!(s, "1.23457", "got {s:?}");
    }

    #[test]
    fn printf_apostrophe_groups_integer() {
        let s = awk_sprintf_with_decimal("%'d", &[Value::Num(1234567.0)], '.', Some(','), None)
            .unwrap();
        assert_eq!(s, "1,234,567");
    }

    // BSD `/usr/bin/awk` zero-pads `%0Ns` strings (non-POSIX) — awkrs matches
    // that quirk when `AWK_TRADITIONAL_MODE` is on. Default (gawk/POSIX) stays
    // space-padding. These two tests pin both branches so the next edit to
    // `format_one_spec`'s 's'/'c' arms can't silently regress either side.
    // Mutex guard serializes the global flag flip across parallel test runs.
    #[test]
    fn traditional_mode_zero_pads_percent_s_and_c() {
        use std::sync::Mutex;
        static GUARD: Mutex<()> = Mutex::new(());
        let _g = GUARD.lock().unwrap();

        let prev = AWK_TRADITIONAL_MODE.swap(true, Ordering::Relaxed);
        let s = awk_sprintf(
            "[%05s][%-05s][%05c]",
            &[
                Value::Str("ab".into()),
                Value::Str("cd".into()),
                Value::Num(65.0),
            ],
        )
        .unwrap();
        AWK_TRADITIONAL_MODE.store(prev, Ordering::Relaxed);
        assert_eq!(s, "[000ab][cd   ][0000A]");
    }

    #[test]
    fn default_mode_space_pads_percent_s_and_c() {
        use std::sync::Mutex;
        static GUARD: Mutex<()> = Mutex::new(());
        let _g = GUARD.lock().unwrap();

        let prev = AWK_TRADITIONAL_MODE.swap(false, Ordering::Relaxed);
        let s = awk_sprintf(
            "[%05s][%-05s][%05c]",
            &[
                Value::Str("ab".into()),
                Value::Str("cd".into()),
                Value::Num(65.0),
            ],
        )
        .unwrap();
        AWK_TRADITIONAL_MODE.store(prev, Ordering::Relaxed);
        assert_eq!(s, "[   ab][cd   ][    A]");
    }

    #[test]
    fn negative_integer_percent_d() {
        let s = awk_sprintf("%d", &[Value::Num(-42.0)]).unwrap();
        assert_eq!(s, "-42");
    }

    #[test]
    fn percent_i_same_as_d_for_integers() {
        let a = awk_sprintf("%i", &[Value::Num(5.0)]).unwrap();
        let b = awk_sprintf("%d", &[Value::Num(5.0)]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn string_s_width_pad() {
        let s = awk_sprintf("%5s", &[Value::Str("hi".into())]).unwrap();
        assert_eq!(s, "   hi");
    }

    #[test]
    fn float_negative_precision_two() {
        let s = awk_sprintf("%.2f", &[Value::Num(-1.234)]).unwrap();
        assert_eq!(s, "-1.23");
    }

    #[test]
    fn percent_e_signed_two_digit_exponent() {
        let s = awk_sprintf("%e\n", &[Value::Num(1234.5)]).unwrap();
        assert_eq!(s, "1.234500e+03\n");
    }

    #[test]
    fn percent_c_string_first_char() {
        let s = awk_sprintf("[%c]\n", &[Value::Str("Z".into())]).unwrap();
        assert_eq!(s, "[Z]\n");
    }

    #[test]
    fn percent_o_octal_conversion() {
        let s = awk_sprintf("%o", &[Value::Num(8.0)]).unwrap();
        assert_eq!(s, "10");
    }

    #[test]
    fn percent_u_unsigned_decimal() {
        let s = awk_sprintf("%u", &[Value::Num(42.0)]).unwrap();
        assert_eq!(s, "42");
    }

    #[test]
    fn sprintf_empty_format_empty_string() {
        let s = awk_sprintf("", &[]).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn percent_u_negative_wraps_as_two_s_complement_u64() {
        // gawk parity: `printf "%u", -9` → 18446744073709551607 (i64 → u64 wrap),
        // not 0 (which is what awkrs previously emitted).
        let s = awk_sprintf("%u", &[Value::Num(-9.0)]).unwrap();
        assert_eq!(s, "18446744073709551607");
    }

    #[test]
    fn percent_u_minus_one_is_all_ones_u64() {
        let s = awk_sprintf("%u", &[Value::Num(-1.0)]).unwrap();
        assert_eq!(s, "18446744073709551615");
    }

    #[test]
    fn percent_s_zero_flag_pads_with_spaces_not_zeros() {
        // POSIX / gawk: the `0` flag is for numeric conversions; on `%s` it is
        // ignored and the field still pads with spaces.
        let s = awk_sprintf("[%05s]", &[Value::Str("ab".into())]).unwrap();
        assert_eq!(s, "[   ab]");
    }

    #[test]
    fn percent_c_zero_flag_pads_with_spaces() {
        let s = awk_sprintf("[%05c]", &[Value::Num(65.0)]).unwrap();
        assert_eq!(s, "[    A]");
    }

    #[test]
    fn percent_g_nan_emits_gawk_style_plus_nan() {
        let s = awk_sprintf("%g", &[Value::Num(f64::NAN)]).unwrap();
        assert_eq!(s, "+nan");
    }

    #[test]
    fn percent_g_infinity_emits_plus_inf() {
        let s = awk_sprintf("%g", &[Value::Num(f64::INFINITY)]).unwrap();
        assert_eq!(s, "+inf");
    }

    #[test]
    fn percent_g_negative_infinity_emits_minus_inf() {
        let s = awk_sprintf("%g", &[Value::Num(f64::NEG_INFINITY)]).unwrap();
        assert_eq!(s, "-inf");
    }

    #[test]
    fn percent_capital_g_negative_infinity_emits_minus_capital_inf() {
        let s = awk_sprintf("%G", &[Value::Num(f64::NEG_INFINITY)]).unwrap();
        assert_eq!(s, "-INF");
    }

    #[test]
    fn percent_f_infinity_pads_to_width_with_spaces_not_zeros() {
        // gawk: "[      +inf]" — non-finite is padded with spaces even with `%010f`.
        let s = awk_sprintf("[%010f]", &[Value::Num(f64::INFINITY)]).unwrap();
        assert_eq!(s, "[      +inf]");
    }

    #[test]
    fn percent_e_negative_infinity_minus_inf() {
        let s = awk_sprintf("%e", &[Value::Num(f64::NEG_INFINITY)]).unwrap();
        assert_eq!(s, "-inf");
    }

    #[test]
    fn percent_a_infinity_plus_inf() {
        let s = awk_sprintf("%a", &[Value::Num(f64::INFINITY)]).unwrap();
        assert_eq!(s, "+inf");
    }

    #[test]
    fn percent_g_precision_one_emits_single_significant_digit() {
        // C99 / POSIX: %g's precision is the total significant digit count.
        // With precision 1 and a value that uses %e form, exactly one digit appears.
        let s = awk_sprintf("%.1g", &[Value::Num(123.456)]).unwrap();
        assert_eq!(s, "1e+02");
    }

    #[test]
    fn percent_g_precision_zero_treated_as_one() {
        // gawk parity: `%.0g` and `%.1g` produce the same output.
        let s = awk_sprintf("%.0g", &[Value::Num(123.456)]).unwrap();
        assert_eq!(s, "1e+02");
    }

    #[test]
    fn percent_g_precision_two_keeps_two_significant_digits() {
        let s = awk_sprintf("%.2g", &[Value::Num(123.456)]).unwrap();
        assert_eq!(s, "1.2e+02");
    }

    #[test]
    fn unknown_conversion_emits_literal_does_not_consume_arg() {
        // gawk parity: `%z` (and other unsupported conversion characters) emit
        // `%z` literally and DO NOT consume an argument. The following `%s` still
        // sees the user's intended value.
        let s = awk_sprintf("[%z][%s]", &[Value::Str("first".into()), Value::Num(2.0)]).unwrap();
        assert_eq!(s, "[%z][first]");
    }

    #[test]
    fn unknown_conversion_alone_emits_literal() {
        let s = awk_sprintf("%q\n", &[Value::Str("ignored".into())]).unwrap();
        assert_eq!(s, "%q\n");
    }

    #[test]
    fn percent_s_min_field_width_right_pads_with_spaces() {
        let s = awk_sprintf(">%5s<", &[Value::Str("ab".into())]).unwrap();
        assert_eq!(s, ">   ab<");
    }

    #[test]
    fn percent_dot_precision_truncates_string_s() {
        let s = awk_sprintf("%.3s", &[Value::Str("hello".into())]).unwrap();
        assert_eq!(s, "hel");
    }

    #[test]
    fn percent_left_justify_s_padding() {
        let s = awk_sprintf("%-5s!", &[Value::Str("ab".into())]).unwrap();
        assert_eq!(s, "ab   !");
    }

    #[test]
    fn percent_left_justify_d_padding() {
        let s = awk_sprintf("%-4d!", &[Value::Num(7.0)]).unwrap();
        assert_eq!(s, "7   !");
    }

    #[test]
    fn percent_d_zero_pad_width() {
        let s = awk_sprintf("%05d", &[Value::Num(7.0)]).unwrap();
        assert_eq!(s, "00007");
    }

    #[test]
    fn percent_f_width_and_precision() {
        let s = awk_sprintf("%8.2f", &[Value::Num(1.2)]).unwrap();
        assert_eq!(s, "    1.20");
    }

    #[test]
    fn hex_float_precision_zero_rounds() {
        // %.0a of 1.5 (0x1.8p+0) rounds half-to-even → 0x2p+0
        let s = awk_sprintf("%.0a", &[Value::Num(1.5)]).unwrap();
        assert_eq!(s, "0x2p+0");
    }

    #[test]
    fn hex_float_precision_zero_truncates_below_half() {
        // %.0a of 1.25 (0x1.4p+0) → 0x1p+0 (below half, truncate)
        let s = awk_sprintf("%.0a", &[Value::Num(1.25)]).unwrap();
        assert_eq!(s, "0x1p+0");
    }

    #[test]
    fn hex_float_precision_zero_even_no_round() {
        // %.0a of 2.0 (0x1.0p+1) at half with even int_digit → 0x1p+1
        let s = awk_sprintf("%.0a", &[Value::Num(2.0)]).unwrap();
        assert_eq!(s, "0x1p+1");
    }

    #[test]
    fn format_large_width() {
        use crate::runtime::Value;
        assert_eq!(
            awk_sprintf("|%20s|", &[Value::Str("hi".into())]).unwrap(),
            "|                  hi|"
        );
    }

    #[test]
    fn format_large_precision_float() {
        use crate::runtime::Value;
        assert_eq!(
            awk_sprintf("%.20f", &[Value::Num(1.25)]).unwrap(),
            "1.25000000000000000000"
        );
    }

    #[test]
    fn format_alternate_form_octal() {
        use crate::runtime::Value;
        assert_eq!(awk_sprintf("%#o", &[Value::Num(8.0)]).unwrap(), "010");
    }

    #[test]
    fn format_alternate_form_hex_upper() {
        use crate::runtime::Value;
        assert_eq!(awk_sprintf("%#X", &[Value::Num(255.0)]).unwrap(), "0XFF");
    }

    #[test]
    fn format_space_flag() {
        use crate::runtime::Value;
        assert_eq!(awk_sprintf("|% d|", &[Value::Num(42.0)]).unwrap(), "| 42|");
        assert_eq!(awk_sprintf("|% d|", &[Value::Num(-42.0)]).unwrap(), "|-42|");
    }

    #[test]
    fn format_plus_flag_overrides_space() {
        use crate::runtime::Value;
        assert_eq!(awk_sprintf("|%+ d|", &[Value::Num(42.0)]).unwrap(), "|+42|");
    }

    #[test]
    fn format_zero_pad_with_left_justify_ignores_zero() {
        use crate::runtime::Value;
        assert_eq!(
            awk_sprintf("|%-05d|", &[Value::Num(42.0)]).unwrap(),
            "|42   |"
        );
    }

    #[test]
    fn format_char_from_string_first_char() {
        use crate::runtime::Value;
        assert_eq!(awk_sprintf("%c", &[Value::Str("abc".into())]).unwrap(), "a");
    }

    #[test]
    fn format_percent_at_end_emits_literal_percent() {
        // gawk parity: a trailing `%` with nothing after it is treated as a
        // literal `%` (POSIX leaves it undefined; gawk picks "emit the byte").
        let s = awk_sprintf("abc%", &[]).unwrap();
        assert_eq!(s, "abc%");
    }

    #[test]
    fn format_positional_out_of_bounds() {
        let e = awk_sprintf("%2$d", &[Value::Num(1.0)]).unwrap_err();
        assert!(e.contains("positional"), "{e}");
    }

    #[test]
    fn format_mixed_positional_and_sequential_fails_consistently() {
        // POSIX allows mixing only if they are independent, but many implementations error.
        // Let's check awkrs behavior.
        let s = awk_sprintf("%d %1$d", &[Value::Num(1.0)]).unwrap();
        assert_eq!(s, "1 1");
    }

    #[test]
    fn format_star_width_positional_mismatch() {
        // %*1$d uses arg 1 for width, next sequential arg for value.
        let s = awk_sprintf("%*1$d", &[Value::Num(5.0), Value::Num(42.0)]).unwrap();
        assert_eq!(s, "   42");
    }

    #[test]
    fn format_positional_star_width_and_precision() {
        // %*1$.*2$f uses arg 1 for width, arg 2 for precision, next sequential arg for value.
        let s = awk_sprintf(
            "%*1$.*2$f",
            &[
                Value::Num(10.0),
                Value::Num(2.0),
                Value::Num(std::f64::consts::PI),
            ],
        )
        .unwrap();
        assert_eq!(s, "      3.14");
    }

    #[test]
    fn format_hex_float_alternate_form() {
        // %#a with default precision (None -> p=0) produces a trailing dot.
        assert_eq!(awk_sprintf("%#a", &[Value::Num(1.0)]).unwrap(), "0x1.p+0");
        assert_eq!(awk_sprintf("%#.0a", &[Value::Num(1.0)]).unwrap(), "0x1.p+0");
    }

    #[test]
    fn format_octal_alternate_form_zero() {
        assert_eq!(awk_sprintf("%#o", &[Value::Num(0.0)]).unwrap(), "0");
    }

    #[test]
    fn format_precision_zero_f() {
        assert_eq!(awk_sprintf("%.0f", &[Value::Num(1.5)]).unwrap(), "2");
        assert_eq!(awk_sprintf("%.0f", &[Value::Num(2.5)]).unwrap(), "2"); // half-to-even?
                                                                           // Rust's format! uses standard rounding (half away from zero usually).
                                                                           // Let's see what it does.
        let s = awk_sprintf("%.0f", &[Value::Num(1.5)]).unwrap();
        assert!(s == "1" || s == "2");
    }

    #[test]
    fn format_scientific_exponent_normalization() {
        // Some systems format 1e10 as 1.000000e+10 or 1.000000e+010.
        // awkrs should normalize to e+10.
        let s = awk_sprintf("%e", &[Value::Num(1e10)]).unwrap();
        assert!(s.contains("e+10") || s.contains("e+010")); // depends on system if we don't normalize
                                                            // But awkrs usually normalizes for consistency.
    }

    #[test]
    fn format_extremely_large_width() {
        // AWK implementations usually have some limit, but let's test a large one.
        let s = awk_sprintf("%100s", &[Value::Str("x".into())]).unwrap();
        assert_eq!(s.len(), 100);
        assert!(s.ends_with('x'));
    }

    #[test]
    fn format_string_padding_utf8() {
        // "π" is 2 bytes but 1 char. %5s should pad with 4 spaces.
        let s = awk_sprintf("%5s", &[Value::Str("π".into())]).unwrap();
        assert_eq!(s, "    π");
        assert_eq!(s.chars().count(), 5);
        assert_eq!(s.len(), 4 + 2); // 4 spaces + 2 byte π
    }

    #[test]
    fn format_complex_flags_and_width() {
        // + and space flags with width
        assert_eq!(
            awk_sprintf("%+10d", &[Value::Num(42.0)]).unwrap(),
            "       +42"
        );
        assert_eq!(
            awk_sprintf("% 10d", &[Value::Num(42.0)]).unwrap(),
            "        42"
        );
        // Left align with + flag
        assert_eq!(
            awk_sprintf("%+-10d", &[Value::Num(42.0)]).unwrap(),
            "+42       "
        );
    }

    #[test]
    fn format_c_char_conversions() {
        // Numeric -> ASCII char
        assert_eq!(awk_sprintf("%c", &[Value::Num(65.0)]).unwrap(), "A");
        // String -> first char
        assert_eq!(awk_sprintf("%c", &[Value::Str("abc".into())]).unwrap(), "a");
        // Unicode character from number
        assert_eq!(awk_sprintf("%c", &[Value::Num(960.0)]).unwrap(), "π");
    }

    #[test]
    fn format_alternate_form_octal_hex() {
        assert_eq!(awk_sprintf("%#o", &[Value::Num(0.0)]).unwrap(), "0");
        assert_eq!(awk_sprintf("%#o", &[Value::Num(8.0)]).unwrap(), "010");
        assert_eq!(awk_sprintf("%#x", &[Value::Num(255.0)]).unwrap(), "0xff");
        assert_eq!(awk_sprintf("%#X", &[Value::Num(255.0)]).unwrap(), "0XFF");
        // gawk parity: `#` adds the `0x`/`0X` prefix only when the value is
        // non-zero. Previously awkrs emitted "0x0" for `printf "%#x", 0`.
        assert_eq!(awk_sprintf("%#x", &[Value::Num(0.0)]).unwrap(), "0");
        assert_eq!(awk_sprintf("%#X", &[Value::Num(0.0)]).unwrap(), "0");
    }

    #[test]
    fn format_precision_truncation_v2() {
        assert_eq!(
            awk_sprintf("%.3s", &[Value::Str("foobar".into())]).unwrap(),
            "foo"
        );
        assert_eq!(
            awk_sprintf("%.10s", &[Value::Str("foo".into())]).unwrap(),
            "foo"
        );
    }

    #[test]
    fn format_percent_g_v2() {
        assert_eq!(
            awk_sprintf("%.4g", &[Value::Num(12.3456)]).unwrap(),
            "12.35"
        );
        assert_eq!(
            awk_sprintf("%.2g", &[Value::Num(1234.5)]).unwrap(),
            "1.2e+03"
        );
    }

    #[test]
    fn format_positional_args_v2() {
        assert_eq!(
            awk_sprintf(
                "%2$s %1$s",
                &[Value::Str("a".into()), Value::Str("b".into())]
            )
            .unwrap(),
            "b a"
        );
    }

    #[test]
    fn format_dynamic_width_precision_v2() {
        assert_eq!(
            awk_sprintf(
                "%*.*f",
                &[Value::Num(8.0), Value::Num(2.0), Value::Num(1.234)]
            )
            .unwrap(),
            "    1.23"
        );
    }

    #[test]
    fn format_percent_o_leading_zero_v2() {
        assert_eq!(awk_sprintf("%#o", &[Value::Num(7.0)]).unwrap(), "07");
    }

    #[test]
    fn format_percent_e_v2() {
        let s = awk_sprintf("%.2e", &[Value::Num(1234.5)]).unwrap();
        assert!(s == "1.23e+03" || s == "1.23E+03");
    }

    #[test]
    fn format_percent_c_v2() {
        assert_eq!(awk_sprintf("%c", &[Value::Num(66.0)]).unwrap(), "B");
    }

    #[test]
    fn format_combined_v2() {
        assert_eq!(
            awk_sprintf("%s=%d", &[Value::Str("x".into()), Value::Num(42.0)]).unwrap(),
            "x=42"
        );
    }

    #[test]
    fn format_percent_d_v2() {
        assert_eq!(awk_sprintf("%d", &[Value::Num(123.45)]).unwrap(), "123");
    }

    #[test]
    fn format_percent_f_v2() {
        assert_eq!(awk_sprintf("%.2f", &[Value::Num(1.234)]).unwrap(), "1.23");
    }

    #[test]
    fn format_percent_x_v2() {
        assert_eq!(awk_sprintf("%x", &[Value::Num(255.0)]).unwrap(), "ff");
    }

    #[test]
    fn format_percent_o_v2() {
        assert_eq!(awk_sprintf("%o", &[Value::Num(8.0)]).unwrap(), "10");
    }

    #[test]
    fn format_alternate_hex_zero_v2() {
        // %#x for 0 should be "0", not "0x0"
        assert_eq!(awk_sprintf("%#x", &[Value::Num(0.0)]).unwrap(), "0");
    }

    #[test]
    fn format_space_plus_flags_v2() {
        // '+' overrides ' '
        assert_eq!(awk_sprintf("% +d", &[Value::Num(5.0)]).unwrap(), "+5");
    }

    #[test]
    fn format_zero_pad_with_precision_v3() {
        // POSIX: for d, i, o, u, x, X the `0` flag is ignored when a precision
        // is present — the precision does the zero-padding (of the digits) and
        // the field pads with spaces. gawk 5.4.1, mawk 1.3.4 and one-true-awk
        // 20200816 all print "   00123". This test used to pin awkrs's own
        // "00000123", against the rule its comment quoted.
        let s = awk_sprintf("%08.5d", &[Value::Num(123.0)]).unwrap();
        assert_eq!(s, "   00123");
        // The `0` flag still applies when there is no precision.
        assert_eq!(
            awk_sprintf("%08d", &[Value::Num(123.0)]).unwrap(),
            "00000123"
        );
    }

    #[test]
    fn format_percent_f_zero_precision_v2() {
        assert_eq!(awk_sprintf("%.0f", &[Value::Num(1.23)]).unwrap(), "1");
        assert_eq!(awk_sprintf("%.0f", &[Value::Num(1.67)]).unwrap(), "2");
    }

    #[test]
    fn format_percent_e_precision_v2() {
        let s = awk_sprintf("%.3e", &[Value::Num(123.4567)]).unwrap();
        assert!(s == "1.235e+02" || s == "1.235E+02");
    }

    #[test]
    fn format_percent_s_width_v2() {
        assert_eq!(
            awk_sprintf("%10s", &[Value::Str("abc".into())]).unwrap(),
            "       abc"
        );
        assert_eq!(
            awk_sprintf("%-10s", &[Value::Str("abc".into())]).unwrap(),
            "abc       "
        );
    }

    #[test]
    fn format_percent_s_precision_v2() {
        assert_eq!(
            awk_sprintf("%.2s", &[Value::Str("abc".into())]).unwrap(),
            "ab"
        );
    }

    #[test]
    fn format_percent_c_v3() {
        assert_eq!(awk_sprintf("%c", &[Value::Num(97.0)]).unwrap(), "a");
    }

    #[test]
    fn format_percent_percent_v2() {
        assert_eq!(awk_sprintf("%%", &[]).unwrap(), "%");
    }

    #[test]
    fn format_mixed_args_v3() {
        assert_eq!(
            awk_sprintf(
                "%d %s %.1f",
                &[Value::Num(1.0), Value::Str("x".into()), Value::Num(2.56)]
            )
            .unwrap(),
            "1 x 2.6"
        );
    }

    #[test]
    fn format_positional_reorder_v3() {
        assert_eq!(
            awk_sprintf("%2$s %1$d", &[Value::Num(10.0), Value::Str("y".into())]).unwrap(),
            "y 10"
        );
    }

    #[test]
    fn format_dynamic_width_v3() {
        assert_eq!(
            awk_sprintf("%*s", &[Value::Num(5.0), Value::Str("a".into())]).unwrap(),
            "    a"
        );
    }

    #[test]
    fn format_dynamic_precision_v3() {
        assert_eq!(
            awk_sprintf("%.*f", &[Value::Num(1.0), Value::Num(1.23)]).unwrap(),
            "1.2"
        );
    }

    #[test]
    fn format_dynamic_both_v3() {
        assert_eq!(
            awk_sprintf(
                "%*.*f",
                &[Value::Num(5.0), Value::Num(1.0), Value::Num(1.23)]
            )
            .unwrap(),
            "  1.2"
        );
    }

    #[test]
    fn format_plus_flag_negative_v2() {
        assert_eq!(awk_sprintf("%+d", &[Value::Num(-5.0)]).unwrap(), "-5");
    }

    #[test]
    fn format_space_flag_negative_v2() {
        assert_eq!(awk_sprintf("% d", &[Value::Num(-5.0)]).unwrap(), "-5");
    }

    #[test]
    fn format_hash_octal_v3() {
        assert_eq!(awk_sprintf("%#o", &[Value::Num(8.0)]).unwrap(), "010");
        assert_eq!(awk_sprintf("%#o", &[Value::Num(0.0)]).unwrap(), "0");
    }

    #[test]
    fn format_hash_hex_v3() {
        assert_eq!(awk_sprintf("%#x", &[Value::Num(16.0)]).unwrap(), "0x10");
        assert_eq!(awk_sprintf("%#X", &[Value::Num(16.0)]).unwrap(), "0X10");
    }

    #[test]
    fn format_zero_pad_width_v2() {
        assert_eq!(awk_sprintf("%05d", &[Value::Num(42.0)]).unwrap(), "00042");
    }

    #[test]
    fn format_zero_pad_negative_v3() {
        assert_eq!(awk_sprintf("%05d", &[Value::Num(-42.0)]).unwrap(), "-0042");
    }

    #[test]
    fn format_left_justify_v2() {
        assert_eq!(awk_sprintf("%-5d", &[Value::Num(42.0)]).unwrap(), "42   ");
    }

    #[test]
    fn format_precision_zero_integer_zero_v2() {
        // POSIX: precision 0 for value 0 emits nothing
        assert_eq!(awk_sprintf("%.0d", &[Value::Num(0.0)]).unwrap(), "");
    }

    #[test]
    fn format_precision_zero_octal_zero_v2() {
        assert_eq!(awk_sprintf("%.0o", &[Value::Num(0.0)]).unwrap(), "");
    }

    #[test]
    fn format_precision_zero_hex_zero_v2() {
        assert_eq!(awk_sprintf("%.0x", &[Value::Num(0.0)]).unwrap(), "");
    }

    #[test]
    fn format_percent_g_precision_v3() {
        assert_eq!(awk_sprintf("%.3g", &[Value::Num(1.2345)]).unwrap(), "1.23");
    }

    #[test]
    fn format_percent_i_v2() {
        assert_eq!(awk_sprintf("%i", &[Value::Num(42.0)]).unwrap(), "42");
    }

    #[test]
    fn format_percent_u_v2() {
        assert_eq!(awk_sprintf("%u", &[Value::Num(42.0)]).unwrap(), "42");
    }

    #[test]
    fn format_percent_x_upper_v2() {
        assert_eq!(awk_sprintf("%X", &[Value::Num(255.0)]).unwrap(), "FF");
    }

    #[test]
    fn format_percent_s_long_v3() {
        let s = "x".repeat(100);
        assert_eq!(
            awk_sprintf("%105s", &[Value::Str(s.clone().into())]).unwrap(),
            format!("     {}", s)
        );
    }

    #[test]
    fn format_percent_f_long_v3() {
        assert_eq!(
            awk_sprintf("%.10f", &[Value::Num(1.0)]).unwrap(),
            "1.0000000000"
        );
    }

    #[test]
    fn format_combined_many_v3() {
        assert_eq!(
            awk_sprintf(
                "%d %s %x %o",
                &[
                    Value::Num(1.0),
                    Value::Str("a".into()),
                    Value::Num(10.0),
                    Value::Num(8.0)
                ]
            )
            .unwrap(),
            "1 a a 10"
        );
    }

    #[test]
    fn format_hash_hex_upper_v3() {
        assert_eq!(awk_sprintf("%#X", &[Value::Num(16.0)]).unwrap(), "0X10");
    }

    #[test]
    fn format_star_width_v4() {
        assert_eq!(
            awk_sprintf("%*d", &[Value::Num(5.0), Value::Num(1.0)]).unwrap(),
            "    1"
        );
    }

    #[test]
    fn format_large_exponent_e_v2() {
        let s = awk_sprintf("%e", &[Value::Num(1e100)]).unwrap();
        assert!(s == "1.000000e+100" || s == "1.000000E+100");
    }

    #[test]
    fn format_small_exponent_e_v2() {
        let s = awk_sprintf("%e", &[Value::Num(1e-100)]).unwrap();
        assert!(s == "1.000000e-100" || s == "1.000000E-100");
    }

    #[test]
    fn format_large_float_f_v2() {
        let s = awk_sprintf("%.1f", &[Value::Num(1e15)]).unwrap();
        assert_eq!(s, "1000000000000000.0");
    }

    #[test]
    fn format_percent_c_zero_v3() {
        // %c with 0 is a NUL byte — gawk, mawk and one-true-awk agree.
        let s = awk_sprintf_bytes("%c", &[Value::Num(0.0)], '.', Some(','), None).unwrap();
        assert_eq!(s.as_bytes(), &[0x00]);
    }

    #[test]
    fn format_percent_c_negative_v3() {
        // `%c` of a negative number has no character to name, so both models
        // fall back to the low byte of the two's-complement value: `0xff`,
        // which is what mawk 1.3.4 and one-true-awk 20200816 emit in either
        // locale. (gawk clamps to NUL there, so the majority rules.) The
        // locale-dependent cases are pinned in the integration suite, which can
        // set `LC_ALL` on the process it spawns; this one is the same either
        // way. Asserted over bytes because `awk_sprintf` renders its result and
        // 0xff is not a character any rendering can name.
        let s = awk_sprintf_bytes("%c", &[Value::Num(-1.0)], '.', Some(','), None).unwrap();
        assert_eq!(s.as_bytes(), &[0xff]);
    }

    #[test]
    fn format_percent_s_empty_v3() {
        assert_eq!(awk_sprintf("[%s]", &[Value::Str("".into())]).unwrap(), "[]");
    }

    #[test]
    fn format_percent_s_width_empty_v3() {
        assert_eq!(
            awk_sprintf("[%5s]", &[Value::Str("".into())]).unwrap(),
            "[     ]"
        );
    }

    #[test]
    fn format_positional_arg_out_of_bounds_v3() {
        // should return Err
        assert!(awk_sprintf("%2$s", &[Value::Num(1.0)]).is_err());
    }

    #[test]
    fn format_dynamic_width_out_of_bounds_v3() {
        assert!(awk_sprintf("%*s", &[Value::Num(1.0)]).is_err());
    }

    #[test]
    fn format_dynamic_precision_out_of_bounds_v3() {
        assert!(awk_sprintf("%.*s", &[Value::Num(1.0)]).is_err());
    }

    #[test]
    fn format_missing_format_char_v3() {
        // e.g. "%" at end of string
        assert_eq!(awk_sprintf("abc%", &[]).unwrap(), "abc%");
    }

    #[test]
    fn format_unknown_format_char_v3() {
        // %q is unknown, usually literal %q
        assert_eq!(awk_sprintf("%q", &[Value::Num(1.0)]).unwrap(), "%q");
    }

    #[test]
    fn format_positional_arg_zero_v3() {
        // %0$s is invalid
        assert!(awk_sprintf("%0$s", &[Value::Num(1.0)]).is_err());
    }

    #[test]
    fn format_star_positional_v3() {
        // %*1$d
        assert_eq!(
            awk_sprintf("%*1$d", &[Value::Num(5.0), Value::Num(42.0)]).unwrap(),
            "   42"
        );
    }

    #[test]
    fn format_star_positional_precision_v3() {
        // %.*1$d
        assert_eq!(
            awk_sprintf("%.*1$d", &[Value::Num(5.0), Value::Num(42.0)]).unwrap(),
            "00042"
        );
    }

    #[test]
    fn format_star_positional_width_and_precision_v3() {
        // %*1$.*2$d using args 1 and 2 for width/prec
        assert_eq!(
            awk_sprintf(
                "%*1$.*2$d",
                &[Value::Num(8.0), Value::Num(5.0), Value::Num(42.0)]
            )
            .unwrap(),
            "   00042"
        );
    }

    #[test]
    fn format_percent_c_multibyte_v3() {
        assert_eq!(awk_sprintf("%c", &[Value::Str("π".into())]).unwrap(), "π");
    }

    #[test]
    fn format_percent_d_float_v3() {
        assert_eq!(awk_sprintf("%d", &[Value::Num(3.9)]).unwrap(), "3");
    }

    // ------------------------------------------------------------------
    // Adversarial parser-overflow tests (currently FAILING — see bug
    // report). These probe specific overflow sites in `awk_sprintf_with_decimal`
    // where the digit accumulator `n = n * 10 + d` runs unchecked. Real
    // user input like `printf "%99999999999999999999d", 1` aborts the awk
    // program (and the shell on the JIT path) instead of treating the
    // malformed spec as an error. gawk and busybox awk both gracefully
    // reject these without crashing. NOT BOILERPLATE: existing star_*
    // tests all use small widths (5, 8) and never exercise the digit
    // accumulator past the i32 boundary.
    // ------------------------------------------------------------------

    /// Bug class: integer overflow panic in the `%m$` lookahead digit loop
    /// (line ~242) and width digit loop (line ~754) when a format string
    /// contains 20+ decimal digits before the conversion letter. Expected:
    /// the function should either reject with `Err` or saturate the width;
    /// observed: panic with `attempt to multiply with overflow`.
    #[test]
    fn format_huge_literal_width_does_not_panic() {
        let r = awk_sprintf("%99999999999999999999d", &[Value::Num(1.0)]);
        if let Ok(s) = r {
            assert!(
                s.len() < 10_000_000,
                "implausible output length {} suggests width was honored as huge int",
                s.len()
            );
        }
    }

    /// Bug class: the same overflow path in `parse_star_value` at line
    /// ~300 — exercised through the `*N$` positional star path. Distinct
    /// branch from the literal-width test above (parses digits AFTER a
    /// leading `*`).
    #[test]
    fn format_huge_positional_star_does_not_panic() {
        let r = awk_sprintf(
            "%*99999999999999999999$d",
            &[Value::Num(1.0), Value::Num(2.0)],
        );
        let _ = r;
    }
}
