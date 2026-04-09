//! Extra end-to-end cases (operators, CLI combinations, field/record edges).

mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_args, run_awkrs_stdin_args_env};
use std::ffi::OsString;
use std::fs;
use std::process::Command;

#[test]
fn logical_and_short_circuit_skips_rhs_division_by_zero() {
    let (c, o, e) = run_awkrs_stdin("BEGIN { print (0 && 1/0) }", "");
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "0\n");
}

#[test]
fn logical_or_short_circuit_skips_rhs_division_by_zero() {
    let (c, o, e) = run_awkrs_stdin("BEGIN { print (1 || 1/0) }", "");
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "1\n");
}

#[test]
fn gsub_returns_substitution_count_on_record() {
    let (c, o, _) = run_awkrs_stdin(r#"{ print gsub("o", "x") }"#, "foo\n");
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn sub_returns_at_most_one_on_record() {
    let (c, o, _) = run_awkrs_stdin(r#"{ print sub("o", "x") }"#, "foo\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn length_empty_string_is_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print length("") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn numeric_lt_false_when_comparing_zero_to_negative() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print (0 < -1) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn substr_start_beyond_string_yields_empty() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print substr("hi", 99) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn index_miss_returns_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print index("abc", "z") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn compound_subtract_assign() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = 5; x -= 2; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn compound_mod_assign() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = 17; x %= 5; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn compound_add_assign_string_coerces() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = \"3\"; x += 2; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "5\n");
}

#[test]
fn print_nr_zero_records_still_prints_begin() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"start\" } { } END { print \"end\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "start\nend\n");
}

#[test]
fn empty_line_produces_empty_field_one() {
    let (c, o, _) = run_awkrs_stdin("{ print NF, length($0) }", "\n");
    assert_eq!(c, 0);
    assert_eq!(o, "0 0\n");
}

#[test]
fn multi_char_field_separator_flag() {
    let (c, o, _) = run_awkrs_stdin_args(["-F", "::"], "{ print $2 }", "a::b::c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn two_v_flags_combine() {
    let (c, o, _) = run_awkrs_stdin_args(["-v", "a=1", "-v", "b=2"], "BEGIN { print a + b }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn include_prepends_script() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let inc = dir.join(format!("awkrs_inc_{id}.awk"));
    fs::write(&inc, "function hi() { return \"hi\" }\n").expect("write include");
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args([
            "-i",
            inc.to_str().expect("utf8"),
            "-e",
            "BEGIN { print hi() }",
        ])
        .output()
        .expect("spawn");
    let _ = fs::remove_file(&inc);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
}

#[test]
fn for_loop_sum_indices() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { s=0; for (i=1;i<=5;i=i+1) s+=i; print s }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "15\n");
}

#[test]
fn jit_array_field_add_const_fused_opcode() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ a[$1] += 1 } END { print a[\"x\"] + 0 }",
        "x\nx\nx\n",
        [(OsString::from("AWKRS_JIT"), OsString::from("1"))],
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "3\n");
}

#[test]
fn awk_style_truthiness_zero_and_empty() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { print (0 ? 1 : 0), (\"\" ? 1 : 0), (\"0\" ? 1 : 0) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0 0 0\n");
}

#[test]
fn postincrement_style_via_assignment() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x=1; x=x+1; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn division_by_zero_yields_inf_or_nan_printable() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 1/0 }", "");
    assert_eq!(c, 0);
    assert!(o.contains("inf") || o == "nan\n" || o.contains("Inf"));
}

#[test]
fn relop_numeric_when_both_operands_are_numeric_strings() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"10\" < \"2\"), (10 < 2) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0 0\n");
}

#[test]
fn regexp_bracket_class() {
    let (c, o, _) = run_awkrs_stdin(r#"/[aeiou]+/ { print "vowels" }"#, "queue\n");
    assert_eq!(c, 0);
    assert_eq!(o, "vowels\n");
}

#[test]
fn field_reference_negative_uses_empty() {
    let (c, o, _) = run_awkrs_stdin("{ print $-1 }", "a b\n");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn printf_width_d() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { printf "%05d\n", 7 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "00007\n");
}

#[test]
fn begin_end_order_with_multiple_rules() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 1 } { } END { print 2 }", "x\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n2\n");
}

