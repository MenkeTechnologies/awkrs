//! Function-call intercept / advice machinery — awkrs/zshrs-original extension.
//!
//! Ported faithfully from zshrs `src/extensions/intercepts.rs`. There is **no
//! POSIX awk counterpart** and no gawk analog: aspect-oriented before/after/around
//! advice on user-defined function calls is unique to this stack. zshrs intercepts
//! *commands* (the shell join point); awkrs adapts the identical semantics onto the
//! natural awk join point — the `funcname(args)` user-function call dispatch.
//!
//! The AOP context is surfaced to advice code as ordinary awk globals mirroring
//! zshrs' names: `INTERCEPT_NAME`, `INTERCEPT_ARGS`, `INTERCEPT_CMD`,
//! `INTERCEPT_MS`, `INTERCEPT_US`, and the `__intercept_proceed` flag. Advice code
//! is awk source evaluated in the current interpreter (no fork).

use crate::bytecode::CompiledProgram;
use crate::runtime::Value;
use std::sync::Arc;

/// AOP advice type — before, after, or around.
///
/// zshrs-original — no C zsh / POSIX awk counterpart. C zsh's closest analog is
/// the function-wrapper hook in `Src/module.c` (`addwrapper()`), but per-function
/// before/after/around AOP intercepts are unique to zshrs (and now awkrs).
#[derive(Debug, Clone)]
pub enum AdviceKind {
    /// Run code before the function executes.
    Before,
    /// Run code after the function executes. `INTERCEPT_MS`/`INTERCEPT_US` available.
    After,
    /// Wrap the function. Code must call `intercept_proceed()` to run the original.
    Around,
}

/// One AOP intercept registered against a function-name pattern.
///
/// zshrs-original — no C / POSIX awk counterpart.
#[derive(Debug, Clone)]
pub struct Intercept {
    /// Pattern to match function names. Supports glob: `"draw_*"`, `"_*"`, `"*"`.
    pub pattern: String,
    /// What kind of advice.
    pub kind: AdviceKind,
    /// awk source to execute as advice (kept for `intercept_list`).
    pub code: String,
    /// Unique ID for removal.
    pub id: u32,
    /// Advice `code` compiled once (at registration) against the running program's
    /// string/slot/function tables so it shares live globals, arrays, and can call
    /// the original via `intercept_proceed()`. `Arc` so cloning an intercept into a
    /// parallel record worker (or the per-hit match set) is a refcount bump, not a
    /// deep copy of the program tables.
    pub program: Arc<CompiledProgram>,
}

/// Internal per-call AOP frame. Pushed when [`Intercept`]s fire for a call so that
/// `intercept_proceed()` (invoked from *around* advice) can reach the original
/// function name and its actual argument [`Value`]s. A stack for re-entrancy
/// (an intercepted function called from inside advice).
#[derive(Debug, Clone)]
pub struct InterceptCall {
    /// Name of the function being intercepted.
    pub name: String,
    /// The original call arguments (awk passes scalars by value; kept faithfully
    /// as `Value`s rather than the lossy space-joined string zshrs must use).
    pub args: Vec<Value>,
    /// Set once `intercept_proceed()` runs the original.
    pub proceeded: bool,
    /// Result of the original function, captured by `intercept_proceed()`.
    pub result: Value,
}

/// Match an intercept pattern against a function name or the full `name args`
/// string. Supports: exact match, glob (`"draw_*"`, `"_*"`, `"*"`), or `"all"`.
///
/// Ported from zshrs `intercept_matches`. zshrs used `glob::Pattern`; awkrs has no
/// `glob` dependency, so the `*`/`?` wildcard match is inlined (see [`glob_match`]).
pub(crate) fn intercept_matches(pattern: &str, fn_name: &str, full_call: &str) -> bool {
    if pattern == "*" || pattern == "all" {
        return true;
    }
    if pattern == fn_name {
        return true;
    }
    if pattern.contains('*') || pattern.contains('?') {
        return glob_match(pattern, fn_name) || glob_match(pattern, full_call);
    }
    false
}

