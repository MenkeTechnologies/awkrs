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
fn posix_exponentiation_caret_star_star_right_assoc() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print 2^3, 2**3, 2^3^2, -2^2 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "8 8 512 -4\n");
}

#[test]
fn getline_expr_compares_return_value() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { if ((getline x) > 0) print x }"#, "hello\n");
    assert_eq!(c, 0);
    assert_eq!(o, "hello\n");
}

#[test]
fn pipe_getline_reads_from_sh_c_pipeline() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { "echo hi" | getline x; print x }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "hi\n");
}

#[test]
fn gawk_regexp_constant_typeof_and_match() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { r = @/[0-9]+/; print typeof(r), ("a1b" ~ r) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "regexp 1\n");
}

#[test]
fn printf_group_flag_inserts_separators() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { printf "%'d\n", 1234567 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1,234,567\n");
}

#[test]
fn substr_start_beyond_string_yields_empty() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print substr("hi", 99) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn substr_start_zero_clamped_like_gawk() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { print substr("hello", 0, 3) }"#, "");
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "hel\n");
}

#[test]
fn substr_large_negative_start_clamped_like_gawk() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { print substr("hello", -10, 3) }"#, "");
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "hel\n");
}

#[test]
fn print_whole_array_scalar_is_fatal() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { a[1] = 1; print a }"#, "");
    assert_ne!(c, 0, "expected nonzero exit, out={o:?} stderr={e:?}");
    assert!(
        e.contains("attempt to use an array in a scalar context"),
        "stderr={e:?}"
    );
}

#[test]
fn print_concat_array_scalar_is_fatal() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { a[1] = 1; print "x=" a "." }"#, "");
    assert_ne!(c, 0, "expected nonzero exit, out={o:?} stderr={e:?}");
    assert!(
        e.contains("attempt to use an array in a scalar context"),
        "stderr={e:?}"
    );
}

#[test]
fn record_pattern_regex_may_start_compound_expression() {
    let (c, o, _) = run_awkrs_stdin(r#"/foo/ && NR > 1 { print $0 }"#, "foo\nfoo\nbar\n");
    assert_eq!(c, 0);
    assert_eq!(o, "foo\n");
}

#[test]
fn negative_field_access_is_fatal_like_gawk() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { print $(-1) }"#, "");
    assert_ne!(c, 0, "out={o:?}");
    assert!(
        e.contains("attempt to access field number -1"),
        "stderr={e:?}"
    );
}

#[test]
fn nf_negative_assignment_is_fatal_like_gawk() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { NF = -1 }"#, "");
    assert_ne!(c, 0, "out={o:?}");
    assert!(e.contains("NF set to negative value"), "stderr={e:?}");
}

#[test]
fn whole_array_in_addition_is_fatal() {
    let (c, _, e) = run_awkrs_stdin(r#"BEGIN { a[1]=1; print a + 1 }"#, "");
    assert_ne!(c, 0);
    assert!(
        e.contains("attempt to use an array in a scalar context"),
        "stderr={e:?}"
    );
}

#[test]
fn whole_array_in_equality_compare_is_fatal() {
    let (c, _, e) = run_awkrs_stdin(r#"BEGIN { a[1]=1; print (a == 0) }"#, "");
    assert_ne!(c, 0);
    assert!(
        e.contains("attempt to use an array in a scalar context"),
        "stderr={e:?}"
    );
}

#[test]
fn whole_array_in_if_condition_is_fatal() {
    let (c, _, e) = run_awkrs_stdin(r#"BEGIN { a[1]=1; if (a) print 1 }"#, "");
    assert_ne!(c, 0);
    assert!(
        e.contains("attempt to use an array in a scalar context"),
        "stderr={e:?}"
    );
}

#[test]
fn division_by_zero_is_fatal_like_gawk() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { print 1/0 }"#, "");
    assert_ne!(c, 0, "stderr={e:?}");
    assert!(
        e.contains("division by zero attempted"),
        "stderr={e:?} stdout={o:?}"
    );
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
        std::iter::empty::<(OsString, OsString)>(),
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
    // POSIX/gawk: number 0 → falsy; empty string → falsy; string literal "0"
    // → TRUTHY (non-empty string literals are truthy regardless of numeric
    // value).
    assert_eq!(o, "0 0 1\n");
}

#[test]
fn postincrement_style_via_assignment() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x=1; x=x+1; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn relop_numeric_when_both_operands_are_numeric_strings() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"10\" < \"2\"), (10 < 2) }", "");
    assert_eq!(c, 0);
    // String constants compare lexicographically; numeric constants compare as numbers (matches gawk).
    assert_eq!(o, "1 0\n");
}

#[test]
fn regexp_bracket_class() {
    let (c, o, _) = run_awkrs_stdin(r#"/[aeiou]+/ { print "vowels" }"#, "queue\n");
    assert_eq!(c, 0);
    assert_eq!(o, "vowels\n");
}

#[test]
fn field_reference_negative_is_fatal_like_gawk() {
    let (c, o, e) = run_awkrs_stdin("{ print $-1 }", "a b\n");
    assert_ne!(c, 0, "out={o:?} stderr={e:?}");
    assert!(
        e.contains("attempt to access field number -1"),
        "stderr={e:?}"
    );
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
    // Literal "00" is not a numeric string for == against a number; falls through to string cmp vs "0" (gawk: 0).
    assert_eq!(o, "0\n");
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
    let (c, o, _) = run_awkrs_stdin("BEGIN { n = sqrt(-1); print (n == n) }", "");
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
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { match("abc", "z"); print RSTART, RLENGTH }"#, "");
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
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { x = "hello"; sub("l", "L", x); print x }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "heLlo\n");
}

#[test]
fn gsub_third_arg_replaces_all_in_scalar_variable() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { x = "ll"; gsub("l", "L", x); print x }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "LL\n");
}

#[test]
fn mawk_w_version_matches_dash_capital_v() {
    let w = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .args(["-W", "version"])
        .output()
        .expect("spawn awkrs -W version");
    assert!(
        w.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&w.stderr)
    );
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
    assert_eq!(o, "number string untyped\nnumber\n");
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
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "q" | "cat"; print close("cat") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "q\n0\n");
}

#[test]
fn getline_from_missing_file_returns_minus_one_and_sets_errno() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_getline_enoent_{id}.txt"));
    assert!(
        !path.exists(),
        "precondition: temp path must not exist: {}",
        path.display()
    );
    let prog = format!(
        r#"BEGIN {{ print (getline x < "{}"), (length(ERRNO) > 0) }}"#,
        path.display()
    );
    let (c, o, _) = run_awkrs_stdin(&prog, "");
    assert_eq!(c, 0);
    assert_eq!(o, "-1 1\n");
}

#[test]
fn close_returns_zero_after_getline_from_named_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_close_after_getline_{id}.txt"));
    fs::write(&path, "row\n").expect("temp");
    let prog = format!(
        r#"BEGIN {{ print (getline x < "{}"), x; print close("{}") }}"#,
        path.display(),
        path.display()
    );
    let (c, o, _) = run_awkrs_stdin(&prog, "");
    let _ = fs::remove_file(&path);
    assert_eq!(c, 0);
    assert_eq!(o, "1 row\n0\n");
}

