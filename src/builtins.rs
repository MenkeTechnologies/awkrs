//! awk builtins: gsub, sub, match, string helpers.

use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use regex::Regex;

/// awk `gsub(ere, repl [, target])` — global replace. `target` defaults to `$0`.
/// Replacement supports `&` (whole match) and `\\`/`&` escapes (subset).
pub fn gsub(
    rt: &mut Runtime,
    re_pat: &str,
    repl: &str,
    target: Option<&mut String>,
) -> Result<f64> {
    let re = Regex::new(re_pat).map_err(|e| Error::Runtime(e.to_string()))?;
    let n = if let Some(t) = target {
        let (new_s, c) = replace_all_awk(&re, t.as_str(), repl)?;
        *t = new_s;
        c
    } else {
        let cur = rt.record.clone();
        let (new_s, c) = replace_all_awk(&re, &cur, repl)?;
        apply_record_string(rt, &new_s);
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
    let re = Regex::new(re_pat).map_err(|e| Error::Runtime(e.to_string()))?;
    let n = if let Some(t) = target {
        if let Some(m) = re.find(t.as_str()) {
            let piece = expand_repl(repl, m.as_str());
            let mut out = String::new();
            out.push_str(&t[..m.start()]);
            out.push_str(&piece);
            out.push_str(&t[m.end()..]);
            *t = out;
            1.0
        } else {
            0.0
        }
    } else {
        let cur = rt.record.clone();
        if let Some(m) = re.find(&cur) {
            let piece = expand_repl(repl, m.as_str());
            let mut out = String::new();
            out.push_str(&cur[..m.start()]);
            out.push_str(&piece);
            out.push_str(&cur[m.end()..]);
            apply_record_string(rt, &out);
            1.0
        } else {
            0.0
        }
    };
    Ok(n)
}

/// `match(s, ere [, arr])` — returns 0-based start index in awk as **1-based RSTART**, sets RSTART, RLENGTH.
pub fn match_fn(rt: &mut Runtime, s: &str, re_pat: &str, arr_name: Option<&str>) -> Result<f64> {
    let re = Regex::new(re_pat).map_err(|e| Error::Runtime(e.to_string()))?;
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

fn replace_all_awk(re: &Regex, s: &str, repl: &str) -> Result<(String, usize)> {
    let mut count = 0usize;
    let mut out = String::new();
    let mut last = 0;
    for m in re.find_iter(s) {
        count += 1;
        out.push_str(&s[last..m.start()]);
        out.push_str(&expand_repl(repl, m.as_str()));
        last = m.end();
    }
    out.push_str(&s[last..]);
    Ok((out, count))
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

fn apply_record_string(rt: &mut Runtime, s: &str) {
    let fs = rt
        .vars
        .get("FS")
        .map(|v| v.as_str())
        .unwrap_or_else(|| " ".into());
    rt.set_field_sep_split(&fs, s);
}
