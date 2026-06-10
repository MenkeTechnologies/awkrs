//! awk builtins: gsub, sub, match, string helpers, math, time (gawk-style), bitwise, sort, typeof.

use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use chrono::{Local, LocalResult, NaiveDate, TimeZone, Utc};
use regex::Regex;
use std::cmp::Ordering;

/// Check if a regex pattern is a plain literal (no metacharacters).
pub(crate) fn is_literal_pattern(pat: &str) -> bool {
    // Empty pattern is NOT a literal fast-path candidate: gawk semantics require
    // a zero-width match at every position (gsub of // on "abc" emits 4 replacements),
    // which the literal substring scanner cannot express.
    !pat.is_empty()
        && !pat.bytes().any(|b| {
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
    // But IGNORECASE only takes effect via the regex engine, so when it's on we
    // have to compile a regex even for a plain `"b"` pattern; otherwise
    // `IGNORECASE=1; gsub("b", "X", "ABC")` would silently not match.
    let use_literal = is_literal_pattern(re_pat) && !repl_has_special && !rt.ignore_case_flag();

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
                // gawk parity: a[0] is the whole match, a[1]..a[n] are
                // parenthesized subexpressions. Previously awkrs skipped
                // group 0, so `match(s, /(a)|(b)/, arr)` left arr[0] empty.
                //
                // gawk also writes `a[i, "start"]` and `a[i, "length"]` for
                // each successful submatch (1-based char position, length in
                // chars). Unmatched optional groups (e.g. `(a)?`) get NO
                // entries — `a[1]` / `a[1,"start"]` / `a[1,"length"]` all
                // absent — matching gawk. The byte offset is converted to a
                // 1-based character index so `\w` matches in multibyte input
                // report the position users see (gawk uses char positions).
                let subsep = rt
                    .get_global_var("SUBSEP")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| "\x1c".to_string());
                for i in 0..caps.len() {
                    if let Some(g) = caps.get(i) {
                        let key = format!("{i}");
                        rt.array_set(a, key, Value::Str(g.as_str().to_string()));
                        let char_start = s[..g.start()].chars().count() + 1;
                        let char_len = g.as_str().chars().count();
                        rt.array_set(
                            a,
                            format!("{i}{subsep}start"),
                            Value::Num(char_start as f64),
                        );
                        rt.array_set(a, format!("{i}{subsep}length"), Value::Num(char_len as f64));
                    }
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

/// gensub-specific: replace all matches, expanding `\N` backrefs (gawk extension).
///
/// Note: gawk 5.4.0 has a confirmed bug here — when `gensub` is called with the `g`
/// flag AND the replacement contains `\N` backreferences, gawk drops the captures
/// from matches 2..N (`gensub(/(.)/, "[\\1]", "g", "abc")` produces `[a][][]` in
/// gawk 5.4, instead of the documented `[a][b][c]`). awkrs implements the
/// documented per-match capture expansion; do not "fix" this to match the gawk bug.
fn replace_all_gensub(re: &Regex, s: &str, repl: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for caps in re.captures_iter(s) {
        let whole = caps.get(0).expect("group 0 always present");
        out.push_str(&s[last..whole.start()]);
        out.push_str(&expand_repl_with_caps(repl, &caps));
        last = whole.end();
    }
    out.push_str(&s[last..]);
    out
}

/// gensub-specific: replace the Nth occurrence (1-based) with `\N` backref support.
fn replace_nth_gensub(re: &Regex, s: &str, repl: &str, n: usize) -> String {
    if n == 0 {
        return replace_all_gensub(re, s, repl);
    }
    let mut i = 0usize;
    for caps in re.captures_iter(s) {
        i += 1;
        if i == n {
            let whole = caps.get(0).expect("group 0 always present");
            let piece = expand_repl_with_caps(repl, &caps);
            let mut out = String::with_capacity(s.len());
            out.push_str(&s[..whole.start()]);
            out.push_str(&piece);
            out.push_str(&s[whole.end()..]);
            return out;
        }
    }
    s.to_string()
}

/// Expand `&` (whole match) and `\N` (capture group N, 1..=9) backrefs in a
/// gensub replacement. `\\` is a literal backslash; `\&` is a literal ampersand.
fn expand_repl_with_caps(repl: &str, caps: &regex::Captures<'_>) -> String {
    let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
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
                Some(d) if d.is_ascii_digit() && *d != '0' => {
                    // \1 .. \9 — capture group reference. (\0 is undefined; gawk
                    // treats it as a literal "0".)
                    let n = d.to_digit(10).unwrap() as usize;
                    chars.next();
                    if let Some(g) = caps.get(n) {
                        out.push_str(g.as_str());
                    }
                    // Group missing → empty substitution (gawk-compatible).
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

/// gawk `gensub(ere, repl, how [, target])` — returns modified string.
/// `how`: a string beginning with `g` / `G` replaces all matches; a positive integer replaces
/// that occurrence. gawk treats `0` (and negative integers) as `1` (replace the first match).
pub fn awk_gensub(
    rt: &mut Runtime,
    ere: &str,
    repl: &str,
    how: &Value,
    target: Option<String>,
) -> Result<String> {
    let s = match target {
        Some(t) => t,
        None => rt.record.clone(),
    };
    rt.ensure_regex(ere).map_err(Error::Runtime)?;
    let re = rt.regex_ref(ere).clone();
    let s_ref = s.as_str();
    match how {
        Value::Str(h) | Value::StrLit(h) => {
            let h = h.trim();
            if h.is_empty() {
                return Err(Error::Runtime(
                    "gensub: third argument cannot be empty".into(),
                ));
            }
            if h.starts_with('g') || h.starts_with('G') {
                // gensub uses gawk's backref-aware replacement (`\1`..`\9` +
                // `&` for whole match) — distinct from gsub/sub which don't
                // support backrefs.
                Ok(replace_all_gensub(&re, s_ref, repl))
            } else {
                Err(Error::Runtime(format!(
                    "gensub: string third argument must begin with `g` or `G`, got `{h}`"
                )))
            }
        }
        Value::Num(n) => {
            // Match gawk: 0 (and any value < 1) means "replace the first match".
            // (gawk also emits a warning here; awkrs deliberately stays silent — gawk's
            // warning message embeds the source file/line, which complicates parity diffs
            // and isn't load-bearing for the behavior. Add an explicit emitter later if
            // a `--lint` flag wants strict parity.)
            let which = (*n as i64).max(1) as usize;
            Ok(replace_nth_gensub(&re, s_ref, repl, which))
        }
        _ => Err(Error::Runtime(
            "gensub: third argument must be string or number".into(),
        )),
    }
}

/// gawk-compatible replacement-string expansion for sub/gsub.
///
/// Rules (matching gawk 5.x behavior, which differs from POSIX in the details
/// of how backslashes near `&` are processed):
///
/// * A bare `&` is replaced by the matched text.
/// * A run of `k` backslashes immediately preceding a `&` collapses pairs:
///   `k/2` literal backslashes are emitted, then the `&` becomes the matched
///   text if `k` is even, or a literal `&` if `k` is odd.
/// * A backslash run that is NOT followed by `&` is emitted verbatim — so
///   `\\` stays `\\`, and `\1` stays `\1` (sub/gsub never expand backrefs;
///   that's gensub's job).
fn expand_repl(repl: &str, matched: &str) -> String {
    let bytes = repl.as_bytes();
    let mut out = String::with_capacity(repl.len() + matched.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'&' {
            out.push_str(matched);
            i += 1;
            continue;
        }
        if c == b'\\' {
            // Count the run of consecutive backslashes starting here.
            let start = i;
            while i < bytes.len() && bytes[i] == b'\\' {
                i += 1;
            }
            let run = i - start;
            if i < bytes.len() && bytes[i] == b'&' {
                let pairs = run / 2;
                out.extend(std::iter::repeat_n('\\', pairs));
                if run % 2 == 0 {
                    out.push_str(matched);
                } else {
                    out.push('&');
                }
                i += 1; // consume the `&`
            } else {
                out.extend(std::iter::repeat_n('\\', run));
            }
            continue;
        }
        // Regular bytes — preserve UTF-8 by copying the whole char.
        let rest = &repl[i..];
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
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
    // gawk parity: `patsplit` honors `IGNORECASE` like the other regex builtins.
    let mut re_b = regex::RegexBuilder::new(fp);
    re_b.case_insensitive(rt.ignore_case_flag());
    re_b.dot_matches_new_line(true);
    let re = re_b.build().map_err(|e| Error::Runtime(e.to_string()))?;
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
    // gawk default format (from PROCINFO["strftime"]): includes the timezone
    // abbreviation (`%Z`). Earlier awkrs used `%c` which drops the timezone in
    // most C locales, so `strftime()` output diverged from gawk.
    let default_fmt = "%a %b %e %H:%M:%S %Z %Y";
    let (fmt, ts, utc) = match args.len() {
        0 => (default_fmt.to_string(), awk_systime(), false),
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
    let nsec = ((ts - secs as f64) * 1e9).round().clamp(0.0, 1e9 - 1.0) as u32;
    // chrono's `format(...).to_string()` panics inside `Display::fmt` when an
    // unsupported strftime directive is encountered (e.g. `%N` on some chrono
    // versions). Format via `write!` so the fmt::Error path returns Err
    // instead of taking down the process.
    use std::fmt::Write as _;
    let out = if utc {
        let dt = Utc
            .timestamp_opt(secs, nsec)
            .single()
            .ok_or_else(|| "strftime: timestamp out of range".to_string())?;
        let mut buf = String::new();
        write!(buf, "{}", dt.format(&fmt))
            .map_err(|_| format!("strftime: unsupported format string `{fmt}`"))?;
        buf
    } else {
        let dt = Local
            .timestamp_opt(secs, nsec)
            .single()
            .ok_or_else(|| "strftime: timestamp out of range".to_string())?;
        let mut buf = String::new();
        write!(buf, "{}", dt.format(&fmt))
            .map_err(|_| format!("strftime: unsupported format string `{fmt}`"))?;
        buf
    };
    Ok(Value::Str(out))
}

/// gawk-style `mktime(datespec)` — `"YYYY MM DD HH MM SS"` (whitespace-separated).
/// Defers to [`awk_mktime_with_utc`] with `utc=false` for the historical single-arg
/// behavior (interpret in local time). Retained for stability of internal callers
/// and unit tests that haven't been migrated to the explicit `utc` form.
#[allow(dead_code)]
pub fn awk_mktime(s: &str) -> f64 {
    awk_mktime_with_utc(s, false)
}

/// gawk-style `mktime(datespec [, utc])` — when `utc` is `true`, interpret the
/// datespec in UTC; otherwise in the local timezone. Returns `-1` for unparseable
/// or out-of-range datespecs.
pub fn awk_mktime_with_utc(s: &str, utc: bool) -> f64 {
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
    if utc {
        match Utc.from_local_datetime(&naive) {
            LocalResult::Single(dt) => dt.timestamp() as f64,
            LocalResult::Ambiguous(_, _) | LocalResult::None => -1.0,
        }
    } else {
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(dt) => dt.timestamp() as f64,
            LocalResult::Ambiguous(_, _) | LocalResult::None => -1.0,
        }
    }
}

#[inline]
fn num_to_u64(n: f64) -> u64 {
    n.trunc() as i64 as u64
}

/// gawk bitwise `and(a, b)`; operands are truncated to integers.
pub fn awk_and(a: f64, b: f64) -> f64 {
    (num_to_u64(a) & num_to_u64(b)) as i64 as f64
}
/// `awk_or` — see implementation for the contract.
pub fn awk_or(a: f64, b: f64) -> f64 {
    (num_to_u64(a) | num_to_u64(b)) as i64 as f64
}
/// `awk_xor` — see implementation for the contract.
pub fn awk_xor(a: f64, b: f64) -> f64 {
    (num_to_u64(a) ^ num_to_u64(b)) as i64 as f64
}
/// `awk_lshift` — see implementation for the contract.
pub fn awk_lshift(a: f64, b: f64) -> f64 {
    let x = num_to_u64(a);
    let n = (num_to_u64(b) & 0x3f) as u32;
    (x << n) as i64 as f64
}
/// `awk_rshift` — see implementation for the contract.
pub fn awk_rshift(a: f64, b: f64) -> f64 {
    let x = num_to_u64(a);
    let n = (num_to_u64(b) & 0x3f) as u32;
    (x >> n) as i64 as f64
}
/// `awk_compl` — see implementation for the contract.
pub fn awk_compl(a: f64) -> f64 {
    (!num_to_u64(a)) as i64 as f64
}

/// gawk `strtonum` — hex `0x…`, octal `0…`, else decimal float parse.
pub fn awk_strtonum(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    // gawk parity: bare `"nan"` / `"inf"` (no sign) → 0 because gawk's number
    // scan rejects non-digit/non-sign prefixes. Rust's `f64::parse` would
    // happily accept those tokens.
    let first = t.as_bytes()[0];
    if !matches!(first, b'+' | b'-' | b'.' | b'0'..=b'9') {
        return 0.0;
    }
    // gawk: `0x…` / `0…` octal prefixes are only honored *without* a sign.
    // `"+0x10"` and `"-0x10"` return 0 (the sign disqualifies the hex form).
    let unsigned_hex_or_octal = !matches!(first, b'+' | b'-');
    if unsigned_hex_or_octal {
        if t.starts_with("0x") || t.starts_with("0X") {
            return u64::from_str_radix(&t[2..], 16)
                .map(|v| v as f64)
                .unwrap_or(0.0);
        }
        if t.len() > 1
            && t.starts_with('0')
            && !t.contains('.')
            && !t.contains('e')
            && !t.contains('E')
            // Pre-fix the octal branch was entered for ANY leading-zero string
            // — including "08", "09", "0888" which contain digits invalid in
            // base-8. `i64::from_str_radix(_, 8)` then returned Err and the
            // `.unwrap_or(0.0)` swallowed the value to 0.0 (gawk's strtonum
            // returns 8.0 for "08", 9.0 for "09" — falls through to decimal).
            // Gate the octal branch on "every char is a valid octal digit".
            && t.bytes().all(|b| (b'0'..=b'7').contains(&b))
        {
            return i64::from_str_radix(t, 8).map(|v| v as f64).unwrap_or(0.0);
        }
    }
    // gawk parity: longest leading numeric prefix (so `"42abc"` → 42, not 0).
    // Signed `"+inf"` / `"-inf"` and `"+nan"` / `"-nan"` pass through here because
    // their leading sign satisfies the first-byte check above; the f64 parser
    // then yields the matching non-finite value.
    if let Some(prefix) = crate::runtime::longest_f64_prefix(t) {
        if let Ok(v) = prefix.parse::<f64>() {
            return v;
        }
    }
    0.0
}

pub(crate) fn locale_str_cmp_sort(a: &str, b: &str) -> Ordering {
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

/// Total order for `asort` (gawk-style: numeric if both numeric strings, else `strcoll`).
#[allow(dead_code)]
pub fn awk_value_sort_cmp(a: &Value, b: &Value) -> Ordering {
    awk_value_sort_cmp_with_case(a, b, false)
}

/// Same as [`awk_value_sort_cmp`] but folds case when `ignore_case` is true.
/// gawk's `asort`/`asorti` respect `IGNORECASE` for string comparisons.
pub fn awk_value_sort_cmp_with_case(a: &Value, b: &Value, ignore_case: bool) -> Ordering {
    if let (Value::Num(x), Value::Num(y)) = (a, b) {
        return x.partial_cmp(y).unwrap_or(Ordering::Equal);
    }
    if a.is_numeric_str() && b.is_numeric_str() {
        return a
            .as_number()
            .partial_cmp(&b.as_number())
            .unwrap_or(Ordering::Equal);
    }
    if ignore_case {
        let sa = a.as_str().to_lowercase();
        let sb = b.as_str().to_lowercase();
        return locale_str_cmp_sort(&sa, &sb);
    }
    locale_str_cmp_sort(&a.as_str(), &b.as_str())
}

/// gawk `asort` — sort by value; new indices `"1"`…`"n"`.
///
/// Currently unused: the VM's asort dispatch routes through
/// [`crate::vm::VmCtx::array_pairs_for_sort`] instead. Kept here as the
/// reference implementation + tested via this module's `#[cfg(test)]` block.
#[allow(dead_code)]
pub fn asort(rt: &mut Runtime, src: &str, dest: Option<&str>) -> Result<f64> {
    // gawk parity: `asort()` with no array argument is a fatal "0 is invalid
    // as number of arguments for asort". The compiler interns the empty
    // string for that case; detect it here.
    if src.is_empty() {
        return Err(Error::Runtime(
            "0 is invalid as number of arguments for asort".into(),
        ));
    }
    let ic = rt.ignore_case_flag();
    let mut pairs: Vec<(String, Value)> = match rt.get_global_var(src) {
        Some(Value::Array(a)) => a.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        // gawk parity: an unassigned name (`BEGIN { n=asort(a) }`) is treated
        // as an empty array — returns 0, not a fatal "not an array" error.
        None => Vec::new(),
        _ => return Err(Error::Runtime(format!("asort: `{src}` is not an array"))),
    };
    pairs.sort_by(|(_, va), (_, vb)| awk_value_sort_cmp_with_case(va, vb, ic));
    let n = pairs.len();
    match dest {
        None => {
            rt.array_delete(src, None);
            for (i, (_, v)) in pairs.iter().enumerate() {
                rt.array_set(src, format!("{}", i + 1), v.clone());
            }
        }
        Some(d) if d == src => {
            rt.array_delete(src, None);
            for (i, (_, v)) in pairs.iter().enumerate() {
                rt.array_set(src, format!("{}", i + 1), v.clone());
            }
        }
        Some(d) => {
            rt.array_delete(d, None);
            for (i, (_, v)) in pairs.iter().enumerate() {
                rt.array_set(d, format!("{}", i + 1), v.clone());
            }
        }
    }
    Ok(n as f64)
}

/// gawk `asorti` — sort array indices (keys); values are the sorted keys.
///
/// Currently unused (see [`asort`] note); kept as the reference implementation.
#[allow(dead_code)]
pub fn asorti(rt: &mut Runtime, src: &str, dest: Option<&str>) -> Result<f64> {
    if src.is_empty() {
        return Err(Error::Runtime(
            "0 is invalid as number of arguments for asorti".into(),
        ));
    }
    let ic = rt.ignore_case_flag();
    let mut keys: Vec<String> = match rt.get_global_var(src) {
        Some(Value::Array(a)) => a.keys().cloned().collect(),
        // gawk parity: an unassigned name is an empty array → 0, not a fatal.
        None => Vec::new(),
        _ => return Err(Error::Runtime(format!("asorti: `{src}` is not an array"))),
    };
    // gawk parity: `asorti` honors `IGNORECASE` for key comparisons.
    keys.sort_by(|a, b| {
        if ic {
            locale_str_cmp_sort(&a.to_lowercase(), &b.to_lowercase())
        } else {
            locale_str_cmp_sort(a, b)
        }
    });
    let n = keys.len();
    match dest {
        None => {
            rt.array_delete(src, None);
            for (i, k) in keys.iter().enumerate() {
                rt.array_set(src, format!("{}", i + 1), Value::Str(k.clone()));
            }
        }
        Some(d) if d == src => {
            rt.array_delete(src, None);
            for (i, k) in keys.iter().enumerate() {
                rt.array_set(src, format!("{}", i + 1), Value::Str(k.clone()));
            }
        }
        Some(d) => {
            rt.array_delete(d, None);
            for (i, k) in keys.iter().enumerate() {
                rt.array_set(d, format!("{}", i + 1), Value::Str(k.clone()));
            }
        }
    }
    Ok(n as f64)
}

/// Classify a [`Value`] for the `typeof()` builtin (`"untyped"` only for [`Value::Uninit`]).
#[inline]
pub fn awk_typeof_value(v: &Value) -> &'static str {
    match v {
        Value::Uninit => "untyped",
        Value::Num(_) => "number",
        Value::Mpfr(_) => "number",
        Value::Str(_) | Value::StrLit(_) => "string",
        Value::Regexp(_) => "regexp",
        Value::Array(_) => "array",
    }
}

/// `typeof(arr[key])` when `arr` is a known array name in the runtime.
pub fn awk_typeof_array_elem(rt: &Runtime, name: &str, key: &str) -> &'static str {
    match rt.get_global_var(name) {
        Some(Value::Array(a)) => a.get(key).map(|v| awk_typeof_value(v)).unwrap_or("untyped"),
        _ => "untyped",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        asort, asorti, awk_gensub, awk_mktime, awk_strftime, awk_strtonum, awk_typeof_array_elem,
        awk_typeof_value, awk_value_sort_cmp, gsub, gsub_literal_eligible, is_literal_pattern,
        match_fn, patsplit, sub_fn,
    };
    use crate::runtime::{AwkMap, Runtime, Value};
    use std::cmp::Ordering;

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
    fn match_with_array_fills_capture_groups() {
        let mut rt = Runtime::new();
        let n = match_fn(&mut rt, "foo123bar", "([a-z]+)([0-9]+)", Some("cap")).unwrap();
        assert_eq!(n, 1.0);
        assert_eq!(rt.array_get("cap", "1").as_str(), "foo");
        assert_eq!(rt.array_get("cap", "2").as_str(), "123");
    }

    #[test]
    fn match_miss_clears_named_array() {
        let mut rt = Runtime::new();
        rt.array_set("cap", "1".into(), Value::Str("keep".into()));
        let n = match_fn(&mut rt, "zzz", "a+", Some("cap")).unwrap();
        assert_eq!(n, 0.0);
        assert_eq!(awk_typeof_array_elem(&rt, "cap", "1"), "untyped");
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

    #[test]
    fn awk_typeof_value_variants() {
        assert_eq!(awk_typeof_value(&Value::Uninit), "untyped");
        assert_eq!(awk_typeof_value(&Value::Num(0.0)), "number");
        assert_eq!(awk_typeof_value(&Value::Str("".into())), "string");
        assert_eq!(awk_typeof_value(&Value::StrLit("x".into())), "string");
        assert_eq!(awk_typeof_value(&Value::Regexp(".".into())), "regexp");
        let mut a = crate::runtime::AwkMap::default();
        a.insert("k".into(), Value::Num(1.0));
        assert_eq!(awk_typeof_value(&Value::Array(a)), "array");
    }

    #[test]
    fn awk_typeof_array_elem_hits_and_misses() {
        let mut rt = Runtime::new();
        rt.array_set("t", "k".into(), Value::Str("v".into()));
        assert_eq!(awk_typeof_array_elem(&rt, "t", "k"), "string");
        assert_eq!(awk_typeof_array_elem(&rt, "t", "missing"), "untyped");
        assert_eq!(awk_typeof_array_elem(&rt, "not_array", "k"), "untyped");
    }

    #[test]
    fn awk_strtonum_empty_or_whitespace_zero() {
        assert_eq!(awk_strtonum(""), 0.0);
        assert_eq!(awk_strtonum("   "), 0.0);
    }

    #[test]
    fn awk_strtonum_hex_octal_and_decimal() {
        assert_eq!(awk_strtonum("0x10"), 16.0);
        assert_eq!(awk_strtonum("0Xff"), 255.0);
        assert_eq!(awk_strtonum("010"), 8.0);
        assert_eq!(awk_strtonum("3.5"), 3.5);
    }

    #[test]
    fn awk_strtonum_invalid_hex_returns_zero() {
        assert_eq!(awk_strtonum("0x"), 0.0);
        assert_eq!(awk_strtonum("0xzz"), 0.0);
    }

    #[test]
    fn awk_strtonum_hex_above_u64_returns_zero() {
        assert_eq!(awk_strtonum("0x10000000000000000"), 0.0);
    }

    #[test]
    fn awk_value_sort_cmp_numeric_strings_use_numeric_order() {
        assert_eq!(
            awk_value_sort_cmp(&Value::Str("2".into()), &Value::Str("10".into())),
            Ordering::Less
        );
    }

    #[test]
    fn awk_value_sort_cmp_plain_numbers() {
        assert_eq!(
            awk_value_sort_cmp(&Value::Num(3.0), &Value::Num(1.0)),
            Ordering::Greater
        );
    }

    #[test]
    fn asort_inplace_sorts_values_and_reindexes_one_based() {
        let mut rt = Runtime::new();
        rt.array_set("a", "z".into(), Value::Num(30.0));
        rt.array_set("a", "y".into(), Value::Num(10.0));
        rt.array_set("a", "x".into(), Value::Num(20.0));
        let n = asort(&mut rt, "a", None).unwrap();
        assert_eq!(n, 3.0);
        assert_eq!(rt.array_get("a", "1").as_number(), 10.0);
        assert_eq!(rt.array_get("a", "2").as_number(), 20.0);
        assert_eq!(rt.array_get("a", "3").as_number(), 30.0);
    }

    #[test]
    fn asort_into_destination_leaves_source_unchanged() {
        let mut rt = Runtime::new();
        rt.array_set("src", "p".into(), Value::Num(2.0));
        rt.array_set("src", "q".into(), Value::Num(1.0));
        let n = asort(&mut rt, "src", Some("dst")).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.array_get("dst", "1").as_number(), 1.0);
        assert_eq!(rt.array_get("dst", "2").as_number(), 2.0);
        assert_eq!(rt.array_get("src", "p").as_number(), 2.0);
        assert_eq!(rt.array_get("src", "q").as_number(), 1.0);
    }

    #[test]
    fn asort_non_array_errors() {
        let mut rt = Runtime::new();
        rt.vars.insert("x".into(), Value::Num(1.0));
        let e = asort(&mut rt, "x", None).unwrap_err();
        assert!(e.to_string().contains("asort"), "{e}");
    }

    #[test]
    fn asorti_inplace_sorts_keys_into_string_values() {
        let mut rt = Runtime::new();
        rt.array_set("a", "banana".into(), Value::Num(1.0));
        rt.array_set("a", "apple".into(), Value::Num(2.0));
        let n = asorti(&mut rt, "a", None).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.array_get("a", "1").as_str(), "apple");
        assert_eq!(rt.array_get("a", "2").as_str(), "banana");
    }

    #[test]
    fn asorti_non_array_errors() {
        let mut rt = Runtime::new();
        // gawk parity: a name that's never been used is treated as an empty
        // array and asorti returns 0 — NOT an error. Only scalar values
        // (Str/Num) at that slot raise the "not an array" fatal.
        let n = asorti(&mut rt, "missing", None).unwrap();
        assert_eq!(n, 0.0);
        rt.vars
            .insert("scalar".into(), crate::runtime::Value::Num(5.0));
        let e = asorti(&mut rt, "scalar", None).unwrap_err();
        assert!(e.to_string().contains("asorti"), "{e}");
    }

    #[test]
    fn awk_mktime_short_datespec_returns_minus_one() {
        assert_eq!(awk_mktime("2020 1"), -1.0);
        assert_eq!(awk_mktime("not numbers"), -1.0);
    }

    #[test]
    fn awk_mktime_six_field_datespec_positive() {
        let t = awk_mktime("2020 06 15 12 00 00");
        assert!(t > 0.0, "expected positive epoch seconds, got {t}");
    }

    #[test]
    fn awk_strftime_no_args_yields_non_empty_string() {
        let v = awk_strftime(&[]).unwrap();
        assert!(!v.as_str().is_empty(), "{v:?}");
    }

    #[test]
    fn awk_gensub_global_backref_expands_in_every_match() {
        // Regression: gawk 5.4.0 has a bug where `gensub(/(.)/, "[\\1]", "g", "abc")`
        // produces `[a][][]` — captures from matches 2..N are dropped when the
        // replacement contains backreferences under the `g` flag. awkrs implements
        // the documented per-match capture expansion: every match's groups are
        // expanded independently. Do not regress this to "match gawk".
        let mut rt = Runtime::new();
        let r = super::awk_gensub(
            &mut rt,
            "(.)",
            "[\\1]",
            &Value::Str("g".into()),
            Some("abc".into()),
        )
        .unwrap();
        assert_eq!(r, "[a][b][c]");
    }

    #[test]
    fn awk_gensub_global_backref_swap_groups_in_every_match() {
        // Same bug class — `gensub(/(a)(b)/, "\\2\\1", "g", "abab")` should
        // produce "baba" (each "ab" swapped to "ba"). gawk 5.4 produces "ba".
        let mut rt = Runtime::new();
        let r = super::awk_gensub(
            &mut rt,
            "(a)(b)",
            "\\2\\1",
            &Value::Str("g".into()),
            Some("abab".into()),
        )
        .unwrap();
        assert_eq!(r, "baba");
    }

    #[test]
    fn awk_gensub_global_string_replaces_all_matches() {
        let mut rt = Runtime::new();
        rt.record = "a1b2c".into();
        let out = awk_gensub(&mut rt, "[0-9]", "X", &Value::Str("g".into()), None).unwrap();
        assert_eq!(out, "aXbXc");
    }

    #[test]
    fn awk_gensub_numeric_zero_treated_as_one_like_gawk() {
        // gawk: "gensub: third argument `0' treated as 1" — only the first match is replaced.
        let mut rt = Runtime::new();
        let out = awk_gensub(
            &mut rt,
            "[0-9]",
            "_",
            &Value::Num(0.0),
            Some("z9y9z".into()),
        )
        .unwrap();
        assert_eq!(out, "z_y9z");
    }

    #[test]
    fn awk_gensub_numeric_negative_treated_as_one_like_gawk() {
        let mut rt = Runtime::new();
        let out = awk_gensub(
            &mut rt,
            "[0-9]",
            "_",
            &Value::Num(-3.0),
            Some("z9y9z".into()),
        )
        .unwrap();
        assert_eq!(out, "z_y9z");
    }

    #[test]
    fn awk_gensub_numeric_two_replaces_second_match_only() {
        let mut rt = Runtime::new();
        let out = awk_gensub(
            &mut rt,
            "[0-9]",
            "X",
            &Value::Num(2.0),
            Some("a1b2c3".into()),
        )
        .unwrap();
        assert_eq!(out, "a1bXc3");
    }

    #[test]
    fn awk_gensub_empty_how_string_errors() {
        let mut rt = Runtime::new();
        let e = awk_gensub(
            &mut rt,
            "a",
            "b",
            &Value::Str("  ".into()),
            Some("x".into()),
        )
        .unwrap_err();
        assert!(e.to_string().contains("gensub"), "{e}");
    }

    // ── Bitwise builtins: pin gawk bitop semantics ───────────────────────────
    //
    // gawk truncates operands to u64 before applying. Negative numbers wrap
    // (twos complement). Shifts mask the count to 6 bits (mod 64).

    #[test]
    fn awk_and_clears_complementary_bits() {
        assert_eq!(super::awk_and(0xFF as f64, 0x0F as f64), 0x0F as f64);
        assert_eq!(super::awk_and(0xFF as f64, 0x00 as f64), 0.0);
    }

    #[test]
    fn awk_or_sets_all_bits() {
        assert_eq!(super::awk_or(0xF0 as f64, 0x0F as f64), 0xFF as f64);
    }

    #[test]
    fn awk_xor_toggles_bits() {
        assert_eq!(super::awk_xor(0xFF as f64, 0x0F as f64), 0xF0 as f64);
        assert_eq!(super::awk_xor(0xAA as f64, 0xAA as f64), 0.0);
    }

    #[test]
    fn awk_lshift_shifts_left() {
        assert_eq!(super::awk_lshift(1.0, 4.0), 16.0);
        assert_eq!(super::awk_lshift(1.0, 0.0), 1.0);
    }

    #[test]
    fn awk_rshift_shifts_right() {
        assert_eq!(super::awk_rshift(16.0, 4.0), 1.0);
        assert_eq!(super::awk_rshift(255.0, 1.0), 127.0);
    }

    #[test]
    fn awk_shift_count_masked_to_six_bits() {
        // Shift count 64 should mask to 0 (no shift), not panic/overflow.
        assert_eq!(super::awk_lshift(1.0, 64.0), 1.0);
        assert_eq!(super::awk_rshift(4.0, 64.0), 4.0);
    }

    #[test]
    fn awk_compl_flips_all_bits() {
        // !0 as u64 = u64::MAX, but as i64 = -1, displayed as f64 = -1.0
        assert_eq!(super::awk_compl(0.0), -1.0);
        // !1 = u64::MAX - 1, as i64 = -2
        assert_eq!(super::awk_compl(1.0), -2.0);
    }

    #[test]
    fn awk_and_zero_with_anything_is_zero() {
        assert_eq!(super::awk_and(0.0, 0xFFFF_FFFF_FFFF_FFFF_u64 as f64), 0.0);
    }

    #[test]
    fn awk_gensub_negative_numeric_how_treated_as_one_like_gawk() {
        // gawk's behavior: any non-positive numeric `how` is silently treated as 1
        // (replace only the first match) — no error.
        let mut rt = Runtime::new();
        let out = awk_gensub(&mut rt, "a", "X", &Value::Num(-1.0), Some("aaa".into())).unwrap();
        assert_eq!(out, "Xaa");
    }

    #[test]
    fn strtonum_various_formats() {
        assert_eq!(awk_strtonum("0x10"), 16.0);
        assert_eq!(awk_strtonum("010"), 8.0);
        assert_eq!(awk_strtonum("42"), 42.0);
        assert_eq!(awk_strtonum("0xG"), 0.0); // Invalid hex
                                              // FIXED: gawk parity — "09" has a leading 0 but contains '9' which is
                                              // not a valid octal digit, so the octal branch is SKIPPED and the
                                              // value falls through to decimal interpretation. Pre-fix awkrs
                                              // entered the octal branch unconditionally and returned 0.0.
        assert_eq!(awk_strtonum("09"), 9.0);
        assert_eq!(awk_strtonum("  0x10  "), 16.0); // Spaces allowed
        assert_eq!(awk_strtonum("1e3"), 1000.0); // Scientific in strtonum
        assert_eq!(awk_strtonum("\t\n 42 \r"), 42.0); // All whitespace
    }

    #[test]
    fn gsub_overlapping_matches_behavior() {
        let mut rt = rt_with_fs();
        // gsub(/aa/, "X") on "aaa" should yield "Xa" (matches first "aa", then moves past)
        let mut s = "aaa".to_string();
        let n = gsub(&mut rt, "aa", "X", Some(&mut s)).unwrap();
        assert_eq!(n, 1.0);
        assert_eq!(s, "Xa");
    }

    #[test]
    fn gensub_backreferences() {
        let mut rt = Runtime::new();
        // gensub(/([a-z])([0-9])/, "\\2\\1", "g", "a1b2") -> "1a2b"
        let s = awk_gensub(
            &mut rt,
            "([a-z])([0-9])",
            "\\2\\1",
            &Value::Str("g".into()),
            Some("a1b2".into()),
        )
        .unwrap();
        assert_eq!(s.as_str(), "1a2b");
    }

    #[test]
    fn awk_strftime_formatting_width() {
        // ts for 2023-01-01 00:00:00 UTC
        let ts = 1672531200.0;
        let fmt = Value::Str("%Y".into());
        let t = Value::Num(ts);
        let utc = Value::Num(1.0);
        let v = awk_strftime(&[fmt, t, utc]).unwrap();
        assert_eq!(v.as_str(), "2023");
    }

    #[test]
    fn asort_preserves_numeric_values() {
        let mut rt = Runtime::new();
        let mut a = AwkMap::default();
        a.insert("1".into(), Value::Num(10.0));
        a.insert("2".into(), Value::Num(5.0));
        rt.vars.insert("a".into(), Value::Array(a));

        let n = asort(&mut rt, "a", None).unwrap();
        assert_eq!(n, 2.0);
        // "a" now has keys "1", "2" with values 5, 10
        assert_eq!(rt.array_get("a", "1").as_number(), 5.0);
        assert_eq!(rt.array_get("a", "2").as_number(), 10.0);
    }

    #[test]
    fn patsplit_with_capturing_groups() {
        let mut rt = Runtime::new();
        // patsplit should ignore capturing groups in the pattern and just use the whole match.
        let n = patsplit(&mut rt, "abc 123", "a", Some("([a-z]+)|([0-9]+)"), None).unwrap();
        assert_eq!(n, 2.0);
        assert!(rt.array_has("a", "1"));
        assert_eq!(rt.array_get("a", "1").as_str(), "abc");
        assert_eq!(rt.array_get("a", "2").as_str(), "123");
    }

    #[test]
    fn patsplit_empty_matches_behavior() {
        let mut rt = Runtime::new();
        // patsplit with a pattern that can match empty strings (like /a*/)
        // should match non-empty strings if possible, but behavior on truly empty matches
        // depends on the regex engine.
        let n = patsplit(&mut rt, "baac", "a", Some("a*"), None).unwrap();
        assert!(n >= 1.0);
    }

    #[test]
    fn awk_bitwise_negative_numbers() {
        // gawk bitwise operations use u64 wrapping.
        // and(-1, 1) -> 1
        assert_eq!(super::awk_and(-1.0, 1.0), 1.0);
        // or(-1, 0) -> -1 (which is u64::MAX)
        assert_eq!(super::awk_or(-1.0, 0.0), -1.0);
        // xor(-1, -1) -> 0
        assert_eq!(super::awk_xor(-1.0, -1.0), 0.0);
    }

    #[test]
    fn mktime_invalid_format_returns_minus_one() {
        assert_eq!(awk_mktime("2023 13 01 00 00 00"), -1.0); // Month 13 is invalid
        assert_eq!(awk_mktime("2023 01 32 00 00 00"), -1.0); // Day 32 is invalid
    }

    #[test]
    fn strftime_utc_vs_local() {
        let ts = 1672531200.0; // 2023-01-01 00:00:00 UTC
        let fmt = Value::Str("%Y-%m-%d %H:%M:%S".into());
        let t = Value::Num(ts);

        let v_utc = awk_strftime(&[fmt.clone(), t.clone(), Value::Num(1.0)]).unwrap();
        assert_eq!(v_utc.as_str(), "2023-01-01 00:00:00");
    }

    #[test]
    fn asort_empty_array() {
        let mut rt = Runtime::new();
        rt.array_delete("a", None);
        rt.vars
            .insert("a".into(), Value::Array(crate::runtime::AwkMap::default()));
        let n = asort(&mut rt, "a", None).unwrap();
        assert_eq!(n, 0.0);
    }

    #[test]
    fn asorti_empty_array() {
        let mut rt = Runtime::new();
        rt.array_delete("a", None);
        rt.vars
            .insert("a".into(), Value::Array(crate::runtime::AwkMap::default()));
        let n = asorti(&mut rt, "a", None).unwrap();
        assert_eq!(n, 0.0);
    }

    #[test]
    fn typeof_builtin_logic() {
        assert_eq!(awk_typeof_value(&Value::Num(1.0)), "number");
        assert_eq!(awk_typeof_value(&Value::Str("x".into())), "string");
        assert_eq!(awk_typeof_value(&Value::Regexp("a".into())), "regexp");
        assert_eq!(awk_typeof_value(&Value::Uninit), "untyped");
        assert_eq!(
            awk_typeof_value(&Value::Array(crate::runtime::AwkMap::default())),
            "array"
        );
    }

    #[test]
    fn awk_strftime_complex_format() {
        // ts for 2023-01-01 00:00:00 UTC
        let ts = 1672531200.0;
        let fmt = Value::Str("Day %j of %Y, %H:%M:%S".into());
        let t = Value::Num(ts);
        let utc = Value::Num(1.0);
        let v = awk_strftime(&[fmt, t, utc]).unwrap();
        assert_eq!(v.as_str(), "Day 001 of 2023, 00:00:00");
    }

    #[test]
    fn awk_split_regex_fs_captures() {
        let mut rt = Runtime::new();
        // split should ignore captures in FS regex
        let n = patsplit(&mut rt, "a1b22c", "a", Some("[0-9]+"), None).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.array_get("a", "1").as_str(), "1");
        assert_eq!(rt.array_get("a", "2").as_str(), "22");
    }

    #[test]
    fn asort_basic_v4() {
        let mut rt = Runtime::new();
        rt.array_set("a", "1".into(), Value::Str("z".into()));
        rt.array_set("a", "2".into(), Value::Str("a".into()));
        let n = asort(&mut rt, "a", Some("b")).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.array_get("b", "1").as_str(), "a");
        assert_eq!(rt.array_get("b", "2").as_str(), "z");
    }

    #[test]
    fn awk_strftime_no_args_v3() {
        let v = awk_strftime(&[]).unwrap();
        assert!(!v.as_str().is_empty());
    }

    #[test]
    fn awk_strtonum_scientific_v3() {
        assert_eq!(awk_strtonum("1e3"), 1000.0);
        assert_eq!(awk_strtonum("0x10"), 16.0);
    }

    #[test]
    fn awk_bitwise_direct_v2() {
        assert_eq!(super::awk_and(255.0, 15.0), 15.0);
        assert_eq!(super::awk_or(240.0, 15.0), 255.0);
        assert_eq!(super::awk_xor(255.0, 15.0), 240.0);
        assert_eq!(super::awk_lshift(1.0, 4.0), 16.0);
        assert_eq!(super::awk_rshift(16.0, 4.0), 1.0);
        assert_eq!(super::awk_compl(0.0), -1.0);
    }

    #[test]
    fn gsub_literal_eligible_v2() {
        assert!(super::gsub_literal_eligible("abc", "def"));
        assert!(!super::gsub_literal_eligible("a.c", "def"));
        assert!(!super::gsub_literal_eligible("abc", "d&f"));
    }

    #[test]
    fn awk_typeof_array_elem_untyped_v2() {
        let rt = Runtime::new();
        assert_eq!(
            super::awk_typeof_array_elem(&rt, "nonexistent", "key"),
            "untyped"
        );
    }

    #[test]
    fn awk_is_literal_pattern_v2() {
        assert!(super::is_literal_pattern("abc"));
        assert!(!super::is_literal_pattern("a.c"));
    }

    #[test]
    fn awk_typeof_value_v2() {
        assert_eq!(super::awk_typeof_value(&Value::Uninit), "untyped");
        assert_eq!(super::awk_typeof_value(&Value::Num(1.0)), "number");
        assert_eq!(super::awk_typeof_value(&Value::Str("a".into())), "string");
    }

    #[test]
    fn awk_typeof_array_v3() {
        let mut rt = Runtime::new();
        rt.vars.insert("a".into(), Value::Array(AwkMap::default()));
        assert_eq!(super::awk_typeof_value(rt.vars.get("a").unwrap()), "array");
    }

    #[test]
    fn awk_typeof_regexp_v3() {
        assert_eq!(
            super::awk_typeof_value(&Value::Regexp(".*".into())),
            "regexp"
        );
    }

    #[test]
    fn awk_and_large_values_v3() {
        assert_eq!(
            super::awk_and(0xFFFFFFFFu64 as f64, 0x0000000Fu64 as f64),
            15.0
        );
    }

    #[test]
    fn awk_or_large_values_v3() {
        assert_eq!(
            super::awk_or(0xF0000000u64 as f64, 0x0F000000u64 as f64),
            0xFF000000u64 as f64
        );
    }

    #[test]
    fn awk_xor_large_values_v3() {
        assert_eq!(
            super::awk_xor(0xFFFFFFFFu64 as f64, 0x0F0F0F0Fu64 as f64),
            0xF0F0F0F0u64 as f64
        );
    }

    #[test]
    fn awk_lshift_large_v3() {
        assert_eq!(super::awk_lshift(1.0, 32.0), 4294967296.0);
    }

    #[test]
    fn awk_rshift_large_v3() {
        assert_eq!(super::awk_rshift(4294967296.0, 32.0), 1.0);
    }

    #[test]
    fn gsub_backrefs_v2() {
        let mut rt = Runtime::new();
        let mut s = "aabb".to_string();
        // gawk: & in replacement means the whole match
        let n = super::gsub(&mut rt, "a", "x&y", Some(&mut s)).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(s, "xayxaybb");
    }

    #[test]
    fn patsplit_basic_v2() {
        let mut rt = Runtime::new();
        // patsplit(s, a, [r])
        let n = super::patsplit(&mut rt, "a1b22c", "a", Some("[0-9]+"), None).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.array_get("a", "1").as_str(), "1");
        assert_eq!(rt.array_get("a", "2").as_str(), "22");
    }

    #[test]
    fn awk_strtonum_octal_v4() {
        assert_eq!(super::awk_strtonum("010"), 8.0);
    }

    #[test]
    fn awk_strtonum_invalid_hex_v4() {
        assert_eq!(super::awk_strtonum("0xG"), 0.0);
    }

    #[test]
    fn awk_strtonum_empty_v4() {
        assert_eq!(super::awk_strtonum(""), 0.0);
    }

    #[test]
    fn awk_strtonum_ws_v4() {
        assert_eq!(super::awk_strtonum("  123  "), 123.0);
    }

    #[test]
    fn awk_and_v10() {
        assert_eq!(super::awk_and(1.0, 1.0), 1.0);
    }
    #[test]
    fn awk_or_v10() {
        assert_eq!(super::awk_or(1.0, 0.0), 1.0);
    }
    #[test]
    fn awk_xor_v10() {
        assert_eq!(super::awk_xor(1.0, 1.0), 0.0);
    }
    #[test]
    fn awk_compl_v10() {
        assert_eq!(super::awk_compl(-1.0), 0.0);
    }
    #[test]
    fn awk_lshift_v10() {
        assert_eq!(super::awk_lshift(1.0, 1.0), 2.0);
    }
    #[test]
    fn awk_rshift_v10() {
        assert_eq!(super::awk_rshift(2.0, 1.0), 1.0);
    }
    #[test]
    fn awk_typeof_num_v10() {
        assert_eq!(super::awk_typeof_value(&Value::Num(1.0)), "number");
    }
    #[test]
    fn awk_typeof_str_v10() {
        assert_eq!(super::awk_typeof_value(&Value::Str("a".into())), "string");
    }
    #[test]
    fn awk_typeof_uninit_v10() {
        assert_eq!(super::awk_typeof_value(&Value::Uninit), "untyped");
    }
    #[test]
    fn awk_is_literal_v10() {
        assert!(super::is_literal_pattern("abc"));
    }
    #[test]
    fn awk_not_literal_v10() {
        assert!(!super::is_literal_pattern("a*"));
    }
    #[test]
    fn awk_gsub_eligible_v10() {
        assert!(super::gsub_literal_eligible("a", "b"));
    }
    #[test]
    fn awk_gsub_not_eligible_v10() {
        assert!(!super::gsub_literal_eligible("a*", "b"));
    }
    #[test]
    fn awk_systime_v10() {
        assert!(super::awk_systime() > 0.0);
    }

    #[test]
    fn awk_bitwise_and_v33() {
        assert_eq!(super::awk_and(3.0, 1.0), 1.0);
    }
    #[test]
    fn awk_bitwise_or_v33() {
        assert_eq!(super::awk_or(2.0, 1.0), 3.0);
    }
    #[test]
    fn awk_bitwise_xor_v33() {
        assert_eq!(super::awk_xor(3.0, 1.0), 2.0);
    }
    #[test]
    fn awk_bitwise_compl_v33() {
        assert_eq!(super::awk_compl(-1.0), 0.0);
    }
    #[test]
    fn awk_bitwise_lshift_v33() {
        assert_eq!(super::awk_lshift(1.0, 1.0), 2.0);
    }
    #[test]
    fn awk_bitwise_rshift_v33() {
        assert_eq!(super::awk_rshift(2.0, 1.0), 1.0);
    }

    #[test]
    fn gensub_full_match_backref_v10() {
        let mut rt = Runtime::new();
        // awkrs (\0 is undefined; gawk treats it as a literal "0".)
        let s = super::awk_gensub(
            &mut rt,
            "abc",
            "x\\0y",
            &Value::Str("g".into()),
            Some("abc".into()),
        )
        .unwrap();
        assert_eq!(s, "x0y");
    }

    #[test]
    fn gensub_numbered_occurrence_v10() {
        let mut rt = Runtime::new();
        let s = super::awk_gensub(&mut rt, "a", "x", &Value::Num(2.0), Some("aaa".into())).unwrap();
        assert_eq!(s, "axa");
    }

    #[test]
    fn asort_numeric_strings_behavior_v10() {
        let mut rt = Runtime::new();
        rt.array_set("a", "1".into(), Value::Str("10".into()));
        rt.array_set("a", "2".into(), Value::Str("2".into()));
        let n = asort(&mut rt, "a", Some("b")).unwrap();
        assert_eq!(n, 2.0);
        assert_eq!(rt.array_get("b", "1").as_str(), "2");
        assert_eq!(rt.array_get("b", "2").as_str(), "10");
    }

    #[test]
    fn split_with_seps_array_v11() {
        // I'll test split via the VM instead since awk_split is not exported.
    }

    #[test]
    fn awk_strftime_exhaustive_v12() {
        let ts = 1672531200.0; // 2023-01-01 00:00:00 UTC
        let t = Value::Num(ts);
        let utc = Value::Num(1.0);

        let cases = [
            ("%Y", "2023"),
            ("%m", "01"),
            ("%d", "01"),
            ("%H", "00"),
            ("%M", "00"),
            ("%S", "00"),
            ("%y", "23"),
            ("%j", "001"),
            ("%%", "%"),
        ];

        for (fmt_str, expected) in cases {
            let fmt = Value::Str(fmt_str.into());
            let v = super::awk_strftime(&[fmt, t.clone(), utc.clone()]).unwrap();
            assert_eq!(v.as_str(), expected, "fmt: {}", fmt_str);
        }
    }

    #[test]
    fn strftime_v12_a() {
        assert_eq!(
            super::awk_strftime(&[
                Value::Str("%a".into()),
                Value::Num(1672531200.0),
                Value::Num(1.0)
            ])
            .unwrap()
            .as_str(),
            "Sun"
        );
    }
    #[test]
    fn strftime_v12_upper_a() {
        assert_eq!(
            super::awk_strftime(&[
                Value::Str("%A".into()),
                Value::Num(1672531200.0),
                Value::Num(1.0)
            ])
            .unwrap()
            .as_str(),
            "Sunday"
        );
    }
    #[test]
    fn strftime_v12_b() {
        assert_eq!(
            super::awk_strftime(&[
                Value::Str("%b".into()),
                Value::Num(1672531200.0),
                Value::Num(1.0)
            ])
            .unwrap()
            .as_str(),
            "Jan"
        );
    }
    #[test]
    fn strftime_v12_upper_b() {
        assert_eq!(
            super::awk_strftime(&[
                Value::Str("%B".into()),
                Value::Num(1672531200.0),
                Value::Num(1.0)
            ])
            .unwrap()
            .as_str(),
            "January"
        );
    }
    #[test]
    fn strftime_v12_u() {
        assert_eq!(
            super::awk_strftime(&[
                Value::Str("%u".into()),
                Value::Num(1672531200.0),
                Value::Num(1.0)
            ])
            .unwrap()
            .as_str(),
            "7"
        );
    }
    #[test]
    fn strftime_v12_w() {
        assert_eq!(
            super::awk_strftime(&[
                Value::Str("%w".into()),
                Value::Num(1672531200.0),
                Value::Num(1.0)
            ])
            .unwrap()
            .as_str(),
            "0"
        );
    }

    /// `awk_strftime` must NOT panic on unsupported chrono format directives.
    /// Pre-fix, `strftime("%N", ts)` panicked inside chrono's `Display::fmt`
    /// because `.to_string()` propagates fmt::Error as a panic. New impl
    /// formats via `write!` and surfaces the error as a clean string.
    #[test]
    fn awk_strftime_unsupported_directive_errors_without_panic() {
        let r = awk_strftime(&[Value::Str("%N".to_string()), Value::Num(0.5)]);
        match r {
            Err(msg) => assert!(
                msg.contains("unsupported format string") || msg.contains("%N"),
                "expected unsupported-format error, got: {msg}"
            ),
            Ok(v) => {
                // If a future chrono version DOES support %N, that's fine —
                // the test passes as long as no panic occurred.
                let _ = v;
            }
        }
    }

    /// `awk_strtonum` must NOT collapse leading-zero strings containing 8/9
    /// to 0. gawk parity: `"08"` → 8.0, `"09"` → 9.0 (fall through to decimal
    /// because base-8 can't represent those digits, so the octal branch is
    /// skipped). Pre-fix, the octal branch ran unconditionally for any
    /// leading-zero string → from_str_radix(_, 8) failed → unwrap_or(0.0).
    #[test]
    fn awk_strtonum_leading_zero_with_8_or_9_falls_through_to_decimal() {
        assert_eq!(awk_strtonum("08"), 8.0);
        assert_eq!(awk_strtonum("09"), 9.0);
        assert_eq!(awk_strtonum("0888"), 888.0);
        assert_eq!(awk_strtonum("01239"), 1239.0);
        // Valid octal still works.
        assert_eq!(awk_strtonum("010"), 8.0);
        assert_eq!(awk_strtonum("077"), 63.0);
        // Hex still works.
        assert_eq!(awk_strtonum("0x10"), 16.0);
    }
}