#[test]
fn gensub_ampersand_substitutes_entire_match() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print gensub(/a+/, "[&]", "g", "xaay") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "x[aa]y\n");
}

#[test]
fn split_accepts_slash_regex_field_separator() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("a1b2c", t, /[0-9]/)
            print n
            for (i = 1; i <= n; i++) print t[i]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3\na\nb\nc\n");
}

#[test]
fn split_reinitializes_destination_array_removing_old_keys() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            a[9] = 1
            n = split("x:y", a, ":")
            print n, a[1], a[2], (9 in a)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 x y 0\n");
}

#[test]
fn for_c_style_infinite_loop_breaks_once() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { for (;;) { break } print "done" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "done\n");
}

#[test]
fn sprintf_percent_s_respects_minimum_width() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { printf ">%5s<\n", "ab" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, ">   ab<\n");
}

#[test]
fn int_of_missing_field_is_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"{ print int($2) }"#, "a\n");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn getline_from_dev_null_returns_zero_immediately() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print (getline x < "/dev/null") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn system_empty_command_reports_success() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print system("") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "0");
}

#[test]
fn multichar_ofs_used_between_print_arguments() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { OFS = "::"; print 1, 2 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1::2\n");
}

#[test]
fn assigning_dollar_one_to_empty_string_keeps_nf() {
    let (c, o, _) = run_awkrs_stdin(r#"{ $1 = ""; print NF, $2 }"#, "a b\n");
    assert_eq!(c, 0);
    assert_eq!(o, "2 b\n");
}

#[test]
fn zero_raised_to_zero_is_one() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print 0^0, 1^0 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 1\n");
}

#[test]
fn chained_assignment_sets_all_lvalues() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { x = y = 3; print x, y }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "3 3\n");
}

#[test]
fn string_concatenation_with_empty_middle_term() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "a" "" "b" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "ab\n");
}

#[test]
fn print_numeric_respects_ofmt() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { OFMT = "%.2f"; print 1.234 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1.23\n");
}

#[test]
fn string_concatenation_uses_convfmt_for_number_coercion() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { CONVFMT = "%.1f"; x = 1.23; print "" x }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1.2\n");
}

#[test]
fn gensub_numeric_how_replaces_single_occurrence() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print gensub(/a/, "A", 1, "aba") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "Aba\n");
}

#[test]
fn regexp_match_operator_accepts_pattern_in_variable() {
    let (c, o, _) = run_awkrs_stdin(r#"{ pat = "[0-9]+"; print ($0 ~ pat) }"#, "abc\n42\n");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n1\n");
}

#[test]
fn modulo_with_negative_dividend_matches_awk_semantics() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print (-7 % 3), (7 % -3) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "-1 1\n");
}

#[test]
fn multichar_ors_between_print_records() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { ORS = "|\n" } { print $1 }"#, "a\nb\n");
    assert_eq!(c, 0);
    assert_eq!(o, "a|\nb|\n");
}

#[test]
fn postfix_increment_uninitialized_scalar_starts_at_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print x++ }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn gensub_numeric_zero_treated_as_one_like_gawk() {
    // gawk emits "third argument `0' treated as 1" and replaces only the first match.
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print gensub(/a/, "A", 0, "aba") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "Aba\n");
}

#[test]
fn intdiv0_truncates_toward_zero_and_zero_divisor_yields_zero() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print intdiv0(7, 3), intdiv0(-7, 3), intdiv0(7, 0) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 -2 0\n");
}

#[test]
fn decrement_nf_drops_trailing_field_and_rebuilds_dollar_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"{ print NF; NF--; print NF, $0 }"#, "a b c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n2 a b\n");
}

#[test]
fn length_of_uninitialized_scalar_is_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print length(s) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn logical_and_empty_string_is_falsy() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print ("" && 1), ("x" && 1) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0 1\n");
}

#[test]
fn compl_bitwise_not_of_one() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print compl(1) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "-2\n");
}

#[test]
fn gsub_with_empty_replacement_removes_matches_in_scalar() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { s = "aba"; n = gsub(/a/, "", s); print n, s }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 b\n");
}

#[test]
fn for_in_delete_removes_all_array_elements() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            a[1] = 1
            a[2] = 2
            for (i in a) delete a[i]
            print length(a)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn isarray_distinguishes_arrays_from_scalars() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { a[1] = 1; print isarray(a), isarray(x) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 0\n");
}

#[test]
fn sub_returns_zero_when_pattern_does_not_match_third_arg() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print sub(/z/, "Z", "abc") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn strftime_third_argument_selects_utc() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print strftime("%H", 0, 1) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "00\n");
}

#[test]
fn print_empty_string_still_writes_output_record_separator() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn argv_zero_names_the_awk_binary() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print (index(ARGV[0], "awkrs") > 0) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn bitwise_xor_same_value_and_or_and_with_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print xor(5, 5), or(0, 0), and(3, 0) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0 0 0\n");
}

#[test]
fn lshift_and_rshift_by_zero_are_identity() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print lshift(1, 0), rshift(8, 0) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 8\n");
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
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    // gawk prints "-inf" for `print log(0)`.
    let (c, o, _) = run_awkrs_stdin("BEGIN { print log(0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "-inf");
}

#[test]
fn sqrt_negative_one_prints_gawk_style_plus_nan() {
    // gawk: `print sqrt(-1)` → "+nan" (sign-tagged, lowercase).
    let (c, o, e) = run_awkrs_stdin("BEGIN { print sqrt(-1) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "+nan", "expected +nan, got {o:?}");
    assert!(
        e.contains("sqrt: received negative argument"),
        "gawk warns on stderr even without LINT; stderr={e:?}"
    );
}

#[test]
fn log_negative_one_warns_and_prints_plus_nan() {
    let (c, o, e) = run_awkrs_stdin("BEGIN { print log(-1) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "+nan", "stdout={o:?}");
    assert!(
        e.contains("log: received negative argument"),
        "stderr={e:?}"
    );
}

#[test]
fn printf_percent_s_on_nan_matches_print_spelling() {
    // Both `print x` and `printf "%s", x` should produce "+nan" so the two display paths
    // agree (gawk parity).
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x=sqrt(-1); printf "%s|%s\n", x, x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "+nan|+nan\n");
}

#[test]
fn printf_percent_s_on_infinity_matches_print_spelling() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x=exp(800); printf "%s\n", x; print x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "+inf\n+inf\n");
}

// ── JIT integration tests ───────────────────────────────────────────────

#[test]
fn jit_print_field_stdout() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ print $1 }",
        "hello\nworld\n",
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "4 4 3");
}

#[test]
fn jit_print_redirect_overwrite() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_jit_redir_{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_string_lossy().replace('\\', "/");
    let prog = format!(r#"BEGIN {{ print "hello" > "{p}" }}"#);
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        &prog,
        "",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert!(o.is_empty());
    let contents = std::fs::read_to_string(&path).expect("read redirected output");
    assert_eq!(contents, "hello\n");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn jit_sum_fields_loop() {
    // for (i=1; i<=NF; i++) sum += $i
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ s=0; for(i=1;i<=NF;i++) s+=$i; print s }",
        "1 2 3\n10 20\n",
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "number string number");
}

