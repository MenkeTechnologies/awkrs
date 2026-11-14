//! Lexer and parser rejections: the exact diagnostic, the line it names, and
//! the exit status.
//!
//! awkrs reports a syntax error as `awkrs: parse error at line N: <msg>` on
//! stderr and exits 1 (`Error::exit_status`, src/error.rs). gawk 5.4.1 also
//! exits 1 for a syntax error; mawk 1.3.4 and one-true-awk 20200816 exit 2.
//! Every program here is rejected by all three references unless a test says
//! otherwise. Line numbers were checked against the same references: where the
//! offending construct sits on one line, mawk names that line in every case and
//! gawk in most, and that is the line pinned here.
//!
//! Each rejected program is checked to print nothing, so a regression that let
//! a broken program start running (a `BEGIN` action firing before the parse
//! error) fails too.

mod common;

use common::run_awkrs_stdin;

/// Run `program`, require a parse error, and return the stderr text.
fn rejected(program: &str) -> String {
    let (code, stdout, stderr) = run_awkrs_stdin(program, "");
    assert_eq!(
        code, 1,
        "program {program:?} should be a syntax error: {stderr}"
    );
    assert_eq!(stdout, "", "program {program:?} ran before it was rejected");
    stderr
}

/// Assert `program` is rejected with exactly `msg` on line `line`.
fn parse_error(program: &str, line: usize, msg: &str) {
    assert_eq!(
        rejected(program),
        format!("awkrs: parse error at line {line}: {msg}\n"),
        "program {program:?}"
    );
}

// ── lexer ──────────────────────────────────────────────────────────────────

