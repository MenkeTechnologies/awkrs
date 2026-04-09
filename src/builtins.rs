//! awk builtins: gsub, sub, match, string helpers, math, and time (gawk-style).

use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use chrono::{Local, LocalResult, NaiveDate, TimeZone, Utc};
use regex::Regex;

/// Check if a regex pattern is a plain literal (no metacharacters).
pub(crate) fn is_literal_pattern(pat: &str) -> bool {
    !pat.bytes().any(|b| {
        matches!(
            b,
            b'.' | b'*'
                | b'+'
                | b'?'
                | b'['
                | b']'
                | b'('
                | b')'
                | b'{'
                | b'}'
                | b'|'
                | b'^'
                | b'$'
                | b'\\'
        )
    })
}

/// Literal `gsub` fast path: plain pattern and replacement without `&` / `\` escapes.
#[inline]
pub(crate) fn gsub_literal_eligible(re_pat: &str, repl: &str) -> bool {
    is_literal_pattern(re_pat) && !repl.contains('&') && !repl.contains('\\')
}

/// True when `needle` does not occur in `hay` (same semantics as `!hay.contains(needle)` for `gsub`).
fn literal_substring_absent(rt: &mut Runtime, needle: &str, hay: &str) -> bool {
    if needle.is_empty() {
        return !hay.contains(needle);
    }
    rt.literal_substring_finder(needle)
        .find(hay.as_bytes())
        .is_none()
}

/// Literal global replace — `memmem::Finder` (cached on `rt`) for repeated scans over the same needle.
fn literal_replace_all(s: &str, needle: &str, repl: &str, rt: &mut Runtime) -> (String, usize) {
    if needle.is_empty() {
        return (s.to_string(), 0);
    }
    let finder = rt.literal_substring_finder(needle);
    let hay = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    let mut last = 0usize;
    let mut off = 0usize;
    while off < hay.len() {
        let Some(rel) = finder.find(&hay[off..]) else {
            break;
        };
        let abs = off + rel;
        out.push_str(&s[last..abs]);
        out.push_str(repl);
        count += 1;
        last = abs + needle.len();
        off = last;
    }
    out.push_str(&s[last..]);
    (out, count)
}

/// awk `gsub(ere, repl [, target])` — global replace. `target` defaults to `$0`.
/// Replacement supports `&` (whole match) and `\\`/`&` escapes (subset).
pub fn gsub(
    rt: &mut Runtime,
    re_pat: &str,
    repl: &str,
    target: Option<&mut String>,
) -> Result<f64> {
    let repl_has_special = repl.contains('&') || repl.contains('\\');
    // Fast path: literal pattern + literal replacement → pure string replacement, no regex.
    let use_literal = is_literal_pattern(re_pat) && !repl_has_special;

    let n = if let Some(t) = target {
        if use_literal {
            if literal_substring_absent(rt, re_pat, t) {
                0
            } else {
                let (new_s, c) = literal_replace_all(t.as_str(), re_pat, repl, rt);
                *t = new_s;
                c
            }
        } else {
            rt.ensure_regex(re_pat).map_err(Error::Runtime)?;
            let re = rt.regex_ref(re_pat);
            if !re.is_match(t.as_str()) {
                0
            } else {
                let (new_s, c) = replace_all_awk(re, t.as_str(), repl, repl_has_special);
                *t = new_s;
                c
            }
        }
    } else {
        // Replace `$0` in one step — do not restore the old record only to overwrite it again.
        let cur = std::mem::take(&mut rt.record);
        let (new_s, c) = if use_literal {
            if literal_substring_absent(rt, re_pat, &cur) {
                rt.record = cur;
                return Ok(0.0);
            }
            literal_replace_all(&cur, re_pat, repl, rt)
        } else {
            rt.ensure_regex(re_pat).map_err(Error::Runtime)?;
            let re = rt.regex_ref(re_pat);
            if !re.is_match(&cur) {
                rt.record = cur;
                return Ok(0.0);
            }
            replace_all_awk(re, &cur, repl, repl_has_special)
        };
        drop(cur);
        let fs = rt
            .vars
            .get("FS")
            .map(|v| v.as_str())
            .unwrap_or_else(|| " ".into());
        rt.set_field_sep_split_owned(&fs, new_s);
        c
    };
    Ok(n as f64)
}