#[test]
fn slurp_file_multiline_sum() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_extra_sum_{id}.txt"));
    fs::write(&path, "10\n20\n30\n").expect("write");
    let (c, o, _) = run_awkrs_file("{ s += $1 } END { print s }", &path);
    let _ = fs::remove_file(&path);
    assert_eq!(c, 0);
    assert_eq!(o, "60\n");
}

#[test]
fn concat_numbers_coerces_to_string() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 1 2 3 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "123\n");
}

#[test]
fn equality_string_vs_number() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"00\" == 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn line_continuation_in_string_not_expected() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "ab" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "ab\n");
}

#[test]
fn subexpr_parentheses_precedence() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (2+3)*4 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "20\n");
}

#[test]
fn unary_plus_on_string() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print +\"42\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "42\n");
}

#[test]
fn array_delete_then_missing_is_empty() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { a[\"x\"]=1; delete a[\"x\"]; print a[\"x\"] }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn split_empty_fs_yields_char_fields() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { n = split(\"ab\", t, \"\"); print n, t[1], t[2] }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 a b\n");
}

#[test]
fn print_ofmt_default_for_float() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 1.5 }", "");
    assert_eq!(c, 0);
    assert!(o.starts_with("1.5") || o.starts_with("1.500"), "o={o:?}");
}

#[test]
fn multiple_print_args_empty_field() {
    let (c, o, _) = run_awkrs_stdin("{ print $1, $2 }", "onlyone\n");
    assert_eq!(c, 0);
    assert_eq!(o, "onlyone \n");
}

#[test]
fn beginfile_runs_before_records() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_bf_{id}.txt"));
    fs::write(&path, "z\n").expect("write");
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg("BEGINFILE { print \"bf\" } { print $1 } ENDFILE { print \"ef\" }")
        .arg(&path)
        .output()
        .expect("spawn");
    let _ = fs::remove_file(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bf\nz\nef\n");
}

#[test]
fn stderr_redirect_unknown_function() {
    let (c, _, e) = run_awkrs_stdin("{ zz() }", "a\n");
    assert_ne!(c, 0);
    assert!(!e.is_empty());
}

#[test]
fn or_operator_both_false() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (0 || 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn and_operator_both_true() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (1 && 1) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn xor_builtin_bitwise() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print xor(1,2) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn strtonum_hex_octal_decimal() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { print strtonum(\"0x1F\"), strtonum(\"017\"), strtonum(\"42\") }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "31 15 42\n");
}

#[test]
fn bitwise_shift_and_compl_zero() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { print and(3,1), or(2,1), lshift(1,4), rshift(16,2) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 3 16 4\n");
}

#[test]
fn asort_by_value_reindexes() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { a[\"x\"]=3; a[\"y\"]=1; n=asort(a); print n, a[\"1\"], a[\"2\"] }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 1 3\n");
}

#[test]
fn asorti_sorts_keys() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { a[\"b\"]=1; a[\"a\"]=2; asorti(a); print a[\"1\"], a[\"2\"] }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "a b\n");
}

#[test]
fn switch_case_first_match() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { switch (2) { case 1: print \"a\"; break; case 2: print \"b\"; break; default: print \"c\" } }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn switch_case_regex_label() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { x = \"foo\"; switch (x) { case /foo/: print \"ok\"; break } }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ok\n");
}

#[test]
fn switch_break_does_not_break_enclosing_while() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { i = 0; while (i < 1) { i++; switch (1) { case 1: break } print \"after\" } }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "after\n");
}

#[test]
fn duplicate_field_sum() {
    let (c, o, _) = run_awkrs_stdin("{ t += $1 } END { print t }", "1\n2\n3\n");
    assert_eq!(c, 0);
    assert_eq!(o, "6\n");
}

#[test]
fn gensub_not_builtin_errors() {
    let (c, _, _) = run_awkrs_stdin("BEGIN { gensub() }", "");
    assert_ne!(c, 0);
}

#[test]
fn record_separator_default_newline() {
    let (c, o, _) = run_awkrs_stdin("{ print $1 }", "p q\nr s\n");
    assert_eq!(c, 0);
    assert_eq!(o, "p\nr\n");
}

#[test]
fn assignment_expression_value() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (x = 5) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "5\n");
}

#[test]
fn empty_program_file_only_end() {
    let (c, o, _) = run_awkrs_stdin("END { print NR }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn compare_nan_never_equals_itself_awk_semantics() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { n = 0/0; print (n == n) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

// ── FILENAME, match() globals, sub/gsub target, system(), mawk -W ─────────

#[test]
fn filename_stdin_is_dash() {
    let (c, o, _) = run_awkrs_stdin("{ print FILENAME }", "a\n");
    assert_eq!(c, 0);
    assert_eq!(o, "-\n");
}

#[test]
fn filename_reflects_input_file_basename() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let base = format!("awkrs_filename_{id}.txt");
    let path = dir.join(&base);
    fs::write(&path, "x\n").expect("write temp");
    let (c, o, e) = run_awkrs_file("{ print FILENAME }", &path);
    let _ = fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={e:?}");
    let printed = o.trim_end();
    assert!(
        printed.ends_with(&base),
        "expected path ending with {base:?}, got {printed:?}"
    );
}

#[test]
fn match_builtin_sets_rstart_rlength() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { match("foo123bar", "[0-9]+"); print RSTART, RLENGTH }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "4 3\n");
}

#[test]
fn match_no_match_sets_rstart_zero_rlength_negative() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { match("abc", "z"); print RSTART, RLENGTH }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0 -1\n");
}

