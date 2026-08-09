//! Behavior pins for parity gaps found by `scripts/fuzz_parity.sh` against the
//! reference awks (gawk 5.4.1, one-true-awk 20200816, mawk 1.3.4).
//!
//! Each test encodes what the references actually do, not what an implementation
//! detail happens to produce. These run without any reference awk installed, so
//! they hold in a headless CI container; `scripts/fuzz_parity.sh` is what checks
//! the same behavior against the real binaries when they are available.

mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_bounded};
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
    let (code, out, err) = run_awkrs_stdin(
        r#"function f() { exit 5 } END { f(); print "never" }"#,
        "",
    );
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
        (r#"NR == 1 { FS = ":" } { print NR, NF, $1 }"#, "1 1 a:b\n2 2 c\n"),
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
        assert!(out.status.success(), "{flag}: {:?}", String::from_utf8_lossy(&out.stderr));
        seen.push(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    assert_eq!(seen[0], "1 1 6\n", "-s (no JIT)");
    assert_eq!(seen[1], seen[0], "-O (JIT) disagrees with -s");
}