#[test]
fn jit_typeof_field_beyond_nf_is_unassigned() {
    // gawk parity: out-of-range fields report "unassigned" (gawk's vocabulary
    // for fields that exist by reference but have no value). Older awkrs
    // reported "untyped".
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"{ print typeof($2) }"#,
        "a\n",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o.trim(), "unassigned");
}

#[test]
fn jit_whitelisted_builtin_sqrt() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { print sqrt(9) }"#,
        "",
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "42\n");
}

#[test]
fn jit_gsub_on_record() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"{ print gsub("o", "x") }"#,
        "foo\n",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "2\n");
}

#[test]
fn jit_sub_on_record() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"{ print sub("o", "x") }"#,
        "foo\n",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "1\n");
}

#[test]
fn jit_nested_user_function_calls() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "function g() { return 1 } function f() { return g() + 1 } BEGIN { print f() }",
        "",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "2\n");
}

#[test]
fn jit_gsub_third_arg_scalar() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        r#"BEGIN { s = "foo"; print gsub("o", "x", s), s }"#,
        "",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "2 fxx\n");
}

#[test]
fn jit_for_in_count_keys() {
    // Uses fused ArrayFieldAddConst for `a[$1] += 1` (correct string-key path),
    // then ForIn to count keys.
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "{ a[$1] += 1 } END { n=0; for (k in a) n++; print n }",
        "x\ny\nz\nx\n",
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
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
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "3\n");
}

#[test]
fn jit_getline_primary_reads_next_line() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "NR==1 { getline; print $0 }",
        "a\nb\n",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "b\n");
}

#[test]
fn jit_getline_into_var() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        "NR==1 { getline x; print x }",
        "a\nb\n",
        std::iter::empty::<(OsString, OsString)>(),
    );
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "b\n");
}

#[test]
fn jit_getline_from_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_jit_getline_{}.txt", std::process::id()));
    std::fs::write(&path, "fromfile\n").unwrap();
    let path_str = path.to_string_lossy();
    let prog = format!(
        "BEGIN {{ getline x < \"{}\"; print x }}",
        path_str.replace('\\', "\\\\")
    );
    let (c, o, e) = run_awkrs_stdin_args_env(
        std::iter::empty::<&str>(),
        &prog,
        "",
        std::iter::empty::<(OsString, OsString)>(),
    );
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr: {e}");
    assert_eq!(o, "fromfile\n");
}

#[test]
fn gawk_style_paren_list_in_and_literals_printf_substr_index() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN {
  a[1, 2] = 1
  print ((1,2) in a), 0x10, 010, 01238
  printf "%e\n", 1234.5
  printf "[%c]\n", "Z"
  print substr("hi", -1, 5)
  print index("hello", "")
}"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(
        o,
        "1 16 8 1238\n\
1.234500e+03\n\
[Z]\n\
hi\n\
1\n"
    );
}

#[test]
fn print_paren_list_emits_multiple_fields() {
    let (c, o, e) = run_awkrs_stdin(r#"BEGIN { OFS=","; print (1, 2) }"#, "");
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "1,2\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Regression suite for bugs found in the v0.4.3 audit:
//   * `gsub(//, …)` zero-width matches
//   * `split(s, a, fs, seps)` populating the seps array
//   * Streaming multi-char and regex `RS` returning only one record from stdin
//   * `printf "%g"/"%f"/"%e"/"%a"` formatting of non-finite values
//   * `print` / `printf "%s"` of NaN / ±inf using gawk's "+nan" / "+inf" spelling
//   * `gensub(re, repl, 0, …)` and negative `how` matching gawk's "treat as 1"
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn gsub_empty_pattern_inserts_replacement_between_every_character() {
    // Before the fix the literal-pattern fast path treated "" as "no match"
    // and returned 0. gawk treats // as a zero-width match at every position.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { s="abc"; n=gsub(//, "-", s); print n, s }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "4 -a-b-c-\n");
}

#[test]
fn gsub_empty_pattern_on_empty_target_matches_once() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { s=""; n=gsub(//, "-", s); print n, "[" s "]" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [-]\n");
}

#[test]
fn sub_empty_pattern_inserts_at_start() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { s="abc"; n=sub(//, "-", s); print n, s }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 -abc\n");
}

#[test]
fn split_four_arg_populates_seps_for_regex_fs() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("a1b22c333d", a, /[0-9]+/, seps);
            printf "n=%d\n", n;
            for (i = 1; i <= n; i++) printf "a[%d]=%s\n", i, a[i];
            for (i = 1; i < n; i++) printf "seps[%d]=%s\n", i, seps[i];
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(
        o,
        "n=4\na[1]=a\na[2]=b\na[3]=c\na[4]=d\nseps[1]=1\nseps[2]=22\nseps[3]=333\n"
    );
}

#[test]
fn split_four_arg_populates_seps_for_single_char_fs() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("x-y-z", a, "-", seps);
            for (i = 1; i < n; i++) printf "[%s]", seps[i];
            print ""
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[-][-]\n");
}

#[test]
fn split_four_arg_captures_whitespace_run_for_default_fs() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("a   b\tc", a, " ", seps);
            for (i = 1; i < n; i++) printf "<%s>", seps[i];
            print ""
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "<   ><\t>\n");
}

#[test]
fn split_four_arg_empty_seps_array_for_empty_fs() {
    // `""` FS: each char becomes a field, separators between them are empty strings.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("xy", a, "", seps);
            print n, "[" seps[1] "]"
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 []\n");
}

#[test]
fn stdin_multi_char_literal_rs_reads_every_record() {
    // Regression: streaming `RS` with multi-byte literal was reading only the first record
    // because chunk reads discarded leftover bytes.
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { RS=\"\\n---\\n\" } { print NR, \"[\" $0 \"]\" }",
        "a\n---\nb\n---\nc",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [a]\n2 [b]\n3 [c]\n");
}

#[test]
fn stdin_regex_rs_reads_every_record() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { RS="[|,]" } { print NR, "[" $0 "]" }"#,
        "a|b,c|d",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [a]\n2 [b]\n3 [c]\n4 [d]\n");
}

#[test]
fn stdin_paragraph_mode_strips_trailing_newlines_from_record() {
    // gawk paragraph mode (RS=="") records do NOT include trailing newlines.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { RS="" } { print NR, "[" $0 "]" }"#,
        "para1\nline2\n\npara2\nline2\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [para1\nline2]\n2 [para2\nline2]\n");
}

#[test]
fn stdin_single_char_rs_strips_separator_from_record() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { RS=":" } { print NR, "[" $0 "]" }"#,
        "a:b:c",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [a]\n2 [b]\n3 [c]\n");
}

#[test]
fn printf_percent_g_on_infinity_emits_signed_inf() {
    // Before the fix this raised "sprintf: non-finite value for %g".
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x=exp(800); printf "%g %G\n", x, x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "+inf +INF\n");
}

#[test]
fn printf_percent_f_on_negative_infinity_emits_minus_inf() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x=-exp(800); printf "%f %F\n", x, x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "-inf -INF\n");
}

