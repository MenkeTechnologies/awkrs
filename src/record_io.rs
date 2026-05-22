//! Record boundaries for configurable `RS` (newline default, multi-byte separator, paragraph mode,
//! gawk-style regex `RS` when `RS` is longer than one character).

use crate::error::{Error, Result};
use crate::runtime::SharedInputReader;
use memchr::memchr;
use regex::bytes::Regex as BytesRegex;
use std::io::BufRead;

/// Trim trailing `\n` from a byte slice (record content).
///
/// gawk parity (POSIX text mode on Unix): only `\n` is stripped — `\r` is
/// preserved so that a CRLF input keeps the `\r` in `$0` and `length`. Older
/// awkrs versions also stripped `\r`, which silently dropped CR bytes on Unix.
/// (Use `BINMODE` / explicit `gsub(/\r$/, "")` to remove them if desired.)
pub fn trim_end_record_bytes(buf: &[u8]) -> usize {
    let mut end = buf.len();
    while end > 0 && buf[end - 1] == b'\n' {
        end -= 1;
    }
    end
}

/// Read the next record from `reader` into `out` using `RS` string `rs`.
/// - `rs == "\n"` → read until `\n` (POSIX default).
/// - `rs == ""` → paragraph mode (records separated by blank lines).
/// - `regex_rs == Some` → gawk: `RS` length > 1, treated as a regex; `rt_sep` is the matched bytes.
/// - else → read until `rs` appears as a byte substring (literal `RS`).
///
/// `leftover` holds bytes that were read past the previous record's terminator;
/// it must be the same `Vec` across consecutive calls on a single stream so that
/// chunked reads (used by the regex path) don't lose input.
pub fn read_next_record(
    reader: &SharedInputReader,
    rs: &str,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
    regex_rs: Option<&BytesRegex>,
    leftover: &mut Vec<u8>,
) -> Result<bool> {
    out.clear();
    rt_sep.clear();
    let mut guard = reader
        .lock()
        .map_err(|_| Error::Runtime("input reader lock poisoned".into()))?;
    let r = &mut *guard;
    if rs == "\n" {
        return read_until_lf(r, out, rt_sep, leftover);
    }
    if rs.is_empty() {
        let ok = read_paragraph_record(r, out, rt_sep, leftover)?;
        return Ok(ok);
    }
    if let Some(re) = regex_rs {
        return read_until_regex_bytes(r, re, out, rt_sep, leftover);
    }
    read_until_bytes(r, rs.as_bytes(), out, rt_sep, leftover)
}

fn read_until_lf<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
    leftover: &mut Vec<u8>,
) -> Result<bool> {
    if !leftover.is_empty() {
        if let Some(pos) = memchr(b'\n', leftover) {
            out.extend_from_slice(&leftover[..=pos]);
            leftover.drain(..=pos);
            rt_sep.extend_from_slice(b"\n");
            return Ok(true);
        }
        out.extend_from_slice(leftover);
        leftover.clear();
    }
    let n = reader.read_until(b'\n', out).map_err(Error::Io)?;
    if !out.is_empty() {
        if out.last() == Some(&b'\n') {
            rt_sep.extend_from_slice(b"\n");
        }
        return Ok(true);
    }
    Ok(n > 0)
}

