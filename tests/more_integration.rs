//! Additional end-to-end coverage (language, builtins, I/O).

mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_args};
use std::process::{Command, Stdio};

#[test]
fn empty_print_prints_dollar0() {
    let (c, o, _) = run_awkrs_stdin("{ print }", "hello world\n");
    assert_eq!(c, 0);
    assert_eq!(o, "hello world\n");
}

#[test]
fn dollar_nf_last_field() {
    let (c, o, _) = run_awkrs_stdin("{ print $NF }", "a b c d\n");
    assert_eq!(c, 0);
    assert_eq!(o, "d\n");
}

#[test]
fn nr_increments_record_rules() {
    let (c, o, _) = run_awkrs_stdin("{ print NR }", "a\nb\nc\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n2\n3\n");
}

#[test]
fn fnr_resets_each_input_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f1 = dir.join(format!("awkrs_fnr1_{id}.txt"));
    let f2 = dir.join(format!("awkrs_fnr2_{id}.txt"));
    std::fs::write(&f1, "a\n").unwrap();
    std::fs::write(&f2, "b\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(r"{ print FNR }")
        .arg(&f1)
        .arg(&f2)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs");
    let _ = std::fs::remove_file(&f1);
    let _ = std::fs::remove_file(&f2);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n1\n");
}

#[test]
fn arithmetic_plus_mul() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 2 + 3 * 4 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "14\n");
}

#[test]
fn string_concat() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"a\" \"b\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "ab\n");
}

#[test]
fn compare_strings_lt() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"a\" < \"b\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn compare_numeric_eq() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (1 == 1) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn ternary() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 1 ? \"yes\" : \"no\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "yes\n");
}

#[test]
fn if_else() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { if (0) print \"a\"; else print \"b\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn while_loop_sum() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { i = 1; s = 0; while (i <= 5) { s += i; i = i + 1 } print s }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "15\n");
}

#[test]
fn for_c_style_loop() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { for (i = 1; i <= 3; i = i + 1) print i }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n2\n3\n");
}

#[test]
fn for_in_array() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { a[\"x\"] = 1; a[\"y\"] = 2; for (k in a) s += a[k]; print s }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn procinfo_sorted_in_ind_str_asc() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { a["b"]=1; a["a"]=2; PROCINFO["sorted_in"]="@ind_str_asc"; for (k in a) print k }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "a\nb\n");
}

#[test]
fn procinfo_sorted_in_custom_user_function() {
    let prog = r#"function cmp(a,b) { if (a < b) return -1; if (a > b) return 1; return 0 }
BEGIN { a["z"]=1; a["m"]=1; a["a"]=1; PROCINFO["sorted_in"]="cmp"; for (k in a) print k }"#;
    let (c, o, _) = run_awkrs_stdin(prog, "");
    assert_eq!(c, 0);
    assert_eq!(o, "a\nm\nz\n");
}

#[test]
fn intdiv_and_mkbool_builtins() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print intdiv(7, 3), intdiv(-7, 3); print mkbool(0), mkbool("x"), mkbool("") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 -2\n0 1 0\n");
}

#[test]
fn break_in_while() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { i = 0; while (1) { i = i + 1; if (i == 2) break } print i }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2\n");
}

#[test]
fn continue_in_for() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { for (i = 1; i <= 3; i = i + 1) { if (i == 2) continue; print i } }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1\n3\n");
}

#[test]
fn in_operator_membership() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { a[\"k\"] = 1; print (\"k\" in a), (\"z\" in a) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 0\n");
}

#[test]
fn in_operator_numeric_index() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { a[7] = 1; print (7 in a), (8 in a) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 0\n");
}

#[test]
fn in_operator_multidimensional_key() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { a[1,2] = 42; k = 1 SUBSEP 2; print (k in a), (k in b) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 0\n");
}

#[test]
fn in_operator_chained_comparison() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { a[\"x\"] = 1; print ((\"x\" in a) == 1), ((\"y\" in a) == 0) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 1\n");
}

#[test]
fn in_operator_false_when_name_not_array() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { s = \"scalar\"; print (\"k\" in s) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn delete_array_element() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { a[1] = 1; delete a[1]; print length(a) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn delete_entire_array() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { a[1] = 1; delete a; print length(a) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn substr_two_args() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print substr(\"abcdef\", 3) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "cdef\n");
}

#[test]
fn substr_three_args() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print substr(\"abcdef\", 2, 3) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "bcd\n");
}

