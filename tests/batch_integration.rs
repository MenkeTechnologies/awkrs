//! Additional CLI and language coverage (batch 2).

mod common;

use common::{run_awkrs_stdin, run_awkrs_stdin_args};
use std::fs;
use std::process::Command;

#[test]
fn begin_sets_ofs_between_print_fields() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { OFS=\":\"; print 1, 2, 3 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1:2:3\n");
}

#[test]
fn begin_sets_ors_between_print_lines() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { ORS=\"|\"; print 1; print 2 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1|2|");
}

#[test]
fn subsep_default_joins_multidim_subscript() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { a[1,2]=9; print a[1,2] }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "9\n");
}

#[test]
fn subsep_custom_changes_key() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { SUBSEP=\"@\"; a[1,2]=3; print a[1,2] }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn print_empty_string_argument() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn printf_no_percent_uses_literal() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { printf \"hi\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "hi");
}

#[test]
fn logical_not_numeric() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print !0, !1 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 0\n");
}

#[test]
fn preincrement_via_assignment_sum() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { i=0; i=i+1; i=i+1; print i }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn while_loop_counter() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { i=0; while (i < 4) i=i+1; print i }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn if_else_picks_branch() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { if (0) print \"a\"; else print \"b\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn ternary_operator() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (1 ? \"yes\" : \"no\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "yes\n");
}

#[test]
fn string_concat_space_in_print() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"a\" \"b\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "ab\n");
}

#[test]
fn nr_equals_one_first_record() {
    let (c, o, _) = run_awkrs_stdin("{ print NR }", "only\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn fnr_equals_one_first_record() {
    let (c, o, _) = run_awkrs_stdin("{ print FNR }", "only\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn field_zero_is_whole_record() {
    let (c, o, _) = run_awkrs_stdin("{ print $0 }", "hello\n");
    assert_eq!(c, 0);
    assert_eq!(o, "hello\n");
}

#[test]
fn default_subsep_printable() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print SUBSEP }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "\x1c\n");
}

#[test]
fn escape_in_string_newline() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"a\\nb\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "a\nb\n");
}

#[test]
fn regexp_match_tilde_operator() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"hello\" ~ /ell/) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn regexp_not_match_operator() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"hello\" !~ /^z/) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn compound_div_assign() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x=8; x/=2; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn compound_mul_assign() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x=3; x*=4; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "12\n");
}

#[test]
fn getline_var_from_file_redirect() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_batch_gl_{id}.txt"));
    fs::write(&path, "line1\n").expect("write");
    let p = path.to_string_lossy();
    let prog = format!(r#"{{ getline x < "{p}"; print x }}"#);
    let (c, o, _) = run_awkrs_stdin(&prog, "stdin-line\n");
    let _ = fs::remove_file(&path);
    assert_eq!(c, 0);
    assert_eq!(o, "line1\n");
}

#[test]
fn begin_only_no_input_file() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 3 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn multiple_rules_sequential() {
    let (c, o, _) = run_awkrs_stdin("{ print \"a\" } { print \"b\" }", "x\n");
    assert_eq!(c, 0);
    assert_eq!(o, "a\nb\n");
}

#[test]
fn print_redirect_overwrite_creates_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f = dir.join(format!("awkrs_pr_{id}.txt"));
    let _ = fs::remove_file(&f);
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(format!(
            "BEGIN {{ print \"hi\" > \"{}\" }}",
            f.to_string_lossy()
        ))
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = fs::read_to_string(&f).expect("read");
    let _ = fs::remove_file(&f);
    assert_eq!(s, "hi\n");
}

#[test]
fn assign_fs_before_record_rule() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { FS=\",\" } { print $2 }", "a,b,c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn two_empty_rules() {
    let (c, o, _) = run_awkrs_stdin("{ } { }", "x\n");
    assert_eq!(c, 0);
    assert_eq!(o, "");
}

#[test]
fn length_builtin_string_arg() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print length(\"abcd\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn index_builtin() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print index(\"foobar\", \"bar\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn substr_two_args_from_one() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print substr(\"abcde\", 2) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "bcde\n");
}

#[test]
fn int_builtin_negative() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print int(-3.7) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "-3\n");
}

#[test]
fn sqrt_builtin() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sqrt(4) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn tolower_builtin() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print tolower(\"AbC\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "abc\n");
}

#[test]
fn toupper_builtin() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print toupper(\"xYz\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "XYZ\n");
}

#[test]
fn sprintf_percent_f() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print sprintf("%.1f", 2.25) }"#, "");
    assert_eq!(c, 0);
    assert!(o.contains("2.2") || o.contains("2.3"), "o={o:?}");
}

#[test]
fn multiple_assigns_minus_v() {
    let (c, o, _) = run_awkrs_stdin_args(
        ["-v", "a=1", "-v", "b=2", "-v", "c=3"],
        "BEGIN { print a+b+c }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "6\n");
}

#[test]
fn stdin_only_one_line() {
    let (c, o, _) = run_awkrs_stdin("{ print $1 }", "single\n");
    assert_eq!(c, 0);
    assert_eq!(o, "single\n");
}

#[test]
fn expr_pattern_nr_equals_two() {
    let (c, o, _) = run_awkrs_stdin("NR == 2 { print \"second\" }", "a\nb\nc\n");
    assert_eq!(c, 0);
    assert_eq!(o, "second\n");
}

#[test]
fn empty_brace_action_runs() {
    let (c, o, _) = run_awkrs_stdin("{ }", "x\n");
    assert_eq!(c, 0);
    assert_eq!(o, "");
}

#[test]
fn comparison_ne_string() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"a\" != \"b\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn comparison_le_string() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"a\" <= \"b\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn comparison_ge_numeric() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (3 >= 3) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn mod_operator_positive() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 7 % 3 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn unary_minus_on_number() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print -(-5) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "5\n");
}

#[test]
fn concat_empty_string() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"\" \"\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
}

#[test]
fn end_runs_after_records() {
    let (c, o, _) = run_awkrs_stdin("{ } END { print \"done\" }", "a\n");
    assert_eq!(c, 0);
    assert_eq!(o, "done\n");
}

#[test]
fn begin_end_nr() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print NR } END { print NR }", "a\nb\n");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n2\n");
}
