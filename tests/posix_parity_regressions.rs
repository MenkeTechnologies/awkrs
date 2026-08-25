//! Behavior pins for parity gaps found by `scripts/fuzz_parity.sh` against the
//! reference awks (gawk 5.4.1, one-true-awk 20200816, mawk 1.3.4).
//!
//! Each test encodes what the references actually do, not what an implementation
//! detail happens to produce. These run without any reference awk installed, so
//! they hold in a headless CI container; `scripts/fuzz_parity.sh` is what checks
//! the same behavior against the real binaries when they are available.

mod common;

use common::{
    run_awkrs_file, run_awkrs_operands, run_awkrs_stdin, run_awkrs_stdin_args,
    run_awkrs_stdin_args_env, run_awkrs_stdin_bounded,
};
use std::io::Write;

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

// ── `break` / `continue` outside a loop ─────────────────────────────────────

/// gawk, mawk and one-true-awk all refuse a program that uses `break` outside a
/// loop, before running any of it. awkrs used to accept it: the compiler emits
/// `break` as a placeholder `Op::Jump(0)` and patches the target when the
/// enclosing loop closes, so with no enclosing loop the placeholder survived and
/// the program jumped to instruction 0 — looping forever on a program every
/// reference rejects outright. The bounded runner is what keeps a regression a
/// test failure instead of a wedged test binary.
#[test]
fn break_outside_a_loop_is_rejected_instead_of_looping_forever() {
    for program in [
        r#"BEGIN { break }"#,
        r#"BEGIN { print "never"; break }"#,
        r#"BEGIN { if (1) break }"#,
        r#"BEGIN { { { break } } }"#,
        r#"{ break }"#,
        r#"END { break }"#,
        r#"function f() { break } BEGIN { f() }"#,
    ] {
        let got = run_awkrs_stdin_bounded(program, "x\n", 10);
        let (code, out, err) = got.unwrap_or_else(|| panic!("{program}: did not terminate"));
        assert_eq!(code, 2, "{program}: stdout {out:?} stderr {err:?}");
        assert!(out.is_empty(), "{program}: rejected programs run nothing");
        assert!(err.contains("break"), "{program}: stderr {err:?}");
    }
}

/// Same for `continue`, which additionally may not target a `switch`: gawk
/// accepts `break` inside a switch with no enclosing loop but rejects
/// `continue` there.
#[test]
fn continue_outside_a_loop_is_rejected_instead_of_looping_forever() {
    for program in [
        r#"BEGIN { continue }"#,
        r#"BEGIN { print "never"; continue }"#,
        r#"{ continue }"#,
        r#"END { continue }"#,
        r#"function f() { continue } BEGIN { f() }"#,
        r#"BEGIN { switch (1) { case 1: continue } }"#,
    ] {
        let got = run_awkrs_stdin_bounded(program, "x\n", 10);
        let (code, out, err) = got.unwrap_or_else(|| panic!("{program}: did not terminate"));
        assert_eq!(code, 2, "{program}: stdout {out:?} stderr {err:?}");
        assert!(out.is_empty(), "{program}: rejected programs run nothing");
        assert!(err.contains("continue"), "{program}: stderr {err:?}");
    }
}

/// The rejection must not swallow the legal uses. Every loop form, both jumps,
/// nested and inside a function.
#[test]
fn break_and_continue_still_work_in_every_loop_form() {
    let (code, out, err) = run_awkrs_stdin(
        r#"function f(n,   i, s) { for (i = 0; i < n; i++) { if (i == 2) continue; s = s i } return s }
BEGIN {
  for (i = 0; i < 5; i++) { if (i == 3) break; s1 = s1 i }
  i = 0; while (i < 5) { i++; if (i == 3) continue; s2 = s2 i }
  i = 0; do { i++; if (i == 4) break; s3 = s3 i } while (i < 5)
  for (j = 0; j < 3; j++) a[j] = j
  c = 0; for (k in a) { c++; if (k == 1) continue; c += 10 }
  for (p = 0; p < 3; p++) for (q = 0; q < 3; q++) { if (q == 1) break; s4 = s4 p q }
  print s1, s2, s3, c, s4, f(5)
}"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "012 1245 123 23 001020 0134\n");
}

/// gawk allows `break` to leave a `switch` that has no enclosing loop, and
/// inside a loop `break` leaves only the switch while `continue` reaches the
/// loop. (mawk and one-true-awk have no `switch` at all.)
#[test]
fn break_targets_the_switch_and_continue_targets_the_loop() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN {
  switch (1) { case 1: break }
  for (i = 0; i < 3; i++) { switch (i) { case 1: break }; b = b i }
  for (i = 0; i < 3; i++) { switch (i) { case 1: continue }; c = c i }
  print b, c
}"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "012 02\n");
}

// ── `exit` raised inside a user function ────────────────────────────────────

/// `exit` from inside a user function is still an ordinary exit: `END` runs and
/// the status is kept. All three references print `end`. awkrs transported the
/// signal out of the call as `Error::Exit`, and an error escaping the run made
/// the driver skip `END` — so the status was right and the output was missing.
#[test]
fn exit_inside_a_function_still_runs_end() {
    for (program, stdin, want_code) in [
        (
            r#"function f() { exit 3 } BEGIN { f(); print "never" } END { print "end" }"#,
            "",
            3,
        ),
        (
            r#"function g() { exit 6 } function f() { g() } BEGIN { f() } END { print "end" }"#,
            "",
            6,
        ),
        (
            r#"function f(n) { exit 5; return 1 } BEGIN { x = f(1); print "never" } END { print "end" }"#,
            "",
            5,
        ),
        (
            r#"function f(n) { if (n == 2) exit 2; return n } BEGIN { for (i = 0; i < 5; i++) f(i) } END { print "end" }"#,
            "",
            2,
        ),
        (
            r#"function f() { exit 4 } { f(); print "never" } END { print "end" }"#,
            "one\n",
            4,
        ),
        (
            r#"function f() { exit } BEGIN { f() } END { print "end" }"#,
            "",
            0,
        ),
    ] {
        let (code, out, err) = run_awkrs_stdin(program, stdin);
        assert_eq!(code, want_code, "{program}: stderr {err:?}");
        assert_eq!(out, "end\n", "{program}");
    }
}

/// `exit` inside a function called *from* `END` still just stops `END`.
#[test]
fn exit_inside_a_function_called_from_end_stops_end() {
    let (code, out, err) =
        run_awkrs_stdin(r#"function f() { exit 5 } END { f(); print "never" }"#, "");
    assert_eq!(code, 5, "stderr: {err}");
    assert_eq!(out, "");
}

// ── assigned fields and records are not numeric strings ─────────────────────

/// POSIX: only input-derived values are numeric strings. A field assigned a
/// string literal or any computed string compares as *text*, so `$1 = "42"`
/// makes `$1 < 7` true — `"42" < "7"`. gawk, mawk and one-true-awk all agree.
/// awkrs stored fields as plain text and re-derived numeric-string status from
/// the characters, so every assignment laundered a computed string back into a
/// numeric string.
#[test]
fn a_field_assigned_a_plain_string_is_not_a_numeric_string() {
    let (code, out, err) = run_awkrs_stdin(
        r#"{ $1 = "42"; print ($1 < 7), ($1 == 42)
     $2 = "4" "2"; print ($2 < 7)
     $3 = substr("42", 1); print ($3 < 7)
     $4 = sprintf("%d", 42); print ($4 < 7) }"#,
        "9 9 9 9\n",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "1 1\n1\n1\n1\n");
}

/// The other half of the rule: a number or another field keeps numeric-string
/// status, so those stay numeric comparisons.
#[test]
fn a_field_assigned_a_number_or_another_field_stays_numeric() {
    let (code, out, err) = run_awkrs_stdin(
        r#"{ $1 = 42; print ($1 < 7)
     $2 = $1; print ($2 < 7)
     $3 = $3; print ($3 < 7)
     $4 += 0; print ($4 < 7) }"#,
        "1 2 42 42\n",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "0\n0\n0\n0\n");
}

/// Same rule for `$0`. Re-splitting an assigned record still yields numeric
/// string *fields* — only `$0` itself loses the status — and `$0 = $0` keeps it.
#[test]
fn an_assigned_record_is_not_a_numeric_string_but_its_fields_are() {
    let (code, out, err) = run_awkrs_stdin(
        r#"{ print ($0 < 7)
     $0 = "42"; print ($0 < 7), ($0 == 42), ($1 < 7)
     $0 = "06"; print ($0 == 6), ($1 == 6)
     $0 = " 42 "; print ($0 < 7)
     $0 = 42; print ($0 < 7) }"#,
        "42\n",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "0\n1 1 0\n0 1\n1\n0\n");
}

/// `gsub` rewriting `$0` produces a computed string even when the replacement
/// leaves the text identical, so the later relational compares as text. A `sub`
/// that matches nothing leaves the record — and its status — alone.
#[test]
fn a_rewritten_record_is_not_a_numeric_string() {
    let (code, out, err) = run_awkrs_stdin(
        r#"{ sub(/x/, "y"); print ($0 < 7); gsub(/2/, "2"); print ($0 < 7) }"#,
        "42\n",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "0\n1\n");
}