#[test]
fn printf_percent_e_padding_on_infinity_uses_spaces_not_zeros() {
    // POSIX: zero-padding does not apply to non-finite values — gawk also outputs
    // "[      +inf]" rather than "[0000000+inf]" when given `%010e`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { x=exp(800); printf "[%010e]\n", x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[      +inf]\n");
}

#[test]
fn printf_percent_g_precision_one_uses_one_significant_digit() {
    // Regression: awkrs's `%.1g` used to keep an extra fractional digit ("1.2e+02"),
    // ignoring the C99 "significant digits" semantics. gawk and the POSIX/ISO C
    // contract emit "1e+02" for precision 1.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%.0g|%.1g|%.2g|%.3g\n", 123.456, 123.456, 123.456, 123.456 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1e+02|1e+02|1.2e+02|123\n");
}

#[test]
fn printf_percent_a_on_nan_uses_signed_nan() {
    // sqrt(-1) yields a positive NaN; `%a` prints "+nan" (gawk parity).
    let (c, o, _e) = run_awkrs_stdin(
        r#"BEGIN { x=sqrt(-1); printf "%a\n", x }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "+nan\n");
}

#[test]
fn gensub_numeric_negative_treated_as_one() {
    // Matches gawk's "third argument `-1' treated as 1" behavior.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print gensub(/a/, "X", -3, "aaaa") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "Xaaa\n");
}

#[test]
fn gensub_numeric_zero_treated_as_one() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print gensub(/a/, "X", 0, "aaaa") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "Xaaa\n");
}

#[test]
fn rs_change_takes_effect_for_subsequent_records() {
    // Switching RS mid-stream: first record uses default newline, then RS becomes ":".
    let (c, o, _) = run_awkrs_stdin(
        r#"NR==1 { RS=":" } { printf "[%d:%s]\n", NR, $0 }"#,
        "first\nrest1:rest2:rest3",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[1:first]\n[2:rest1]\n[3:rest2]\n[4:rest3]\n");
}

#[test]
fn pipe_getline_advances_through_subprocess_output() {
    // Regression: `cmd | getline x` used to respawn `cmd` on every call, so the
    // expression returned the same first line forever and `while ((cmd|getline x)>0)`
    // looped indefinitely. Now the subprocess runs once per pipe key.
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN {
            cmd = "echo one; echo two; echo three"
            while ((cmd | getline line) > 0) print "got:", line
            close(cmd)
            print "done"
        }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "got: one\ngot: two\ngot: three\ndone\n");
}

#[test]
fn pipe_getline_explicit_calls_advance_line_by_line() {
    // Each `(cmd | getline x)` returns the *next* line; the fourth call hits EOF and
    // returns 0 (leaving `x` unchanged at the last successfully read value).
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            cmd = "echo a; echo b; echo c"
            r1 = (cmd | getline x); print r1, x
            r2 = (cmd | getline x); print r2, x
            r3 = (cmd | getline x); print r3, x
            r4 = (cmd | getline x); print r4, x
            close(cmd)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 a\n1 b\n1 c\n0 c\n");
}

#[test]
fn close_on_never_opened_path_returns_minus_one() {
    // gawk parity: `close("name")` returns -1 when nothing is open under that name.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print close("/tmp/awkrs_never_opened_xyz_path_abc123") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "-1\n");
}

#[test]
fn close_returns_zero_on_first_close_then_minus_one_on_second() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            print "a" > "/tmp/awkrs_close_twice_test.txt"
            print close("/tmp/awkrs_close_twice_test.txt")
            print close("/tmp/awkrs_close_twice_test.txt")
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0\n-1\n");
    // Cleanup
    let _ = std::fs::remove_file("/tmp/awkrs_close_twice_test.txt");
}

#[test]
fn system_flushes_stdout_before_invoking_subprocess() {
    // Regression: `system()` used to run *before* the buffered `print "before"` was
    // flushed, producing the wrong interleaving "middle\nbefore\nafter".
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print "before"; system("echo middle"); print "after" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "before\nmiddle\nafter\n");
}

#[test]
fn system_flushes_pending_redirect_output_before_subprocess() {
    // The same buffering hazard applies to `print >` files — make sure the bytes
    // are on disk before the subprocess reads them back.
    let path = "/tmp/awkrs_sys_flush_test.txt";
    let _ = std::fs::remove_file(path);
    let prog = format!(
        r#"BEGIN {{ print "hi" > "{path}"; system("cat {path}"); }}"#,
        path = path,
    );
    let (c, o, _) = run_awkrs_stdin(&prog, "");
    assert_eq!(c, 0);
    assert_eq!(o, "hi\n");
    let _ = std::fs::remove_file(path);
}

#[test]
fn close_after_pipe_getline_reaps_subprocess() {
    // After `close(cmd)`, the pipe reader/child are gone; another pipe getline with
    // the same key respawns cleanly and starts from the top.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            cmd = "echo line1; echo line2"
            (cmd | getline x); print "first:", x
            close(cmd)
            (cmd | getline y); print "after-reopen:", y
            close(cmd)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "first: line1\nafter-reopen: line1\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Second round of regressions: FIELDWIDTHS, IGNORECASE single-char FS,
// unknown printf conversion specifiers, regex `.` matching newline.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn fieldwidths_clamps_last_field_to_specified_width() {
    // Regression: the last FIELDWIDTHS entry used to auto-extend to the end of
    // the record (taking 3 bytes for "H99" with width 2), instead of clamping
    // to the declared width and leaving the trailing byte unused.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { FIELDWIDTHS="3 4 2" } { print NF, "[" $1 "]", "[" $2 "]", "[" $3 "]" }"#,
        "abcDEFGH99\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3 [abc] [DEFG] [H9]\n");
}

#[test]
fn fieldwidths_supports_skip_colon_width_syntax() {
    // gawk syntax: `M:N` means skip M bytes, then take N bytes for the field.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { FIELDWIDTHS="3 2:2 3" } { print "[" $1 "]", "[" $2 "]", "[" $3 "]" }"#,
        "abcXXdefghi\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[abc] [de] [fgh]\n");
}

#[test]
fn fieldwidths_supports_star_token_for_remaining() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { FIELDWIDTHS="3 2:2 *" } { print "[" $1 "]", "[" $2 "]", "[" $3 "]" }"#,
        "abcXXdefghi\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[abc] [de] [fghi]\n");
}

#[test]
fn ignorecase_does_not_apply_to_single_char_fs() {
    // gawk: `IGNORECASE` affects multi-char regex FS but **not** a single-char
    // literal FS. With FS="x" on input "aXbXc", no splitting happens.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { IGNORECASE=1; FS="x" } { print NF, "[" $0 "]" }"#,
        "aXbXc\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [aXbXc]\n");
}

#[test]
fn ignorecase_does_not_apply_to_single_char_split_fs() {
    // Same rule for the `split` builtin's single-char FS string.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            IGNORECASE = 1
            n = split("aXbXc", a, "x")
            print n, "[" a[1] "]"
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 [aXbXc]\n");
}

#[test]
fn ignorecase_does_apply_to_multi_char_regex_fs() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { IGNORECASE=1; FS="xx" } { print NF, $1, $2, $3, $4 }"#,
        "aXXbXXcXXd\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "4 a b c d\n");
}

