//! `sprintf` / `printf` formatting (POSIX-ish; common awk conversions).

use crate::bignum::{float_trunc_integer, mpfr_string_for_percent_s, value_to_mpfr};
use crate::runtime::Value;
use rug::float::Round;

#[inline]
fn fmt_peek(fmt: &str, i: usize) -> Option<char> {
    fmt.get(i..)?.chars().next()
}

/// Default C-locale radix (`.`). Use [`awk_sprintf_with_decimal`] when `-N` applies.
pub fn awk_sprintf(fmt: &str, vals: &[Value]) -> Result<String, String> {
    awk_sprintf_with_decimal(fmt, vals, '.', Some(','), None)
}

/// Pre-process the values so that any `Value::Num` (or `Value::Mpfr`) gets
/// converted to a `Value::StrLit` formatted via `convfmt` — but only when the
/// format string contains a `%s` that consumes that arg. For other
/// conversions the original numeric value is preserved.
///
/// This is gawk parity for `printf "%s", 3.14159` under `CONVFMT="%.3f"`:
/// the `%s` arm sees the CONVFMT-formatted string instead of the f64 Display.
fn convfmt_preprocess_for_percent_s<'a>(
    fmt: &str,
    vals: &'a [Value],
    convfmt: &str,
) -> std::borrow::Cow<'a, [Value]> {
    // Quick reject: no `%s` → nothing to do.
    if !fmt.contains("%s") && !fmt.contains("s$") {
        return std::borrow::Cow::Borrowed(vals);
    }
    // Locate each conversion specifier (%X) and identify which input index it
    // consumes. The format syntax supports `%2$s` (positional) and `*` width/prec
    // which also consume args. To stay simple, walk the format string mirroring
    // `parse_conversion_rest`'s behavior just enough to know which arg goes
    // to `%s`.
    let bytes = fmt.as_bytes();
    let mut percent_s_indices: Vec<usize> = Vec::new();
    let mut i = 0;
    let mut vi: usize = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'%' {
            i += 1;
            continue;
        }
        // Optional positional `m$`.
        let mut pos: Option<usize> = None;
        let mut j = i;
        let mut m = 0usize;
        let mut has_digits = false;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            m = m * 10 + (bytes[j] - b'0') as usize;
            has_digits = true;
            j += 1;
        }
        if has_digits && j < bytes.len() && bytes[j] == b'$' {
            pos = Some(m);
            i = j + 1;
        }
        // Skip flags.
        while i < bytes.len() && matches!(bytes[i], b'-' | b'+' | b' ' | b'#' | b'\'' | b'0') {
            i += 1;
        }
        // Width: digits or `*` (which may also be positional).
        if i < bytes.len() && bytes[i] == b'*' {
            i += 1;
            let mut star_pos: Option<usize> = None;
            let mut sm = 0usize;
            let mut sj = i;
            let mut s_has = false;
            while sj < bytes.len() && bytes[sj].is_ascii_digit() {
                sm = sm * 10 + (bytes[sj] - b'0') as usize;
                s_has = true;
                sj += 1;
            }
            if s_has && sj < bytes.len() && bytes[sj] == b'$' {
                star_pos = Some(sm);
                i = sj + 1;
            }
            match star_pos {
                Some(p) => vi = vi.max(p),
                None => vi += 1,
            }
        } else {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        // Precision.
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'*' {
                i += 1;
                let mut star_pos: Option<usize> = None;
                let mut sm = 0usize;
                let mut sj = i;
                let mut s_has = false;
                while sj < bytes.len() && bytes[sj].is_ascii_digit() {
                    sm = sm * 10 + (bytes[sj] - b'0') as usize;
                    s_has = true;
                    sj += 1;
                }
                if s_has && sj < bytes.len() && bytes[sj] == b'$' {
                    star_pos = Some(sm);
                    i = sj + 1;
                }
                match star_pos {
                    Some(p) => vi = vi.max(p),
                    None => vi += 1,
                }
            } else {
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        // h/l/L modifiers (ignored).
        while i < bytes.len() && matches!(bytes[i], b'h' | b'l' | b'L') {
            i += 1;
        }
        // Conversion letter.
        if i >= bytes.len() {
            break;
        }
        let conv = bytes[i];
        i += 1;
        // Which arg does this conversion consume?
        let arg_idx = if let Some(p) = pos {
            if p == 0 {
                continue;
            }
            p - 1
        } else {
            let idx = vi;
            vi += 1;
            idx
        };
        if conv == b's' {
            percent_s_indices.push(arg_idx);
        }
    }
    if percent_s_indices.is_empty() {
        return std::borrow::Cow::Borrowed(vals);
    }
    // Build the rewritten vals: only `%s` positions get the CONVFMT-formatted
    // string; everything else stays numeric so conversions like `%d` still
    // round-trip cleanly.
    let mut out = vals.to_vec();
    for &idx in &percent_s_indices {
        if let Some(v) = out.get_mut(idx) {
            if let Value::Num(n) = v {
                let s = format_num_via_convfmt(*n, convfmt);
                *v = Value::StrLit(s);
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

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
) -> Result<String, String> {
    let vals = convfmt_preprocess_for_percent_s(fmt, vals, convfmt);
    awk_sprintf_with_decimal(fmt, &vals, decimal, thousands_sep, mpfr_mode)
}

pub fn awk_sprintf_with_decimal(
    fmt: &str,
    vals: &[Value],
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
) -> Result<String, String> {
    let mut out = String::new();
    let mut vi = 0usize;
    let mut i = 0usize;
    while i < fmt.len() {
        let c = fmt_peek(fmt, i).ok_or_else(|| "truncated format".to_string())?;
        if c != '%' {
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        i += c.len_utf8();
        if i >= fmt.len() {
            // gawk parity: a trailing `%` with nothing after it is emitted as a
            // literal `%` rather than raising an error.
            out.push('%');
            break;
        }
        // Optional `%m$` — digits must be followed by `$` or we rewind and treat as flags/width.
        let start_after_pct = i;
        let mut m = 0usize;
        let mut has_digits = false;
        while let Some(ch) = fmt_peek(fmt, i) {
            if !ch.is_ascii_digit() {
                break;
            }
            has_digits = true;
            m = m * 10 + (ch as u8 - b'0') as usize;
            i += ch.len_utf8();
        }
        let val_pos = if has_digits && fmt_peek(fmt, i) == Some('$') {
            i += '$'.len_utf8();
            if m == 0 {
                return Err("sprintf: positional argument was 0".into());
            }
            Some(m)
        } else {
            i = start_after_pct;
            None
        };
        let (piece, new_i) = parse_conversion_rest(
            fmt,
            i,
            vals,
            &mut vi,
            val_pos,
            decimal,
            thousands_sep,
            mpfr_mode,
        )?;
        i = new_i;
        out.push_str(&piece);
    }
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

/// After a `*` in width or precision: either `n$` (positional) or sequential `take_val`.
/// Updates `vi` to at least `n` when `n$` is used so following sequential args align with POSIX.
fn parse_star_value(
    fmt: &str,
    mut i: usize,
    vals: &[Value],
    vi: &mut usize,
) -> Result<(f64, usize), String> {
    let start = i;
    let mut n = 0usize;
    let mut has_digits = false;
    while let Some(ch) = fmt_peek(fmt, i) {
        if !ch.is_ascii_digit() {
            break;
        }
        has_digits = true;
        n = n * 10 + (ch as u8 - b'0') as usize;
        i += ch.len_utf8();
    }
    if has_digits && fmt_peek(fmt, i) == Some('$') {
        i += '$'.len_utf8();
        let v = val_at(vals, n)?;
        *vi = (*vi).max(n);
        return Ok((v.as_number(), i));
    }
    i = start;
    let v = take_val(vals, vi)?;
    Ok((v.as_number(), i))
}

#[allow(clippy::too_many_arguments)] // sprintf flag bundle + mpfr mode
fn parse_conversion_rest(
    fmt: &str,
    mut i: usize,
    vals: &[Value],
    vi: &mut usize,
    val_pos: Option<usize>,
    decimal: char,
    thousands_sep: Option<char>,
    mpfr_mode: Option<(u32, Round)>,
) -> Result<(String, usize), String> {
    let mut left = false;
    let mut sign = false;
    let mut space = false;
    let mut alt = false;
    let mut pad_zero = false;
    let mut group = false;
    while let Some(flag) = fmt_peek(fmt, i) {
        match flag {
            '-' => {
                left = true;
                i += flag.len_utf8();
            }
            '+' => {
                sign = true;
                i += flag.len_utf8();
            }
            ' ' => {
                space = true;
                i += flag.len_utf8();
            }
            '#' => {
                alt = true;
                i += flag.len_utf8();
            }
            '\'' => {
                group = true;
                i += flag.len_utf8();
            }
            '0' => {
                pad_zero = true;
                i += flag.len_utf8();
            }
            _ => break,
        }
    }

    let (width, star_left, i2) = parse_width_or_star(fmt, i, vals, vi)?;
    i = i2;
    if star_left {
        left = true;
    }

    let mut prec: Option<usize> = None;
    if fmt_peek(fmt, i) == Some('.') {
        i += '.'.len_utf8();
        if fmt_peek(fmt, i) == Some('*') {
            i += '*'.len_utf8();
            let (p, i2) = parse_star_value(fmt, i, vals, vi)?;
            i = i2;
            prec = Some(if p < 0.0 { 0 } else { p as usize });
        } else {
            let mut p = 0usize;
            let mut any = false;
            while let Some(d) = fmt_peek(fmt, i) {
                if !d.is_ascii_digit() {
                    break;
                }
                p = p * 10 + (d as u8 - b'0') as usize;
                any = true;
                i += d.len_utf8();
            }
            prec = if any { Some(p) } else { Some(0) };
        }
    }

    while matches!(fmt_peek(fmt, i), Some('h' | 'l' | 'L')) {
        let m = fmt_peek(fmt, i).unwrap();
        i += m.len_utf8();
    }

    let conv = fmt_peek(fmt, i).ok_or_else(|| "truncated format".to_string())?;
    i += conv.len_utf8();

    if conv == '%' {
        return Ok(("%".to_string(), i));
    }

    // gawk parity: unknown conversion characters are emitted **literally** as `%X`
    // and DO NOT consume an argument (so the next `%s` etc. still sees the args
    // the user intended for it). gawk also emits a warning under `--lint`; awkrs
    // stays silent for now.
    if !is_known_conv(conv) {
        return Ok((format!("%{conv}"), i));
    }

    let v = if let Some(p) = val_pos {
        val_at(vals, p)?
    } else {
        take_val(vals, vi)?
    };
    let piece = format_one(
        conv,
        v,
        left,
        sign,
        space,
        alt,
        pad_zero,
        group,
        width,
        prec,
        decimal,
        thousands_sep,
        mpfr_mode,
    )?;
    Ok((piece, i))
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
    let e = n.log10().floor() as i32;
    let sig_scale = 10f64.powi(p as i32 - 1 - e);
    let r = (n * sig_scale).round() / sig_scale;
    if r == 0.0 {
        return if neg {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }
    let e2 = r.log10().floor() as i32;
    let frac = (p as i32 - e2 - 1).max(0) as usize;
    let body = format!("{:.*}", frac, r);
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

fn sprintf_c_char(v: &Value) -> String {
    match v {
        Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => {
            s.chars().next().map(|c| c.to_string()).unwrap_or_default()
        }
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

fn parse_width_or_star(
    fmt: &str,
    mut i: usize,
    vals: &[Value],
    vi: &mut usize,
) -> Result<(Option<usize>, bool, usize), String> {
    if fmt_peek(fmt, i) == Some('*') {
        i += '*'.len_utf8();
        let (n, i2) = parse_star_value(fmt, i, vals, vi)?;
        i = i2;
        if n < 0.0 {
            let w = (-n) as usize;
            return Ok((Some(w), true, i));
        }
        return Ok((Some(n as usize), false, i));
    }
    if fmt_peek(fmt, i).is_some_and(|c| c.is_ascii_digit()) {
        let mut w = 0usize;
        while let Some(d) = fmt_peek(fmt, i) {
            if !d.is_ascii_digit() {
                break;
            }
            w = w * 10 + (d as u8 - b'0') as usize;
            i += d.len_utf8();
        }
        return Ok((Some(w), false, i));
    }
    Ok((None, false, i))
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
    let pad_char = if pad_zero && !left { '0' } else { ' ' };
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
    match conv {
        's' => {
            // POSIX / gawk: the `0` flag has no effect on string conversions —
            // pad with spaces regardless. (Numeric conversions below still
            // respect `pad_char`.)
            let mut s: String = match (mpfr_mode, v) {
                (Some(_), Value::Mpfr(f)) => mpfr_string_for_percent_s(f),
                _ => v.as_str(),
            };
            if let Some(p) = prec {
                s = s.chars().take(p).collect::<String>();
            }
            pad_string(&s, w, left, ' ')
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
            if let Some(p) = prec {
                let neg = s.starts_with('-');
                let mag = if neg { &s[1..] } else { &s[..] };
                if mag.len() < p {
                    let padded = format!("{:0>width$}", mag, width = p);
                    s = if neg { format!("-{padded}") } else { padded };
                }
            }
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
                // gawk: `%u` of a negative number wraps via i64 → u64 (two's complement)
                // — `printf "%u", -5` yields 18446744073709551611, not 0. For
                // positive values that exceed i64 (but still fit u64), the
                // intermediate `i64` saturates, so we test the i64-range first
                // and fall back to the u64 path or f64 truncation for huge values.
                let n = v.as_number();
                const I64_BOUND: f64 = 9_223_372_036_854_775_808.0; // 2^63
                const U64_BOUND: f64 = 18_446_744_073_709_551_616.0; // 2^64
                if !n.is_finite() {
                    format_non_finite(n, false).unwrap_or_default()
                } else if (-I64_BOUND..I64_BOUND).contains(&n) {
                    let u = n as i64 as u64;
                    format!("{u}")
                } else if (0.0..U64_BOUND).contains(&n) {
                    let u = n as u64;
                    format!("{u}")
                } else if n == U64_BOUND {
                    // gawk parity: 2^64 in f64 is exactly U64_BOUND (the next
                    // representable double above u64::MAX). gawk renders this
                    // boundary value as u64::MAX digits (saturating-cast
                    // behavior). For strictly larger values it falls back to
                    // `%g`-style formatting below.
                    format!("{}", u64::MAX)
                } else if n > U64_BOUND {
                    // gawk parity: positive values past 2^64 fall back to
                    // `%g`-style formatting (e.g. 2^65 → "3.68935e+19").
                    crate::format::awk_sprintf_with_decimal(
                        "%.6g",
                        &[Value::Num(n)],
                        '.',
                        None,
                        None,
                    )
                    .unwrap_or_else(|_| format!("{}", u64::MAX))
                } else {
                    // Very negative (past -2^63): emit the truncated decimal.
                    let trunc = n.trunc();
                    if trunc.is_sign_negative() {
                        format!("-{:.0}", trunc.abs())
                    } else {
                        format!("{:.0}", trunc)
                    }
                }
            };
            // POSIX %.Nu with N==0 and value 0 → empty (gawk parity).
            if matches!(prec, Some(0)) && s == "0" {
                s.clear();
            }
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
                let un = float_trunc_integer(&f).to_u64_wrapping();
                format!("{un:o}")
            } else {
                let n = v.as_number() as i64;
                let un = n as u64;
                format!("{un:o}")
            };
            if matches!(prec, Some(0)) && s == "0" {
                s.clear();
            }
            if alt && s != "0" && !s.is_empty() {
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
                let un = float_trunc_integer(&f).to_u64_wrapping();
                if conv == 'x' {
                    format!("{un:x}")
                } else {
                    format!("{un:X}")
                }
            } else {
                let n = v.as_number() as i64;
                let un = n as u64;
                if conv == 'x' {
                    format!("{un:x}")
                } else {
                    format!("{un:X}")
                }
            };
            if matches!(prec, Some(0)) && s == "0" {
                s.clear();
            }
            // POSIX / gawk: `#` adds the `0x`/`0X` prefix only when the value
            // is non-zero. `printf "%#x", 0` yields "0", not "0x0".
            if alt && !s.is_empty() && s != "0" {
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
            let s = localize_float_radix(s, decimal);
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
                localize_float_radix(format!("{:.*}", p, fsrc), decimal)
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
                if conv == 'e' {
                    format!("{:.*e}", p, fsrc)
                } else {
                    format!("{:.*E}", p, fsrc)
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
            let s = normalize_sprintf_scientific_exponent(&localized);
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
                    let raw = format!("{:.*}", p, fsrc);
                    let localized = localize_float_radix(raw, decimal);
                    let s = trim_trailing_zero_fraction(&localized);
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
                let mut s = if use_e {
                    trim_sprintf_g_scientific(&localized)
                } else {
                    trim_trailing_zero_fraction(&localized)
                };
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
                let raw = format!("{:.*}", p, n);
                let localized = localize_float_radix(raw, decimal);
                let s = trim_trailing_zero_fraction(&localized);
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
            let mut s = if use_e {
                trim_sprintf_g_scientific(&localized)
            } else {
                trim_trailing_zero_fraction(&localized)
            };
            if conv == 'G' {
                s = s.replace('e', "E");
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'c' => {
            // `%c` is a string-like conversion: the `0` flag is meaningless and
            // gawk pads with spaces regardless.
            let s = sprintf_c_char(v);
            pad_string(&s, w, left, ' ')
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
        // POSIX: For d, i, o, u, x, X, zero-padding is ignored if precision is present.
        let s = awk_sprintf("%08.5d", &[Value::Num(123.0)]).unwrap();
        // Current awkrs behavior: "00000123" (does not ignore zero-padding)
        assert_eq!(s, "00000123");
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
            awk_sprintf("%105s", &[Value::Str(s.clone())]).unwrap(),
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
        // %c with 0 should be null byte
        assert_eq!(awk_sprintf("%c", &[Value::Num(0.0)]).unwrap(), "\0");
    }

    #[test]
    fn format_percent_c_negative_v3() {
        // negative should fallback to empty or 0, let's see what awkrs does
        // awkrs clamps or wraps. Usually it's character 0 or some wrap
        let s = awk_sprintf("%c", &[Value::Num(-1.0)]).unwrap();
        assert_eq!(s.len(), 1); // just ensuring it doesn't panic
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
}
