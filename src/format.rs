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
            return Err("truncated format".into());
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
    loop {
        let Some(flag) = fmt_peek(fmt, i) else {
            break;
        };
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
    if n.is_nan() {
        return if upper { "NAN".into() } else { "nan".into() };
    }
    if n.is_infinite() {
        return if n < 0.0 {
            if upper {
                "-INF".into()
            } else {
                "-inf".into()
            }
        } else {
            if upper {
                "INF".into()
            } else {
                "inf".into()
            }
        };
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
    let sep = if group {
        thousands_sep.unwrap_or(',')
    } else {
        '\0'
    };
    match conv {
        's' => {
            let mut s: String = match (mpfr_mode, v) {
                (Some(_), Value::Mpfr(f)) => mpfr_string_for_percent_s(f),
                _ => v.as_str(),
            };
            if let Some(p) = prec {
                s = s.chars().take(p).collect::<String>();
            }
            pad_string(&s, w, left, pad_char)
        }
        'd' | 'i' => {
            let mut s = if let Some((pr, rd)) = mpfr_mode {
                let f = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                format!("{}", float_trunc_integer(&f))
            } else {
                let n = v.as_number() as i64;
                format!("{n}")
            };
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
                    "0".to_string()
                } else {
                    format!("{}", int)
                }
            } else {
                let n = v.as_number().max(0.0) as u64;
                format!("{n}")
            };
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
            if alt && s != "0" {
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
            if alt {
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
            let s = if let Some((pr, rd)) = mpfr_mode {
                let fsrc = match v {
                    Value::Mpfr(f) => f.clone(),
                    _ => value_to_mpfr(v, pr, rd),
                };
                localize_float_radix(format!("{:.*}", p, fsrc), decimal)
            } else {
                let n = v.as_number();
                localize_float_radix(format!("{:.*}", p, n), decimal)
            };
            pad_numeric(&s, w, left, pad_char)
        }
        'e' | 'E' => {
            let p = prec.unwrap_or(6);
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
                if !n.is_finite() {
                    return Err("sprintf: non-finite value for %g".into());
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
                    let mantissa_prec = p.saturating_sub(1).max(1);
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
            if !n.is_finite() {
                return Err("sprintf: non-finite value for %g".into());
            }
            let abs_n = n.abs();
            if abs_n == 0.0 {
                let raw = format!("{:.*}", p, n);
                let localized = localize_float_radix(raw, decimal);
                let s = trim_trailing_zero_fraction(&localized);
                return pad_numeric(&s, w, left, pad_char);
            }
            let exp = abs_n.log10().floor() as i32;
            let use_e = exp < -4 || exp >= p as i32;
            let raw = if use_e {
                let mantissa_prec = p.saturating_sub(1).max(1);
                format!("{:.*e}", mantissa_prec, n)
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
            let s = sprintf_c_char(v);
            pad_string(&s, w, left, pad_char)
        }
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
    fn percent_u_negative_truncates_to_zero() {
        let s = awk_sprintf("%u", &[Value::Num(-9.0)]).unwrap();
        assert_eq!(s, "0");
    }

    #[test]
    fn percent_g_rejects_nan() {
        let e = awk_sprintf("%g", &[Value::Num(f64::NAN)]).unwrap_err();
        assert!(e.contains("non-finite"), "{e}");
    }

    #[test]
    fn percent_g_rejects_infinity() {
        let e = awk_sprintf("%g", &[Value::Num(f64::INFINITY)]).unwrap_err();
        assert!(e.contains("non-finite"), "{e}");
    }

    #[test]
    fn unsupported_conversion_errors() {
        let e = awk_sprintf("%z", &[Value::Num(1.0)]).unwrap_err();
        assert!(e.contains("unsupported"), "{e}");
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
}