#[test]
fn printf_unknown_conversion_emits_literal_and_keeps_arg() {
    // gawk parity: `%q` (any letter not in the known set) emits "%q" literally
    // and does NOT consume an argument, so the next conversion sees the args
    // the user intended for it.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "[%q][%s]\n", "first", "second" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[%q][first]\n");
}

#[test]
fn regex_dot_matches_embedded_newline_in_match() {
    // gawk: in ERE, `.` matches any byte including `\n`. Rust regex defaults to
    // NOT matching newline; awkrs explicitly enables `dot_matches_new_line`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { s = "ab\ncd"; print s ~ /a.*d/ ? "yes" : "no" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "yes\n");
}

#[test]
fn printf_percent_u_negative_wraps_two_s_complement() {
    // gawk parity: `printf "%u", -5` emits the 64-bit two's complement of -5,
    // not 0. Previously awkrs clamped negatives to 0.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%u %u %u\n", -1, -5, 5 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "18446744073709551615 18446744073709551611 5\n");
}

#[test]
fn stray_semicolons_are_empty_statements() {
    // gawk: bare `;` between statements is an empty statement (POSIX C-style).
    // awkrs previously rejected `} ; print` as a parse error.
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { ; print "a" ; ; ; if (1) { print "b" } ; print "c" }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "a\nb\nc\n");
}

#[test]
fn crlf_line_terminator_preserves_cr_in_record() {
    // gawk parity: only `\n` is the record terminator on Unix; a trailing `\r`
    // is part of `$0` and counts toward `length`. Previously awkrs stripped
    // both `\n` and `\r`, silently dropping CR bytes on CRLF input.
    let (c, o, _) = run_awkrs_stdin(
        r#"{ printf "%d|%d|[%s]\n", NR, length, $0 }"#,
        "a\r\nb\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1|2|[a\r]\n2|1|[b]\n");
}

#[test]
fn bignum_print_uses_full_precision_for_integers() {
    // Regression: `print` of a bignum integer used to go through OFMT (%.6g)
    // and truncate to scientific form ("1.2677e+30"). With the integer fast
    // path the full 31-digit value of 2^100 is preserved.
    let (c, o, _) = run_awkrs_stdin_args(
        ["-M"],
        "BEGIN { print 2^100 }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1267650600228229401496703205376\n");
}

#[test]
fn bignum_factorial_uses_full_precision() {
    let (c, o, _) = run_awkrs_stdin_args(
        ["-M"],
        "function f(n) { return n<=1?1:n*f(n-1) } BEGIN { print f(25) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "15511210043330985984000000\n");
}

#[test]
fn user_assignment_to_nr_persists_and_is_incremented_per_record() {
    // gawk parity: NR is user-assignable. BEGIN sets NR=5; reading the first
    // record bumps it to 6 (not back to 1).
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { NR = 5 } NR == 5 { print "five:", $0 } NR == 6 { print "six:", $0 }"#,
        "x\ny\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "six: x\n");
}

#[test]
fn user_assignment_to_fnr_persists_per_record() {
    let (c, o, _) = run_awkrs_stdin(
        r#"{ if (FNR == 1) FNR = 99; print FNR }"#,
        "a\nb\nc\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "99\n100\n101\n");
}

#[test]
fn at_namespace_directive_followed_by_inline_program_on_same_line() {
    // gawk accepts `@namespace "name"; rest_of_program` on a single line.
    // awkrs previously dropped the trailing program text on the same line.
    let (c, o, _) = run_awkrs_stdin(
        r#"@namespace "awk"; BEGIN { print "ok" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ok\n");
}

#[test]
fn strtonum_takes_longest_numeric_prefix() {
    // Regression: `strtonum("42abc")` used to return 0; gawk returns 42 (the
    // longest leading numeric prefix). Trailing junk is ignored.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print strtonum("42abc"), strtonum("  -5.5xyz  "), strtonum("1e3and") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "42 -5.5 1000\n");
}

#[test]
fn strtonum_bare_inf_nan_return_zero() {
    // gawk's strtonum requires a digit / sign / dot as the first char. Bare
    // "inf" and "nan" return 0 even though Rust's `f64::parse` would accept them.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print strtonum("nan"), strtonum("inf"), strtonum("NaN") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0 0 0\n");
}

#[test]
fn strtonum_signed_inf_passes_through() {
    // With an explicit sign, the parser accepts non-finite values.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print strtonum("+inf"), strtonum("-inf"), strtonum("+nan") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "+inf -inf +nan\n");
}

#[test]
fn strtonum_signed_hex_returns_zero() {
    // gawk: `strtonum("+0x10")` / `"-0x10"` return 0 because the hex prefix is
    // only honored without a sign.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print strtonum("0x10"), strtonum("+0x10"), strtonum("-0x10") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "16 0 0\n");
}

#[test]
fn errno_message_strips_rust_os_error_suffix() {
    // Regression: ERRNO used to contain Rust's full `io::Error` Display
    // ("No such file or directory (os error 2)"). gawk emits just the strerror
    // text — the " (os error N)" suffix is now stripped.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { r = (getline line < "/nonexistent_xyz123"); print r, "[" line "]"; print "errno:" ERRNO }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "-1 []\nerrno:No such file or directory\n");
}

#[test]
fn getline_from_missing_file_does_not_print_runtime_error() {
    // Regression: the file-open error used to be wrapped as `Error::Runtime`
    // with a long message that polluted ERRNO and bypassed the `Error::Io` path.
    // Now ERRNO holds the OS strerror and stderr stays clean.
    let (_c, o, e) = run_awkrs_stdin(
        r#"BEGIN { getline line < "/nonexistent_xyz123" }"#,
        "",
    );
    assert!(
        !e.contains("runtime error"),
        "stderr should not contain Rust panic-style text; got {e:?}"
    );
    // The program produces no output either way (no print statements).
    assert!(o.is_empty(), "stdout={o:?}");
}

#[test]
fn unknown_escape_sequence_drops_backslash() {
    // gawk parity: `\q` in a string literal emits just `q` (with a `--lint`
    // warning). awkrs previously kept the backslash, so `"a\qb"` came out as
    // `a\qb` instead of `aqb`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print "a\qb", length("a\qb"), "x\z" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "aqb 3 xz\n");
}

#[test]
fn sandbox_rejects_pipe_getline_fatally() {
    // gawk parity: `-S` / `--sandbox` should make pipe getline a *fatal* error,
    // not silently return -1. The expression form used to absorb the sandbox
    // violation through `getline_error_code_for_key`.
    let (c, _o, e) = run_awkrs_stdin_args(
        ["-S"],
        r#"BEGIN { r = ("echo hi" | getline x); print "r=" r }"#,
        "",
    );
    assert_ne!(c, 0, "expected non-zero exit");
    assert!(e.contains("sandbox"), "stderr={e:?}");
}

#[test]
fn concat_with_parenthesized_ternary_uses_then_value() {
    // Regression: `"a" (cond ? "b" : "c")` used to produce `"bc"` (then ++ else)
    // because the `PushStr; Concat` peephole fused the *outer* Concat with the
    // ELSE branch's PushStr, scrambling the post-ternary join. The optimizer
    // now refuses to fuse when the Concat is itself a jump target.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            print "a" (1 ? "b" : "c")
            print "a" (0 ? "b" : "c")
            x = "x" (1 ? "y" : "z"); print x
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ab\nac\nxy\n");
}

#[test]
fn newlines_allowed_after_continuation_tokens() {
    // POSIX / gawk: after `,`, `||`, `&&`, `?`, `:`, a newline is whitespace
    // (the statement continues on the next line). Previously awkrs's parser
    // rejected newlines in these positions.
    let (c, o, _) = run_awkrs_stdin(
        r#"function f(a, b, c) { return a + b + c }
BEGIN {
    printf "%s %s %s\n",
        "x",
        "y",
        "z"
    x = 0 ||
        1 ||
        0
    y = 1 &&
        1 &&
        1
    t = (x > 0 ?
            "yes" :
            "no")
    print x, y, t, f(1,
                     2,
                     3)
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "x y z\n1 1 yes 6\n");
}

#[test]
fn array_by_ref_propagates_through_function_param_use() {
    // Regression: when a caller's variable was only mentioned as a function
    // argument (never indexed at the call site), awkrs's compiler allocated a
    // slot for it instead of treating it as an array. The function's array
    // writes were then lost because the bind-back wrote to `vars[]` while the
    // caller read from the slot. The static analysis now propagates the
    // array-ness from each user function's parameter back through call sites.
    let (c, o, _) = run_awkrs_stdin(
        r#"function fill(a) { a[1]=10; a[2]=20 }
           BEGIN {
               fill(x)
               print length(x), x[1], x[2]
           }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 10 20\n");
}

#[test]
fn recursive_array_population_through_function_propagates_to_caller() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function fill(a, n) { if (n > 0) { a[n] = n; fill(a, n-1) } }
           BEGIN {
               fill(x, 5)
               print length(x)
               for (i = 1; i <= 5; i++) printf "%d ", x[i]
               print ""
           }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "5\n1 2 3 4 5 \n");
}

#[test]
fn asort_honors_ignorecase_for_string_values() {
    // gawk parity: `asort` uses case-insensitive string comparison when
    // `IGNORECASE` is set. Without it, "B" and "C" sort before "a"; with it,
    // they all sort case-insensitively.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            IGNORECASE = 1
            a[1] = "B"; a[2] = "a"; a[3] = "C"
            n = asort(a)
            for (i = 1; i <= n; i++) print a[i]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "a\nB\nC\n");
}

#[test]
fn asorti_honors_ignorecase_for_keys() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            IGNORECASE = 1
            a["B"] = 1; a["a"] = 2; a["C"] = 3
            n = asorti(a)
            for (i = 1; i <= n; i++) print a[i]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "a\nB\nC\n");
}

