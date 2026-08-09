//! Behavior pins for parity gaps found by `scripts/fuzz_parity.sh` against the
//! reference awks (gawk 5.4.1, one-true-awk 20200816, mawk 1.3.4).
//!
//! Each test encodes what the references actually do, not what an implementation
//! detail happens to produce. These run without any reference awk installed, so
//! they hold in a headless CI container; `scripts/fuzz_parity.sh` is what checks
//! the same behavior against the real binaries when they are available.

mod common;

use common::run_awkrs_stdin;

/// `<` `<=` `>` `>=` between two fields follow awk's strnum rule: a string
/// comparison unless both sides look numeric. A peephole used to fuse
/// `PushNum(N); GetField` into `PushFieldNum(N)` ahead of a relational operator,
/// coercing the right-hand field to a number before the comparator saw it — so
/// `$1 < $2` on the record `a b` compared 0 against 0 and answered 0 where gawk,
/// mawk and one-true-awk all answer 1.
#[test]
fn relational_between_two_string_fields_compares_as_strings() {
    let (code, stdout, _) = run_awkrs_stdin(
        "{ print ($1 < $2), ($2 < $1), ($1 > $2), ($2 > $1), ($1 <= $2), ($1 >= $2) }",
        "a b\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 0 0 1 1 0\n");
}

/// The same fusion must not be reintroduced for the numeric case either: two
/// numeric-looking fields still compare as numbers, so `2 < 10` is true even
/// though the string "2" sorts after "10".
#[test]
fn relational_between_two_numeric_fields_compares_as_numbers() {
    let (code, stdout, _) = run_awkrs_stdin("{ print ($1 < $2), ($1 > $2) }", "2 10\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 0\n");
}

/// Two empty fields are equal, so no strict relational operator holds between
/// them. The coercion bug made `$1 < $2` true and `$1 >= $2` false on a record
/// with no fields at all.
#[test]
fn relational_between_two_empty_fields_is_equality() {
    let (code, stdout, _) = run_awkrs_stdin(
        "{ print ($1 == $2), ($1 < $2), ($1 > $2), ($1 <= $2), ($1 >= $2) }",
        "\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 0 0 1 1\n");
}

/// POSIX: `exit` with no expression leaves the exit status unchanged, so an
/// `exit 3` in `BEGIN` survives a bare `exit` in `END`. All three references
/// agree; awkrs used to reset the status to 0.
#[test]
fn bare_exit_in_end_preserves_earlier_status() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { exit 3 } END { print \"e\"; exit }", "");
    assert_eq!(stdout, "e\n");
    assert_eq!(code, 3);
}

/// An `exit` *with* an expression in `END` still overrides the earlier status.
#[test]
fn exit_expression_in_end_overrides_earlier_status() {
    let (code, _, _) = run_awkrs_stdin("BEGIN { exit 3 } END { exit 5 }", "");
    assert_eq!(code, 5);
}

/// Plain `getline` once the main input is exhausted is an ordinary end-of-file:
/// it returns 0. awkrs used to report -1 (an I/O error) because the primary
/// reader had already been detached by the record loop.
#[test]
fn getline_after_main_input_is_exhausted_returns_zero() {
    let (code, stdout, _) = run_awkrs_stdin("END { print \"eof=\" getline, \"NR=\" NR }", "a\nb\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "eof=0 NR=2\n");
}

/// Paragraph mode (`RS == ""`) makes <newline> an extra field separator when FS
/// is a single character — gawk rewrites FS to `[<fs>\n]` and one-true-awk does
/// the same. The record `a:b\nc:d` is therefore four fields, not three.
#[test]
fn paragraph_mode_adds_newline_to_single_char_fs() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { RS = \"\"; FS = \":\" } { print NF \"|\" $1 \"|\" $2 \"|\" $3 \"|\" $4 \"|\" }",
        "a:b\nc:d\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "4|a|b|c|d|\n");
}

/// The newline rule stops at a single-character FS. With a regex FS every
/// reference awk leaves the embedded newline inside the field, so the same
/// record is three fields with "b\nc" in the middle.
#[test]
fn paragraph_mode_leaves_regex_fs_alone() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { RS = \"\"; FS = \"[0-9]+\" } { print NF \"|\" $1 \"|\" $2 \"|\" $3 \"|\" }",
        "a12b\nc345d\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3|a|b\nc|d|\n");
}