#[test]
fn index_found() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print index(\"foobar\", \"bar\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn index_empty_needle() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print index(\"abc\", \"\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn length_no_args_is_record() {
    let (c, o, _) = run_awkrs_stdin("{ print length() }", "abcd\n");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn tolower_toupper() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print tolower(\"AbC\"), toupper(\"xYz\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "abc XYZ\n");
}

#[test]
fn int_truncates() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print int(-3.7), int(3.9) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "-3 3\n");
}

#[test]
fn sqrt_builtin() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sqrt(16) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn sub_on_record() {
    let (c, o, _) = run_awkrs_stdin("{ sub(\"l\", \"L\"); print }", "hello\n");
    assert_eq!(c, 0);
    assert_eq!(o, "heLlo\n");
}

#[test]
fn gsub_global() {
    let (c, o, _) = run_awkrs_stdin("{ gsub(\"l\", \"L\"); print }", "hello\n");
    assert_eq!(c, 0);
    assert_eq!(o, "heLLo\n");
}

#[test]
fn match_returns_position() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print match(\"foo123\", \"[0-9]+\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "4\n");
}

#[test]
fn sprintf_hex_upper() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%X\", 255) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "FF\n");
}

#[test]
fn sprintf_octal_alt() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%#o\", 8) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "010\n");
}

#[test]
fn sprintf_unsigned() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%u\", 42) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "42\n");
}

#[test]
fn sprintf_scientific() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%.2e\", 3.14159) }", "");
    assert_eq!(c, 0);
    assert!(o.starts_with("3.14e"), "got {o:?}");
}

#[test]
fn sprintf_char_c() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%c\", 65) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "A\n");
}

#[test]
fn ofs_join_fields() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { OFS = \"-\" } { print $1, $2 }", "a b\n");
    assert_eq!(c, 0);
    assert_eq!(o, "a-b\n");
}

#[test]
fn fs_single_char() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { FS = \",\" } { print $2 }", "a,b,c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn default_fs_whitespace() {
    let (c, o, _) = run_awkrs_stdin("{ print $2 }", "  x   y  z  \n");
    assert_eq!(c, 0);
    assert_eq!(o, "y\n");
}

#[test]
fn getline_from_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_gl_{}.txt", std::process::id()));
    std::fs::write(&path, "fileline\n").unwrap();
    let p = path.to_string_lossy().replace('\\', "/");
    let prog = format!("BEGIN {{ getline x < \"{p}\"; print x }}");
    let (c, o, _) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0);
    assert_eq!(o, "fileline\n");
}

#[test]
fn append_redirect() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_app_{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_string_lossy().replace('\\', "/");
    let prog = format!("BEGIN {{ print \"a\" > \"{p}\"; print \"b\" >> \"{p}\" }}");
    let (c, _, _) = run_awkrs_stdin(&prog, "");
    assert_eq!(c, 0);
    let contents = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(contents, "a\nb\n");
}

#[test]
fn fflush_empty_string_stdout() {
    let (c, _, _) = run_awkrs_stdin(r#"BEGIN { fflush("") }"#, "");
    assert_eq!(c, 0);
}

#[test]
fn pipe_fflush() {
    let (c, _, e) = run_awkrs_stdin(r#"BEGIN { print "z" | "cat"; fflush("cat") }"#, "");
    assert_eq!(c, 0, "stderr={e:?}");
}

#[test]
fn srand_returns_previous_seed() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print srand(42); print rand() }", "");
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn system_true_exit() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print system("exit 0") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "0");
}

#[test]
fn pattern_expression_gt() {
    let (c, o, _) = run_awkrs_stdin("$1 > 1 { print $2 }", "1 a\n3 b\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn range_pattern_two_lines() {
    let (c, o, _) = run_awkrs_stdin("/start/,/end/ { print $1 }", "x\nstart\nm\nend\ny\n");
    assert_eq!(c, 0);
    assert_eq!(o, "start\nm\nend\n");
}

#[test]
fn awk_style_mod() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 7 % 3 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn unary_minus() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print - -5 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "5\n");
}

#[test]
fn not_operator() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print !0, !1 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 0\n");
}