#[test]
fn newline_allowed_between_control_flow_head_and_body() {
    // POSIX / gawk: `if (cond) <NL> stmt`, `while (cond) <NL> stmt`,
    // `for (...) <NL> stmt`, and `else <NL> stmt` are all legal — a single
    // newline before the single-statement body is whitespace. Previously
    // awkrs's parser rejected this with "unexpected token: Newline".
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            if (1)
                print "if-yes"
            if (0)
                print "if-no"
            else
                print "else-yes"
            for (i = 0; i < 3; i++)
                print "for", i
            while (j < 2)
                j++
            print "after", j
            do
                k++
            while (k < 2)
            print "after-do", k
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(
        o,
        "if-yes\nelse-yes\nfor 0\nfor 1\nfor 2\nafter 2\nafter-do 2\n"
    );
}

#[test]
fn unbounded_recursion_errors_cleanly_instead_of_stack_overflow() {
    // Regression: infinite recursion used to overrun Rust's thread stack and
    // abort with "fatal runtime error: stack overflow". The call-depth cap is
    // now low enough to error cleanly before native overflow.
    let (c, _o, e) = run_awkrs_stdin(
        r#"function f(n) { return f(n + 1) } BEGIN { print f(0) }"#,
        "",
    );
    assert_ne!(c, 0, "expected non-zero exit");
    assert!(
        e.contains("maximum user function call depth"),
        "stderr should mention the depth cap, got: {e:?}"
    );
    assert!(
        !e.contains("stack overflow"),
        "should error cleanly, not via native stack overflow: {e:?}"
    );
}

#[test]
fn split_ignores_zero_width_regex_matches() {
    // gawk parity: a regex that can match the empty string (e.g. `/x*/`)
    // contributes no splits at positions where it matched zero-width. Without
    // this skip, `split("abc", a, /x*/)` would split between every character.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("abc", a, /x*/)
            print "/x*/:", n, "[" a[1] "]"

            n = split("aaab", b, /a*/)
            print "/a*/:", n
            for (i = 1; i <= n; i++) printf "  %d=[%s]\n", i, b[i]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "/x*/: 1 [abc]\n/a*/: 2\n  1=[]\n  2=[b]\n");
}

#[test]
fn printf_alternate_form_hex_zero_emits_just_zero() {
    // POSIX / gawk: the `#` flag adds the `0x`/`0X` prefix only when the
    // value is non-zero. `printf "%#x", 0` is `"0"`, not `"0x0"`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%#x|%#X|%#x|%#o\n", 0, 0, 255, 0 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0|0|0xff|0\n");
}

#[test]
fn jit_correctly_handles_short_circuit_with_regex_match() {
    // Critical regression: after JIT compilation kicks in (typically on the
    // 3rd invocation), `($0 ~ /a/) && ($0 ~ /b/)` patterns silently dropped
    // matches because the JIT's merge-block stack handling lost the
    // RegexMatch result across the short-circuit branch. The optimizer now
    // refuses to JIT chunks that mix `~`/`!~` with `JumpIf{False,True}Pop`.
    //
    // Test with 4+ records so we definitely cross the JIT-compile threshold.
    let (c, o, _) = run_awkrs_stdin(
        r#"/a/ && /b/"#,
        "ab\nac\nabc\nzz\nabcd\nbb\nbab\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ab\nabc\nabcd\nbab\n");
}

