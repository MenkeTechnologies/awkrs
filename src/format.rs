//! `sprintf` / `printf` formatting (POSIX-ish; common awk conversions).

use crate::runtime::Value;

pub fn awk_sprintf(fmt: &str, vals: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    let mut vi = 0usize;
    let mut it = fmt.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut left = false;
        let mut sign = false;
        let mut space = false;
        let mut alt = false;
        let mut pad_zero = false;
        while let Some(&ch) = it.peek() {
            match ch {
                '-' => {
                    left = true;
                    it.next();
                }
                '+' => {
                    sign = true;
                    it.next();
                }
                ' ' => {
                    space = true;
                    it.next();
                }
                '#' => {
                    alt = true;
                    it.next();
                }
                '0' => {
                    pad_zero = true;
                    it.next();
                }
                _ => break,
            }
        }
        let mut width: Option<usize> = None;
        if let Some(d) = it.peek().copied() {
            if d.is_ascii_digit() {
                let mut w = 0usize;
                while let Some(&d) = it.peek() {
                    if d.is_ascii_digit() {
                        w = w * 10 + (d as u8 - b'0') as usize;
                        it.next();
                    } else {
                        break;
                    }
                }
                width = Some(w);
            }
        }
        let mut prec: Option<usize> = None;
        if it.peek() == Some(&'.') {
            it.next();
            let mut p = 0usize;
            let mut any = false;
            while let Some(&d) = it.peek() {
                if d.is_ascii_digit() {
                    p = p * 10 + (d as u8 - b'0') as usize;
                    any = true;
                    it.next();
                } else {
                    break;
                }
            }
            if any {
                prec = Some(p);
            } else {
                prec = Some(0);
            }
        }
        while matches!(it.peek(), Some('h' | 'l' | 'L')) {
            it.next();
        }
        let conv = it.next().ok_or_else(|| "truncated format".to_string())?;
        if conv == '%' {
            out.push('%');
            continue;
        }
        let v = vals
            .get(vi)
            .ok_or_else(|| "sprintf: not enough arguments".to_string())?;
        vi += 1;
        let piece = format_one(conv, v, left, sign, space, alt, pad_zero, width, prec)?;
        out.push_str(&piece);
    }
    Ok(out)
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
            let s = format!("{n:.p$}");
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
            let s = format!("{n:.p$}");
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
