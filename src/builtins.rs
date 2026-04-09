//! awk builtins: gsub, sub, match, string helpers.

use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use regex::Regex;

/// Check if a regex pattern is a plain literal (no metacharacters).
fn is_literal_pattern(pat: &str) -> bool {
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

/// Literal string gsub — uses stdlib `match_indices` for SIMD-optimized search.
fn literal_replace_all(s: &str, needle: &str, repl: &str) -> (String, usize) {
    if needle.is_empty() {
        return (s.to_string(), 0);
    }
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    let mut last = 0;
    for (start, _) in s.match_indices(needle) {
        out.push_str(&s[last..start]);
        out.push_str(repl);
        count += 1;
        last = start + needle.len();
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
            if !t.contains(re_pat) {
                0
            } else {
                let (new_s, c) = literal_replace_all(t.as_str(), re_pat, repl);
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
            if !cur.contains(re_pat) {
                rt.record = cur;
                return Ok(0.0);
            }
            literal_replace_all(&cur, re_pat, repl)
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