#[test]
fn jit_correctly_handles_explicit_match_with_short_circuit() {
    // Same regression with the explicit `$0 ~ /regex/` spelling.
    let (c, o, _) = run_awkrs_stdin(
        r#"($0 ~ /x/) && ($0 ~ /y/)"#,
        "xy\nxa\nay\nxay\nyx\nxyy\nyxy\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "xy\nxay\nyx\nxyy\nyxy\n");
}

#[test]
fn sprintf_percent_s_of_numeric_uses_convfmt() {
    // gawk parity: `sprintf "%s"` on a numeric value stringifies via CONVFMT,
    // not the f64 default Display. `CONVFMT="%.3f"; printf "%s", 3.14159`
    // should print `"3.142"`, not `"3.14159"`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            CONVFMT = "%.3f"
            printf "%s|", 3.14159     # via CONVFMT
            printf "%d|", 3.14159     # %d ignores CONVFMT
            printf "%g|", 3.14159     # %g ignores CONVFMT
            printf "%s\n", 42         # integer-valued bypasses CONVFMT
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3.142|3|3.14159|42\n");
}

#[test]
fn match_three_arg_populates_array_zero_with_whole_match() {
    // gawk parity: `match(s, re, arr)` stores the whole match in `arr[0]` and
    // the captures in `arr[1]`..`arr[n]`. awkrs previously skipped `arr[0]`,
    // so `match(s, /(a)|(b)/, arr)` left arr[0] empty for an alternation that
    // matched group 1.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            s = "abc"
            if (match(s, /(a)|(b)/, arr)) {
                printf "0=[%s] 1=[%s] 2=[%s]\n", arr[0], arr[1], arr[2]
            }
            t = "hello world"
            if (match(t, /(\w+) (\w+)/, arr2)) {
                printf "0=[%s] 1=[%s] 2=[%s]\n", arr2[0], arr2[1], arr2[2]
            }
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0=[a] 1=[a] 2=[]\n0=[hello world] 1=[hello] 2=[world]\n");
}

#[test]
fn deep_but_bounded_recursion_succeeds() {
    // 100 levels deep is comfortably under the production cap.
    let (c, o, _) = run_awkrs_stdin(
        r#"function f(n) { return n == 0 ? "ok" : f(n - 1) }
           BEGIN { print f(100) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ok\n");
}

#[test]
fn backslash_newline_is_line_continuation() {
    // POSIX / gawk: a backslash immediately followed by a newline is treated
    // as whitespace — the statement continues on the next physical line.
    // Previously awkrs's lexer reported "unexpected character '\\'".
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { \\\n  x = \"line1\\n\" \\\n      \"line2\"; \\\n  print x \\\n}",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "line1\nline2\n");
}

#[test]
fn getline_statement_with_missing_file_does_not_abort() {
    // gawk parity: `getline var < missing_file` as a STATEMENT silently sets
    // ERRNO and continues; only the expression-form returns -1. Previously
    // awkrs's statement form raised a fatal runtime error.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            line = "unchanged"
            getline line < "/nonexistent_path_xyz_zzz"
            print "[" line "]"
            print "ERRNO:" ERRNO
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[unchanged]\nERRNO:No such file or directory\n");
}

#[test]
fn printf_apostrophe_groups_float_integer_part() {
    // gawk parity: the `'` flag groups the integer portion of `%f` values too.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%'f|%'f|%'.2f\n", 1234567.89, 1.5, 0.5 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1,234,567.890000|1.500000|0.50\n");
}

#[test]
fn ignorecase_applies_to_bare_regex_pattern() {
    // Regression: `/abc/` as a record pattern took a literal-substring fast
    // path that bypassed IGNORECASE entirely. The inline / slurp / VM paths
    // now defer to the regex engine when the flag is on.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { IGNORECASE = 1 } /abc/"#,
        "ABC\nxyz\nAbCdef\nNoMatch\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ABC\nAbCdef\n");
}

#[test]
fn index_honors_ignorecase() {
    // gawk parity: `index()` is case-insensitive when `IGNORECASE` is set.
    // awkrs's `index()` used to bypass IGNORECASE entirely.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            IGNORECASE = 1
            print index("ABC", "b")        # 2 (matches `B` case-insensitively)
            print index("ABC", "B")        # 2 (still works case-sensitively)
            print index("Hello World", "WORLD") # 7
            print index("abc", "Z")        # 0 (not present at all)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2\n2\n7\n0\n");
}

#[test]
fn patsplit_honors_ignorecase() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            IGNORECASE = 1
            n = patsplit("xBYbZBA", a, "b")
            print n
            for (i = 1; i <= n; i++) print i, a[i]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3\n1 B\n2 b\n3 B\n");
}

#[test]
fn sprintf_trailing_percent_emits_literal() {
    // gawk parity: a format string ending in a stray `%` (no conversion letter
    // after it) emits the literal `%` rather than raising "truncated format".
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print sprintf("abc%") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "abc%\n");
}

#[test]
fn gsub_honors_ignorecase_for_literal_pattern() {
    // Regression: gsub's literal-pattern fast path bypassed regex compilation
    // entirely, so `IGNORECASE=1; gsub("b", "X", "ABC")` silently produced
    // "ABC" instead of gawk's "AXC". The fast path now defers to the regex
    // engine whenever IGNORECASE is set.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            IGNORECASE = 1
            s = "ABC";    gsub("b",  "X", s); print "1", s
            s = "aBcBaB"; gsub(/b/,  "X", s); print "2", s
            s = "AbC";    gsub(/B/,  "X", s); print "3", s
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 AXC\n2 aXcXaX\n3 AXC\n");
}

#[test]
fn sub_replacement_preserves_backslashes_per_gawk_rules() {
    // gawk parity for sub/gsub replacement-string semantics:
    //   `&`     → match
    //   `\&`    → literal `&`
    //   `\\&`   → literal `\` + match  (each `\\` collapses to `\` only when
    //                                    immediately followed by `&`)
    //   `\\`    → `\\` (two backslashes kept verbatim outside the `&` context)
    //   `\X`    → `\X` (sub/gsub do NOT expand backrefs — that's gensub)
    //
    // Previously awkrs collapsed any `\X` to `X`, so `sub(/(a)/, "[\\1]", s)`
    // produced "[1]bc" instead of gawk's "[\1]bc".
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            s = "AbB"
            t = s; gsub(/B/, "X",     t); print "1 [" t "]"
            t = s; gsub(/B/, "&",     t); print "2 [" t "]"
            t = s; gsub(/B/, "\\&",   t); print "3 [" t "]"
            t = s; gsub(/B/, "\\\\",  t); print "4 [" t "]"
            t = s; gsub(/B/, "\\\\&", t); print "5 [" t "]"
            t = s; gsub(/B/, "\\X",   t); print "6 [" t "]"

            # sub() of a captured-group pattern: \1 stays literal
            u = "abc"
            sub(/(a)/, "[\\1]", u)
            print "7 [" u "]"
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(
        o,
        "1 [AbX]\n2 [AbB]\n3 [Ab&]\n4 [Ab\\\\]\n5 [Ab\\B]\n6 [Ab\\X]\n7 [[\\1]bc]\n"
    );
}

#[test]
fn indirect_function_call_via_at_var() {
    // gawk parity: `@var(args)` calls the function whose name is held in
    // `var`. The previous parser consumed `var(args)` as a complete call
    // before looking for the indirect `(args)` syntax, so the form always
    // failed to parse.
    let (c, o, _) = run_awkrs_stdin(
        r#"function double(x) { return x*2 }
           function add(a,b) { return a+b }
           BEGIN {
               fn = "double"; print @fn(5)
               fn = "add";    print @fn(3, 4)
           }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "10\n7\n");
}

#[test]
fn indirect_function_call_via_at_array_element() {
    // gawk also accepts indirect calls through array elements: `@a[k](args)`.
    let (c, o, _) = run_awkrs_stdin(
        r#"function triple(x) { return x*3 }
           BEGIN {
               names["t"] = "triple"
               print @names["t"](4)
           }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "12\n");
}