/// Shell-style wildcard match supporting `*` (any run, incl. empty) and `?` (one
/// char). Iterative with backtracking — no recursion, no allocation of the whole
/// suffix set. Any other pattern char (including `[`) matches literally, so an
/// invalid bracket pattern never matches (mirrors the zshrs behavior where a
/// pattern without `*`/`?` never reaches glob parsing).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway compiled program to fill the `program` field in construction tests.
    fn empty_prog() -> Arc<CompiledProgram> {
        let ast = crate::parser::parse_program("BEGIN{}").unwrap();
        Arc::new(crate::compiler::Compiler::compile_program(&ast).unwrap())
    }

    #[test]
    fn star_matches_anything() {
        assert!(intercept_matches("*", "anything", "anything here"));
        assert!(intercept_matches("*", "", ""));
    }

    #[test]
    fn all_matches_anything() {
        assert!(intercept_matches("all", "draw", "draw 1 2"));
        assert!(intercept_matches("all", "log", "log status"));
    }

    #[test]
    fn exact_match_on_fn_name() {
        assert!(intercept_matches("draw", "draw", "draw x"));
        assert!(intercept_matches("log", "log", "log a b"));
    }

    #[test]
    fn exact_pattern_does_not_match_different_name() {
        assert!(!intercept_matches("draw", "paint", "paint blue"));
        assert!(!intercept_matches("log", "login", "login user"));
    }

    #[test]
    fn glob_star_matches_prefix() {
        // "draw *" should match the full call string like "draw a b".
        assert!(intercept_matches("draw *", "draw", "draw a b"));
    }

    #[test]
    fn glob_star_underscore_prefix_matches_helper_funcs() {
        // "_*" is the canonical convention for private helper functions.
        assert!(intercept_matches("_*", "_helper", "_helper"));
        assert!(intercept_matches("_*", "_impl", "_impl"));
    }

    #[test]
    fn glob_star_does_not_match_non_prefix() {
        assert!(!intercept_matches("_*", "helper", "helper"));
    }

    #[test]
    fn question_mark_glob_matches_single_char() {
        assert!(intercept_matches("f?", "fx", "fx"));
        assert!(!intercept_matches("f?", "fxyz", "fxyz"));
    }

    #[test]
    fn glob_star_in_middle_matches() {
        assert!(glob_match("a*z", "abcz"));
        assert!(glob_match("a*z", "az"));
        assert!(!glob_match("a*z", "abc"));
    }

    #[test]
    fn unmatched_pattern_without_glob_chars_returns_false() {
        assert!(!intercept_matches("nope", "draw", "draw x"));
    }

    #[test]
    fn invalid_glob_pattern_returns_false() {
        // "[invalid" has no `*`/`?`, never reaches glob matching, isn't exact.
        assert!(!intercept_matches("[invalid", "draw", "draw x"));
    }

    #[test]
    fn empty_pattern_does_not_match_non_empty_fn() {
        assert!(!intercept_matches("", "draw", "draw x"));
    }

    #[test]
    fn empty_pattern_matches_empty_fn_exactly() {
        assert!(intercept_matches("", "", ""));
    }

    #[test]
    fn advice_kind_variants_round_trip_clone() {
        assert!(matches!(AdviceKind::Before.clone(), AdviceKind::Before));
        assert!(matches!(AdviceKind::After.clone(), AdviceKind::After));
        assert!(matches!(AdviceKind::Around.clone(), AdviceKind::Around));
    }

    #[test]
    fn intercept_struct_clone_preserves_fields() {
        let i = Intercept {
            pattern: "draw_*".into(),
            kind: AdviceKind::Before,
            code: "print \"before\"".into(),
            id: 42,
            program: empty_prog(),
        };
        let c = i.clone();
        assert_eq!(c.pattern, "draw_*");
        assert!(matches!(c.kind, AdviceKind::Before));
        assert_eq!(c.code, "print \"before\"");
        assert_eq!(c.id, 42);
    }
}
