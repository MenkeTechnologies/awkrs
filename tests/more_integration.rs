//! Additional end-to-end coverage (language, builtins, I/O).

mod common;

use common::{run_awkrs_stdin, run_awkrs_stdin_args};
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
fn j1_silences_parallel_warning() {
    let (c, _, e) = run_awkrs_stdin_args(["-j", "1"], "{ print $1 }", "a\nb\n");
    assert_eq!(c, 0);
    assert!(
        !e.contains("not parallel-safe"),
        "unexpected warning: {e:?}"
    );
}