#[test]
fn field_assignment_of_number_uses_convfmt_for_record_rebuild() {
    // gawk parity for the common case `$1 = float_value; print`: the rebuilt
    // record uses CONVFMT to stringify the assigned number. Previously awkrs
    // kept the full-precision Rust Display form, so `CONVFMT="%.2f";
    // $1=3.14159; print` emitted "3.14159" instead of gawk's "3.14".
    //
    // Note: gawk additionally preserves the numeric type so that later
    // `print $1, $2` re-stringifies via OFMT (not CONVFMT). awkrs stores
    // fields as strings only, so the OFMT-on-individual-field-read case
    // diverges; this test exercises only the rebuild path that the fix
    // actually addresses.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { CONVFMT="%.2f"; OFS=":"; $1=3.14159; $2=2.71828; print }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3.14:2.72\n");
}

#[test]
fn printf_percent_d_handles_values_past_i64_max() {
    // Regression: `printf "%d", 2^63` saturated at i64::MAX = 9223372036854775807,
    // instead of printing the actual value 9223372036854775808. f64 represents
    // every power of two exactly up to 2^1023, so the printed integer should
    // match gawk's behavior of preserving precision out to f64's limits.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%d %d %d %d\n", 2^63, -2^63, 1e20, -1e20 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(
        o,
        "9223372036854775808 -9223372036854775808 100000000000000000000 -100000000000000000000\n"
    );
}

#[test]
fn printf_percent_u_saturates_above_u64_max() {
    // For `printf "%u"`, gawk saturates the (uintmax_t)val cast at u64::MAX
    // for positive values larger than 2^64.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%u %u %u\n", 2^63, 2^64, -1 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(
        o,
        "9223372036854775808 18446744073709551615 18446744073709551615\n"
    );
}

#[test]
fn typeof_field_returns_strnum_for_numeric_field_value() {
    // gawk parity: a field whose string value parses as a number reports its
    // type as "strnum" (numeric string). Previously awkrs reported plain
    // "string", losing the numeric-comparison hint.
    let (c, o, _) = run_awkrs_stdin(
        r#"{ printf "%s %s %s\n", typeof($1), typeof($2), typeof($3) }"#,
        "42 abc 3.14\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "strnum string strnum\n");
}

#[test]
fn typeof_field_beyond_nf_returns_unassigned() {
    // gawk's typeof vocabulary uses "unassigned" for fields past NF (not
    // "untyped", which is reserved for never-touched scalars).
    let (c, o, _) = run_awkrs_stdin(r#"{ print typeof($5) }"#, "a b\n");
    assert_eq!(c, 0);
    assert_eq!(o, "unassigned\n");
}

#[test]
fn switch_case_falls_through_to_next_arm_without_break() {
    // gawk parity: `switch` is C-style — without `break`, control falls
    // through to the next arm's body. Previously awkrs auto-broke after each
    // case, so `case "a"` only executed its own body.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            x = "a"
            switch (x) {
                case "a": print "A"
                case "b": print "B"
                case "c": print "C"
            }
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "A\nB\nC\n");
}

#[test]
fn switch_case_break_stops_fallthrough() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            x = "a"
            switch (x) {
                case "a": print "A"; break
                case "b": print "B"
                case "c": print "C"
            }
            print "after"
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "A\nafter\n");
}

#[test]
fn switch_falls_into_default_when_explicitly_chained() {
    // Match in the middle, fall through to the rest including default.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            x = "b"
            switch (x) {
                case "a": print "A"
                case "b": print "B"
                case "c": print "C"
                default:  print "D"
            }
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "B\nC\nD\n");
}

#[test]
fn switch_no_match_with_default_jumps_to_default() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            switch ("zzz") {
                case "a": print "A"; break
                default:  print "D"
            }
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "D\n");
}

#[test]
fn switch_no_match_no_default_falls_through_to_end() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            switch ("zzz") {
                case "a": print "A"
                case "b": print "B"
            }
            print "after"
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "after\n");
}

#[test]
fn printf_precision_zero_on_value_zero_emits_empty_string() {
    // POSIX: `printf "%.0d", 0` produces NO digits. Same for `%.0i`, `%.0u`,
    // `%.0o`, `%.0x`. Non-zero values still print at least one digit.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            printf "[%.0d|%.0i|%.0u|%.0o|%.0x]\n", 0, 0, 0, 0, 0
            printf "[%.0d|%.0u|%.0x]\n", 5, 5, 255
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[||||]\n[5|5|ff]\n");
}

#[test]
fn negative_zero_prints_as_plain_zero() {
    // gawk parity: `-0.0` is printed as `"0"`, not `"-0"`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print -0.0; print 0.0 - 0; print -1 * 0 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0\n0\n0\n");
}

#[test]
fn bignum_non_integer_value_still_uses_ofmt() {
    // The integer fast path must not affect non-integer bignums — they still
    // go through OFMT (`%.6g` default).
    let (c, o, _) = run_awkrs_stdin_args(
        ["-M"],
        "BEGIN { print 1/3 }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0.333333\n");
}

#[test]
fn for_in_block_followed_by_semicolon_and_statement() {
    // Regression: `for (k in a) { … } ; print …` parsed as if the `;` started
    // an expression.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            a[1] = 1; a[2] = 2; a[3] = 3
            for (k in a) { if (k == "2") delete a[k] }
            ;
            print length(a)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn printf_zero_flag_on_string_ignored_pads_with_spaces() {
    // POSIX: `0` flag is for numeric conversions only.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "[%05s][%05c]\n", "ab", 65 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[   ab][    A]\n");
}

#[test]
fn regex_dot_matches_newline_in_gsub() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { s = "ab\ncd"; n = gsub(/./, "X", s); print n, "[" s "]" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "5 [XXXXX]\n");
}

#[test]
fn split_with_seps_variable_keeps_target_array_isolated() {
    // The `seps` parameter must be a separate array — assigning to it should not
    // perturb the destination array `a`.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            n = split("a-b-c", a, "-", seps);
            seps[1] = "MUTATED";
            print a[1], a[2], a[3], "|", seps[1], seps[2]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "a b c | MUTATED -\n");
}

#[test]
fn cmp_num_to_string_literal_uses_convfmt() {
    // gawk parity: when comparing a Num to a string literal (non-numeric-string),
    // the Num is stringified via CONVFMT before string-compare. Before the fix
    // awkrs treated literals as numeric strings and did numeric compare.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            CONVFMT = "%.2f"
            x = 3.14159
            print (x == "3.14")
            print (x == "3.14159")
            print (x != "3.14")
            print (x < "3.2")
            print (x > "3.0")
            CONVFMT = "%.6f"
            print (42 == "42")
            print (42 == "42.000000")
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1\n0\n0\n1\n1\n1\n0\n");
}

#[test]
fn printf_apostrophe_flag_skipped_in_c_locale() {
    // gawk parity: the `'` flag groups via `localeconv()->thousands_sep`. In the
    // C locale that field is empty — `printf "%'d", 1234567` must NOT insert
    // commas. Earlier awkrs unconditionally fell back to ",".
    let env = vec![
        (OsString::from("LC_ALL"), OsString::from("C")),
        (OsString::from("LANG"), OsString::from("C")),
    ];
    let (c, o, _) = run_awkrs_stdin_args_env(
        Vec::<&str>::new(),
        r#"BEGIN { printf "%'d\n%'f\n%'.2f\n", 1234567, 1234567.89, 9876543.21 }"#,
        "",
        env,
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1234567\n1234567.890000\n9876543.21\n");
}