#[test]
fn regexp_match_operator() {
    let (c, o, _) = run_awkrs_stdin("{ print ($0 ~ /[0-9]+/) }", "abc42\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn block_statement() {
    let (c, o, _) = run_awkrs_stdin("{ { print $1 } }", "ok\n");
    assert_eq!(c, 0);
    assert_eq!(o, "ok\n");
}

#[test]
fn multidimensional_delete_one_key() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { SUBSEP = \",\"; a[1,2] = 9; delete a[1,2]; print a[1,2]+0 }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn printf_function_to_stdout() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { printf "%d", 7 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "7");
}

#[test]
fn split_default_fs_from_fs_var() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { FS = \":\"; n = split(\"a:b:c\", p); print n, p[2] }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3 b\n");
}

#[test]
fn compound_assign_add() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = 1; x += 5; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "6\n");
}

#[test]
fn or_short_circuit() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (1 || 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn and_short_circuit() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (1 && 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn default_thread_count_silences_parallel_unsafe_warning() {
    let (c, _, e) = run_awkrs_stdin(r#"{ print $1 |& "cat" }"#, "a\n");
    assert_eq!(c, 0);
    assert!(
        !e.contains("not parallel-safe"),
        "unexpected warning: {e:?}"
    );
}

#[test]
fn expr_pattern_nr_equals_one() {
    let (c, o, _) = run_awkrs_stdin("NR == 1 { print \"first\" }", "a\nb\n");
    assert_eq!(c, 0);
    assert_eq!(o, "first\n");
}

#[test]
fn two_rules_same_record() {
    let (c, o, _) = run_awkrs_stdin("{ print \"A\" } { print \"B\" }", "x\n");
    assert_eq!(c, 0);
    assert_eq!(o, "A\nB\n");
}

#[test]
fn field_assignment_updates_dollar_zero() {
    let (c, o, _) = run_awkrs_stdin("{$1 = \"new\"; print}", "old rest\n");
    assert_eq!(c, 0);
    assert_eq!(o, "new rest\n");
}

#[test]
fn compound_assign_mul() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = 3; x *= 4; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "12\n");
}

#[test]
fn compound_assign_div() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = 10; x /= 4; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "2.5\n");
}

#[test]
fn length_with_string_arg() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print length(\"abc\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn return_from_function() {
    let (c, o, _) = run_awkrs_stdin("function id(z){ return z } BEGIN { print id(\"ok\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "ok\n");
}

#[test]
fn nested_function_calls() {
    let (c, o, _) = run_awkrs_stdin(
        "function a(x){return x+1} function b(y){return y*2} BEGIN { print b(a(3)) }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "8\n");
}

#[test]
fn regexp_negated_match_operator() {
    let (c, o, _) = run_awkrs_stdin("{ print ($0 !~ /[0-9]+/) }", "abc\n42\n");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n0\n");
}

#[test]
fn string_compare_gt() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"z\" > \"a\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn concat_with_numeric_in_middle() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"a\" 2 \"b\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "a2b\n");
}

#[test]
fn empty_input_still_runs_begin_end() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print \"B\" } END { print \"E\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "B\nE\n");
}

#[test]
fn next_in_middle_rule_chain() {
    let (c, o, _) = run_awkrs_stdin("{ if (NR == 1) next; print $1 }", "skip\nkeep\n");
    assert_eq!(c, 0);
    assert_eq!(o, "keep\n");
}

#[test]
fn sprintf_percent_s_mixed_types() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%s-%d\", \"x\", 7) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "x-7\n");
}

#[test]
fn rand_bounded_after_srand() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { srand(1); print (rand() < 1 && rand() >= 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn match_with_array_captures() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { n = match(\"foo123bar\", \"([0-9]+)\", a); print n, a[1] }",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "4 123\n");
}

#[test]
fn sub_amp_backreference_style() {
    let (c, o, _) = run_awkrs_stdin("{ sub(\"a\", \"[&]\"); print }", "abc\n");
    assert_eq!(c, 0);
    assert_eq!(o, "[a]bc\n");
}

#[test]
fn gsub_ampersand_replacement() {
    let (c, o, _) = run_awkrs_stdin("{ gsub(\"o\", \"(&)\"); print }", "foo\n");
    assert_eq!(c, 0);
    assert_eq!(o, "f(o)(o)\n");
}

#[test]
fn begin_end_nr_fnr() {
    let (c, o, _) = run_awkrs_stdin(
        "BEGIN { print NR, FNR } { print NR, FNR } END { print NR, FNR }",
        "a\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0 0\n1 1\n1 1\n");
}

#[test]
fn empty_field_between_commas() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { FS = \",\" } { print NF, $2 }", "a,,c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "3 \n");
}

#[test]
fn default_ofmt_prints_number() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print 1.5 }", "");
    assert_eq!(c, 0);
    assert!(o.contains('1') && o.contains('5'), "got {o:?}");
}