// ── `typeof($0)` before the record exists ───────────────────────────────────

/// gawk reports `$0` as `"unassigned"` until the record is first given a value.
/// Reading `$0` does not give it one — matching a regex against it, taking its
/// length or splitting it all leave it unassigned — while assigning `$0`, a
/// field, or `NF` makes it a `"string"`.
#[test]
fn typeof_dollar_zero_is_unassigned_until_the_record_is_set() {
    let (code, out, err) = run_awkrs_stdin(
        r#"BEGIN {
  print typeof($0)
  x = $0 ""; print typeof($0), length($0)
  if ($0 ~ /x/) q = 1
  n = split($0, A); print typeof($0), n
  print typeof($1)
  $0 = "abc"; print typeof($0)
}"#,
        "",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(
        out,
        "unassigned\nunassigned 0\nunassigned 0\nunassigned\nstring\n"
    );
}

/// Once a record has been read, `$0` is a `"string"` in every block.
#[test]
fn typeof_dollar_zero_is_string_once_a_record_has_been_read() {
    let (code, out, err) = run_awkrs_stdin(
        r#"{ print typeof($0) } END { print typeof($0) }"#,
        "hello\n",
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, "string\nstring\n");
}

// ── field assignment on the file-input path ─────────────────────────────────

/// Assigning a field before any field has been *read* must not lose the rest of
/// the record.
///
/// awk splits a record lazily, so a record whose first use is `$1 = "Z"` still
/// has no split recorded. `set_field` materialised its owned field vector from
/// that empty split, which produced zero fields — `$2`, `$3` and most of `$0`
/// simply vanished, and `NF` collapsed to the assigned index. Only the file /
/// mmap input path deferred the split that long; the streaming path split early
/// enough to hide it. Both differential corpora feed every case on **stdin**, so
/// nothing exercised it. `scripts/fuzz_parity.sh -F` now runs the same corpus
/// with the input as a file argument.
#[test]
fn assigning_a_field_before_reading_one_keeps_the_rest_of_the_record() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_lazy_split_{}.txt", std::process::id()));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(b"9 42 42 42 42\n").expect("write fixture");
    drop(f);

    for (program, want) in [
        (r#"{ $1 = "Z"; print $2, $3, NF }"#, "42 42 5\n"),
        (r#"{ $1 = "Z"; print $0 }"#, "Z 42 42 42 42\n"),
        (r#"{ $3 = "Z"; print $2, $4, NF }"#, "42 42 5\n"),
        (r#"{ $1 = 1; print $2, NF }"#, "42 5\n"),
        (r#"{ $2 = $2; print $0, NF }"#, "9 42 42 42 42 5\n"),
        // The strnum rule has to hold on this path too: assigning a plain string
        // then assigning a field back makes it numeric again.
        (
            r#"{ $1 = "42"; print ($1 < 7); $1 = $2; print ($1 < 7) }"#,
            "1\n0\n",
        ),
    ] {
        let (code, out, err) = run_awkrs_file(program, &path);
        assert_eq!(code, 0, "{program}: stderr {err:?}");
        assert_eq!(out, want, "{program}");
    }
    let _ = std::fs::remove_file(&path);
}

/// The same programs on stdin, so the two input paths are pinned to the same
/// answers rather than only being individually plausible.
#[test]
fn field_assignment_agrees_between_the_file_and_stdin_paths() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_lazy_split_agree_{}.txt", std::process::id()));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(b"9 42 42 42 42\n").expect("write fixture");
    drop(f);

    for program in [
        r#"{ $1 = "Z"; print $2, $3, NF }"#,
        r#"{ $1 = "Z"; print $0 }"#,
        r#"{ $7 = "x"; print NF, $0 }"#,
        r#"{ $1 = "42"; print ($1 < 7); $1 = $2; print ($1 < 7) }"#,
    ] {
        let (fc, fout, ferr) = run_awkrs_file(program, &path);
        let (sc, sout, serr) = run_awkrs_stdin(program, "9 42 42 42 42\n");
        assert_eq!(fc, sc, "{program}: file {ferr:?} stdin {serr:?}");
        assert_eq!(fout, sout, "{program}: file vs stdin");
    }
    let _ = std::fs::remove_file(&path);
}

/// A rule assigning `FS` changes how the *next* record is split. The file /
/// mmap input path read `FS` once before the record loop and reused it for
/// every record, so `NR == 1 { FS = ":" }` never took effect on a file argument
/// even though it worked when the same bytes arrived on stdin. gawk, mawk and
/// one-true-awk all resplit from record 2 onward.
#[test]
fn assigning_fs_in_a_rule_affects_the_next_record_on_the_file_path() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_fs_next_{}.txt", std::process::id()));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(b"a:b\nc:d\n").expect("write fixture");
    drop(f);

    for (program, want) in [
        (
            r#"NR == 1 { FS = ":" } { print NR, NF, $1 }"#,
            "1 1 a:b\n2 2 c\n",
        ),
        (r#"{ FS = ":"; print NR, NF, $1 }"#, "1 1 a:b\n2 2 c\n"),
        (r#"BEGIN { FS = ":" } { print NF, $1 }"#, "2 a\n2 c\n"),
    ] {
        let (fc, fout, ferr) = run_awkrs_file(program, &path);
        assert_eq!(fc, 0, "{program}: stderr {ferr:?}");
        assert_eq!(fout, want, "{program}: file path");
        // The two input paths must not disagree about it either.
        let (_, sout, _) = run_awkrs_stdin(program, "a:b\nc:d\n");
        assert_eq!(sout, want, "{program}: stdin path");
    }
    let _ = std::fs::remove_file(&path);
}

/// POSIX: a numeric array subscript is converted to a string with `CONVFMT`
/// (integral values excepted — those convert exactly). That conversion is the
/// array's *identity function*, so every operation that takes a subscript has to
/// use it, or two spellings of the same subscript name two different entries.
///
/// Five of the eight subscript-taking opcodes did not: `in`, `delete a[k]`,
/// `a[k] op= v`, `a[k]++` and `typeof(a[k])` each rendered the key their own way
/// while the store used `CONVFMT`. gawk 5.4.1, mawk 1.3.4 and one-true-awk
/// 20200816 all agree with the assertions below.
///
/// The gap survived a large probe corpus and a seeded generator because every
/// case they contained used either an **integral** subscript — which bypasses
/// `CONVFMT`, so any rendering round-trips — or the **multidimensional** form,
/// where the `SUBSEP` join has already reduced the key to a string before the
/// opcode runs. A non-integral *single* subscript is the shape that shows it.
#[test]
fn every_subscript_operation_uses_the_convfmt_key() {
    // `x in a` must be true for the entry `a[x] = …` just created.
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; A[x] = 1; print (x in A), ("1.23" in A) }"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 1\n");

    // `delete a[x]` must remove that same entry, not miss it.
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; A[x] = 1; delete A[x]; print length(A) }"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");

    // A compound assignment read through one key and wrote through another, so
    // it left the array with two entries and the increment on the wrong one.
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; A[x] = 5; A[x] += 1; print length(A), A[x] }"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 6\n");

    // Same for the increment/decrement forms, pre and post.
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; A[x] = 5; A[x]++; ++A[x]; --A[x]; print length(A), A[x] }"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 6\n");

    // `typeof` looked up an entry that was never stored and reported the
    // "untyped" of a missing element instead of the value's own type.
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; A[x] = "s"; B[x] = 7; print typeof(A[x]), typeof(B[x]) }"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "string number\n");
}

/// The same invariant has to hold whatever `CONVFMT` is, including formats that
/// round to fewer digits than the subscript needs and formats that keep more.
/// A single hard-coded `CONVFMT` would pass on a lucky value that round-trips.
#[test]
fn convfmt_subscript_round_trips_under_every_format() {
    for fmt in ["%.2f", "%.6g", "%.17g", "%.3e", "%.1g"] {
        let program = format!(
            r#"BEGIN {{ CONVFMT = "{fmt}"; x = 0.1 + 0.2; A[x] = 1; A[x] += 1; A[x]++; print (x in A), length(A), A[x] }}"#
        );
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "CONVFMT={fmt}: stderr {stderr:?}");
        assert_eq!(stdout, "1 1 3\n", "CONVFMT={fmt}");
    }
}

