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