#[test]
fn relop_string_vs_number_mixed() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (\"10\" < 9), (\"10\" < \"9\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0 0\n");
}

#[test]
fn print_multiple_args_ofs() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { OFS=\":\"; print 1,2,3 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1:2:3\n");
}

#[test]
fn record_ors_concat_print() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { ORS=\"|\"; print \"a\"; print \"b\" }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "a|b|");
}

#[test]
fn nested_arrays_subscript_expr() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { i=1; a[i]=10; print a[1] }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "10\n");
}

#[test]
fn comparison_chain() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (1 < 2 && 2 < 3) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "1\n");
}

#[test]
fn exit_in_action_end_still_runs() {
    let (c, o, _) = run_awkrs_stdin("{ exit 0 } END { print \"e\" }", "x\n");
    assert_eq!(c, 0);
    assert_eq!(o, "e\n");
}

#[test]
fn getline_into_var_from_string_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_gl2_{}.txt", std::process::id()));
    std::fs::write(&path, "onlyline\n").unwrap();
    let p = path.to_string_lossy().replace('\\', "/");
    let prog = format!("BEGIN {{ getline x < \"{p}\"; print x }}");
    let (c, o, _) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0);
    assert_eq!(o, "onlyline\n");
}

#[test]
fn split_returns_zero_on_empty_string() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { n = split(\"\", a); print n }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn match_returns_zero_for_no_match_end_to_end() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print match(\"abc\", \"z\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn sprintf_escaped_percent() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print sprintf(\"100%%\") }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "100%\n");
}

#[test]
fn delete_scalar_then_use() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x=1; delete x; print x+0 }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn file_input_slurped_fast_path_two_lines() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_slurp_{}.txt", std::process::id()));
    std::fs::write(&path, "aa bb\ncc dd\n").expect("temp data");
    let (c, o, e) = run_awkrs_file("{ print $1 }", &path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "aa\ncc\n");
}

/// Memory-mapped `print $N` fast path must use `ORS` from BEGIN (not a literal newline).
#[test]
fn slurp_print_field_respects_custom_ors() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_ors_{}.txt", std::process::id()));
    std::fs::write(&path, "a b\n").expect("temp data");
    let (c, o, e) = run_awkrs_file(r#"BEGIN { ORS = "X" } { print $1 }"#, &path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={e:?}");
    assert_eq!(o, "aX");
}

/// Slurp inline paths must not truncate `ORS` to 64 bytes (regression).
#[test]
fn slurp_inline_long_ors_not_truncated() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_orslong_{}.txt", std::process::id()));
    std::fs::write(&path, "a b\n").expect("temp data");
    let ors: String = "Y".repeat(70);
    let prog = format!(r#"BEGIN {{ ORS = "{ors}" }} {{ print $1 }}"#);
    let (c, o, e) = run_awkrs_file(&prog, &path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={e:?}");
    let want = format!("a{ors}");
    assert_eq!(o, want);
}

// ── FPAT / CSV (gawk-style) ───────────────────────────────────────────────

#[test]
fn fpat_field_by_content_regex() {
    // Non-empty FPAT: each match is a field (gawk-style); whitespace-separated tokens here.
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { FPAT = "[^ ]+" } { print NF, $2 }"#, "a b c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "3 b\n");
}

