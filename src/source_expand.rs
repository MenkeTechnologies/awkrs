//! Gawk-style source directives before parse: `@include`, `@load` (`.awk` only, like include), `@namespace` (ignored).

use crate::error::{Error, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn take_double_quoted(rest: &str) -> Option<(String, &str)> {
    let rest = rest.trim_start();
    let b = rest.as_bytes();
    if b.first() != Some(&b'"') {
        return None;
    }
    let mut out = String::new();
    let mut i = 1usize;
    while i < b.len() {
        if b[i] == b'"' {
            return Some((out, &rest[i + 1..]));
        }
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            match b[i] {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'\\' | b'"' => out.push(b[i] as char),
                x => out.push(x as char),
            }
            i += 1;
            continue;
        }
        if b[i] == b'\n' {
            return None;
        }
        let ch = rest[i..].chars().next()?;
        out.push(ch);
        i += ch.len_utf8();
    }
    None
}

/// Expand `@include` / `@load "*.awk"` recursively; warn and drop `@namespace` lines.
pub fn expand_source_directives(src: &str) -> Result<String> {
    let mut visited = HashSet::new();
    expand_inner(src, None, &mut visited)
}

fn expand_inner(
    text: &str,
    base_dir: Option<&Path>,
    visited: &mut HashSet<PathBuf>,
) -> Result<String> {
    let mut out = String::new();
    for (line_no, line) in text.lines().enumerate() {
        let line_no = line_no + 1;
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("@include") {
            let rest = rest.trim_start();
            let Some((path_str, _after)) = take_double_quoted(rest) else {
                return Err(Error::Parse {
                    line: line_no,
                    msg: "malformed `@include` (expected `@include \"file\"`)".into(),
                });
            };
            let resolved = resolve_include_path(base_dir, &path_str)?;
            let canon = fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
            if !visited.insert(canon.clone()) {
                return Err(Error::Parse {
                    line: line_no,
                    msg: format!("@include cycle: {}", canon.display()),
                });
            }
            let inner =
                fs::read_to_string(&resolved).map_err(|e| Error::ProgramFile(resolved.clone(), e))?;
            let expanded = expand_inner(&inner, resolved.parent(), visited)?;
            visited.remove(&canon);
            out.push_str(&expanded);
            if !expanded.is_empty() && !expanded.ends_with('\n') {
                out.push('\n');
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("@load") {
            let rest = rest.trim_start();
            let Some((path_str, _after)) = take_double_quoted(rest) else {
                return Err(Error::Parse {
                    line: line_no,
                    msg: "malformed `@load` (expected `@load \"file\"`)".into(),
                });
            };
            let pl = path_str.to_ascii_lowercase();
            if pl.ends_with(".awk") {
                let resolved = resolve_include_path(base_dir, &path_str)?;
                let canon = fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
                if !visited.insert(canon.clone()) {
                    return Err(Error::Parse {
                        line: line_no,
                        msg: format!("@load cycle: {}", canon.display()),
                    });
                }
                let inner =
                    fs::read_to_string(&resolved).map_err(|e| Error::ProgramFile(resolved.clone(), e))?;
                let expanded = expand_inner(&inner, resolved.parent(), visited)?;
                visited.remove(&canon);
                out.push_str(&expanded);
                if !expanded.is_empty() && !expanded.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
            return Err(Error::Parse {
                line: line_no,
                msg: format!(
                    "`@load` {path_str}: only `.awk` source is supported (shared-object extensions are not loaded)"
                ),
            });
        }
        if trimmed.starts_with("@namespace") {
            eprintln!("awkrs: warning: `@namespace` is ignored (not implemented)");
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

fn resolve_include_path(base_dir: Option<&Path>, path_str: &str) -> Result<PathBuf> {
    let p = Path::new(path_str);
    if p.is_absolute() {
        Ok(p.to_path_buf())
    } else if let Some(dir) = base_dir {
        Ok(dir.join(p))
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .map_err(Error::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_quoted_parses_path() {
        let (s, tail) = take_double_quoted(r#" "a/b.awk" x"#).unwrap();
        assert_eq!(s, "a/b.awk");
        assert_eq!(tail.trim(), "x");
    }

    #[test]
    fn namespace_line_dropped() {
        let out = expand_source_directives("@namespace \"ns\"\nBEGIN { }\n").unwrap();
        assert!(!out.contains("@namespace"));
        assert!(out.contains("BEGIN"));
    }

    #[test]
    fn load_awk_file_inlines_like_include() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let inc = dir.join(format!("awkrs_load_inc_{id}.awk"));
        std::fs::write(&inc, "function f() { return 1 }\n").unwrap();
        let main = format!("@load \"{}\"\nBEGIN {{ print f() }}\n", inc.display());
        let out = expand_source_directives(&main).unwrap();
        assert!(out.contains("function f"));
        let _ = std::fs::remove_file(&inc);
    }
}