fn read_paragraph_record<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
    leftover: &mut Vec<u8>,
) -> Result<bool> {
    let mut line = Vec::<u8>::new();
    let mut saw_content = false;
    loop {
        line.clear();
        if !leftover.is_empty() {
            if let Some(pos) = memchr(b'\n', leftover) {
                line.extend_from_slice(&leftover[..=pos]);
                leftover.drain(..=pos);
            } else {
                line.extend_from_slice(leftover);
                leftover.clear();
                let _ = reader.read_until(b'\n', &mut line).map_err(Error::Io)?;
            }
        } else {
            let n = reader.read_until(b'\n', &mut line).map_err(Error::Io)?;
            if n == 0 {
                // EOF: trim trailing newlines accumulated in `out` to match
                // gawk paragraph mode and capture them in RT so the last
                // record reports its terminator.
                let end = trim_end_record_bytes(out);
                rt_sep.extend_from_slice(&out[end..]);
                out.truncate(end);
                return Ok(saw_content);
            }
        }
        if line.is_empty() {
            let end = trim_end_record_bytes(out);
            rt_sep.extend_from_slice(&out[end..]);
            out.truncate(end);
            return Ok(saw_content);
        }
        let is_blank = line
            .iter()
            .all(|b| matches!(*b, b' ' | b'\t' | b'\r' | b'\n'));
        if is_blank {
            if saw_content {
                // gawk parity: RT for paragraph mode is the FULL run of trailing
                // newlines from the last content line PLUS the blank lines
                // separating records (e.g. `b\n\n` → RT == "\n\n"). Capture the
                // trailing newlines stripped from `out` first, then the current
                // blank line, then drain any additional consecutive blank lines.
                let end = trim_end_record_bytes(out);
                rt_sep.extend_from_slice(&out[end..]);
                out.truncate(end);
                rt_sep.extend_from_slice(&line);
                let mut peek = Vec::<u8>::new();
                loop {
                    peek.clear();
                    if !leftover.is_empty() {
                        if let Some(pos) = memchr(b'\n', leftover) {
                            peek.extend_from_slice(&leftover[..=pos]);
                            leftover.drain(..=pos);
                        } else {
                            peek.extend_from_slice(leftover);
                            leftover.clear();
                            let _ = reader.read_until(b'\n', &mut peek).map_err(Error::Io)?;
                        }
                    } else {
                        let n = reader.read_until(b'\n', &mut peek).map_err(Error::Io)?;
                        if n == 0 {
                            break;
                        }
                    }
                    let peek_blank = peek
                        .iter()
                        .all(|b| matches!(*b, b' ' | b'\t' | b'\r' | b'\n'));
                    if peek_blank {
                        rt_sep.extend_from_slice(&peek);
                    } else {
                        // Push the non-blank line back into leftover for the
                        // next record.
                        let mut new_leftover = peek.clone();
                        new_leftover.extend_from_slice(leftover);
                        *leftover = new_leftover;
                        break;
                    }
                }
                return Ok(true);
            }
            continue;
        }
        saw_content = true;
        out.extend_from_slice(&line);
    }
}

fn read_until_regex_bytes<R: BufRead>(
    reader: &mut R,
    re: &BytesRegex,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
    leftover: &mut Vec<u8>,
) -> Result<bool> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(m) = re.find(leftover) {
            out.extend_from_slice(&leftover[..m.start()]);
            rt_sep.extend_from_slice(m.as_bytes());
            leftover.drain(..m.end());
            return Ok(true);
        }
        let n = reader.read(&mut chunk).map_err(Error::Io)?;
        if n == 0 {
            if leftover.is_empty() {
                return Ok(false);
            }
            out.extend_from_slice(leftover);
            leftover.clear();
            return Ok(true);
        }
        leftover.extend_from_slice(&chunk[..n]);
    }
}

fn read_until_bytes<R: BufRead>(
    reader: &mut R,
    delim: &[u8],
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
    leftover: &mut Vec<u8>,
) -> Result<bool> {
    if delim.is_empty() {
        return Ok(false);
    }
    // Single-byte literal: serve from leftover first, then fall through to `read_until`.
    if delim.len() == 1 {
        if !leftover.is_empty() {
            if let Some(pos) = memchr(delim[0], leftover) {
                out.extend_from_slice(&leftover[..pos]);
                leftover.drain(..=pos);
                rt_sep.push(delim[0]);
                return Ok(true);
            }
            out.extend_from_slice(leftover);
            leftover.clear();
        }
        let n = reader.read_until(delim[0], out).map_err(Error::Io)?;
        if !out.is_empty() {
            if out.last() == Some(&delim[0]) {
                out.pop();
                rt_sep.push(delim[0]);
            }
            return Ok(true);
        }
        return Ok(n > 0);
    }
    // Multi-byte literal: scan leftover; on miss, refill from reader.
    let mut chunk = [0u8; 4096];
    loop {
        if leftover.len() >= delim.len() {
            for start in 0..=leftover.len() - delim.len() {
                if &leftover[start..start + delim.len()] == delim {
                    out.extend_from_slice(&leftover[..start]);
                    rt_sep.extend_from_slice(delim);
                    leftover.drain(..start + delim.len());
                    return Ok(true);
                }
            }
        }
        let n = reader.read(&mut chunk).map_err(Error::Io)?;
        if n == 0 {
            if leftover.is_empty() {
                return Ok(false);
            }
            out.extend_from_slice(leftover);
            leftover.clear();
            return Ok(true);
        }
        leftover.extend_from_slice(&chunk[..n]);
    }
}