/// Outside paragraph mode a single-character FS is unaffected by the rule.
#[test]
fn single_char_fs_outside_paragraph_mode_is_unchanged() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { FS = \":\" } { print NF \"|\" $1 \"|\" $2 \"|\" }",
        "a:b\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "2|a|b|\n");
}

/// gawk's `typeof` separates an array element that does not exist ("untyped")
/// from one that a bare read auto-created but never assigned ("unassigned").
/// `typeof` itself must not create the element. awkrs used to report "untyped"
/// for both.
#[test]
fn typeof_distinguishes_unassigned_element_from_missing_one() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { print typeof(a[\"k\"]); y = a[\"k\"]; print typeof(a[\"k\"]), typeof(a[\"never\"]) }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "untyped\nunassigned untyped\n");
}

/// Fatal runtime faults exit 2 in gawk, mawk and one-true-awk alike. awkrs used
/// to exit 1 for every error, which made a fatal indistinguishable from a parse
/// diagnostic to a calling script.
#[test]
fn fatal_runtime_error_exits_two() {
    let (code, _, stderr) = run_awkrs_stdin("BEGIN { print 1 / 0 }", "");
    assert_eq!(code, 2, "stderr: {stderr}");
    assert!(stderr.contains("division by zero"), "stderr: {stderr}");
}

/// Parse diagnostics keep exit 1, matching gawk. (The references disagree here —
/// mawk and one-true-awk exit 2 — so this pins the choice rather than a
/// consensus.)
#[test]
fn parse_error_exits_one() {
    let (code, _, stderr) = run_awkrs_stdin("BEGIN {", "");
    assert_eq!(code, 1, "stderr: {stderr}");
}

/// An empty record has zero fields under every `FS`. The single-character and
/// regex splitters used to push one empty range unconditionally, so a blank line
/// under `FS=":"` reported `NF == 1` where gawk, mawk and one-true-awk all
/// report 0 — a wrong answer on the most ordinary shape there is, a blank line
/// in a delimited file.
#[test]
fn empty_record_has_zero_fields_under_any_fs() {
    for fs in ["\":\"", "\",\"", "\"[0-9]+\"", "\"\\t\""] {
        let prog = format!("BEGIN {{ FS = {fs} }} {{ print NF }}");
        let (code, out, err) = run_awkrs_stdin(&prog, "\n");
        assert_eq!(code, 0, "FS={fs} stderr: {err}");
        assert_eq!(out, "0\n", "FS={fs}");
    }
    // Assigning an empty `$0` resplits to zero fields too.
    let (_, out, _) = run_awkrs_stdin(r#"BEGIN { FS = ":" } { $0 = ""; print NF }"#, "a:b\n");
    assert_eq!(out, "0\n");
}

/// A `sub`/`gsub` that matches nothing leaves the target completely alone. Every
/// reference keeps an uninitialized variable uninitialized, so it still compares
/// equal to 0; awkrs used to store the unchanged string back, turning `Uninit`
/// into `Str("")` and flipping `z == 0` from 1 to 0.
#[test]
fn sub_with_no_match_does_not_disturb_the_target() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { print sub(/x/, "y", z), (z == 0), (z == ""); n = 1; print gsub(/q/, "r", n), (n == 1) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "0 1 1\n0 1\n");
}

/// A bare `return` yields the uninitialized value, which compares equal to both
/// `0` and `""`. Returning `Str("")` made `f() == 0` false, because a real
/// string never compares numerically against a number.
#[test]
fn bare_return_yields_the_uninitialized_value() {
    let (code, out, err) = run_awkrs_stdin(
        r#"function f() { return } function g() { } BEGIN { print (f() == 0), (f() == ""), (g() == 0) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "1 1 1\n");
}

/// POSIX numeric strings come from input only. A string a builtin computed is a
/// plain string, so comparing it against a number is a *string* comparison:
/// `substr("065",1,2) == 6` is 0 because "06" and "6" differ as text. awkrs used
/// to return the strnum-carrying `Value::Str` from these builtins and answer 1.
#[test]
fn computed_strings_are_not_numeric_strings() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { print (substr("065",1,2) == 6), (sprintf("%s","06") == 6), (toupper("06") == 6), (tolower("06") == 6) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "0 0 0 0\n");
}

/// Concatenation always produces a plain string, even of two numeric-looking
/// fields — so `$1 ""` of a field holding "06" compares against 6 as text. The
/// field itself stays a numeric string, so `$1 == 6` is still 1.
#[test]
fn concatenation_result_is_never_a_numeric_string() {
    let (code, out, err) = run_awkrs_stdin(
        r#"{ print ($1 == 6), ($1 "" == 6), ($1 $2 == 66) }"#,
        "06 6\n",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "1 0 0\n");
}

/// A regex *literal* separator for `split` is always a regex, so neither the
/// `" "` whitespace-run rule nor the single-character-literal rule applies:
/// `/ /` splits on one space and `/./` matches any character. An empty
/// separator still splits into characters however it was written.
#[test]
fn split_with_a_regex_literal_bypasses_the_fs_shorthands() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { print split("  a  b  ", A, " "), split("  a  b  ", B, / /), split("a.b", C, "."), split("a.b", D, /./), split("abc", E, //) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "2 7 2 4 3\n");
}

/// Each subscript of a multidimensional key converts the way a single subscript
/// does: integral values exactly, everything else through `CONVFMT`. The join
/// used the default number formatting and ignored `CONVFMT` entirely.
#[test]
fn multidimensional_subscripts_honour_convfmt() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; SUBSEP = "-"; A[1.234, 2] = 1; for (k in A) print k; print ((1.234, 2) in A) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "1.23-2\n1\n");
}

/// `%c` prints the character with the given code for a numeric operand and the
/// first character for a string one. A field is a numeric string, hence numeric:
/// `echo 65 | awk '{printf "%c", $1}'` prints `A` in every reference. The string
/// literal `"65"` stays a string and prints `6`.
#[test]
fn printf_percent_c_treats_a_numeric_string_as_a_number() {
    let (code, out, err) =
        run_awkrs_stdin(r#"{ printf "[%c][%c][%c]\n", $1, $2, "65" }"#, "65 A\n");
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "[A][A][6]\n");
}

/// ISO C: "A negative precision argument is taken as if the precision were
/// omitted." awkrs clamped a negative `*` precision to zero, so `%.*f` with -2
/// printed `3` where every reference prints the default six places.
#[test]
fn printf_negative_star_precision_means_precision_omitted() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { printf "[%.*f][%.*d]\n", -2, 3.14159, -2, 42 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "[3.141590][42]\n");
}