/// A regex literal may not span a newline, and a `/` inside a bracket
/// expression does not close it, so `/a[/` runs off the end of the line.
#[test]
fn unterminated_regex() {
    parse_error("BEGIN {\n  x = 1\n  y = /abc\n}", 3, "unterminated regex");
    parse_error("$0 ~ /abc", 1, "unterminated regex");
    parse_error(r#"BEGIN{print "x" ~ /a[/}"#, 1, "unterminated regex");
}

/// A raw newline inside a string literal is its own diagnostic; a string cut
/// off by end of program after a `\`-newline continuation is "unterminated",
/// reported on the line where the string opened (gawk and mawk both name it).
#[test]
fn unterminated_and_newline_strings() {
    parse_error("BEGIN {\n  y = \"abc\n}", 2, "newline in string");
    parse_error("BEGIN{print \"abc", 1, "newline in string");
    parse_error("BEGIN {\n  y = \"abc\\", 2, "unterminated string");
}

/// Characters that are not tokens: a lone `&`, a backtick, and a `::` that is
/// not followed by a name.
#[test]
fn stray_characters() {
    parse_error("BEGIN{x = 1 & 2}", 1, "unexpected `&`");
    parse_error("BEGIN{x = 1 ` 2}", 1, "unexpected character '`'");
    parse_error("BEGIN{x = a:: }", 1, "expected identifier after `::`");
}

// ── function definitions ───────────────────────────────────────────────────

/// A second definition of a name is reported on the line where that
/// definition starts — all three references say line 2 here.
#[test]
fn duplicate_function_is_reported_where_it_starts() {
    parse_error(
        "function f(){}\nfunction f(){\n}\n\nBEGIN{}",
        2,
        "duplicate function name",
    );
}

#[test]
fn malformed_function_headers() {
    parse_error("BEGIN {\n}\nfunction (a) {}", 3, "expected function name");
    parse_error("function 1(){}", 1, "expected function name");
    parse_error("function f {}", 1, "expected `(` after function name");
    parse_error("function f(a,\n 1) {}", 2, "expected parameter name");
    parse_error("function f(a,){}", 1, "expected parameter name");
    parse_error("function f(a b){}", 1, "expected `)` after parameters");
    parse_error("function f(a) x", 1, "expected `{` before function body");
}

/// End-of-program errors name the line after the last one, because the
/// program text is newline-terminated before lexing. mawk reports the same
/// lines for these command-line programs (gawk names the last line).
#[test]
fn unclosed_bodies_at_end_of_program() {
    parse_error("function f(a){ x", 2, "expected `}` after function body");
    parse_error("BEGIN {\n x = 1", 3, "expected `}`");
}

// ── statements ─────────────────────────────────────────────────────────────

/// `delete` needs an array name. The error is detected at the newline ending
/// the statement and still names the statement's own line.
#[test]
fn delete_without_array_name() {
    parse_error(
        "BEGIN {\n  x = 1\n  delete\n}",
        3,
        "expected array name after `delete`",
    );
    parse_error("BEGIN{delete 1}", 1, "expected array name after `delete`");
}

/// `printf` with no format is refused before anything runs. The references
/// disagree on how: gawk dies at run time with exit 2, mawk refuses at compile
/// time with exit 2, and one-true-awk accepts it and prints nothing. awkrs
/// refuses at parse time like mawk but exits 1 (as for any other syntax
/// error), which matches none of them — so only the message, the line and
/// the empty stdout are pinned, not the status value.
#[test]
fn printf_without_format() {
    for (program, line) in [
        ("BEGIN {\n  x = 1\n  printf\n}", 3),
        ("BEGIN{printf; print \"after\"}", 1),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(program, "");
        assert_ne!(code, 0, "{program:?}");
        assert_eq!(stdout, "", "{program:?}");
        assert_eq!(
            stderr,
            format!(
                "awkrs: parse error at line {line}: `printf` requires at least a format string\n"
            )
        );
    }
}

#[test]
fn control_flow_headers() {
    parse_error("BEGIN{if 1 print}", 1, "expected `(` after `if`");
    parse_error("BEGIN{while 1 ;}", 1, "expected `(` after `while`");
    parse_error("BEGIN{ while (1 ;}", 1, "expected `)`");
    parse_error("BEGIN{do x=1; x=2}", 1, "expected `while` after `do` body");
    parse_error("BEGIN{ do ; while 1 }", 1, "expected `(` after `while`");
    parse_error("BEGIN{for x}", 1, "expected `(` after `for`");
    parse_error(
        "BEGIN{for (x in 1) ;}",
        1,
        "expected array name in `for (x in a)`",
    );
    parse_error("BEGIN{ for (x in a ; }", 1, "expected `)`");
}

/// Both separators of a C-style `for` are mandatory; the error names the
/// line holding the header.
#[test]
fn for_header_missing_semicolons() {
    parse_error(
        "BEGIN {\n  x = 1\n  for (i = 0 i < 3; i++) ;\n}",
        3,
        "expected `;` in `for`",
    );
    parse_error("BEGIN{for(i=0;i<3 i++);}", 1, "expected `;` in `for`");
}

/// `next` and `break` take no operand.
#[test]
fn junk_after_simple_statement() {
    parse_error("BEGIN{next 1}", 1, "expected `;`, newline, or `}`");
    parse_error("BEGIN{break 1}", 1, "expected `;`, newline, or `}`");
}

/// gawk's `switch` grammar (gawk accepts the well-formed statement; these
/// malformed ones it rejects).
#[test]
fn malformed_switch() {
    parse_error("BEGIN{switch 1 {}}", 1, "expected `(` after `switch`");
    parse_error(
        "BEGIN{switch (1 {}}",
        1,
        "expected `)` after switch expression",
    );
    parse_error(
        "BEGIN{switch (1) x}",
        1,
        "expected `{` after `switch (...)`",
    );
    parse_error(
        "BEGIN{switch (1) {case 1 x}}",
        1,
        "expected `:` after `case` label",
    );
    parse_error(
        "BEGIN{switch (1) {default x}}",
        1,
        "expected `:` after `default`",
    );
    parse_error(
        "BEGIN{switch (1) {x}}",
        1,
        "expected `case`, `default`, or `}` in switch body",
    );
}

/// `getline var expr` at statement level is `(getline var) expr`: gawk, mawk
/// and one-true-awk accept it and read the record into `var`. awkrs used to
/// reject the statement form with "expected `;`, newline, or `}`" while
/// accepting the same text on the right of an assignment.
#[test]
fn getline_var_followed_by_concatenation_is_a_statement() {
    let (code, stdout, stderr) =
        run_awkrs_stdin(r#"BEGIN{y = "Y"; getline x y; print "[" x "]"}"#, "hi\n");
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "[hi]\n");
    let (code, stdout, _) =
        run_awkrs_stdin(r#"BEGIN{y = "Y"; v = getline x y; print v, x}"#, "hi\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1Y hi\n");
}

// ── expressions ────────────────────────────────────────────────────────────

/// `1 ? 2 3` parses `2 3` as a concatenation and then finds no `:`. Only the
/// second of two ternaries on the line is malformed.
#[test]
fn ternary_without_colon() {
    parse_error("BEGIN{x = 1 ? 2 3}", 1, "expected `:` in ternary");
    parse_error(
        "BEGIN {\n  y = 1 ? 2 : 3; z = 1 ? 2 3; w = 1\n}",
        2,
        "expected `:` in ternary",
    );
}

/// Only variables, fields and array elements can be assigned. `x=1 y=2` is
/// `x = (1 y) = 2`, whose inner target is a concatenation.
#[test]
fn invalid_assignment_target() {
    parse_error(
        "BEGIN {\n  x = 1\n  3 = 2\n}",
        3,
        "invalid assignment target",
    );
    parse_error("BEGIN{x=1 y=2}", 1, "invalid assignment target");
    parse_error("BEGIN{$(1 = 2}", 1, "invalid assignment target");
}

#[test]
fn invalid_increment_operand() {
    parse_error(
        "BEGIN {\n  x = 1\n  y = ++2\n}",
        3,
        "invalid `++`/`--` operand",
    );
    parse_error("BEGIN{1++}", 1, "invalid `++`/`--` operand");
    parse_error("BEGIN{x=a[1]++ ++}", 1, "invalid `++`/`--` operand");
}

#[test]
fn malformed_operands() {
    parse_error("BEGIN{x = 1 in 2}", 1, "expected array name after `in`");
    parse_error("BEGIN{a[1}", 1, "expected `]` after array index");
    parse_error("BEGIN{x = $(1}", 1, "expected `)` after `$(`");
    parse_error("BEGIN{++$(1}", 1, "expected `)` after `$(`");
    parse_error("BEGIN{x=f(1}", 1, "expected `)`");
    parse_error("BEGIN{x=(1+2}", 1, "expected `)`");
    parse_error(
        "BEGIN{x = 1 +}",
        1,
        "unexpected token in expression: RBrace",
    );
}

/// Non-associative comparison: gawk and one-true-awk reject `1 < 2 < 3`
/// (mawk accepts it). awkrs follows gawk and one-true-awk.
#[test]
fn chained_comparison() {
    parse_error(
        "BEGIN{x = (1 < 2 < 3)}",
        1,
        "syntax error: comparison operators do not chain",
    );
}

/// gawk's indirect call `@name(args)` needs a callee and an argument list.
#[test]
fn malformed_indirect_call() {
    parse_error(
        "BEGIN{@}",
        1,
        "expected variable, `$<field>`, or `(expr)` after `@` for indirect call",
    );
    parse_error(
        "BEGIN{@f}",
        1,
        "expected `(` after `@` expression for indirect call",
    );
    parse_error("BEGIN{@(f}", 1, "expected `)` around `@<expr>` callee");
}

// ── parenthesized comma lists ──────────────────────────────────────────────

/// `(a, b)` is only meaningful as `(i, j) in arr` or as the whole argument
/// list of `print`/`printf`. Everywhere else it is rejected.
#[test]
fn parenthesized_comma_list_misuse() {
    parse_error(
        "BEGIN{print (1,2),3}",
        1,
        "parenthesized comma list may not be followed by `,` in `print`",
    );
    parse_error(
        "BEGIN{print ((1,2),3)}",
        1,
        "nested parenthesized comma lists are not allowed",
    );
    parse_error(
        "BEGIN{printf (1,2),3}",
        1,
        "parenthesized comma list is not allowed in `printf` arguments",
    );
    parse_error(
        "BEGIN{(1,2)}",
        1,
        "parenthesized comma list cannot be used as a statement",
    );
    parse_error(
        "BEGIN{x = (1,2) + 1}",
        1,
        "invalid use of parenthesized comma list",
    );
    for pattern in ["(1,2)", "(1,2),1 {print}", "1,(1,2) {print}"] {
        parse_error(pattern, 1, "parenthesized comma list cannot be a pattern");
    }
}

/// A comma list on the right of an assignment is caught by the program
/// validator rather than the parser: still exit 1, but no line number.
#[test]
fn comma_list_assignment_is_a_validation_error() {
    assert_eq!(
        rejected("BEGIN{x=(1,2)}"),
        "awkrs: parenthesized comma list is not allowed in this context\n"
    );
}

/// Program files report the same lines as command-line text.
#[test]
fn program_file_reports_the_same_line() {
    let path = common::unique_tmp_path("awkrs_parse_err.awk");
    std::fs::write(&path, "BEGIN {\n  x = 1\n  delete\n}\n").expect("write program");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("-f")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn awkrs");
    std::fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "awkrs: parse error at line 3: expected array name after `delete`\n"
    );
}