#[test]
fn system_reports_nonzero_exit_status() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print system("exit 4") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "4");
}

#[test]
fn sub_third_arg_replaces_in_scalar_variable() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x = "hello"; sub("l", "L", x); print x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "heLlo\n");
}

#[test]
fn gsub_third_arg_replaces_all_in_scalar_variable() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x = "ll"; gsub("l", "L", x); print x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "LL\n");
}

#[test]
fn mawk_w_version_matches_dash_capital_v() {
    let w = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .args(["-W", "version"])
        .output()
        .expect("spawn awkrs -W version");
    assert!(w.status.success(), "stderr={}", String::from_utf8_lossy(&w.stderr));
    let dash_v = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("-V")
        .output()
        .expect("spawn awkrs -V");
    assert_eq!(w.stdout, dash_v.stdout);
}

// ── typeof(), regexp alternation, empty OFS ─────────────────────────────────

#[test]
fn typeof_builtin_classifies_numbers_strings_and_uninitialized() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print typeof(1), typeof("x"), typeof(x); a[1]=2; print typeof(a[1]) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "number string uninitialized\nnumber\n");
}

#[test]
fn regexp_pattern_alternation_matches_either_branch() {
    let (c, o, _) = run_awkrs_stdin(r#"/a|b/ { print $0 }"#, "xx\nb\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn empty_ofs_joins_print_args_without_separator() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { OFS = ""; print 1, 2, 3 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "123\n");
}

// ── close(), ENDFILE, parse errors, IEEE math edges ─────────────────────────

#[test]
fn close_one_way_pipe_returns_zero() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print "q" | "cat"; print close("cat") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "q\n0\n");
}

#[test]
fn endfile_runs_once_per_input_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f1 = dir.join(format!("awkrs_endfile_a_{id}.txt"));
    let f2 = dir.join(format!("awkrs_endfile_b_{id}.txt"));
    fs::write(&f1, "x\n").expect("temp");
    fs::write(&f2, "y\n").expect("temp");
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg(r#"ENDFILE { print "E" }"#)
        .arg(&f1)
        .arg(&f2)
        .output()
        .expect("spawn awkrs two files");
    let _ = fs::remove_file(&f1);
    let _ = fs::remove_file(&f2);
    assert_eq!(out.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "E\nE\n");
}

#[test]
fn unclosed_brace_program_exits_nonzero_with_parse_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("{")
        .output()
        .expect("spawn awkrs invalid program");
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("parse error"),
        "expected parse error on stderr, got: {stderr:?}"
    );
}

#[test]
fn log_zero_is_negative_infinity() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print log(0) }", "");
    assert_eq!(c, 0);
    let t = o.trim();
    assert!(
        t.eq_ignore_ascii_case("-inf"),
        "expected -inf, got {o:?}"
    );
}

#[test]
fn sqrt_negative_one_is_nan() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sqrt(-1) }", "");
    assert_eq!(c, 0);
    let t = o.trim();
    assert!(
        t.eq_ignore_ascii_case("nan"),
        "expected NaN, got {o:?}"
    );
}

// ── JIT integration tests (AWKRS_JIT=1) ──────────────────────────────────

fn jit_env() -> [(OsString, OsString); 1] {
    [(OsString::from("AWKRS_JIT"), OsString::from("1"))]
}

#[test]
fn jit_print_field_stdout() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ print $1 }",
        "hello\nworld\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "hello\nworld\n");
}