/// The JIT and the plain VM must not disagree about the key either: `-O` and
/// `-s` select different execution tiers, and the subscript conversion lives in
/// the interpreter's opcode handlers, so a fix applied to one tier only would
/// leave the other answering differently for the same program.
#[test]
fn convfmt_subscript_key_agrees_across_execution_tiers() {
    let program = r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; A[x] = 5; A[x] += 1; print (x in A), length(A), A[x] }"#;
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut seen = Vec::new();
    for flag in ["-s", "-O"] {
        let out = std::process::Command::new(bin)
            .arg(flag)
            .arg(program)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run awkrs");
        assert!(
            out.status.success(),
            "{flag}: {:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        seen.push(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    assert_eq!(seen[0], "1 1 6\n", "-s (no JIT)");
    assert_eq!(seen[1], seen[0], "-O (JIT) disagrees with -s");
}

// ── CONVFMT string coercion at every use site ───────────────────────────────
//
// POSIX gives one rule for turning a number into a string outside `print`: it
// renders through `CONVFMT` (integral values excepted). `Value::as_str` renders
// at full f64 precision instead, and it was reaching every string builtin and
// every dynamic-regex operand — so `CONVFMT` was honoured by concatenation,
// comparison and array subscripts but ignored by `length`, `substr`, `index`,
// `toupper`, `tolower`, `split`, `match`, `sub`, `gsub`, `gensub`, `~`, `!~`
// and the `getline` redirect target. gawk 5.4.1, mawk 1.3.4 and one-true-awk
// 20200816 agree on every case pinned below.

/// The string builtins measure and slice the **CONVFMT rendering** of a number.
/// `1.23456` under `%.2f` is the four-character string `1.23`, so `length` is 4
/// and `substr(x, 3)` is `23` — not `23456` off the full-precision spelling.
#[test]
fn convfmt_applies_to_every_string_builtin() {
    let cases: &[(&str, &str)] = &[
        ("print length(x)", "4\n"),
        ("print substr(x, 3), substr(x, 1, 99)", "23 1.23\n"),
        (
            "print index(x, \"456\"), index(x, \"23\"), index(\"a1.23b\", x)",
            "0 3 2\n",
        ),
        ("print toupper(x), tolower(x)", "1.23 1.23\n"),
        ("n = split(x, A, \"z\"); print n, A[1]", "1 1.23\n"),
    ];
    for (body, want) in cases {
        let program = format!("BEGIN {{ CONVFMT = \"%.2f\"; x = 1.23456; {body} }}");
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "{body}: stderr {stderr:?}");
        assert_eq!(stdout, *want, "{body}");
    }
}

/// A **dynamic regex** is the string value of its operand, so a number becomes a
/// pattern through CONVFMT too. Only the subject side of `~` / `!~` used to
/// convert this way, which made `"a1.23b" ~ x` false while `"a1.23b" == x` — the
/// same coercion, one operator apart — was true.
#[test]
fn convfmt_applies_to_dynamic_regex_operands() {
    let cases: &[(&str, &str)] = &[
        ("print (\"a1.23b\" ~ x), (\"a1.23b\" !~ x)", "1 0\n"),
        ("print match(\"a1.23b\", x), RSTART, RLENGTH", "2 2 4\n"),
        ("print split(\"a1.23b\", A, x)", "2\n"),
        ("s = \"a1.23b\"; print sub(x, \"Z\", s), s", "1 aZb\n"),
        ("s = \"a1.23b\"; print gsub(x, \"Z\", s), s", "1 aZb\n"),
    ];
    for (body, want) in cases {
        let program = format!("BEGIN {{ CONVFMT = \"%.2f\"; x = 1.23456; {body} }}");
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "{body}: stderr {stderr:?}");
        assert_eq!(stdout, *want, "{body}");
    }
}

/// `sub` / `gsub` read the **subject** as a string and write a string back, so a
/// numeric target is rewritten from its CONVFMT rendering: substituting in
/// `1.23456` under `%.2f` edits `1.23` and can never leave the `456` behind.
/// The replacement text converts the same way. Every target kind the compiler
/// emits is covered — a bare variable, an array element, and a field.
#[test]
fn convfmt_applies_to_sub_target_and_replacement() {
    let cases: &[(&str, &str)] = &[
        ("gsub(/3/, \"9\", x); print x", "1.29\n"),
        ("sub(/2/, \"9\", x); print x", "1.93\n"),
        ("A[1] = x; gsub(/3/, \"9\", A[1]); print A[1]", "1.29\n"),
        ("s = \"aXb\"; sub(/X/, x, s); print s", "a1.23b\n"),
        ("s = \"aXb\"; gsub(/X/, x, s); print s", "a1.23b\n"),
    ];
    for (body, want) in cases {
        let program = format!("BEGIN {{ CONVFMT = \"%.2f\"; x = 1.23456; {body} }}");
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "{body}: stderr {stderr:?}");
        assert_eq!(stdout, *want, "{body}");
    }
}

/// A field target goes through the same path: `$1` holding a computed number is
/// rewritten from its CONVFMT rendering, not from the full-precision spelling.
#[test]
fn convfmt_applies_to_sub_on_a_field_target() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        "{ CONVFMT = \"%.2f\"; $1 = 1.23456 + 0; gsub(/3/, \"9\", $1); print $1 }",
        "a\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1.29\n");
}

/// The `getline < expr` redirect names a file as a *string*, so a numeric
/// operand opens the CONVFMT rendering of it. awkrs used to look for a file
/// named `1.23456` and return -1 where the references read `1.23` and return 1.
#[test]
fn convfmt_applies_to_getline_redirect_filename() {
    let dir = std::env::temp_dir().join(format!("awkrs-convfmt-getline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    std::fs::write(dir.join("1.23"), "hi\n").expect("write fixture");
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = std::process::Command::new(bin)
        .arg(r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; r = (getline l < x); print r, l }"#)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run awkrs");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(stdout, "1 hi\n");
}

/// The coercion is performed **at the point of use**, never cached at
/// assignment: changing `CONVFMT` between two reads of the same variable changes
/// both answers. Pinning this rules out "convert once when the value is stored",
/// which would pass every single-format test above and still be wrong.
#[test]
fn convfmt_coercion_is_read_at_each_use_not_cached() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; a = length(x); CONVFMT = "%.4f"; b = length(x); print a, b }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "4 6\n");
}

/// Two values that must NOT be reshaped by `CONVFMT`, guarding the fix from
/// overreaching. An integral number bypasses the format entirely, and a field is
/// a string carrying the original input text — so `$1` of the record `1.23456`
/// stays seven characters under `%.2f` in all three references.
#[test]
fn convfmt_leaves_integral_numbers_and_input_text_alone() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT = "%.2f"; x = 100000; print length(x), toupper(x), index(x, "00000") }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "6 100000 2\n");

    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"{ CONVFMT = "%.2f"; print length($1), toupper($1), substr($1, 3) }"#,
        "1.23456\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "7 1.23456 23456\n");
}

/// The conversion lives in the interpreter's opcode handlers and builtin
/// dispatch, so a fix applied to one execution tier only would leave `-s` and
/// `-O` answering differently for the same program.
#[test]
fn convfmt_string_coercion_agrees_across_execution_tiers() {
    let program = r#"BEGIN { CONVFMT = "%.2f"; x = 1.23456; gsub(/3/, "9", x); print length(x), x, ("a1.23b" ~ x) }"#;
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut seen = Vec::new();
    for flag in ["-s", "-O"] {
        let out = std::process::Command::new(bin)
            .arg(flag)
            .arg(program)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run awkrs");
        assert!(
            out.status.success(),
            "{flag}: {:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        seen.push(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    assert_eq!(seen[0], "4 1.29 0\n", "-s (no JIT)");
    assert_eq!(seen[1], seen[0], "-O (JIT) disagrees with -s");
}

/// POSIX `printf`: the `+` and space flags prefix a sign onto a *non-negative*
/// value for every signed conversion, not just the integer ones. awkrs applied
/// them to `%d`/`%i` and dropped them everywhere else, so `printf "% .2e", 1234.5`
/// printed `1.23e+03` where gawk, mawk and one-true-awk all print ` 1.23e+03`.
/// A leading space is invisible in a terminal but shifts every column of a
/// report, so this silently misformatted output rather than failing loudly.
#[test]
fn printf_sign_flags_apply_to_every_float_conversion() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[% f][% e][% g][%+f][%+e][%+g]\n", 1.5, 1.5, 1.5, 1.5, 1.5, 1.5 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(
        stdout,
        "[ 1.500000][ 1.500000e+00][ 1.5][+1.500000][+1.500000e+00][+1.5]\n"
    );
}

/// Zero is non-negative, so it takes the sign prefix too — the flags key off the
/// absence of a `-`, not off the value being greater than zero.
#[test]
fn printf_sign_flags_apply_to_zero() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%+f][% f][%+e][% g][%+d]\n", 0, 0, 0, 0, 0 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[+0.000000][ 0.000000][+0.000000e+00][ 0][+0]\n");
}

/// A negative value already carries its own `-`, so neither flag may add a
/// second sign character.
#[test]
fn printf_sign_flags_leave_negative_values_alone() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%+f][% f][%+g][% e]\n", -1.5, -1.5, -1.5, -1.5 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[-1.500000][-1.500000][-1.5][-1.500000e+00]\n");
}

/// The sign counts toward the field width, and under `0` padding the zeros go
/// *between* the sign and the magnitude. Left justification pushes the padding
/// to the right of the whole signed number.
#[test]
fn printf_sign_flags_combine_with_width_and_zero_padding() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%010.2f][% 010.2f][%+010.2f][%-+10.2f][%- 10.2f]\n", 3.5, 3.5, 3.5, 3.5, 3.5 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(
        stdout,
        "[0000003.50][ 000003.50][+000003.50][+3.50     ][ 3.50     ]\n"
    );
}

