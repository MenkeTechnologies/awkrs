//! `sprintf` / `printf` formatting (POSIX-ish; common awk conversions).

use crate::runtime::Value;

pub fn awk_sprintf(fmt: &str, vals: &[Value]) -> Result<String, String> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut vi = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            return Err("truncated format".into());
        }
        // Optional `%m$` — digits must be followed by `$` or we rewind and treat as flags/width.
        let start_after_pct = i;
        let mut m = 0usize;
        let mut has_digits = false;
        while i < chars.len() && chars[i].is_ascii_digit() {
            has_digits = true;
            m = m * 10 + (chars[i] as u8 - b'0') as usize;
            i += 1;
        }
        let val_pos = if has_digits && i < chars.len() && chars[i] == '$' {
            i += 1;
            if m == 0 {
                return Err("sprintf: positional argument was 0".into());
            }
            Some(m)
        } else {
            i = start_after_pct;
            None
        };
        let (piece, new_i) = parse_conversion_rest(&chars, i, vals, &mut vi, val_pos)?;
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
    chars: &[char],
    mut i: usize,
    vals: &[Value],
    vi: &mut usize,
) -> Result<(f64, usize), String> {
    let start = i;
    let mut n = 0usize;
    let mut has_digits = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        has_digits = true;
        n = n * 10 + (chars[i] as u8 - b'0') as usize;
        i += 1;
    }
    if has_digits && i < chars.len() && chars[i] == '$' {
        i += 1;
        let v = val_at(vals, n)?;
        *vi = (*vi).max(n);
        return Ok((v.as_number(), i));
    }
    i = start;
    let v = take_val(vals, vi)?;
    Ok((v.as_number(), i))
}

fn parse_conversion_rest(
    chars: &[char],
    mut i: usize,
    vals: &[Value],
    vi: &mut usize,
    val_pos: Option<usize>,
) -> Result<(String, usize), String> {
    let mut left = false;
    let mut sign = false;
    let mut space = false;
    let mut alt = false;
    let mut pad_zero = false;
    while i < chars.len() {
        match chars[i] {
            '-' => {
                left = true;
                i += 1;
            }
            '+' => {
                sign = true;
                i += 1;
            }
            ' ' => {
                space = true;
                i += 1;
            }
            '#' => {
                alt = true;
                i += 1;
            }
            '0' => {
                pad_zero = true;
                i += 1;
            }
            _ => break,
        }
    }

    let (width, star_left, i2) = parse_width_or_star(chars, i, vals, vi)?;
    i = i2;
    if star_left {
        left = true;
    }

    let mut prec: Option<usize> = None;
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        if i < chars.len() && chars[i] == '*' {
            i += 1;
            let (p, i2) = parse_star_value(chars, i, vals, vi)?;
            i = i2;
            prec = Some(if p < 0.0 { 0 } else { p as usize });
        } else {
            let mut p = 0usize;
            let mut any = false;
            while i < chars.len() {
                let d = chars[i];
                if d.is_ascii_digit() {
                    p = p * 10 + (d as u8 - b'0') as usize;
                    any = true;
                    i += 1;
                } else {
                    break;
                }
            }
            prec = if any { Some(p) } else { Some(0) };
        }
    }

    while i < chars.len() && matches!(chars[i], 'h' | 'l' | 'L') {
        i += 1;
    }

    let conv = chars
        .get(i)
        .copied()
        .ok_or_else(|| "truncated format".to_string())?;
    i += 1;

    if conv == '%' {
        return Ok(("%".to_string(), i));
    }

    let v = if let Some(p) = val_pos {
        val_at(vals, p)?
    } else {
        take_val(vals, vi)?
    };
    let piece = format_one(conv, v, left, sign, space, alt, pad_zero, width, prec)?;
    Ok((piece, i))
}

fn parse_width_or_star(
    chars: &[char],
    mut i: usize,
    vals: &[Value],
    vi: &mut usize,
) -> Result<(Option<usize>, bool, usize), String> {
    if i < chars.len() && chars[i] == '*' {
        i += 1;
        let (n, i2) = parse_star_value(chars, i, vals, vi)?;
        i = i2;
        if n < 0.0 {
            let w = (-n) as usize;
            return Ok((Some(w), true, i));
        }
        return Ok((Some(n as usize), false, i));
    }
    if i < chars.len() && chars[i].is_ascii_digit() {
        let mut w = 0usize;
        while i < chars.len() {
            let d = chars[i];
            if d.is_ascii_digit() {
                w = w * 10 + (d as u8 - b'0') as usize;
                i += 1;
            } else {
                break;
            }
        }
        return Ok((Some(w), false, i));
    }
    Ok((None, false, i))
}

fn format_one(
    conv: char,
    v: &Value,
    left: bool,
    sign: bool,
    space: bool,
    alt: bool,
    pad_zero: bool,
    width: Option<usize>,
    prec: Option<usize>,
) -> Result<String, String> {
    let pad_char = if pad_zero && !left { '0' } else { ' ' };
    let w = width.unwrap_or(0);
    match conv {
        's' => {
            let mut s = v.as_str();
            if let Some(p) = prec {
                s = s.chars().take(p).collect::<String>();
            } else {
                s = s.to_string();
            }
            pad_string(&s, w, left, pad_char)
        }
        'd' | 'i' => {
            let n = v.as_number() as i64;
            let mut s = format!("{n}");
            apply_sign(&mut s, n >= 0, sign, space);
            pad_numeric(&s, w, left, pad_char)
        }
        'u' => {
            let n = v.as_number().max(0.0) as u64;
            let s = format!("{n}");
            pad_numeric(&s, w, left, pad_char)
        }
        'o' => {
            let n = v.as_number() as i64;
            let un = if n < 0 {
                (n as u64) & 0xffff_ffff_ffff_ffff
            } else {
                n as u64
            };
            let mut s = format!("{un:o}");
            if alt && s != "0" {
                s = format!("0{s}");
            }
            pad_numeric(&s, w, left, pad_char)
        }
        'x' | 'X' => {
            let n = v.as_number() as i64;
            let un = if n < 0 {
                (n as u64) & 0xffff_ffff_ffff_ffff
            } else {
                n as u64
            };
            let mut s = if conv == 'x' {
                format!("{un:x}")
            } else {
                format!("{un:X}")
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
        'f' | 'F' => {
            let n = v.as_number();
            let p = prec.unwrap_or(6);
            let s = format!("{:.*}", p, n);
            pad_numeric(&s, w, left, pad_char)
        }
        'e' | 'E' => {
            let n = v.as_number();
            let p = prec.unwrap_or(6);
            let s = if conv == 'e' {
                format!("{:.*e}", p, n)
            } else {
                format!("{:.*E}", p, n)
            };
            pad_numeric(&s, w, left, pad_char)
        }
        'g' | 'G' => {
            let n = v.as_number();
            let p = prec.unwrap_or(6);
            let s = format!("{:.*}", p, n);
            pad_numeric(&s, w, left, pad_char)
        }
        'c' => {
            let n = v.as_number() as u32;
            let ch = char::from_u32(n).unwrap_or('\u{fffd}');
            let s = ch.to_string();
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
    let pad_s: String = std::iter::repeat(pad).take(padn).collect();
    if left {
        Ok(format!("{s}{pad_s}"))
    } else {
        Ok(format!("{pad_s}{s}"))
    }
}

#[cfg(test)]
mod tests {
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
        let s = awk_sprintf(
            "%*.*f",
            &[Value::Num(8.0), Value::Num(2.0), Value::Num(3.14159)],
        )
        .unwrap();
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
}