#[test]
fn csv_flag_k_matches_gawk_quoted_comma() {
    let (c, o, _) = run_awkrs_stdin_args(["-k"], r#"{ print NF, $2 }"#, "a,\"b,c\",d\n");
    assert_eq!(c, 0);
    assert_eq!(o, "3 b,c\n");
}

#[test]
fn fpat_empty_falls_back_to_fs() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { FPAT = ""; FS = "," } { print $2 }"#, "a,b,c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

// ── Multi-char FS regex (bug fix) ──────────────────────────────────────────

#[test]
fn fs_regex_character_class() {
    // FS="[,:]" should split on comma OR colon (regex), not the literal "[,:]".
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN{FS="[,:]"} {print $2}"#, "a,b:c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn fs_regex_alternation() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN{FS="::|-"} {print $1, $2, $3}"#, "x::y-z\n");
    assert_eq!(c, 0);
    assert_eq!(o, "x y z\n");
}

#[test]
fn fs_regex_plus_whitespace() {
    // FS=" +" (one or more spaces, not default whitespace trimming)
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN{FS=" +"} {print $2}"#, "a   b   c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

#[test]
fn split_uses_regex_for_multichar_fs() {
    let (c, o, _) = run_awkrs_stdin(
        r#"{ n = split($0, a, "[,:]"); print n; print a[1]; print a[2]; print a[3] }"#,
        "x,y:z\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3\nx\ny\nz\n");
}

#[test]
fn fs_flag_regex_character_class() {
    // -F '[,:]' should also work as regex.
    let (c, o, _) = run_awkrs_stdin_args(["-F", "[,:]"], "{ print $2 }", "a,b:c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

// ── getline var should not clobber NF (bug fix) ────────────────────────────

#[test]
fn getline_var_preserves_nf() {
    // Read a line into variable x — NF should reflect $0's fields, not change.
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_gl_nf_{}.txt", std::process::id()));
    std::fs::write(&path, "extra\n").expect("temp data");
    let prog = format!(
        r#"{{ print NF; getline x < "{}"; print NF }}"#,
        path.display()
    );
    let (c, o, e) = run_awkrs_stdin(&prog, "a b c\n");
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={e:?}");
    // NF should be 3 before and after getline-into-var.
    assert_eq!(o, "3\n3\n");
}

// ── Regex literal with escaped backslash (bug fix) ─────────────────────────

#[test]
fn regex_escaped_backslash_terminates_correctly() {
    // /\\/ should match a literal backslash. The regex body is "\\".
    let (c, o, _) = run_awkrs_stdin(r#"/\\/ { print "yes" }"#, "a\\b\nno\n");
    assert_eq!(c, 0);
    assert_eq!(o, "yes\n");
}

// ── gawk-style ++/-- and do-while ──────────────────────────────────────────

#[test]
fn incdec_scalar_pre_post() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { i = 0; print i++, ++i, i; j = 5; print --j, j--, j }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0 2 2\n4 4 3\n");
}

#[test]
fn do_while_runs_at_least_once() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { k = 0; do { k++ } while (k < 3); print k }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn incdec_field_and_array() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { $1 = 10; print $1++; a[1] = 7; print ++a[1] }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "10\n8\n");
}

#[test]
fn do_while_continue_skips_to_condition() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { do { print "a"; continue; print "b" } while (0) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "a\n");
}

// ── POSIX math, ARGC/ARGV, nextfile, time builtins ─────────────────────────

#[test]
fn posix_math_builtins() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            print sin(0), cos(0), atan2(0, 1), exp(0), log(1)
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "0 1 0 1 0\n");
}

#[test]
fn argc_argv_includes_executable_and_input_paths() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f1 = dir.join(format!("awkrs_argv1_{id}.txt"));
    let f2 = dir.join(format!("awkrs_argv2_{id}.txt"));
    std::fs::write(&f1, "x\n").unwrap();
    std::fs::write(&f2, "y\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(
            r#"BEGIN {
            print ARGC
            for (i = 0; i < ARGC; i++) print ARGV[i]
        }"#,
        )
        .arg(&f1)
        .arg(&f2)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs");
    let _ = std::fs::remove_file(&f1);
    let _ = std::fs::remove_file(&f2);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("3"));
    assert_eq!(lines.next(), Some(bin));
    assert!(lines
        .next()
        .unwrap()
        .ends_with(&format!("awkrs_argv1_{id}.txt")));
    assert!(lines
        .next()
        .unwrap()
        .ends_with(&format!("awkrs_argv2_{id}.txt")));
    assert_eq!(lines.next(), None);
}

#[test]
fn nextfile_skips_remaining_records_in_current_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f1 = dir.join(format!("awkrs_nf_a_{id}.txt"));
    let f2 = dir.join(format!("awkrs_nf_b_{id}.txt"));
    std::fs::write(&f1, "skip\nmore\n").unwrap();
    std::fs::write(&f2, "keep\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(r#"{ if (NR == 1) nextfile; print }"#)
        .arg(&f1)
        .arg(&f2)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs");
    let _ = std::fs::remove_file(&f1);
    let _ = std::fs::remove_file(&f2);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "keep\n");
}

#[test]
fn systime_positive() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (systime() > 1000000000) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn strftime_epoch_utc() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print strftime("%Y-%m-%d", 0, 1) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1970-01-01");
}

#[test]
fn mktime_invalid_is_minus_one() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print mktime("nope") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "-1");
}

