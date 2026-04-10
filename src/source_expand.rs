//! Gawk-style source directives before parse: `@include`, `@load` (`.awk` only, like include), `@namespace`.

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

fn take_bare_ident(rest: &str) -> Option<(String, &str)> {
    let rest = rest.trim_start();
    let mut i = 0usize;
    let b = rest.as_bytes();
    let c0 = *b.first()?;
    if !(c0.is_ascii_alphabetic() || c0 == b'_') {
        return None;
    }
    i += 1;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphanumeric() || c == b'_' {
            i += 1;
        } else {
            break;
        }
    }
    Some((rest[..i].to_string(), &rest[i..]))
}

/// Expanded program text plus `@namespace` default (gawk-style).
#[derive(Debug, Clone)]
pub struct ExpandedSource {
    pub text: String,
    pub default_namespace: Option<String>,
}

/// Expand `@include` / `@load "*.awk"` recursively; apply `@namespace` (line removed; namespace recorded).
pub fn expand_source_directives(src: &str) -> Result<ExpandedSource> {
    let mut visited = HashSet::new();
    let mut default_ns = None;
    let text = expand_inner(src, None, &mut visited, &mut default_ns)?;
    Ok(ExpandedSource {
        text,
        default_namespace: default_ns,
    })
}

fn expand_inner(
    text: &str,
    base_dir: Option<&Path>,
    visited: &mut HashSet<PathBuf>,
    default_ns: &mut Option<String>,
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
            let inner = fs::read_to_string(&resolved)
                .map_err(|e| Error::ProgramFile(resolved.clone(), e))?;
            let expanded = expand_inner(&inner, resolved.parent(), visited, default_ns)?;
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
                let inner = fs::read_to_string(&resolved)
                    .map_err(|e| Error::ProgramFile(resolved.clone(), e))?;
                let expanded = expand_inner(&inner, resolved.parent(), visited, default_ns)?;
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
                    "`@load` {path_str}: awkrs only inlines `.awk` source (like `@include`). \
                     gawk `.so` loadable modules are not loaded; the usual builtins from those \
                     extensions (`chdir`, `stat`, `fts`, `ord`, `readfile`, …) are implemented \
                     in awkrs and do not require `@load`"
                ),
            });
        }
        if trimmed.starts_with("@namespace") {
            let rest = trimmed.strip_prefix("@namespace").unwrap().trim_start();
            if let Some((ns, _)) = take_double_quoted(rest) {
                *default_ns = Some(ns);
            } else if let Some((ns, _)) = take_bare_ident(rest) {
                *default_ns = Some(ns);
            } else {
                return Err(Error::Parse {
                    line: line_no,
                    msg: "malformed `@namespace` (expected `@namespace \"name\"` or `@namespace name`)"
                        .into(),
                });
            }
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
    fn namespace_line_dropped_and_recorded() {
        let e = expand_source_directives("@namespace \"ns\"\nBEGIN { }\n").unwrap();
        assert!(!e.text.contains("@namespace"));
        assert!(e.text.contains("BEGIN"));
        assert_eq!(e.default_namespace.as_deref(), Some("ns"));
    }

    #[test]
    fn load_awk_file_inlines_like_include() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let inc = dir.join(format!("awkrs_load_inc_{id}.awk"));
        std::fs::write(&inc, "function f() { return 1 }\n").unwrap();
        let main = format!("@load \"{}\"\nBEGIN {{ print f() }}\n", inc.display());
        let e = expand_source_directives(&main).unwrap();
        assert!(e.text.contains("function f"));
        let _ = std::fs::remove_file(&inc);
    }
}