/// The flags survive a `*`-supplied width and precision, which take a different
/// path through the conversion parser than literal digits do.
#[test]
fn printf_sign_flags_survive_star_width_and_precision() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%+*.*f][% *.*e]\n", 10, 2, 3.5, 12, 3, 3.5 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[     +3.50][   3.500e+00]\n");
}

/// A non-finite value is spelled `+inf`/`-nan` and already carries a sign, so
/// the `+` flag must not produce `++inf`. This is the one path where re-signing
/// the formatted magnitude would be wrong.
#[test]
fn printf_sign_flags_do_not_double_sign_infinity() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { i = 1e308 * 10; printf "[%+f][% f][%+g][%+e][%+a]\n", i, i, i, i, i }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[+inf][+inf][+inf][+inf][+inf]\n");
}

/// C's `#` (alternate form) flag forces the radix point onto a floating
/// conversion even when the precision leaves no fractional digits. awkrs
/// honoured `#` for `%x`/`%o`/`%a` and ignored it for `%f`/`%e`/`%g`, so
/// `printf "%#.0f", 2` printed `2` where all three references print `2.`.
#[test]
fn printf_alt_flag_forces_radix_point_on_float_conversions() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%#.0f][%#.1f][%#.0e][%#.0E][%#.1g][%#.0g]\n", 2, 2, 2, 2, 2.0, 5 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[2.][2.0][2.e+00][2.E+00][2.][5.]\n");
}

/// For `%g` the `#` flag additionally suppresses the removal of trailing zeros,
/// so the full significant-digit count survives in both the fixed and the
/// exponent form. `%#g` of 0.0001 keeps six significant digits.
#[test]
fn printf_alt_flag_keeps_trailing_zeros_on_g() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%#.6g][%#.3g][%#g][%#G]\n", 1.5, 100.0, 0.0001, 2.0 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[1.50000][100.][0.000100000][2.00000]\n");
}

/// `%g` precision is a count of *significant* digits, so a zero under `#` keeps
/// p-1 fractional digits — `%#g` of 0 is `0.00000`, five zeros, not six. The
/// non-alternate path trims the fraction away entirely and never exposed this.
#[test]
fn printf_alt_flag_on_g_of_zero_keeps_significant_digit_count() {
    let (code, stdout, stderr) =
        run_awkrs_stdin(r#"BEGIN { printf "[%#g][%#.3g][%#.1g]\n", 0, 0, 0 }"#, "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[0.00000][0.00][0.]\n");
}

/// In the exponent form the point is inserted before the `e`, not appended to
/// the field, and the two-digit exponent is preserved.
#[test]
fn printf_alt_flag_inserts_radix_point_before_the_exponent() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[%#.3g][%#g][%#.1g][%#.10g]\n", 0.0000001, 1e20, 1e20, 1.5 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "[1.00e-07][1.00000e+20][1.e+20][1.500000000]\n");
}

/// `#` and a sign flag apply together, and the radix point counts toward the
/// field width so zero padding still lands between the sign and the digits.
#[test]
fn printf_alt_and_sign_flags_combine() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "[% #.0f][%+#.0e][% #g][%#010.0f][% #010.0f][%+#010.0e]\n", 1, 1, 1.0, 2, 2, 2 }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(
        stdout,
        "[ 1.][+1.e+00][ 1.00000][000000002.][ 00000002.][+0002.e+00]\n"
    );
}

/// awkrs deliberately makes character semantics UTF-8 rather than locale-driven
/// (`docs/COMPATIBILITY.md` §9), and `-b` / `--characters-as-bytes` is the
/// documented opt-out into byte semantics. `-b` already switched `length`,
/// `substr` and `index` to counting bytes, but case folding stayed Unicode-aware
/// regardless — so one `-b` program reported `length("café") == 5` while
/// `toupper("café")` returned the folded `CAFÉ`, mixing the byte world and the
/// character world in a single run. Under `-b` the fold is ASCII-only, which is
/// what gawk, mawk and one-true-awk all do in the C locale.
#[test]
fn characters_as_bytes_makes_case_folding_ascii_only() {
    let (code, stdout, stderr) = run_awkrs_stdin_args(
        ["-b"],
        r#"BEGIN { print toupper("café"), tolower("CAFÉ"), toupper("aBc1!~"), tolower("AbC1!~") }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "CAFé cafÉ ABC1!~ abc1!~\n");
}

/// The self-consistency this fixes: under `-b`, `length` and the fold must agree
/// on which world they are in. Unicode uppercases `ß` to the two-character `SS`,
/// so the Unicode fold silently grew a byte-counted record by one.
#[test]
fn characters_as_bytes_case_folding_preserves_length() {
    let (code, stdout, stderr) = run_awkrs_stdin_args(
        ["-b"],
        r#"{ u = toupper($0); print u, length(u), length($0) }"#,
        "Straße\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "STRAßE 7 7\n");
}

/// Without `-b` the Unicode fold is the documented awkrs behaviour and must stay
/// put — this is a deliberate deviation from the references in the C locale, not
/// an accident, and the character world is self-consistent too: `length` counts
/// scalars and the fold maps them.
#[test]
fn default_mode_keeps_unicode_case_folding() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { print toupper("café"), tolower("CAFÉ"), length("café") }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "CAFÉ café 4\n");
}

/// Neither mode consults the environment: the choice is the flag, so the answer
/// is identical under `LC_ALL=C` and under a UTF-8 locale. That is what keeps
/// this test meaningful in a bare CI container, which may have no locales
/// generated at all.
#[test]
fn case_folding_does_not_depend_on_the_locale_environment() {
    for locale in ["C", "en_US.UTF-8", "POSIX"] {
        let env = [("LC_ALL".into(), locale.into())];
        let (code, stdout, stderr) = run_awkrs_stdin_args_env(
            Vec::<String>::new(),
            r#"BEGIN { print toupper("café") }"#,
            "",
            env,
        );
        assert_eq!(code, 0, "{locale}: stderr {stderr:?}");
        assert_eq!(stdout, "CAFÉ\n", "LC_ALL={locale} changed the default fold");

        let env = [("LC_ALL".into(), locale.into())];
        let (code, stdout, stderr) =
            run_awkrs_stdin_args_env(["-b"], r#"BEGIN { print toupper("café") }"#, "", env);
        assert_eq!(code, 0, "{locale}: stderr {stderr:?}");
        assert_eq!(stdout, "CAFé\n", "LC_ALL={locale} changed the -b fold");
    }
}

/// Both fixes live in code the two execution tiers reach by different routes:
/// `toupper` has a separate implementation in the fusevm host from the one in
/// the tree-walking builtin dispatch, and the JIT re-runs the formatter. A fix
/// applied to one tier only would leave `-s` and `-O` disagreeing on the same
/// program, which no reference awk ever does. `-b` is included because the
/// fusevm host reads the flag from the runtime rather than being handed it.
#[test]
fn float_flags_and_case_folding_agree_across_execution_tiers() {
    let program = r#"BEGIN { printf "[%#.0f][%+f][% g][%#.6g]|", 2, 0, 0, 1.5; print toupper("café"), tolower("CAFÉ") }"#;
    let bin = env!("CARGO_BIN_EXE_awkrs");
    for (extra, want) in [
        ("-b", "[2.][+0.000000][ 0][1.50000]|CAFé cafÉ\n"),
        ("--", "[2.][+0.000000][ 0][1.50000]|CAFÉ café\n"),
    ] {
        let mut seen = Vec::new();
        for flag in ["-s", "-O"] {
            let mut cmd = std::process::Command::new(bin);
            cmd.arg(flag);
            if extra != "--" {
                cmd.arg(extra);
            }
            let out = cmd
                .arg(program)
                .stdin(std::process::Stdio::null())
                .output()
                .expect("run awkrs");
            assert!(
                out.status.success(),
                "{flag} {extra}: {:?}",
                String::from_utf8_lossy(&out.stderr)
            );
            seen.push(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        assert_eq!(seen[0], want, "-s (no JIT) with {extra}");
        assert_eq!(seen[1], seen[0], "-O (JIT) disagrees with -s under {extra}");
    }
}

/// The awk-ERE to Rust-regex translator walked the pattern byte by byte and
/// finished each iteration with `byte as char`, which latin-1-widens every half
/// of a multi-byte UTF-8 sequence: `é` (0xC3 0xA9) compiled as the two-character
/// `Ã©` and could never match itself. `~`, `!~` and `match()` therefore answered
/// "no match" for any pattern containing a non-ASCII character.
#[test]
fn regex_matches_a_non_ascii_literal() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { s = "café"; print (s ~ /é/), (s ~ "é"), (s !~ /é/), ("aéb" ~ /éb/), ("abc" ~ /é/) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1 1 0 1 0\n");
}

/// `gsub` and `split` masked the bug: a metacharacter-free pattern takes the
/// literal-substring fast path and never reaches the translator. Pinning them
/// alongside `~` keeps the two paths from drifting apart again.
#[test]
fn non_ascii_patterns_agree_between_the_regex_and_literal_paths() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { s = "café"; n = gsub(/é/, "E", s); print n, s, ("café" ~ /é/)
                  m = split("aébéc", A, /é/); print m, A[1], A[2], A[3] }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1 cafE 1\n3 a b c\n");
}

/// The translator's own rewrites are all keyed on ASCII and must survive the
/// switch from a byte walk to a character walk: POSIX ERE has no `\d` digit
/// class and no `\1` backreference, and a bracket expression suppresses escape
/// rewriting inside it.
#[test]
fn regex_translator_ascii_rules_survive_the_character_walk() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { print ("d" ~ /\d/), ("1" ~ /\d/), ("a]b" ~ /[]]/), ("a.c" ~ /a\.c/), ("abc" ~ /a\.c/)
                  print ("a-b" ~ /[a-]/), ("x" ~ /[^abc]/), ("A" ~ /[[:upper:]]/), ("ab" ~ /a{1,2}b/) }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1 0 1 1 0\n1 1 1 1\n");
}

