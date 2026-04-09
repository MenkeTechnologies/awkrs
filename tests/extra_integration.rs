//! Extra end-to-end cases (operators, CLI combinations, field/record edges).

mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_args};
use std::fs;
use std::process::Command;

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
fn xor_via_mod_not_in_language_skip() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print xor(1,2) }", "");
    assert_ne!(c, 0);
    let _ = o;
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