/// Split `data` into records for mmap / slurp paths.
pub fn split_input_into_records<'a>(
    data: &'a [u8],
    rs: &str,
    regex_rs: Option<&BytesRegex>,
) -> Vec<&'a [u8]> {
    if data.is_empty() {
        return Vec::new();
    }
    if rs == "\n" {
        return split_lines_unix(data);
    }
    if rs.is_empty() {
        return split_paragraph_mmap(data);
    }
    if let Some(re) = regex_rs {
        return split_by_regex_mmap(data, re);
    }
    split_by_delimiter_mmap(data, rs.as_bytes())
}

fn split_by_regex_mmap<'a>(data: &'a [u8], re: &BytesRegex) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(data) {
        out.push(&data[last..m.start()]);
        last = m.end();
    }
    out.push(&data[last..]);
    out
}

fn split_lines_unix(data: &[u8]) -> Vec<&[u8]> {
    let mut v = Vec::new();
    let mut pos = 0usize;
    let len = data.len();
    while pos < len {
        let eol = memchr(b'\n', &data[pos..len])
            .map(|i| pos + i)
            .unwrap_or(len);
        // gawk parity: do NOT strip a trailing `\r` here — `\r\n` input yields a
        // record that includes the `\r`. Only the `\n` is the record terminator.
        v.push(&data[pos..eol]);
        pos = eol + 1;
    }
    v
}

/// `RS == ""` — records separated by one or more blank lines (gawk paragraph mode).
fn split_paragraph_mmap(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut cur_end: usize = 0;
    let mut pos = 0usize;
    let len = data.len();
    while pos < len {
        let eol = memchr(b'\n', &data[pos..len])
            .map(|i| pos + i)
            .unwrap_or(len);
        let line = &data[pos..eol];
        let blank = line.iter().all(|b| b.is_ascii_whitespace());
        if blank {
            if let Some(s) = start.take() {
                // gawk paragraph mode strips trailing newlines/CRs from each record.
                let trimmed_end = s + trim_end_record_bytes(&data[s..cur_end]);
                out.push(&data[s..trimmed_end]);
            }
        } else if start.is_none() {
            start = Some(pos);
            cur_end = if eol < len { eol + 1 } else { eol };
        } else {
            cur_end = if eol < len { eol + 1 } else { eol };
        }
        pos = if eol < len { eol + 1 } else { len };
    }
    if let Some(s) = start {
        let trimmed_end = s + trim_end_record_bytes(&data[s..cur_end]);
        out.push(&data[s..trimmed_end]);
    }
    out
}