#[test]
fn jit_print_two_fields() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ print $1, $2 }",
        "a b\nc d\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "a b\nc d\n");
}

#[test]
fn jit_bare_print() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ print }",
        "line1\nline2\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "line1\nline2\n");
}

#[test]
fn jit_match_regexp_pattern() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "/yes/ { print $0 }",
        "no\nyes\nmaybe\nyes please\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "yes\nyes please\n");
}

#[test]
fn jit_next_skips_rule() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "$1 == \"skip\" { next } { print $0 }",
        "keep\nskip\nalso keep\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "keep\nalso keep\n");
}

#[test]
fn jit_exit_default() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ print; exit }",
        "first\nsecond\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "first\n");
}

#[test]
fn jit_exit_with_code() {
    let (c, _o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "BEGIN { exit 42 }",
        "",
        jit_env(),
    );
    assert_eq!(c, 42, "stderr: {e}");
}

#[test]
fn jit_array_count_pattern() {
    // Classic: count[$1]++ then print
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ a[$1] += 1 } END { print a[\"x\"] + 0, a[\"y\"] + 0 }",
        "x\ny\nx\nx\ny\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "3 2\n");
}

#[test]
fn jit_split_explicit_fs() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "BEGIN { n = split(\"a,b\", arr, \",\"); print n, arr[1], arr[2] }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "2 a b\n");
}

#[test]
fn jit_split_uses_fs_variable() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "BEGIN { FS = \",\"; n = split(\"a,b\", arr); print n, arr[1], arr[2] }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "2 a b\n");
}

#[test]
fn jit_patsplit_fpat() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "BEGIN { FPAT=\"[^,]+\"; n = patsplit(\"a,b\", arr); print n, arr[1], arr[2] }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "2 a b\n");
}

#[test]
fn jit_match_builtin_rstart() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { print match("foo123bar", "[0-9]+"), RSTART, RLENGTH }"#,
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "4 4 3");
}

#[test]
fn jit_sum_fields_loop() {
    // for (i=1; i<=NF; i++) sum += $i
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ s=0; for(i=1;i<=NF;i++) s+=$i; print s }",
        "1 2 3\n10 20\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "6\n30\n");
}

#[test]
fn jit_mixed_string_slot_preinc() {
    // Mixed chunk: string literal + `++` on slot must coerce "5" → 5 → 6 (not raw f64 add on NaN bits).
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "BEGIN { s=\"5\"; s++; print s }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "6");
}

#[test]
fn jit_mixed_field_assign_concat() {
    // `$2 = $1 "x"` must store a real string via MIXED_SET_FIELD (not Value::Num bits as text).
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ $2 = $1 \"x\"; print $2 }",
        "a\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "ax");
}

#[test]
fn jit_multidim_array_subscript() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "BEGIN { a[1,2]=42; print a[1,2] }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "42");
}

#[test]
fn jit_typeof_expressions_and_slot() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { x=1; print typeof(3), typeof("x"), typeof(x) }"#,
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "number string number");
}

#[test]
fn jit_typeof_field_unassigned() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"{ print typeof($2) }"#,
        "a\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "uninitialized");
}

#[test]
fn jit_whitelisted_builtin_sqrt() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { print sqrt(9) }"#,
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "3");
}

#[test]
fn jit_sprintf_builtin() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { print sprintf("%d", 42) }"#,
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "42");
}

#[test]
fn jit_printf_statement_stdout() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { printf "%d\n", 42 }"#,
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "42");
}

#[test]
fn jit_return_from_function() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "function double(x) { return x * 2 } BEGIN { print double(21) }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "42\n");
}

#[test]
fn jit_for_in_count_keys() {
    // Uses fused ArrayFieldAddConst for `a[$1] += 1` (correct string-key path),
    // then ForIn to count keys.
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ a[$1] += 1 } END { n=0; for (k in a) n++; print n }",
        "x\ny\nz\nx\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "3\n");
}

#[test]
fn jit_for_in_sum_values() {
    // Uses fused ArrayFieldAddConst for array population,
    // then ForIn + fused ArrayFieldAddConst to sum values.
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ a[$1] += $2 } END { s=0; for (k in a) s += a[k]; print s }",
        "x 10\ny 20\nz 30\n",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "60\n");
}

#[test]
fn jit_asort_returns_count() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "END { a[1]=30; a[2]=10; a[3]=20; print asort(a) }",
        "",
        jit_env(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "3\n");
}