/// `match()` published RSTART and RLENGTH as *byte* offsets while writing the
/// `arr[i, "start"]` entries as *character* offsets, so one call disagreed with
/// itself on multibyte input: `match("ééx", /x/, A)` set RSTART to 5 and
/// `A[0,"start"]` to 3. awk reports character positions — the same unit
/// `substr`, `index` and `length` use — so 3 is the answer in both places.
#[test]
fn match_reports_rstart_and_rlength_in_characters() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { print match("ééx", /x/), RSTART, RLENGTH
                  print match("ééxy", /xy/), RSTART, RLENGTH
                  print match("café", /é/), RSTART, RLENGTH
                  print match("abc", /z/), RSTART, RLENGTH }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "3 3 1\n3 3 2\n4 4 1\n0 0 -1\n");
}

/// The self-consistency half of the same fix: the scalar specials and the
/// submatch array must describe the identical match.
#[test]
fn match_specials_agree_with_the_submatch_array() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { match("ééx", /x/, A); print RSTART, RLENGTH, A[0,"start"], A[0,"length"], A[0] }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "3 1 3 1 x\n");
}

/// ASCII positions are the overwhelmingly common case and must be untouched by
/// the character-offset conversion.
#[test]
fn match_positions_are_unchanged_for_ascii_subjects() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { print match("hello world", /o w/), RSTART, RLENGTH
                  print match("hello", //), RSTART, RLENGTH }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "5 5 3\n1 1 0\n");
}

/// Both execution tiers reach `match` through different opcodes but share one
/// implementation; a divergence here would mean the fusevm host had grown its
/// own copy.
#[test]
fn non_ascii_regex_and_match_agree_across_execution_tiers() {
    let program = r#"BEGIN { print ("café" ~ /é/), match("ééx", /x/), RSTART, RLENGTH }"#;
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut seen = Vec::new();
    for flag in ["-s", "-O"] {
        let out = std::process::Command::new(bin)
            .arg(flag)
            .arg(program)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run awkrs");
        assert!(
            out.status.success(),
            "{flag}: {:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        seen.push(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    assert_eq!(seen[0], "1 3 3 1\n", "-s (no JIT)");
    assert_eq!(seen[1], seen[0], "-O (JIT) disagrees with -s");
}

/// A `var=value` operand is a command-line assignment, not an input file, and
/// it takes effect at the point it occupies among the operands.
///
/// awkrs read every operand as a file name and died with
/// `cannot open file "v=1"`, which made the whole POSIX operand-assignment
/// form unavailable. gawk, mawk and one-true-awk all run this and print an
/// unset `v` for the first file and `1` for the second.
#[test]
fn a_var_equals_value_operand_assigns_rather_than_naming_a_file() {
    let dir = std::env::temp_dir().join(format!("awkrs-operand-assign-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    std::fs::write(&first, b"x\n").expect("write first");
    std::fs::write(&second, b"y\n").expect("write second");

    let (code, stdout, stderr) = run_awkrs_operands(
        "{ print $0, v }",
        [
            first.to_str().expect("utf-8 path"),
            "v=1",
            second.to_str().expect("utf-8 path"),
        ],
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "x \ny 1\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// When no operand names a real file the program still reads standard input,
/// with the assignments applied first — `awk '{print v}' v=7` is a working
/// program in every reference, not a request to open a file called `v=7`.
#[test]
fn operands_that_are_all_assignments_still_read_standard_input() {
    let (code, stdout, stderr) = run_awkrs_operands("{ print v, $0 }", ["v=7"], "z\n");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "7 z\n");
}

/// A command-line assignment is a POSIX *numeric string*, so it compares
/// numerically as well as textually. All three references answer `1 1 1`.
#[test]
fn a_command_line_assignment_is_a_numeric_string() {
    let (code, stdout, stderr) = run_awkrs_operands(
        r#"{ print (v == 7), (v == "7"), (v < 10) }"#,
        ["v=7"],
        "r\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1 1 1\n");
}

/// Only an operand whose left side is a valid awk identifier is an assignment.
/// `2x=3` is not one, so it names a file — and there is no such file, which is
/// the error every reference reports. Losing this test would let the
/// assignment rule swallow ordinary file names that happen to contain `=`.
#[test]
fn an_operand_whose_name_is_not_an_identifier_is_still_a_file() {
    let (code, _stdout, _stderr) = run_awkrs_operands("{ print }", ["2x=3"], "");
    assert_ne!(
        code, 0,
        "`2x=3` must be read as a (missing) file, not an assignment"
    );
}

/// POSIX processes the value of `-v var=value` "as if it were a string
/// literal", so the escapes are decoded. awkrs stored the raw argument and
/// `length` answered 6 for `a\tb\n` where gawk, mawk and one-true-awk all
/// answer 4. The octal case pins that the lexer's full escape table is in play,
/// not a hand-rolled subset that only knows `\t` and `\n`.
#[test]
fn dash_v_assignment_values_undergo_string_literal_escape_processing() {
    let (code, stdout, stderr) =
        run_awkrs_stdin_args(["-v", r"s=a\tb\n"], "BEGIN { print length(s) }", "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "4\n");

    let (code, stdout, _) =
        run_awkrs_stdin_args(["-v", r"s=a\101b"], r#"BEGIN { printf "%s", s }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "aAb");

    // Quotes and backslashes together. A `"` is never a string terminator here,
    // but it still has to reach the lexer inside real quotes, and the escaping
    // that arranges that must not double-escape a `"` the caller already wrote
    // as `\"` — doing so turned the preceding backslash into a literal `\` that
    // swallowed the closing quote and made `a\"b` come out as `a\`. Every row
    // below is what gawk, mawk and one-true-awk all produce.
    for (value, want) in [
        (r"s=a\", r"a\"),           // trailing lone backslash is kept
        ("s=a\"b", "a\"b"),         // bare quote is a plain character
        ("s=a\\\"b", "a\"b"),       // already-escaped quote decodes to one quote
        ("s=a\\\\\"b", "a\\\"b"),   // escaped backslash, then a bare quote
        ("s=a\\\\\\\"b", "a\\\"b"), // escaped backslash, then an escaped quote
        (r"s=a\\b", r"a\b"),        // escaped backslash mid-value
        ("s=\"", "\""),             // a lone bare quote
    ] {
        let (_, stdout, _) = run_awkrs_stdin_args(["-v", value], r#"BEGIN { printf "%s", s }"#, "");
        assert_eq!(stdout, want, "-v {value}");
    }
}

/// An operand assignment gets the same escape processing as `-v`.
#[test]
fn operand_assignment_values_undergo_escape_processing_too() {
    let (code, stdout, stderr) = run_awkrs_operands("{ print length(v) }", [r"v=a\tb"], "line\n");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "3\n");
}

/// `ARGV` is writable and awk consults it while walking the operands: deleting
/// an element, or setting it to the empty string, skips that file, and
/// rewriting one redirects the read. awkrs iterated the argv it started with,
/// so a deleted file was read anyway.
#[test]
fn argv_edits_change_which_files_are_read() {
    let dir = std::env::temp_dir().join(format!("awkrs-argv-edit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    std::fs::write(&first, b"one\n").expect("write first");
    std::fs::write(&second, b"two\n").expect("write second");
    let (a, b) = (
        first.to_str().expect("utf-8 path").to_string(),
        second.to_str().expect("utf-8 path").to_string(),
    );

    for (program, want) in [
        ("BEGIN { delete ARGV[1] } { print }", "two\n"),
        (r#"BEGIN { ARGV[1] = "" } { print }"#, "two\n"),
        ("{ print }", "one\ntwo\n"),
    ] {
        let (code, stdout, stderr) = run_awkrs_operands(program, [&a, &b], "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }

    // Rewriting an entry redirects the read to the named file.
    let program = format!(r#"BEGIN {{ ARGV[2] = "{a}" }} {{ print }}"#);
    let (code, stdout, stderr) = run_awkrs_operands(&program, [&a, &b], "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "one\none\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Every redirected `getline` split on newlines regardless of `RS`, while the
/// main record loop honoured it — so one program could disagree with itself
/// about where a record ends. Verified against all three references on the
/// file `L1\nL2\nL3\n`:
///
/// ```text
/// BEGIN { RS="2"; while ((getline l < "in") > 0) printf "[%s]", l }
/// gawk 5.4.1 / one-true-awk 20200816 / mawk 1.3.4 → [L1\nL][\nL3\n]
/// awkrs before                                    → [L1][L2][L3]
/// ```
///
/// The `$0` form, the pipe form, a regex `RS` and paragraph mode were all wrong
/// the same way; all five spellings are pinned here.
#[test]
fn redirected_getline_honours_rs() {
    let dir = std::env::temp_dir().join(format!("awkrs-getline-rs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("in.txt");
    std::fs::write(&path, b"L1\nL2\nL3\n").expect("write fixture");
    let p = path.to_string_lossy().into_owned();

    for (program, want) in [
        // Literal multi-character-position RS, into a variable and into `$0`.
        (
            format!(r#"BEGIN {{ RS="2"; while ((getline l < "{p}") > 0) printf "[%s]", l }}"#),
            "[L1\nL][\nL3\n]",
        ),
        (
            format!(r#"BEGIN {{ RS="2"; while ((getline < "{p}") > 0) printf "[%s]", $0 }}"#),
            "[L1\nL][\nL3\n]",
        ),
        // The same stream through a pipe rather than a file redirect.
        (
            format!(r#"BEGIN {{ RS="2"; while (("cat {p}" | getline l) > 0) printf "[%s]", l }}"#),
            "[L1\nL][\nL3\n]",
        ),
        // A regex RS: every digit is a separator, so the records are the `L`s
        // and the newlines between them, plus a final empty one.
        (
            format!(r#"BEGIN {{ RS="[0-9]"; while ((getline l < "{p}") > 0) printf "[%s]", l }}"#),
            "[L][\nL][\nL][\n]",
        ),
        // gawk publishes RT from a redirected getline too.
        (
            format!(r#"BEGIN {{ RS="2"; getline l < "{p}"; printf "[%s][%s]", l, RT }}"#),
            "[L1\nL][2]",
        ),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// Paragraph mode is the one `RS` form a redirected `getline` did get right by
/// accident, because blank-line-separated records also end at newlines. Pinned
/// so the RS rework above cannot silently regress it.
#[test]
fn redirected_getline_paragraph_mode_matches_the_record_loop() {
    let dir = std::env::temp_dir().join(format!("awkrs-getline-para-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("para.txt");
    std::fs::write(&path, b"a1\na2\n\n\nb1\n\nc1\nc2\nc3\n").expect("write fixture");
    let p = path.to_string_lossy().into_owned();

    // gawk, mawk and one-true-awk all give three records, runs of blank lines
    // collapsing to one separator.
    let program =
        format!(r#"BEGIN {{ RS=""; while ((getline < "{p}") > 0) printf "<%s|%d>", $0, NF }}"#);
    let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "<a1\na2|2><b1|1><c1\nc2\nc3|3>");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A `\r` before the record's newline belongs to the record — awkrs's own
/// record loop already kept it, and all three references keep it in `getline`
/// too. `getline` trimmed `['\n', '\r']`, so the same CRLF file measured one
/// character shorter through `getline` than through the main loop.
#[test]
fn getline_keeps_a_carriage_return_like_the_record_loop() {
    let dir = std::env::temp_dir().join(format!("awkrs-getline-crlf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("crlf.txt");
    std::fs::write(&path, b"a\r\nb\r\n").expect("write fixture");
    let p = path.to_string_lossy().into_owned();

    for program in [
        format!(r#"BEGIN {{ while ((getline l < "{p}") > 0) print length(l) }}"#),
        format!(r#"BEGIN {{ while ((getline < "{p}") > 0) print length($0) }}"#),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, "2\n2\n", "{program}");
    }

    // The main record loop, for the same bytes — the two paths must agree.
    let (code, stdout, _) = run_awkrs_file("{ print length($0) }", &path);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n2\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// POSIX walks the operands as `for (i = 1; i < ARGC; i++)` and re-reads `ARGC`
/// each pass, so a program can shorten the list, extend it, or cut it short
/// mid-run. awkrs iterated the argv vector it was launched with and honoured
/// none of the three. Every expectation below is gawk 5.4.1, one-true-awk and
/// mawk agreeing.
#[test]
fn argc_bounds_the_operand_walk() {
    let dir = std::env::temp_dir().join(format!("awkrs-argc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    std::fs::write(&a, b"a1\na2\n").expect("write a");
    std::fs::write(&b, b"b1\n").expect("write b");
    let (sa, sb) = (
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    );

    for (program, want) in [
        // Lowered before any input is read: no operand is a file, so the program
        // reads standard input, which is empty here.
        ("BEGIN { ARGC = 1 } END { print NR }".to_string(), "0\n"),
        // Cut to the first operand only.
        ("BEGIN { ARGC = 2 } END { print NR }".to_string(), "2\n"),
        // Raised past the entries that exist: the missing ones are skipped.
        ("BEGIN { ARGC = 9 } END { print NR }".to_string(), "3\n"),
        // Below one — including a non-numeric value, which coerces to 0 the way
        // it does in gawk and mawk — leaves no operands at all.
        ("BEGIN { ARGC = 0 } END { print NR }".to_string(), "0\n"),
        (
            r#"BEGIN { ARGC = "x" } END { print NR }"#.to_string(),
            "0\n",
        ),
        // Untouched, for the baseline.
        ("END { print NR }".to_string(), "3\n"),
        // Shortened while the first file is still being read.
        (
            "FNR == 1 && NR == 1 { ARGC = 2 } END { print NR }".to_string(),
            "2\n",
        ),
    ] {
        let (code, stdout, stderr) = run_awkrs_operands(&program, [&sa, &sb], "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }

    // Extending the list adds a file that was never on the command line.
    let program = format!(r#"BEGIN {{ ARGV[2] = "{sb}"; ARGC = 3 }} {{ print }}"#);
    let (code, stdout, stderr) = run_awkrs_operands(&program, [&sa], "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "a1\na2\nb1\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Only a regular file can be memory-mapped, and awkrs mapped every input file
/// unconditionally — so a character device or a pipe named as an operand came
/// back as a fatal "cannot open file" (`ENODEV` / `EINVAL`) where gawk, mawk and
/// one-true-awk all read it. `/dev/null` is the portable case: it exists on
/// every Unix CI image and is a character device on all of them.
#[test]
fn a_non_regular_file_operand_is_readable() {
    let dir = std::env::temp_dir().join(format!("awkrs-devnull-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let a = dir.join("a.txt");
    std::fs::write(&a, b"one\ntwo\n").expect("write a");
    let sa = a.to_string_lossy().into_owned();

    // Alone: no records, no error.
    let (code, stdout, stderr) = run_awkrs_operands("END { print NR }", ["/dev/null"], "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "0\n");

    // And it must not derail the operands around it.
    let (code, stdout, stderr) = run_awkrs_operands("{ print FNR, $0 }", ["/dev/null", &sa], "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1 one\n2 two\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// POSIX's print statement is `print expr_list_opt output_redirection` — the
/// expression list is optional *with* a redirection as well as without one, and
/// the bare form prints `$0`. awkrs only recognised the bare form at a statement
/// end, so every redirected spelling was a parse error:
///
/// ```text
/// $ printf 'r1\nr2\n' | awk '{ print > "/dev/stdout" }'
/// gawk 5.4.1 / one-true-awk 20200816 / mawk 1.3.4 → r1\nr2\n   [exit 0]
/// awkrs                                           → parse error at line 1:
///                                                    unexpected token in expression: Gt
/// ```
#[test]
fn bare_print_accepts_a_redirection() {
    let dir = std::env::temp_dir().join(format!("awkrs-bare-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let out = dir.join("out.txt");
    let p = out.to_string_lossy().into_owned();

    // `>` and `>>` against a real file, so the record content is checked and not
    // just the exit status.
    for (program, want) in [
        (format!(r#"{{ print > "{p}" }}"#), "r1\nr2\n"),
        (format!(r#"{{ print >> "{p}" }}"#), "r1\nr2\n"),
    ] {
        let _ = std::fs::remove_file(&out);
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "r1\nr2\n");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, "", "{program} wrote to stdout");
        let got = std::fs::read_to_string(&out).expect("redirect target");
        assert_eq!(got, want, "{program}");
    }

    // The pipe form reaches stdout through the child.
    let (code, stdout, stderr) = run_awkrs_stdin(r#"{ print | "cat" }"#, "r1\nr2\n");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "r1\nr2\n");

    // The relational reading of `>` inside a parenthesised print argument is
    // unchanged — `print (1>2)` is a comparison, not a redirect to the file `2`.
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { print (1>2) }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `FPAT` matches case-sensitively whatever `IGNORECASE` says. gawk is the only
/// reference that implements `FPAT`, and it is unambiguous:
///
/// ```text
/// $ printf 'AB cd EF\n' | gawk 'BEGIN { FPAT="[a-z]+"; IGNORECASE=1 } { print NF, $1 }'
/// 1 cd
/// $ printf 'AB cd EF\n' | gawk 'BEGIN { FPAT="ab";      IGNORECASE=1 } { print NF }'
/// 0
/// ```
///
/// awkrs compiled the `FPAT` alternatives case-insensitively, so the first
/// answered `3 AB` — `AB` and `EF` became fields too — and the second `1`.
#[test]
fn fpat_does_not_honour_ignorecase() {
    for (program, want) in [
        (
            r#"BEGIN { FPAT="[a-z]+"; IGNORECASE=1 } { print NF, $1 }"#,
            "1 cd\n",
        ),
        (
            r#"BEGIN { FPAT="[a-z]+"; IGNORECASE=0 } { print NF, $1 }"#,
            "1 cd\n",
        ),
        (r#"BEGIN { FPAT="ab"; IGNORECASE=1 } { print NF }"#, "0\n"),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(program, "AB cd EF\n");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }
}

/// The regex `FS`, the `split()` separator and the `FPAT` alternatives are all
/// memoised now, so the cases that must still invalidate the memo are pinned
/// here: a mid-run change of the pattern, and a mid-run change of `IGNORECASE`
/// (which only `FS` honours). The `FS`-change and `split()` expectations are
/// gawk, one-true-awk and mawk agreeing; the `IGNORECASE` and `FPAT` ones are
/// gawk alone, because neither extension exists in the other two — mawk and
/// one-true-awk answer `2 1 1` to the `IGNORECASE` case, having no such
/// variable.
#[test]
fn memoised_split_patterns_still_track_changes() {
    // A regex FS replaced between records.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { FS="[0-9]" } NR==2 { FS="," } { print NF "|" $1 }"#,
        "a1b,c\nx2y,z\nq3r,s\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    // The new FS takes effect on the *next* record, so record 2 still splits on
    // the digit and record 3 splits on the comma.
    assert_eq!(stdout, "2|a\n2|x\n2|q3r\n");

    // IGNORECASE flipped between records, with a multi-character FS — the one
    // separator form that honours it.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { FS="XY" } NR==2 { IGNORECASE=1 } { print NF }"#,
        "aXYb\naxyb\naxyb\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "2\n1\n2\n");

    // The same separator text used by `split()` many times, and a different one
    // afterwards: the memo must not answer the second with the first's engine.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN {
             for (i = 0; i < 3; i++) n = split("a1b22c", A, "[0-9]+")
             m = split("a1b22c", B, "[0-9]")
             print n, A[3], m, B[3]
           }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "3 c 4 \n"); // gawk, one-true-awk and mawk agree

    // `IGNORECASE` flipped between two `split()` calls that share a separator,
    // in both directions — the memo must not answer the second with the first's
    // engine. gawk (the only reference with `IGNORECASE`) prints `1 2` and
    // `2 1`; dropping the flag from the memo key makes both `1 1` and `2 2`.
    for (program, want) in [
        (
            r#"BEGIN { n = split("aXYb", A, "xy"); IGNORECASE = 1; m = split("aXYb", B, "xy"); print n, m }"#,
            "1 2\n",
        ),
        (
            r#"BEGIN { IGNORECASE = 1; n = split("aXYb", A, "xy"); IGNORECASE = 0; m = split("aXYb", B, "xy"); print n, m }"#,
            "2 1\n",
        ),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(program, "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }

    // A separator that Rust's regex crate refuses but every reference accepts
    // still falls back to a literal split, and does so identically on every
    // call — memoising the failure must not make the first call differ from the
    // rest. `))` is the case: an unmatched `)` is a literal in gawk, mawk and
    // one-true-awk alike, all three print `1` here, and Rust rejects it.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { for (i = 1; i <= 3; i++) { n = split("a)b)c", A, "))"); printf "%d:%d ", i, n } print "" }"#,
        "",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1:1 2:1 3:1 \n");

    // FPAT replaced between records.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { FPAT="[0-9]+" } NR==2 { FPAT="[a-z]+" } { print NF "|" $1 }"#,
        "a1b\nc2d\ne3f\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1|1\n1|2\n2|e\n"); // gawk 5.4.1
}

/// An `FS` or `split()` separator that is not a valid ERE is fatal, exactly
/// where gawk 5.4.1, mawk 1.3.4 and one-true-awk 20200816 all make it fatal.
///
/// awkrs used to split on the separator text literally and exit 0. The three
/// references phrase the diagnostic differently ("unbalanced [", "bad class",
/// "nonterminated character class"), so only the status and the fatality are
/// portable enough to pin; the message text is awkrs's own.
#[test]
fn an_invalid_regex_separator_is_fatal_like_every_reference() {
    // Unanimously fatal in all three references — verified by running each of
    // `gawk`, `mawk` and `/usr/bin/awk` on `split("xayb", arr, PAT)`.
    for pat in [
        "[[", "((", "a{2,1}", "{2}", "*x", "+x", "?x", "[]", "[a-", "[[:alpha:]", "[^]", "(?:a)",
        "(?i)a",
    ] {
        let program = format!(r#"BEGIN {{ n = split("xayb", A, "{pat}"); print n }}"#);
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 2, "split separator {pat:?} should be fatal: {stdout:?}");
        assert!(
            stderr.contains("invalid regexp"),
            "split separator {pat:?}: stderr {stderr:?}"
        );
        assert_eq!(stdout, "", "split separator {pat:?}");
    }

    // The same patterns are fatal once a record is read under them as `FS`.
    for pat in ["[[", "((", "a{2,1}"] {
        let (code, stdout, stderr) =
            run_awkrs_stdin_args([format!("-F{pat}")], "{ print NR }", "a b\nc d\n");
        assert_eq!(code, 2, "FS {pat:?} should be fatal: {stdout:?}");
        assert!(
            stderr.contains("invalid regexp"),
            "FS {pat:?}: stderr {stderr:?}"
        );
    }

    // Nothing the references disagree about becomes fatal. Each of these is
    // accepted by at least two of the three, so awkrs keeps accepting it:
    //   `))` `}` `{` `a{` `a}`   — all three accept
    //   `a{1,`                   — mawk accepts
    //   `|x` `x|`                — gawk accepts
    //   `[z-a]` `[a-b-c]`        — mawk and one-true-awk accept
    //   `[[:foo:]]`              — one-true-awk accepts
    //   `()` `(|)`               — gawk accepts
    for pat in [
        "))", "}", "{", "a{", "a}", "a{1,", "|x", "x|", "[z-a]", "[a-b-c]", "[[:foo:]]", "()",
        "(|)",
    ] {
        let program = format!(r#"BEGIN {{ n = split("xayb", A, "{pat}"); print n }}"#);
        let (code, _stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "split separator {pat:?}: stderr {stderr:?}");
    }

    // A single character is a literal separator, never a regex, so the bracket
    // metacharacters are not errors on their own. All three print `2`.
    for (pat, want) in [("[", "2\n"), ("(", "2\n"), ("*", "2\n"), ("{", "2\n")] {
        let program = format!(r#"BEGIN {{ n = split("a{pat}b", A, "{pat}"); print n }}"#);
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "single-char separator {pat:?}: stderr {stderr:?}");
        assert_eq!(stdout, want, "single-char separator {pat:?}");
    }

    // An `FS` that no record is ever split with stays silent: gawk and mawk are
    // fatal here but one-true-awk exits 0, so the majority rules.
    let (code, stdout, _stderr) = run_awkrs_stdin(r#"BEGIN { FS = "[[" } END { print "done" }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "done\n");
}

/// A `split()` separator goes through the same awk→Rust regex rewrite the `~`
/// operator uses. It did not, so the awk-only spellings were compiled by Rust's
/// parser raw, failed, and silently degraded to a literal split.
#[test]
fn a_split_separator_honours_the_awk_regex_escapes() {
    // Octal escapes, in and out of a bracket expression: `\101` is `A`.
    // gawk 5.4.1, mawk 1.3.4 and one-true-awk 20200816 all print `2|a|b`.
    for sep in [r"\\101", r"[\\101]"] {
        let program = format!(r#"BEGIN {{ n = split("aAb", x, "{sep}"); print n "|" x[1] "|" x[2] }}"#);
        let (code, stdout, stderr) = run_awkrs_stdin(&program, "");
        assert_eq!(code, 0, "separator {sep:?}: stderr {stderr:?}");
        assert_eq!(stdout, "2|a|b\n", "separator {sep:?}");
    }

    // `\d` is not an ERE operator: every reference matches the literal letter
    // `d`, where Rust's regex crate would read a digit class.
    let (code, stdout, stderr) =
        run_awkrs_stdin(r#"BEGIN { n = split("a1bdc", x, "\\d"); print n "|" x[1] "|" x[2] }"#, "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "2|a1b|c\n");

    // `\8` is not an octal digit, so it is the plain character `8`.
    let (code, stdout, stderr) =
        run_awkrs_stdin(r#"BEGIN { n = split("a8b", x, "\\8"); print n "|" x[1] "|" x[2] }"#, "");
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "2|a|b\n");
}

/// The fused `a[$n] += <literal>` opcode has to answer exactly what the
/// unfused form answers.
///
/// The fusion only fires for a non-integer literal delta — an integer literal
/// compiles to `PushNumDecimalStr`, which the pattern does not match — so each
/// case below pairs the fused spelling with the unfused one that proves the
/// divergence was the fusion's. All expectations are from gawk 5.4.1, mawk
/// 1.3.4 and one-true-awk 20200816, which agree on every one.
#[test]
fn the_fused_array_field_add_matches_the_unfused_form() {
    // An array passed to a function is by reference: the update lands in the
    // caller's array. The fused path wrote a *global* named after the parameter
    // instead, so `A` came back empty.
    let (code, stdout, stderr) = run_awkrs_stdin(
        "function f(arr) { arr[$1] += 1.5 } { f(A) } \
         END { for (k in A) print \"A[\" k \"]=\" A[k]; \
               for (k in arr) print \"LEAK arr[\" k \"]=\" arr[k] }",
        "x\nx\ny\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["A[x]=3", "A[y]=1.5"]);

    // A function's own local array (an extra parameter) must not survive the
    // call, and must not accumulate across calls.
    let (code, stdout, stderr) = run_awkrs_stdin(
        "function f(  loc) { loc[$1] += 1.5; return loc[$1] } { print f() } \
         END { for (k in loc) print \"LEAK loc[\" k \"]\" }",
        "x\nx\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(stdout, "1.5\n1.5\n");

    // `$0` as the subscript keys on the whole record. The fused path keyed
    // every record under `""`, collapsing the array to one element.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"{ a[$0] += 1.5 } END { for (k in a) print "[" k "]=" a[k] }"#,
        "p q\np q\nz\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["[p q]=3", "[z]=1.5"]);

    // The global case the fusion exists for is unchanged.
    let (code, stdout, stderr) = run_awkrs_stdin(
        "{ a[$1] += 1.5 } END { for (k in a) print k, a[k] }",
        "x\nx\ny\n",
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["x 3", "y 1.5"]);

    // Under `-M` the sum is MPFR, so the fused and unfused spellings have to
    // agree bit for bit. The fused path used to do the arithmetic in `f64`.
    // (`-M` has no mawk or one-true-awk counterpart; the reference here is
    // awkrs's own unfused path.)
    let input = "x\n".repeat(10);
    let (code, fused, stderr) = run_awkrs_stdin_args(
        ["-M", "-v", "PREC=200"],
        r#"{ a[$1] += 0.1 } END { for (k in a) printf "%.30f\n", a[k] }"#,
        &input,
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    let (code, unfused, stderr) = run_awkrs_stdin_args(
        ["-M", "-v", "PREC=200"],
        r#"{ a[$1] = a[$1] + 0.1 } END { for (k in a) printf "%.30f\n", a[k] }"#,
        &input,
    );
    assert_eq!(code, 0, "stderr {stderr:?}");
    assert_eq!(fused, unfused, "fused -M diverged from the unfused form");
}

/// C / POSIX precision on the integer conversions (`d i o u x X`): a minimum
/// digit count, with the `0` flag ignored while it is present.
///
/// Every expectation below was produced by running gawk 5.4.1, mawk 1.3.4 and
/// one-true-awk 20200816 on the same program under `LC_ALL=C`; all three agree
/// on every line.
#[test]
fn integer_conversions_honour_their_precision() {
    for (program, want) in [
        // The `0` flag is ignored once a precision is present; the precision
        // pads the digits and the field pads with spaces.
        (r#"BEGIN { printf "%08.5d|", 123 }"#, "   00123|"),
        (r#"BEGIN { printf "%08.2d|", 42 }"#, "      42|"),
        (r#"BEGIN { printf "%08.0d|", 0 }"#, "        |"),
        (r#"BEGIN { printf "%08.3d|", -42 }"#, "    -042|"),
        (r#"BEGIN { printf "%05.1d|", 256 }"#, "  256|"),
        (r#"BEGIN { printf "%08.5x|", 255 }"#, "   000ff|"),
        (r#"BEGIN { printf "%#08.5o|", 8 }"#, "   00010|"),
        (r#"BEGIN { printf "%+012.3X|", 3.14159 }"#, "         003|"),
        // …and still applies when there is no precision.
        (r#"BEGIN { printf "%08d|", 42 }"#, "00000042|"),
        // `o`, `u`, `x` and `X` honour the precision at all, which they did not.
        (r#"BEGIN { printf "%.5o|", 8 }"#, "00010|"),
        (r#"BEGIN { printf "%.5u|", 42 }"#, "00042|"),
        (r#"BEGIN { printf "%.5x|", 255 }"#, "000ff|"),
        (r#"BEGIN { printf "%.5X|", 255 }"#, "000FF|"),
        (r#"BEGIN { printf "%+0.10X|", 0 }"#, "0000000000|"),
        (r#"BEGIN { printf "%-8.5x|", 255 }"#, "000ff   |"),
        // The `#` prefix goes outside the precision padding, not inside it.
        (r#"BEGIN { printf "%#.5x|", 255 }"#, "0x000ff|"),
        (r#"BEGIN { printf "%#.5X|", 255 }"#, "0X000FF|"),
        (r#"BEGIN { printf "%#.5o|", 8 }"#, "00010|"),
        (r#"BEGIN { printf "%#.2o|", 8 }"#, "010|"),
        // `#` on `%o` forces a leading zero even where the precision emptied
        // the magnitude, which plain `%.0o` leaves empty.
        (r#"BEGIN { printf "%#.0o|", 0 }"#, "0|"),
        (r#"BEGIN { printf "%.0o|", 0 }"#, "|"),
        // `#` on `%x` still keys off the value, so a zero takes no prefix.
        (r#"BEGIN { printf "%#.0x|", 0 }"#, "|"),
        (r#"BEGIN { printf "%#.5x|", 0 }"#, "00000|"),
        // A precision never truncates: more digits than asked for all survive.
        (r#"BEGIN { printf "%.3d|", -42 }"#, "-042|"),
        (r#"BEGIN { printf "%.1d|", 12345 }"#, "12345|"),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(program, "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }
}

/// `%g` / `%G` round the way C does — from the exact binary value, halves to
/// even — not by scaling the value into an integer and calling `f64::round`.
///
/// The old spelling was wrong twice: `f64::round` rounds halves away from zero,
/// and the scaling multiply moved the value before the rounding could see it.
/// gawk 5.4.1, mawk 1.3.4 and one-true-awk 20200816 agree on every line.
#[test]
fn percent_g_rounds_like_c() {
    for (program, want) in [
        // Exact halves round to even. 1.5 and 3.5 already agreed (both rules
        // give the same answer); 2.5 and 4.5 are where they part.
        (r#"BEGIN { printf "%.1g|", 2.5 }"#, "2|"),
        (r#"BEGIN { printf "%.1g|", 4.5 }"#, "4|"),
        (r#"BEGIN { printf "%.1g|", 1.5 }"#, "2|"),
        (r#"BEGIN { printf "%.1g|", 3.5 }"#, "4|"),
        (r#"BEGIN { printf "%.1g|", -2.5 }"#, "-2|"),
        (r#"BEGIN { printf "%.1g|", 0.25 }"#, "0.2|"),
        (r#"BEGIN { printf "%.2g|", 1.25 }"#, "1.2|"),
        (r#"BEGIN { printf "%.2g|", 2.25 }"#, "2.2|"),
        (r#"BEGIN { printf "%.3g|", 1.125 }"#, "1.12|"),
        // 1.35 is not an exact half in binary — it is a shade above — so it
        // rounds up, and a ties-to-even rule applied to the decimal text would
        // get this one wrong in the other direction.
        (r#"BEGIN { printf "%.2g|", 1.35 }"#, "1.4|"),
        // 0.15 is a shade *below*, and the old scaling multiply rounded it up
        // to exactly 1.5 before the rounding ran.
        (r#"BEGIN { printf "%.1g|", 0.15 }"#, "0.1|"),
        (r#"BEGIN { printf "%-05.1G|", 2.5 }"#, "2    |"),
        (r#"BEGIN { printf "%+.1g|", 2.5 }"#, "+2|"),
        // Cases the rewrite must not disturb.
        (r#"BEGIN { printf "%.17g|", 0.1 }"#, "0.10000000000000001|"),
        (r#"BEGIN { printf "%g|", 0.0001 }"#, "0.0001|"),
        (r#"BEGIN { printf "%g|", 123456789 }"#, "1.23457e+08|"),
        (r#"BEGIN { printf "%.3g|", 999.9 }"#, "1e+03|"),
        (r#"BEGIN { printf "%.2g|", 0.000999 }"#, "0.001|"),
        (r#"BEGIN { printf "%#.5g|", 1.5 }"#, "1.5000|"),
        (r#"BEGIN { printf "%.1g|", 0.5 }"#, "0.5|"),
        (r#"BEGIN { printf "%.1g|", 9.5 }"#, "1e+01|"),
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(program, "");
        assert_eq!(code, 0, "{program}: stderr {stderr:?}");
        assert_eq!(stdout, want, "{program}");
    }
}