// ── Bitwise compl, asort/asorti two-arg, -e chaining, switch default, $expr ─

#[test]
fn compl_bitwise_not_zero() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print compl(0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "-1");
}

#[test]
fn asort_two_arg_fills_dest_leaves_src() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { a["x"]=3; a["y"]=1; n=asort(a,t); print n, t["1"], t["2"], a["x"]+0 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 1 3 3\n");
}

#[test]
fn asorti_two_arg_fills_dest_leaves_src() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { a["b"]=1; a["a"]=2; n=asorti(a,t); print n, t["1"], t["2"], a["a"]+0 }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "2 a b 2\n");
}

#[test]
fn multiple_dash_e_program_fragments_concatenate() {
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .args(["-e", "BEGIN { x = 7 }", "-e", "BEGIN { print x }"])
        .output()
        .expect("spawn awkrs -e ... -e ...");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn switch_default_only_runs_when_no_case_matches() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { switch (99) { case 1: print "a"; break; default: print "ok" } }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "ok\n");
}

#[test]
fn strtonum_empty_string_is_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print strtonum("") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "0\n");
}

#[test]
fn non_decimal_data_flag_coerces_hex_strings_in_numeric_context() {
    let (c, o, _) = run_awkrs_stdin_args(["-n"], r#"BEGIN { v = "0x10"; print v + 0 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "16\n");
}

#[test]
fn dollar_field_dynamic_expression() {
    let (c, o, _) = run_awkrs_stdin("{ print $(1 + 1) }", "a b c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "b\n");
}

// ── strftime(), typeof(array), match(var, re), $1=$1 rebuild ───────────────

#[test]
fn strftime_zero_args_produces_non_empty_timestamp() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = strftime(); print (length(x) > 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn typeof_array_variable_is_array() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { a[1] = 1; print typeof(a) }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "array\n");
}

#[test]
fn match_second_argument_can_be_regex_string_variable() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { pat = "[0-9]+"; print match("ab12cd", pat) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn dollar_one_equals_dollar_one_rebuilds_record_from_fields() {
    let (c, o, _) = run_awkrs_stdin("{$1 = $1; print}", "  a  b  \n");
    assert_eq!(c, 0);
    assert_eq!(o, "a b\n");
}

#[test]
fn regexp_pattern_anchors_line_start_and_end() {
    let (c, o, _) = run_awkrs_stdin(r#"/^only$/ { print "yes" }"#, "only\nno\nonly\n");
    assert_eq!(c, 0);
    assert_eq!(o, "yes\nyes\n");
}

// ── gawk: `@namespace`, `SYMTAB` lvalue, `ns::name` ─────────────────────────

#[test]
fn gawk_namespace_default_prefixes_unqualified_globals() {
    let prog = "@namespace \"n\"\nBEGIN { x = 7; print x }\n";
    let (c, o, _) = run_awkrs_stdin(prog, "");
    assert_eq!(c, 0);
    assert_eq!(o, "7\n");
}

#[test]
fn gawk_symtab_subscript_assigns_global_scalar() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { SYMTAB[\"q\"] = 99; print q }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "99\n");
}

#[test]
fn gawk_qualified_identifier_two_colon_lexes() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { n::x = 3; print n::x }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "3\n");
}

#[test]
fn symtab_length_is_positive() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { print (length(SYMTAB) > 0) }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn index_utf8_respects_characters_as_bytes_flag() {
    let prog = r#"BEGIN { print index("αβ", "β") }"#;
    let (c, o, _) = run_awkrs_stdin(prog, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "2");

    let (c_b, o_b, _) = run_awkrs_stdin_args(["-b"], prog, "");
    assert_eq!(c_b, 0);
    assert_eq!(o_b.trim(), "3");
}

#[test]
fn length_substr_utf8_respects_characters_as_bytes_flag() {
    let prog = r#"BEGIN { print length("αβ"); print substr("αβ", 3, 2) }"#;
    let (c, o, _) = run_awkrs_stdin(prog, "");
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[0], "2");
    assert_eq!(lines.get(1).copied().unwrap_or(""), "");

    let (c_b, o_b, _) = run_awkrs_stdin_args(["-b"], prog, "");
    assert_eq!(c_b, 0);
    let lines_b: Vec<&str> = o_b.lines().collect();
    assert_eq!(lines_b[0], "4");
    assert_eq!(lines_b[1], "β");
}