fn split_by_delimiter_mmap<'a>(data: &'a [u8], delim: &[u8]) -> Vec<&'a [u8]> {
    if delim.is_empty() {
        return vec![data];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    let finder = memchr::memmem::Finder::new(delim);
    while start < data.len() {
        let hay = &data[start..];
        if let Some(rel) = finder.find(hay) {
            let abs = start + rel;
            out.push(&data[start..abs]);
            start = abs + delim.len();
        } else {
            out.push(&data[start..]);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::SharedInputReader;
    use std::io::{BufReader, Cursor, Read};
    use std::sync::{Arc, Mutex};

    fn shared_reader(data: &[u8]) -> SharedInputReader {
        Arc::new(Mutex::new(BufReader::new(
            Box::new(Cursor::new(data.to_vec())) as Box<dyn Read + Send>,
        )))
    }

    #[test]
    fn trim_end_record_bytes_strips_only_trailing_lf_not_cr() {
        // gawk parity: `\r` is part of the record on Unix; only `\n` is stripped.
        assert_eq!(trim_end_record_bytes(b"abc\n"), 3);
        assert_eq!(trim_end_record_bytes(b"abc\r\n"), 4); // keeps the CR
        assert_eq!(trim_end_record_bytes(b"abc\r\r\n"), 5); // keeps both CRs
        assert_eq!(trim_end_record_bytes(b"abc\n\n"), 3); // strips multiple trailing LFs
    }

    #[test]
    fn trim_end_record_bytes_empty() {
        assert_eq!(trim_end_record_bytes(b""), 0);
    }

    #[test]
    fn trim_end_record_bytes_only_lf_yields_zero_len_content() {
        assert_eq!(trim_end_record_bytes(b"\n"), 0);
        // `\r` is no longer stripped — only `\n` between them is.
        assert_eq!(trim_end_record_bytes(b"\r\n"), 1);
    }

    #[test]
    fn trim_end_record_bytes_preserves_inner_newlines() {
        assert_eq!(trim_end_record_bytes(b"a\nb"), 3);
    }

    #[test]
    fn split_empty_input_yields_empty_vec() {
        assert!(split_input_into_records(b"", "\n", None).is_empty());
        assert!(split_input_into_records(b"", "XX", None).is_empty());
    }

    #[test]
    fn split_newline_default() {
        let d = b"a\nb\n";
        let r = split_input_into_records(d, "\n", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..]]);
    }

    #[test]
    fn split_newline_preserves_cr_before_lf() {
        // gawk parity (Unix text mode): `\r` is NOT stripped; only `\n` is the
        // record terminator. A CRLF input yields a record that includes `\r`.
        let d = b"a\r\nb\r\n";
        let r = split_input_into_records(d, "\n", None);
        assert_eq!(r, vec![&b"a\r"[..], &b"b\r"[..]]);
    }

    #[test]
    fn split_newline_last_record_without_newline() {
        let d = b"only";
        let r = split_input_into_records(d, "\n", None);
        assert_eq!(r, vec![&b"only"[..]]);
    }

    #[test]
    fn split_paragraph_mode_blank_line_separator_strips_trailing_newlines() {
        // gawk paragraph mode: trailing newlines/CRs are stripped from each record.
        let d = b"para one line\n\npara two\n";
        let r = split_input_into_records(d, "", None);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], &b"para one line"[..]);
        assert_eq!(r[1], &b"para two"[..]);
    }

    #[test]
    fn split_paragraph_leading_blank_lines_skipped() {
        let d = b"\n\nbody\n\n";
        let r = split_input_into_records(d, "", None);
        assert_eq!(r, vec![&b"body"[..]]);
    }

    #[test]
    fn split_paragraph_whitespace_only_input_yields_no_records() {
        let d = b"\n\n  \t\n";
        let r = split_input_into_records(d, "", None);
        assert!(r.is_empty(), "expected no paragraph records, got {r:?}");
    }

    #[test]
    fn split_custom_rs() {
        let d = b"aXXbXX";
        let r = split_input_into_records(d, "XX", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..]]);
    }

    #[test]
    fn split_single_byte_literal_rs() {
        let d = b"a|b|c";
        let r = split_input_into_records(d, "|", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
    }

    #[test]
    fn split_custom_rs_ending_at_delimiter_omits_trailing_empty_mmap_chunk() {
        // `split_by_delimiter_mmap` stops after the last full record; there is no empty slice
        // after a trailing delimiter (differs from some awk edge cases — behavior is intentional).
        let d = b"aXX";
        let r = split_input_into_records(d, "XX", None);
        assert_eq!(r, vec![&b"a"[..]]);
    }

    #[test]
    fn split_multibyte_literal_rs() {
        let d = "α•β•γ".as_bytes();
        let r = split_input_into_records(d, "•", None);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], "α".as_bytes());
        assert_eq!(r[1], "β".as_bytes());
        assert_eq!(r[2], "γ".as_bytes());
    }

    #[test]
    fn split_regex_rs_mmap() {
        let d = b"axxbxx";
        let re = BytesRegex::new("x+").unwrap();
        let r = split_input_into_records(d, "x+", Some(&re));
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b""[..]]);
    }

    #[test]
    fn read_next_record_default_rs_reads_until_lf() {
        let rdr = shared_reader(b"hi\nthere\n");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "\n", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"hi\n");
        assert_eq!(sep, b"\n");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, "\n", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"there\n");
        assert!(!read_next_record(&rdr, "\n", &mut out, &mut sep, None, &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_literal_multibyte_delimiter() {
        let rdr = shared_reader(b"axXXbXX");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "XX", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"ax");
        assert_eq!(sep, b"XX");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, "XX", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"b");
        assert_eq!(sep, b"XX");
    }

    #[test]
    fn read_next_record_regex_rs_reads_every_record() {
        // Regression: prior to the persistent-leftover fix, only the first record was returned.
        let rdr = shared_reader(b"a---b--c-d");
        let re = BytesRegex::new("-+").unwrap();
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "-+", &mut out, &mut sep, Some(&re), &mut lo).unwrap());
        assert_eq!(out, b"a");
        assert_eq!(sep, b"---");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, "-+", &mut out, &mut sep, Some(&re), &mut lo).unwrap());
        assert_eq!(out, b"b");
        assert_eq!(sep, b"--");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, "-+", &mut out, &mut sep, Some(&re), &mut lo).unwrap());
        assert_eq!(out, b"c");
        assert_eq!(sep, b"-");
        out.clear();
        sep.clear();
        // Final tail (no trailing match) — record is what's left, no separator.
        assert!(read_next_record(&rdr, "-+", &mut out, &mut sep, Some(&re), &mut lo).unwrap());
        assert_eq!(out, b"d");
        assert_eq!(sep, b"");
        assert!(!read_next_record(&rdr, "-+", &mut out, &mut sep, Some(&re), &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_paragraph_mode_blank_line_boundary() {
        // gawk paragraph mode (RS == "") strips trailing newlines from the
        // record. RT captures the full run of newlines/blank lines between
        // records — for `first para line\n\nsecond` that is `\n\n`
        // (the newline ending the content line plus the blank separator).
        let rdr = shared_reader(b"first para line\n\nsecond\n");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"first para line");
        assert_eq!(sep, b"\n\n");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"second");
        assert!(!read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_paragraph_skips_leading_blanks() {
        let rdr = shared_reader(b"\n\nbody\n\n");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"body");
        assert!(!read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_custom_rs_char_strips_separator_from_record() {
        // gawk: `BEGIN { RS=":" } { ... }` on "a:b:c" yields records "a", "b", "c".
        let rdr = shared_reader(b"a:b:c");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, ":", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"a");
        assert_eq!(sep, b":");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, ":", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"b");
        assert_eq!(sep, b":");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, ":", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"c");
        assert!(!read_next_record(&rdr, ":", &mut out, &mut sep, None, &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_multi_char_rs_returns_every_record() {
        // Regression: streaming multi-char `RS` used to return only the first record because
        // the over-read chunk was thrown away after the first match.
        let rdr = shared_reader(b"a--b--c");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "--", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"a");
        assert_eq!(sep, b"--");
        out.clear();
        sep.clear();
        assert!(read_next_record(&rdr, "--", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"b");
        assert_eq!(sep, b"--");
        out.clear();
        sep.clear();
        // Tail with no trailing separator: record is "c", sep is empty, EOF after.
        assert!(read_next_record(&rdr, "--", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"c");
        assert_eq!(sep, b"");
        assert!(!read_next_record(&rdr, "--", &mut out, &mut sep, None, &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_empty_rs_at_eof() {
        let rdr = shared_reader(b"para1\n\npara2");
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"para1");
        out.clear();
        assert!(read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"para2");
        assert!(!read_next_record(&rdr, "", &mut out, &mut sep, None, &mut lo).unwrap());
    }

    #[test]
    fn read_next_record_large_buffer() {
        let data = vec![b'x'; 10000];
        let rdr = shared_reader(&data);
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "\n", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, data);
    }

    #[test]
    fn split_multichar_rs_v2() {
        let d = b"aXYbXYc";
        let r = split_input_into_records(d, "XY", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
    }

    #[test]
    fn split_single_char_rs_v3() {
        let d = b"a:b:c";
        let r = split_input_into_records(d, ":", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
    }

    #[test]
    fn split_multibyte_rs_v3() {
        // UTF-8 'π' is 0xCF 0x80
        let d = "aπbπc".as_bytes();
        let r = split_input_into_records(d, "π", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
    }

    #[test]
    fn split_multichar_rs_boundary_v12() {
        let d = b"abcXYZdef";
        let r = split_input_into_records(d, "XYZ", None);
        assert_eq!(r, vec![&b"abc"[..], &b"def"[..]]);
    }

    #[test]
    fn split_regex_rs_no_match_v12() {
        let d = b"abc";
        let re = regex::bytes::Regex::new("z+").unwrap();
        let r = split_input_into_records(d, "unused", Some(&re));
        assert_eq!(r, vec![&b"abc"[..]]);
    }

    #[test]
    fn split_regex_rs_start_match_v12() {
        let d = b"123abc456";
        let re = regex::bytes::Regex::new("[0-9]+").unwrap();
        let r = split_input_into_records(d, "unused", Some(&re));
        assert_eq!(r, vec![&b""[..], &b"abc"[..], &b""[..]]);
    }

    #[test]
    fn read_next_record_at_boundary_v2() {
        // RS="XY", input="aXY", buffer size might matter but here we test the logic.
        let data = b"aXYb";
        let rdr = shared_reader(data);
        let mut out = Vec::new();
        let mut sep = Vec::new();
        let mut lo = Vec::new();
        assert!(read_next_record(&rdr, "XY", &mut out, &mut sep, None, &mut lo).unwrap());
        assert_eq!(out, b"a");
        assert_eq!(sep, b"XY");
    }

    #[test]
    fn split_space_rs_v2() {
        // RS=" " means split on EACH literal space.
        let d = b"a b  c";
        let r = split_input_into_records(d, " ", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b""[..], &b"c"[..]]);
    }

    #[test]
    fn split_regex_rs_v2() {
        use regex::bytes::Regex;
        let d = b"a1b22c";
        let re = Regex::new("[0-9]+").unwrap();
        let r = split_input_into_records(d, "unused", Some(&re));
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
    }

    #[test]
    fn split_empty_input_v7() {
        let r = split_input_into_records(b"", "\n", None);
        assert!(r.is_empty());
    }

    #[test]
    fn split_only_rs_v7() {
        let r = split_input_into_records(b"\n", "\n", None);
        assert_eq!(r, vec![&b""[..]]);
    }

    #[test]
    fn split_trailing_rs_v7() {
        let r = split_input_into_records(b"a\n", "\n", None);
        assert_eq!(r, vec![&b"a"[..]]);
    }

    #[test]
    fn split_multiple_rs_v7() {
        let r = split_input_into_records(b"a\n\nb", "\n", None);
        assert_eq!(r, vec![&b"a"[..], &b""[..], &b"b"[..]]);
    }

    #[test]
    fn split_paragraph_multiple_blank_lines_v10() {
        // RS="" -> paragraph mode
        let d = b"p1\n\n\n\np2\n\np3";
        let r = split_input_into_records(d, "", None);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], b"p1");
        assert_eq!(r[1], b"p2");
        assert_eq!(r[2], b"p3");
    }

    #[test]
    fn split_paragraph_with_whitespace_lines_v10() {
        // gawk: lines with only spaces/tabs are also blank lines in paragraph mode.
        let d = b"p1\n  \t  \np2";
        let r = split_input_into_records(d, "", None);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], b"p1");
        assert_eq!(r[1], b"p2");
    }

    #[test]
    fn split_paragraph_leading_trailing_blank_lines_v11() {
        let d = b"\n\np1\n\np2\n\n";
        let r = split_input_into_records(d, "", None);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], b"p1");
        assert_eq!(r[1], b"p2");
    }

    #[test]
    fn split_paragraph_record_with_internal_blank_line_v11() {
        // Only a line that is ENTIRELY blank (or whitespace) separates paragraphs.
        let d = b"line1\nline2\n\nline3";
        let r = split_input_into_records(d, "", None);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], b"line1\nline2");
        assert_eq!(r[1], b"line3");
    }
}
