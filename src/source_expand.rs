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

/// gawk’s **bundled** extension module names (typically `@load "filefuncs"` or `filefuncs.so`).
/// awkrs implements these in Rust; the directive is accepted and ignored (no `dlopen`).
const NATIVE_GAWK_EXTENSIONS: &[&str] = &[
    "filefuncs",
    "readdir",
    "time",
    "inplace",
    "ordchr",
    "readfile",
    "revoutput",
    "revtwoway",
    "rwarray",
    "intdiv",
];

/// True when `path_str` refers to one of those modules (with or without `.so`, any directory prefix).
pub(crate) fn is_native_gawk_extension_path(path_str: &str) -> bool {
    let stem = Path::new(path_str)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path_str);
    let name = stem.to_ascii_lowercase();
    NATIVE_GAWK_EXTENSIONS.contains(&name.as_str())
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
    /// `text` field.
    pub text: String,
    /// `default_namespace` field.
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
            let Some((path_str, after)) = take_double_quoted(rest) else {
                return Err(Error::Parse {
                    line: line_no,
                    msg: "malformed `@include` (expected `@include \"file\"`)".into(),
                });
            };
            include_once(&path_str, base_dir, line_no, visited, default_ns, &mut out)?;
            push_directive_tail(&mut out, after);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("@load") {
            let rest = rest.trim_start();
            let Some((path_str, after)) = take_double_quoted(rest) else {
                return Err(Error::Parse {
                    line: line_no,
                    msg: "malformed `@load` (expected `@load \"file\"`)".into(),
                });
            };
            let pl = path_str.to_ascii_lowercase();
            if pl.ends_with(".awk") {
                include_once(&path_str, base_dir, line_no, visited, default_ns, &mut out)?;
                push_directive_tail(&mut out, after);
                continue;
            }
            if is_native_gawk_extension_path(&path_str) {
                // Builtins already present for the whole run; gawkapi / dlopen not used.
                push_directive_tail(&mut out, after);
                continue;
            }
            return Err(Error::Parse {
                line: line_no,
                msg: format!(
                    "`@load` {path_str}: awkrs only inlines `.awk` source or recognizes gawk’s \
                     bundled extension names (implemented natively). Arbitrary third-party `.so` \
                     modules (gawkapi) are not loaded"
                ),
            });
        }
        if trimmed.starts_with("@namespace") {
            let rest = trimmed.strip_prefix("@namespace").unwrap().trim_start();
            // After the namespace identifier, capture any trailing source (e.g.
            // `; BEGIN { … }` on the same line) so we don't silently drop it.
            let after_ns: &str = if let Some((ns, after)) = take_double_quoted(rest) {
                *default_ns = Some(ns);
                after
            } else if let Some((ns, after)) = take_bare_ident(rest) {
                *default_ns = Some(ns);
                after
            } else {
                return Err(Error::Parse {
                    line: line_no,
                    msg: "malformed `@namespace` (expected `@namespace \"name\"` or `@namespace name`)"
                        .into(),
                });
            };
            push_directive_tail(&mut out, after_ns);
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// Emit the source that follows a directive on its line.
///
/// gawk's grammar ends `@include`, `@load` and `@namespace` with a
/// `statement_term` (a newline, or `;` plus optional newlines), so
/// `@load "ordchr"; BEGIN { … }` is one valid line. The remainder is emitted on
/// a line of its own, as if the directive line had ended where the directive
/// did; dropping it used to discard the rest of the program without a
/// diagnostic.
fn push_directive_tail(out: &mut String, after: &str) {
    let after = after.trim_start();
    let rest = after.strip_prefix(';').unwrap_or(after);
    if !rest.trim().is_empty() {
        out.push_str(rest);
        out.push('\n');
    }
}

/// Inline one `@include` (or `@load "x.awk"`) source, at most once per run.
///
/// gawk's `add_srcfile` drops a file that was already included (only a lint
/// warning), so including a library twice, or two libraries that include each
/// other, loads each one once instead of failing on a duplicate function or a
/// cycle. A file that cannot be found is a parse-time error (exit 1), as in
/// gawk's `include_source`.
fn include_once(
    name: &str,
    base_dir: Option<&Path>,
    line_no: usize,
    visited: &mut HashSet<PathBuf>,
    default_ns: &mut Option<String>,
    out: &mut String,
) -> Result<()> {
    let not_found = |e: std::io::Error| Error::Parse {
        line: line_no,
        msg: format!("cannot open source file `{name}' for reading: {e}"),
    };
    let resolved = match find_source(name) {
        Ok(p) => p,
        // awkrs extension: a name gawk cannot find relative to the working
        // directory or AWKPATH is also tried next to the including file.
        Err(e) => match base_dir.map(|d| d.join(name)).filter(|p| p.is_file()) {
            Some(p) => p,
            None => return Err(not_found(e)),
        },
    };
    let canon = fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
    if !visited.insert(canon) {
        return Ok(());
    }
    let inner = fs::read_to_string(&resolved).map_err(not_found)?;
    let expanded = expand_inner(&inner, resolved.parent(), visited, default_ns)?;
    out.push_str(&expanded);
    if !expanded.is_empty() && !expanded.ends_with('\n') {
        out.push('\n');
    }
    Ok(())
}

/// Locate an awk source file the way gawk's `find_source` does (io.c).
///
/// A name containing `/` is used as given. Any other name is searched for in
/// each `AWKPATH` directory (`.` when the variable is unset or empty; an empty
/// component also means `.`). If that fails, the whole search is repeated with
/// `.awk` appended, so `-f lib`, `-i lib` and `@include "lib"` all find
/// `lib.awk`. `-f -` (standard input) is returned unchanged.
pub(crate) fn find_source(name: &str) -> std::io::Result<PathBuf> {
    if name == "-" {
        return Ok(PathBuf::from(name));
    }
    search_awkpath(name).or_else(|e| search_awkpath(&format!("{name}.awk")).map_err(|_| e))
}

fn search_awkpath(name: &str) -> std::io::Result<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return fs::metadata(&p).map(|_| p);
    }
    let awkpath = std::env::var("AWKPATH")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".to_string());
    let mut last_err = std::io::Error::from(std::io::ErrorKind::NotFound);
    for dir in awkpath.split(':') {
        let p = if dir.is_empty() || dir == "." || dir == "./" {
            PathBuf::from(name)
        } else {
            Path::new(dir).join(name)
        };
        match fs::metadata(&p) {
            Ok(_) => return Ok(p),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
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
    fn take_double_quoted_parses_escapes() {
        let (s, tail) = take_double_quoted(r#" "a\nb\t\"\\" tail"#).unwrap();
        assert_eq!(s, "a\nb\t\"\\");
        assert_eq!(tail.trim(), "tail");
    }

    #[test]
    fn take_double_quoted_unclosed_returns_none() {
        assert!(take_double_quoted(r#" "no_close"#).is_none());
    }

    #[test]
    fn take_double_quoted_raw_newline_in_string_returns_none() {
        assert!(take_double_quoted(" \"x\ny\"").is_none());
    }

    #[test]
    fn namespace_last_line_wins() {
        let e = expand_source_directives("@namespace \"first\"\n@namespace second\nBEGIN {}\n")
            .unwrap();
        assert_eq!(e.default_namespace.as_deref(), Some("second"));
        assert!(!e.text.contains("@namespace"));
    }

    #[test]
    fn namespace_line_dropped_and_recorded() {
        let e = expand_source_directives("@namespace \"ns\"\nBEGIN { }\n").unwrap();
        assert!(!e.text.contains("@namespace"));
        assert!(e.text.contains("BEGIN"));
        assert_eq!(e.default_namespace.as_deref(), Some("ns"));
    }

    #[test]
    fn load_bundled_extension_name_is_noop() {
        let e = expand_source_directives("@load \"filefuncs\"\nBEGIN { x = 1 }\n").unwrap();
        assert!(!e.text.contains("@load"));
        assert!(e.text.contains("BEGIN"));
    }

    #[test]
    fn load_bundled_extension_so_suffix_is_noop() {
        let e = expand_source_directives("@load \"./filefuncs.so\"\nBEGIN { }\n").unwrap();
        assert!(!e.text.contains("@load"));
    }

    #[test]
    fn load_arbitrary_so_still_errors() {
        let r = expand_source_directives("@load \"vendor_foo.so\"\n");
        assert!(r.is_err(), "{r:?}");
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

    #[test]
    fn namespace_bare_identifier_accepted() {
        let e = expand_source_directives("@namespace myns\nBEGIN { }\n").unwrap();
        assert_eq!(e.default_namespace.as_deref(), Some("myns"));
        assert!(!e.text.contains("@namespace"));
        assert!(e.text.contains("BEGIN"));
    }

    #[test]
    fn namespace_malformed_errors() {
        let r = expand_source_directives("@namespace\nBEGIN {}\n");
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn include_malformed_missing_quote_errors() {
        let r = expand_source_directives("@include foo.awk\n");
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn include_cycle_loads_each_file_once() {
        // gawk's add_srcfile skips a file that is already included, so two
        // libraries that include each other load once each instead of failing.
        // (This test used to pin a "cycle" error that gawk never reports.)
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let a = dir.join(format!("awkrs_inc_a_{id}.awk"));
        let b = dir.join(format!("awkrs_inc_b_{id}.awk"));
        std::fs::write(
            &a,
            format!(
                "@include \"{}\"\n",
                b.file_name().unwrap().to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::write(
            &b,
            format!(
                "@include \"{}\"\n",
                a.file_name().unwrap().to_string_lossy()
            ),
        )
        .unwrap();
        let main = format!("@include \"{}\"\n", a.display());
        let e = expand_source_directives(&main).expect("a cycle is not an error");
        assert_eq!(e.text.matches("@include").count(), 0, "{}", e.text);
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn load_native_extension_case_insensitive_stem() {
        let e = expand_source_directives("@load \"./FileFuncs.So\"\nBEGIN {}\n").unwrap();
        assert!(!e.text.contains("@load"));
        assert!(e.text.contains("BEGIN"));
    }

    #[test]
    fn include_inlines_twice_sequential() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let one = dir.join(format!("awkrs_inc_one_{id}.awk"));
        let two = dir.join(format!("awkrs_inc_two_{id}.awk"));
        std::fs::write(&one, "function one() { return 1 }\n").unwrap();
        std::fs::write(&two, "function two() { return 2 }\n").unwrap();
        let main = format!(
            "@include \"{}\"\n@include \"{}\"\nBEGIN {{ }}\n",
            one.display(),
            two.display()
        );
        let e = expand_source_directives(&main).unwrap();
        assert!(e.text.contains("function one"));
        assert!(e.text.contains("function two"));
        let _ = std::fs::remove_file(&one);
        let _ = std::fs::remove_file(&two);
    }

    #[test]
    fn include_empty_file_expands_to_nothing_between_directives() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let empty = dir.join(format!("awkrs_inc_empty_{id}.awk"));
        std::fs::write(&empty, "").unwrap();
        let main = format!("@include \"{}\"\nBEGIN {{ x = 1 }}\n", empty.display());
        let e = expand_source_directives(&main).unwrap();
        assert!(e.text.contains("BEGIN") && e.text.contains("x = 1"));
        let _ = std::fs::remove_file(&empty);
    }

    #[test]
    fn include_missing_file_errors() {
        let p = std::env::temp_dir().join(format!(
            "awkrs_no_such_include_{}_{}.awk",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let main = format!("@include \"{}\"\nBEGIN {{}}\n", p.display());
        let r = expand_source_directives(&main);
        assert!(r.is_err(), "expected error for missing include, got {r:?}");
    }

    #[test]
    fn include_nested_recursive() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let p1 = dir.join(format!("awkrs_inc_n1_{id}.awk"));
        let p2 = dir.join(format!("awkrs_inc_n2_{id}.awk"));

        std::fs::write(
            &p1,
            format!(
                "@include \"{}\"\nfunction f1() {{}}",
                p2.file_name().unwrap().to_str().unwrap()
            ),
        )
        .unwrap();
        std::fs::write(&p2, "function f2() {}").unwrap();

        let main = format!("@include \"{}\"", p1.display());
        let e = expand_source_directives(&main).unwrap();
        assert!(e.text.contains("function f2"));
        assert!(e.text.contains("function f1"));

        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn take_bare_ident_logic() {
        assert_eq!(take_bare_ident("  abc_123 def").unwrap().0, "abc_123");
        assert_eq!(take_bare_ident("_start ").unwrap().0, "_start");
        assert!(take_bare_ident("  123abc").is_none());
    }

    #[test]
    fn find_source_absolute_path_is_used_as_given() {
        let p = std::env::temp_dir().join(format!("awkrs_fs_abs_{}.awk", std::process::id()));
        std::fs::write(&p, "").unwrap();
        let res = find_source(p.to_str().unwrap()).unwrap();
        assert_eq!(res, p);
        // gawk retries with `.awk` appended when the name itself is missing.
        let stem = p.with_extension("");
        assert_eq!(find_source(stem.to_str().unwrap()).unwrap(), p);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn include_relative_path_v2() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let inc1 = dir.join(format!("awkrs_inc1_{id}.awk"));
        let inc2 = dir.join(format!("awkrs_inc2_{id}.awk"));

        // inc1 includes inc2 via relative path
        std::fs::write(
            &inc1,
            format!(
                "@include \"{}\"\n",
                inc2.file_name().unwrap().to_str().unwrap()
            ),
        )
        .unwrap();
        std::fs::write(&inc2, "function f() { return 2 }\n").unwrap();

        // main includes inc1 via absolute path
        let main = format!("@include \"{}\"\nBEGIN {{ print f() }}\n", inc1.display());
        let e = expand_source_directives(&main).unwrap();
        assert!(e.text.contains("function f"));

        let _ = std::fs::remove_file(&inc1);
        let _ = std::fs::remove_file(&inc2);
    }

    #[test]
    fn multiple_directives_on_one_line_v2() {
        // gawk doesn't typically support multiple @directives on one line if they consume the rest of the line,
        // but let's see how our expander handles it.
        let main = "@load \"filefuncs\" @include \"nonexistent.awk\"\nBEGIN {}";
        // If it treats '@load' as consuming the line, it might ignore @include.
        let e = expand_source_directives(main).unwrap();
        assert!(!e.text.contains("@load"));
    }

    #[test]
    fn namespace_with_trailing_comment_v7() {
        let e = expand_source_directives("@namespace \"ns\" # comment\nBEGIN {}").unwrap();
        assert_eq!(e.default_namespace.as_deref(), Some("ns"));
        // The expander preserves trailing text after the namespace identifier
        assert!(e.text.contains("# comment"));
    }

    #[test]
    fn include_with_leading_whitespace_v7() {
        let dir = std::env::temp_dir();
        let id = std::process::id();
        let inc = dir.join(format!("awkrs_inc_ws_{id}.awk"));
        std::fs::write(&inc, "BEGIN { x=1 }\n").unwrap();

        let main = format!("  @include \"{}\"\n", inc.display());
        let e = expand_source_directives(&main).unwrap();
        assert!(e.text.contains("BEGIN"));

        let _ = std::fs::remove_file(&inc);
    }

    #[test]
    fn load_with_relative_path_and_no_base_v7() {
        // If no base_dir, it uses current_dir.
        // We can't easily rely on current_dir containing a specific file,
        // but we can test it doesn't panic.
        let r = expand_source_directives("@load \"nonexistent.awk\"");
        assert!(r.is_err());
    }

    #[test]
    fn take_bare_ident_leading_underscore_v7() {
        assert_eq!(take_bare_ident("  _var").unwrap().0, "_var");
    }

    #[test]
    fn take_bare_ident_empty_fails_v7() {
        assert!(take_bare_ident("  ").is_none());
    }
}