/// awk `sub(ere, repl [, target])` — first match only.
pub fn sub_fn(
    rt: &mut Runtime,
    re_pat: &str,
    repl: &str,
    target: Option<&mut String>,
) -> Result<f64> {
    rt.ensure_regex(re_pat).map_err(Error::Runtime)?;
    let repl_has_special = repl.contains('&') || repl.contains('\\');
    let n = if let Some(t) = target {
        if let Some(m) = rt.regex_ref(re_pat).find(t.as_str()) {
            let piece = if repl_has_special {
                expand_repl(repl, m.as_str())
            } else {
                repl.to_string()
            };
            let mut out = String::with_capacity(t.len() + piece.len());
            out.push_str(&t[..m.start()]);
            out.push_str(&piece);
            out.push_str(&t[m.end()..]);
            *t = out;
            1.0
        } else {
            0.0
        }
    } else {
        let cur = std::mem::take(&mut rt.record);
        if let Some(m) = rt.regex_ref(re_pat).find(&cur) {
            let piece = if repl_has_special {
                expand_repl(repl, m.as_str())
            } else {
                repl.to_string()
            };
            let mut out = String::with_capacity(cur.len() + piece.len());
            out.push_str(&cur[..m.start()]);
            out.push_str(&piece);
            out.push_str(&cur[m.end()..]);
            drop(cur);
            let fs = rt
                .vars
                .get("FS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".into());
            rt.set_field_sep_split_owned(&fs, out);
            1.0
        } else {
            rt.record = cur;
            0.0
        }
    };
    Ok(n)
}

/// `match(s, ere [, arr])` — returns 0-based start index in awk as **1-based RSTART**, sets RSTART, RLENGTH.
pub fn match_fn(rt: &mut Runtime, s: &str, re_pat: &str, arr_name: Option<&str>) -> Result<f64> {
    rt.ensure_regex(re_pat).map_err(Error::Runtime)?;
    let re = rt.regex_ref(re_pat).clone();
    if let Some(m) = re.find(s) {
        let rstart = (m.start() + 1) as f64;
        let rlength = m.len() as f64;
        rt.vars.insert("RSTART".into(), Value::Num(rstart));
        rt.vars.insert("RLENGTH".into(), Value::Num(rlength));
        if let Some(a) = arr_name {
            rt.array_delete(a, None);
            if let Some(caps) = re.captures(s) {
                // awk: a[1]..a[n] are parenthesized subexpressions (1-based).
                for i in 1..caps.len() {
                    let key = format!("{i}");
                    let val = caps
                        .get(i)
                        .map(|x| x.as_str().to_string())
                        .unwrap_or_default();
                    rt.array_set(a, key, Value::Str(val));
                }
            }
        }
        Ok(rstart)
    } else {
        rt.vars.insert("RSTART".into(), Value::Num(0.0));
        rt.vars.insert("RLENGTH".into(), Value::Num(-1.0));
        if let Some(a) = arr_name {
            rt.array_delete(a, None);
        }
        Ok(0.0)
    }
}

fn replace_all_awk(re: &Regex, s: &str, repl: &str, repl_has_special: bool) -> (String, usize) {
    let mut count = 0usize;
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for m in re.find_iter(s) {
        count += 1;
        out.push_str(&s[last..m.start()]);
        if repl_has_special {
            out.push_str(&expand_repl(repl, m.as_str()));
        } else {
            out.push_str(repl);
        }
        last = m.end();
    }
    out.push_str(&s[last..]);
    (out, count)
}

