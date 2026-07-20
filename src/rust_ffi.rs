//! awk wiring for inline Rust FFI (`rust { ... }` blocks).
//!
//! The heavy lifting lives in fusevm: [`fusevm::RustSugar`] scans and rewrites
//! the block at the source level, and [`fusevm::ffi`] compiles/loads/marshals
//! it. This module only supplies the awk-flavored [`fusevm::RustSugar`] config
//! and the desugar entry the parser calls. The emitted `__rust_compile(...)`
//! call and every exported bareword are resolved in
//! [`crate::vm_builtins::exec_builtin_dispatch`].
//!
//! awk's top level is a sequence of `pattern { action }` rules — a bare call
//! statement is not valid there — so a `rust { ... }` block desugars to a whole
//! `BEGIN` rule, `BEGIN { __rust_compile("<b64>", <line>) }`, which runs before
//! any record processing and before later `BEGIN` rules that call the exports.

use fusevm::RustSugar;

/// Emit the awk rule a `rust { ... }` block desugars to: a `BEGIN` rule whose
/// action calls the `__rust_compile` builtin with the base64-encoded block body
/// and its source line. A `BEGIN` wrap is required because awk has no bare
/// top-level statements — only `pattern { action }` rules.
fn emit(b64: &str, line: usize) -> String {
    format!("BEGIN {{ __rust_compile(\"{b64}\", {line}) }}")
}

/// awk desugar config: `rust` keyword, `#` line comments, no block comments.
/// `newline_boundary` is `true` so a block on its own line (the normal top-level
/// form) is recognized — `rust {` is never valid awk otherwise, so this only
/// ever matches an intended FFI block.
pub const SUGAR: RustSugar = RustSugar {
    keyword: "rust",
    line_comments: &["#"],
    block_comment: None,
    newline_boundary: true,
    emit,
};

/// Rewrite every top-level `rust { ... }` block in awk source into a
/// `BEGIN { __rust_compile(...) }` rule, before lexing. No-op when the source
/// has no `rust` token.
pub fn desugar(src: &str) -> String {
    SUGAR.desugar(src)
}

#[cfg(test)]
mod tests {
    #[test]
    fn desugars_top_level_rust_block_to_begin_rule() {
        let src = "rust { pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b } }\nBEGIN { print add(2, 3) }\n";
        let out = super::desugar(src);
        assert!(out.contains("BEGIN {"), "no BEGIN wrap: {out}");
        assert!(out.contains("__rust_compile("), "no builtin call: {out}");
        assert!(!out.contains("pub extern"), "Rust body leaked: {out}");
        assert!(out.contains("print add(2, 3)"), "user rule dropped: {out}");
    }

    #[test]
    fn leaves_ordinary_awk_untouched() {
        let src = "BEGIN { print length(\"hi\") }\n";
        assert_eq!(super::desugar(src), src);
    }

    #[test]
    fn hash_comment_is_not_a_false_boundary() {
        // A `#`-comment mentioning `rust` must not be desugared.
        let src = "# a rust { } comment\nBEGIN { print 1 }\n";
        assert_eq!(super::desugar(src), src);
    }
}