/// POSIX makes `;` a statement in its own right, so it is a legal empty body for
/// every control-flow header. awkrs rejected all of them with a parse error.
#[test]
fn semicolon_is_a_legal_empty_control_flow_body() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { if (1) ; print "A"; for (i = 0; i < 2; i++) ; print i; while (0) ; if (0) ; else print "C" }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "A\n2\nC\n");
}

/// Using a function's name as a scalar or an array is fatal in gawk, mawk and
/// one-true-awk alike; awkrs used to accept it and run the program. The check is
/// program-wide (a use before the definition is just as fatal) and does not fire
/// for a *parameter* that shadows a function name, which is legal everywhere.
#[test]
fn function_name_used_as_a_variable_is_rejected() {
    for prog in [
        "function f(){} BEGIN{ f = 1 }",
        "function f(){} BEGIN{ f[1] = 1 }",
        "function f(){} BEGIN{ print f }",
        "function f(){} BEGIN{ split(\"a\", f) }",
        "function f(){} BEGIN{ getline f }",
        "function f(){} f { print }",
        "function f(){} $0 ~ f { print }",
        "BEGIN{ f = 1 } function f(){}",
    ] {
        let (code, out, _) = run_awkrs_stdin(prog, "");
        assert_ne!(code, 0, "should be rejected: {prog}");
        assert_eq!(out, "", "should not have run: {prog}");
    }
    let (code, out, err) = run_awkrs_stdin(
        "function f(){ return 7 } function g(f) { f = 1; return f } BEGIN{ print f(), g(2) }",
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "7 1\n");
}

/// gawk separates a name the program never mentions (`"untyped"`) from one that
/// was read or assigned an uninitialized value (`"unassigned"`). `typeof` itself
/// does not make that transition, so asking twice still reports `"untyped"`.
#[test]
fn typeof_separates_untyped_from_unassigned() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN { print typeof(z); print typeof(z); x = z; print typeof(z), typeof(x); print typeof(never) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "untyped\nuntyped\nunassigned unassigned\nuntyped\n");
}

/// `split()` makes its target an array even when it produces no fields.
#[test]
fn split_of_an_empty_string_still_creates_the_array() {
    let (code, out, err) =
        run_awkrs_stdin(r#"BEGIN { print split("", z), typeof(z), length(z) }"#, "");
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "0 array 0\n");
}