fn expand_repl(repl: &str, matched: &str) -> String {
    let mut out = String::new();
    let mut chars = repl.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            out.push_str(matched);
        } else if c == '\\' {
            match chars.peek() {
                Some('&') => {
                    chars.next();
                    out.push('&');
                }
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                }
                Some(x) => {
                    let x = *x;
                    chars.next();
                    out.push(x);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `patsplit(string, array [, fieldpat [, seps ]])` — split `string` into `array` using successive
/// matches of `fieldpat`, or `FPAT` when omitted. Empty `FPAT` uses `[^[:space:]]+`.
/// When `seps` is set, `seps[i]` holds the text between `array[i]` and `array[i+1]` (1-based keys).
pub fn patsplit(
    rt: &mut Runtime,
    s: &str,
    arr_name: &str,
    fieldpat: Option<&str>,
    seps_name: Option<&str>,
) -> Result<f64> {
    let fp_owned = match fieldpat {
        Some(s) => s.to_string(),
        None => rt
            .get_global_var("FPAT")
            .map(|v| v.as_str())
            .unwrap_or_default(),
    };
    let fp = if fp_owned.is_empty() {
        "[^[:space:]]+"
    } else {
        fp_owned.as_str()
    };
    let re = Regex::new(fp).map_err(|e| Error::Runtime(e.to_string()))?;
    let matches: Vec<regex::Match> = re.find_iter(s).collect();
    let n = matches.len();

    rt.array_delete(arr_name, None);
    for (i, m) in matches.iter().enumerate() {
        rt.array_set(
            arr_name,
            format!("{}", i + 1),
            Value::Str(m.as_str().to_string()),
        );
    }

    if let Some(sep_arr) = seps_name {
        rt.array_delete(sep_arr, None);
        for i in 1..n {
            let prev = &matches[i - 1];
            let curr = &matches[i];
            let sep = &s[prev.end()..curr.start()];
            rt.array_set(sep_arr, format!("{i}"), Value::Str(sep.to_string()));
        }
    }

    Ok(n as f64)
}

/// Seconds since the Unix epoch (same idea as POSIX `awk` / gawk `systime()`).
pub fn awk_systime() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// gawk-style `strftime([format [, timestamp[, utc]]])`.
pub fn awk_strftime(args: &[Value]) -> std::result::Result<Value, String> {
    let (fmt, ts, utc) = match args.len() {
        0 => ("%c".to_string(), awk_systime(), false),
        1 => (args[0].as_str(), awk_systime(), false),
        2 => (args[0].as_str(), args[1].as_number(), false),
        3 => (
            args[0].as_str(),
            args[1].as_number(),
            args[2].as_number() != 0.0,
        ),
        _ => return Err("strftime: expected 0 to 3 arguments".into()),
    };
    let secs = ts.floor() as i64;
    let nsec = ((ts - secs as f64) * 1e9).round().max(0.0).min(1e9 - 1.0) as u32;
    let out = if utc {
        Utc.timestamp_opt(secs, nsec)
            .single()
            .ok_or_else(|| "strftime: timestamp out of range".to_string())?
            .format(&fmt)
            .to_string()
    } else {
        Local
            .timestamp_opt(secs, nsec)
            .single()
            .ok_or_else(|| "strftime: timestamp out of range".to_string())?
            .format(&fmt)
            .to_string()
    };
    Ok(Value::Str(out))
}

/// gawk-style `mktime(datespec)` — `"YYYY MM DD HH MM SS"` (whitespace-separated).
pub fn awk_mktime(s: &str) -> f64 {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 6 {
        return -1.0;
    }
    let y: i32 = match parts[0].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let mo: u32 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let d: u32 = match parts[2].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let h: u32 = match parts[3].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let mi: u32 = match parts[4].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let se: u32 = match parts[5].parse() {
        Ok(v) => v,
        Err(_) => return -1.0,
    };
    let naive = match NaiveDate::from_ymd_opt(y, mo, d) {
        Some(date) => match date.and_hms_opt(h, mi, se) {
            Some(n) => n,
            None => return -1.0,
        },
        None => return -1.0,
    };
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.timestamp() as f64,
        LocalResult::Ambiguous(_, _) | LocalResult::None => -1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::{gsub, gsub_literal_eligible, is_literal_pattern, match_fn, patsplit, sub_fn};
    use crate::runtime::{Runtime, Value};

    fn rt_with_fs() -> Runtime {
        let mut rt = Runtime::new();
        rt.vars.insert("FS".into(), Value::Str(" ".into()));
        rt
    }

    #[test]
    fn gsub_literal_on_record_replaces_and_resplits() {
        let mut rt = rt_with_fs();
        rt.record = "foofoo".into();
        let n = gsub(&mut rt, "foo", "bar", None).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.record, "barbar");
    }

    #[test]
    fn gsub_regex_with_amp_replacement() {
        let mut rt = rt_with_fs();
        rt.record = "ab".into();
        let n = gsub(&mut rt, "a", "X&Y", None).unwrap();
        assert_eq!(n, 1.0);
        assert_eq!(rt.record, "XaYb");
    }

    #[test]
    fn sub_first_match_only() {
        let mut rt = rt_with_fs();
        rt.record = "aaa".into();
        let n = sub_fn(&mut rt, "a", "b", None).unwrap();
        assert_eq!(n, 1.0);
        assert_eq!(rt.record, "baa");
    }

    #[test]
    fn match_sets_rstart_rlength_on_hit() {
        let mut rt = Runtime::new();
        let n = match_fn(&mut rt, "foo123bar", "[0-9]+", None).unwrap();
        assert_eq!(n, 4.0);
        assert_eq!(rt.vars.get("RSTART").unwrap().as_number(), 4.0);
        assert_eq!(rt.vars.get("RLENGTH").unwrap().as_number(), 3.0);
    }

    #[test]
    fn match_sets_rstart_zero_on_miss() {
        let mut rt = Runtime::new();
        let n = match_fn(&mut rt, "abc", "[0-9]+", None).unwrap();
        assert_eq!(n, 0.0);
        assert_eq!(rt.vars.get("RSTART").unwrap().as_number(), 0.0);
        assert_eq!(rt.vars.get("RLENGTH").unwrap().as_number(), -1.0);
    }

    #[test]
    fn patsplit_fills_array() {
        let mut rt = Runtime::new();
        let n = patsplit(&mut rt, "x y z", "parts", Some("[a-z]+"), None).unwrap();
        assert_eq!(n, 3.0);
        assert_eq!(rt.array_get("parts", "1").as_str(), "x");
        assert_eq!(rt.array_get("parts", "2").as_str(), "y");
        assert_eq!(rt.array_get("parts", "3").as_str(), "z");
    }

    #[test]
    fn is_literal_pattern_accepts_plain_text() {
        assert!(is_literal_pattern("hello"));
    }

    #[test]
    fn is_literal_pattern_rejects_regex_metachar() {
        assert!(!is_literal_pattern("a.c"));
    }

    #[test]
    fn gsub_literal_eligible_rejects_ampersand_in_replacement() {
        assert!(!gsub_literal_eligible("x", "a&b"));
    }

    #[test]
    fn gsub_literal_eligible_accepts_simple_pair() {
        assert!(gsub_literal_eligible("needle", "repl"));
    }
}
