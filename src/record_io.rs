//! Record boundaries for configurable `RS` (newline default, multi-byte separator, paragraph mode,
//! gawk-style regex `RS` when `RS` is longer than one character).

use crate::error::{Error, Result};
use crate::runtime::SharedInputReader;
use memchr::memchr;
use regex::bytes::Regex as BytesRegex;
use std::io::BufRead;

/// Trim trailing `\r` / `\n` from a byte slice (record content).
pub fn trim_end_record_bytes(buf: &[u8]) -> usize {
    let mut end = buf.len();
    while end > 0 && (buf[end - 1] == b'\n' || buf[end - 1] == b'\r') {
        end -= 1;
    }
    end
}

/// Read the next record from `reader` into `out` using `RS` string `rs`.
/// - `rs == "\n"` → read until `\n` (POSIX default).
/// - `rs == ""` → paragraph mode (records separated by blank lines).
/// - `regex_rs == Some` → gawk: `RS` length > 1, treated as a regex; `rt_sep` is the matched bytes.
/// - else → read until `rs` appears as a byte substring (literal `RS`).
pub fn read_next_record(
    reader: &SharedInputReader,
    rs: &str,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
    regex_rs: Option<&BytesRegex>,
) -> Result<bool> {
    out.clear();
    rt_sep.clear();
    let mut guard = reader
        .lock()
        .map_err(|_| Error::Runtime("input reader lock poisoned".into()))?;
    let r = &mut *guard;
    if rs == "\n" {
        let n = r.read_until(b'\n', out).map_err(Error::Io)?;
        if n > 0 {
            rt_sep.extend_from_slice(b"\n");
        }
        return Ok(n > 0);
    }
    if rs.is_empty() {
        let ok = read_paragraph_record(r, out, rt_sep)?;
        return Ok(ok);
    }
    if let Some(re) = regex_rs {
        return read_until_regex_bytes(r, re, out, rt_sep);
    }
    read_until_bytes(r, rs.as_bytes(), out, rt_sep)
}

fn read_paragraph_record<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
) -> Result<bool> {
    let mut line = String::new();
    let mut saw_content = false;
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            return Ok(saw_content);
        }
        if line.trim().is_empty() {
            if saw_content {
                rt_sep.extend_from_slice(b"\n");
                return Ok(true);
            }
            continue;
        }
        saw_content = true;
        out.extend_from_slice(line.as_bytes());
    }
}

fn read_until_regex_bytes<R: BufRead>(
    reader: &mut R,
    re: &BytesRegex,
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
) -> Result<bool> {
    let mut buf = Vec::<u8>::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(m) = re.find(&buf) {
            out.extend_from_slice(&buf[..m.start()]);
            rt_sep.extend_from_slice(m.as_bytes());
            buf.drain(..m.end());
            return Ok(true);
        }
        let n = reader.read(&mut chunk).map_err(Error::Io)?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(false);
            }
            out.extend_from_slice(&buf);
            return Ok(true);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn read_until_bytes<R: BufRead>(
    reader: &mut R,
    delim: &[u8],
    out: &mut Vec<u8>,
    rt_sep: &mut Vec<u8>,
) -> Result<bool> {
    if delim.is_empty() {
        return Ok(false);
    }
    if delim.len() == 1 {
        let n = reader.read_until(delim[0], out).map_err(Error::Io)?;
        if n > 0 {
            rt_sep.push(delim[0]);
        }
        return Ok(n > 0);
    }
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Ok(!out.is_empty()),
            Ok(_) => {}
            Err(e) => return Err(Error::Io(e)),
        }
        out.push(byte[0]);
        if out.len() >= delim.len() && out[out.len() - delim.len()..] == *delim {
            out.truncate(out.len() - delim.len());
            rt_sep.extend_from_slice(delim);
            return Ok(true);
        }
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
        let end = if eol > pos && data[eol - 1] == b'\r' {
            eol - 1
        } else {
            eol
        };
        v.push(&data[pos..end]);
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
                out.push(&data[s..cur_end]);
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
        out.push(&data[s..cur_end]);
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

    #[test]
    fn split_newline_default() {
        let d = b"a\nb\n";
        let r = split_input_into_records(d, "\n", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..]]);
    }

    #[test]
    fn split_custom_rs() {
        let d = b"aXXbXX";
        let r = split_input_into_records(d, "XX", None);
        assert_eq!(r, vec![&b"a"[..], &b"b"[..]]);
    }

    #[test]
    fn split_regex_rs_mmap() {
        let d = b"axxbxx";
        let re = BytesRegex::new("x+").unwrap();
        let r = split_input_into_records(d, "x+", Some(&re));
        assert_eq!(r, vec![&b"a"[..], &b"b"[..], &b""[..]]);
    }
}
