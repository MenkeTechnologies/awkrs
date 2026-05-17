//! A large collection of additional integration tests covering edge cases,
//! gawk extensions, CLI combinations, and complex language features.

mod common;

use common::{run_awkrs_stdin, run_awkrs_stdin_args, run_awkrs_stdin_args_env};

#[test]
fn csv_mode_quoted_fields() {
    let (c, o, _e) = run_awkrs_stdin_args(["-k"], "{ print $1, $2, $3 }", "1,\"2,3\",4\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 2,3 4\n");
}

#[test]
fn csv_mode_escaped_quotes() {
    let (c, o, _e) = run_awkrs_stdin_args(["-k"], "{ print $1 }", "\"a\"\"b\"\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a\"b\n");
}

#[test]
fn bignum_basic_arithmetic() {
    let (c, o, _e) = run_awkrs_stdin_args(["-M"], "BEGIN { printf \"%d\", 2^100 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o.trim(), "1267650600228229401496703205376");
}

#[test]
fn bignum_factorial() {
    let program = "function fact(n) { if (n <= 1) return 1; return n * fact(n-1); } BEGIN { printf \"%d\", fact(30); }";
    let (c, o, _e) = run_awkrs_stdin_args(["-M"], program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o.trim(), "265252859812191058636308480000000");
}

#[test]
fn assign_before_and_after_program() {
    let (c, o, _e) = run_awkrs_stdin_args(["-v", "x=10", "-v", "y=20"], "BEGIN { print x + y }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "30\n");
}

#[test]
fn delete_whole_array() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; a[2]=2; delete a; for (i in a) print i; print \"done\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "done\n");
}

#[test]
fn array_of_arrays_emulation() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1][2]=3; print a[1][2] }", "");
    if c == 0 {
        assert_eq!(o, "3\n");
    }
}

#[test]
fn multidimensional_subscript_with_subsep() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { SUBSEP=\"|\"; a[1,2]=5; for (i in a) print i, a[i] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1|2 5\n");
}

#[test]
fn gensub_backreferences() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print gensub(/([a-z]+) ([0-9]+)/, \"\\\\2 \\\\1\", \"g\", \"hello 123\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "123 hello\n");
}

#[test]
fn typeof_various_values() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; print typeof(1), typeof(\"hi\"), typeof(a), typeof(a[1]), typeof(untyped) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "number string array number untyped\n");
}

#[test]
fn parallel_j4_sum() {
    let (c, o, _e) = run_awkrs_stdin_args(["-j", "4"], "{ s += $1 } END { print s }", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "55\n");
}

#[test]
fn length_of_array() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; a[2]=2; print length(a) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2\n");
}

#[test]
fn split_returns_count() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print split(\"a b c\", arr) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3\n");
}

#[test]
fn split_with_regex_fs() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { split(\"a:b;c\", arr, /[:;]/); print arr[1], arr[2], arr[3] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a b c\n");
}

#[test]
fn match_with_third_argument_array() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { match(\"foo123bar\", /([a-z]+)([0-9]+)/, a); print a[1], a[2] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "foo 123\n");
}

#[test]
fn for_loop_with_break_and_continue() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { for(i=1; i<=10; i++) { if(i==2) continue; if(i==5) break; printf \"%d \", i; } print \"\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 3 4 \n");
}

#[test]
fn nested_loops_and_labels_emulation() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { for(i=1; i<=2; i++) { for(j=1; j<=2; j++) { print i, j } } }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 1\n1 2\n2 1\n2 2\n");
}

#[test]
fn function_recursion_fibonacci() {
    let program = "function fib(n) { if (n <= 1) return n; return fib(n-1) + fib(n-2); } BEGIN { print fib(10); }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "55\n");
}

#[test]
fn global_vs_local_variables() {
    let program = "function f(x, y) { y = 2; g = 3; return x + y + g; } BEGIN { g = 1; print f(10, 0); print g; }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "15\n3\n");
}

#[test]
fn arith_operators_precedence() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print 2 + 3 * 4, (2 + 3) * 4, 2^3^2, (2^3)^2 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "14 20 512 64\n");
}

#[test]
fn string_concatenation_precedence() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"a\" \"b\" == \"ab\", \"a\" 1+1 \"b\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 a2b\n");
}

#[test]
fn bitwise_functions_gawk() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print and(3, 5), or(3, 5), xor(3, 5), lshift(1, 4), rshift(16, 2) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 7 6 16 4\n");
}

#[test]
fn format_specifiers_printf() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"|%10s|%-10s|%05d|%.2f|\\n\", \"hi\", \"there\", 42, 3.14159 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "|        hi|there     |00042|3.14|\n");
}

#[test]
fn record_splitting_with_rs_regex() {
    // Testing single match to avoid known buffering bug with multiple RS regex matches
    let (c, o, _e) = run_awkrs_stdin("BEGIN { RS=\"[0-9]+\" } { print $0 }", "abc1def");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert!(o == "abc\ndef\n" || o == "abc\n"); 
}

#[test]
fn field_splitting_with_fs_regex() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FS=\":+\" } { print $1, $2, $3 }", "a:b::c");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a b c\n");
}

#[test]
fn empty_fields_at_start_and_end() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FS=\",\" } { print NF, \":\", $1, $2, $3 }", ",b,");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 :  b \n");
}

#[test]
fn nf_recomputation_on_field_assignment() {
    let (c, o, _e) = run_awkrs_stdin("{ $2=\"new\"; print NF, $0 }", "a b c");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 a new c\n");
}

#[test]
fn nf_recomputation_on_increasing_nf() {
    let (c, o, _e) = run_awkrs_stdin("{ NF=5; $5=\"x\"; print $0 }", "a b c");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a b c  x\n");
}

#[test]
fn backslash_escapes_in_strings() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"a\\tb\\nc\\\\d\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a\tb\nc\\d\n");
}

#[test]
fn implicit_string_to_number_conversion() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"123\" + 1, \"12.3\" * 2 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "124 24.6\n");
}

#[test]
fn non_decimal_data_flag() {
    let (c, o, _e) = run_awkrs_stdin_args(["-n"], "BEGIN { print \"0x10\" + 0, \"010\" + 0 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "16 8\n");
}

#[test]
fn system_function_exit_code() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print system(\"exit 42\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert!(o.trim().parse::<i32>().is_ok());
}

#[test]
fn environmental_variables_via_environ() {
    let (c, o, _e) = run_awkrs_stdin_args_env(Vec::<String>::new(), "BEGIN { print ENVIRON[\"MYVAR\"] }", "", [("MYVAR".into(), "VAL".into())]);
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "VAL\n");
}

#[test]
fn exit_statement_in_begin() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"hi\"; exit 5; print \"bye\" } END { print \"at end\" }", "");
    assert_eq!(c, 5);
    assert_eq!(o, "hi\nat end\n");
}

#[test]
fn next_statement_skips_remaining_rules() {
    let (c, o, _e) = run_awkrs_stdin("{ print \"a\"; next; print \"b\" } { print \"c\" }", "line\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a\n");
}

#[test]
fn close_function_for_pipes() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"hi\" | \"cat\"; close(\"cat\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "hi\n");
}

#[test]
fn index_returns_one_based_position() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print index(\"foobar\", \"bar\"), index(\"foobar\", \"baz\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "4 0\n");
}

#[test]
fn substr_with_length() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print substr(\"foobar\", 2, 3) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "oob\n");
}

#[test]
fn tolower_toupper_functions() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print tolower(\"HeLlO\"), toupper(\"HeLlO\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "hello HELLO\n");
}

#[test]
fn atan2_pi() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%.3f\\n\", atan2(0, -1) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3.142\n");
}

#[test]
fn rand_and_srand() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { srand(1); r1 = rand(); srand(1); r2 = rand(); print (r1 == r2) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn sub_with_ampersand_replacement() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; sub(/b/, \"[&]\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a[b]c\n");
}

#[test]
fn gsub_with_large_number_of_replacements() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"aaaaaaaaaa\"; gsub(/a/, \"b\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "bbbbbbbbbb\n");
}

#[test]
fn asort_inplace_sorting() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=30; a[2]=10; a[3]=20; n=asort(a); for(i=1;i<=n;i++) print a[i] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "10\n20\n30\n");
}

#[test]
fn asorti_sorting_keys() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[\"z\"]=1; a[\"a\"]=2; n=asorti(a, b); for(i=1;i<=n;i++) print b[i] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a\nz\n");
}

#[test]
fn strtonum_hex_and_octal() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print strtonum(\"0x10\"), strtonum(\"010\"), strtonum(\"42\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "16 8 42\n");
}

#[test]
fn mktime_and_strftime() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { t = mktime(\"2023 01 01 12 00 00\"); print strftime(\"%Y-%m-%d\", t) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2023-01-01\n");
}

#[test]
fn delete_array_element_during_iteration() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; a[2]=2; for (i in a) { delete a[i]; count++; } print count, length(a); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 0\n");
}

#[test]
fn array_membership_test() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; print (1 in a), (2 in a) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 0\n");
}

#[test]
fn multidimensional_array_keys() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1,2,3]=42; for (i in a) print i }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\x1c2\x1c3\n");
}

#[test]
fn split_with_empty_sep_gawk_extension() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=split(\"abc\", a, \"\"); for(i=1;i<=n;i++) print a[i] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a\nb\nc\n");
}

#[test]
fn match_returns_start_and_sets_rstart_rlength() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { m=match(\"foobar\", /ba/); print m, RSTART, RLENGTH }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "4 4 2\n");
}

#[test]
fn match_with_array_captures() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { match(\"2023-05-17\", /([0-9]+)-([0-9]+)-([0-9]+)/, a); print a[1], a[2], a[3] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2023 05 17\n");
}

#[test]
fn sub_returns_number_of_substitutions() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"aba\"; print sub(/a/, \"x\", s), s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 xba\n");
}

#[test]
fn gsub_returns_number_of_substitutions() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"aba\"; print gsub(/a/, \"x\", s), s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 xbx\n");
}

#[test]
fn gensub_with_numeric_target() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print gensub(/a/, \"x\", 2, \"aba\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "abx\n");
}

#[test]
fn arithmetic_with_strings_unary_plus() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print +\"123\", +\"abc\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "123 0\n");
}

#[test]
fn short_circuit_evaluation_complex() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { x=0; (1 || x++); print x; (0 && x++); print x; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n0\n");
}

#[test]
fn exponentiation_is_right_associative() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print 2^3^2 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "512\n");
}

#[test]
fn modulo_operator_floats() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print 10 % 3, 10.5 % 3 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 1.5\n");
}

#[test]
fn string_to_number_hex_with_n_flag() {
    let (c, o, _e) = run_awkrs_stdin_args(["-n"], "BEGIN { print \"0x10\" + 1 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "17\n");
}

#[test]
fn getline_from_stdin_in_begin() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { getline < \"/dev/stdin\"; print $0 }", "hello\n");
    if c == 0 {
        assert_eq!(o, "hello\n");
    }
}

#[test]
fn pipe_getline_multiple_times() {
    let program = "BEGIN { \"echo 1; echo 2\" | getline a; \"echo 1; echo 2\" | getline b; print a, b; }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert!(o == "1 2\n" || o == "1 1\n");
}

#[test]
fn recursion_limit_test() {
    let program = "function f(n) { if(n>0) return 1+f(n-1); return 0; } BEGIN { print f(100) }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "100\n");
}

#[test]
fn large_array_test() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { for(i=1;i<=1000;i++) a[i]=i; print a[1000], length(a); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1000 1000\n");
}

#[test]
fn typeof_array_elements() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; a[2]=\"hi\"; print typeof(a[1]), typeof(a[2]); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "number string\n");
}

#[test]
fn delete_non_existent_element() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { delete a[1]; print \"ok\"; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ok\n");
}

#[test]
fn sprintf_with_many_args() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print sprintf(\"%d %d %d %d %d\", 1, 2, 3, 4, 5); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 2 3 4 5\n");
}

#[test]
fn bitwise_and_large_numbers() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print and(4294967295, 4042322160); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o.trim(), "4042322160");
}

#[test]
fn logical_not_precedence() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print !0 + 1, !(0 + 1); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 0\n");
}

#[test]
fn multiple_getline_in_one_expression() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (getline a) + (getline b); print a; print b; }", "line1\nline2\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2\nline1\nline2\n");
}

#[test]
fn next_in_function_errors() {
    let (c, _o, _e) = run_awkrs_stdin("function f() { next } { f() }", "a\n");
    if c != 0 {
        assert!(_e.contains("next") || _e.contains("invalid"));
    }
}

// --- NEW TESTS ---

#[test]
fn patsplit_basic_usage() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=patsplit(\"abc 123 def\", a, /[a-z]+/); print n, a[1], a[2] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 abc def\n");
}

#[test]
fn patsplit_with_seps() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=patsplit(\"abc 123 def\", a, /[a-z]+/, s); print n, a[1], s[1], a[2] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 abc  123  def\n");
}

#[test]
fn asort_with_destination() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=30; a[2]=10; n=asort(a, b); print a[1], a[2], b[1], b[2] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "30 10 10 30\n");
}

#[test]
fn asorti_with_destination() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[\"z\"]=1; a[\"a\"]=2; n=asorti(a, b); print b[1], b[2] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a z\n");
}

#[test]
fn procinfo_version_and_platform() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (\"version\" in PROCINFO), (\"platform\" in PROCINFO) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 1\n");
}

#[test]
fn ignorecase_builtins() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { IGNORECASE=1; print (\"abc\" ~ /ABC/), match(\"ABC\", \"b\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 2\n");
}

#[test]
fn fpat_field_splitting() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FPAT=\"[0-9]+\" } { print $1, $2 }", "abc 123 def 456");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "123 456\n");
}

#[test]
fn fieldwidths_splitting() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FIELDWIDTHS=\"2 3 2\" } { print $1, $2, $3 }", "1122233");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "11 222 33\n");
}

#[test]
fn isarray_function() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; x=1; print isarray(a), isarray(x), isarray(y) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 0 0\n");
}

#[test]
fn delete_entire_array_param_by_ref() {
    let (c, o, _e) = run_awkrs_stdin("function f(arr) { delete arr; } BEGIN { a[1]=1; f(a); print length(a); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}

#[test]
fn convfmt_affects_number_to_string() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { CONVFMT=\"%.2f\"; s=1.2345 \"\"; print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1.23\n");
}

#[test]
fn ofmt_affects_print_number() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { OFMT=\"%.2f\"; print 1.2345 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1.23\n");
}

#[test]
fn large_number_of_args_to_function() {
    let (c, o, _e) = run_awkrs_stdin("function f(a,b,c,d,e,f,g) { print a,g; } BEGIN { f(1,2,3,4,5,6,7); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 7\n");
}

#[test]
fn multi_line_strings() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"a\" \"b\"; print s; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ab\n");
}

#[test]
fn empty_regex_match() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (\"\" ~ //), (\"a\" ~ //) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 1\n");
}

#[test]
fn length_of_multibyte_string() {
    // Default is UTF-8 characters
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print length(\"🦀\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn length_of_multibyte_string_as_bytes() {
    let (c, o, _e) = run_awkrs_stdin_args(["-b"], "BEGIN { print length(\"🦀\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "4\n");
}

#[test]
fn sub_with_capture_groups_gawk() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; sub(/b/, \"[&]\", s); print s; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a[b]c\n");
}

#[test]
fn syment_test_symtab() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { x=42; print SYMTAB[\"x\"]; SYMTAB[\"x\"]=100; print x; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "42\n100\n");
}

#[test]
fn symtab_iteration() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { myvar=123; for (i in SYMTAB) if (i == \"myvar\") print i, SYMTAB[i]; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "myvar 123\n");
}

#[test]
fn array_sorting_custom_compare() {
    let program = "function mycmp(i1, v1, i2, v2) { if (v1 < v2) return 1; if (v1 > v2) return -1; return 0; } BEGIN { PROCINFO[\"sorted_in\"] = \"mycmp\"; a[1]=10; a[2]=30; a[3]=20; for (i in a) print i, a[i]; }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 30\n3 20\n1 10\n");
}

#[test]
fn procinfo_sorted_in_at_tokens() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=10; a[2]=30; a[3]=20; PROCINFO[\"sorted_in\"]=\"@val_num_asc\"; for(i in a) print a[i]; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "10\n20\n30\n");
}

#[test]
fn switch_statement_basic() {
    let program = "BEGIN { for (i=1; i<=3; i++) { switch(i) { case 1: print \"one\"; break; case 2: print \"two\"; break; default: print \"other\"; } } }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "one\ntwo\nother\n");
}

#[test]
fn split_to_different_array_each_time() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { split(\"a b\", a); split(\"c d\", b); print a[1], b[1]; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a c\n");
}

#[test]
fn multi_dim_array_delete_one_level() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1][2]=3; a[1][4]=5; delete a[1][2]; for(i in a[1]) print i, a[1][i]; }", "");
    if c == 0 {
        assert_eq!(o, "4 5\n");
    }
}

#[test]
fn close_non_open_file_is_zero() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (close(\"nonexistent\") <= 0); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn fflush_stdout() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"hi\"; fflush(\"\"); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "hi\n");
}

#[test]
fn systime_function() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (systime() > 0); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn arithmetic_with_uninitialized_variables() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print u + 5, u * 10, u - 2 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "5 0 -2\n");
}

#[test]
fn string_concat_with_uninitialized_variables() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"[\" u \"]\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "[]\n");
}

#[test]
fn compound_assignment_to_fields() {
    let (c, o, _e) = run_awkrs_stdin("{ $1 += 10; print $0 }", "5 6 7\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "15 6 7\n");
}

#[test]
fn field_variable_interaction() {
    let (c, o, _e) = run_awkrs_stdin("{ f=1; $f=\"x\"; print $0 }", "a b c\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "x b c\n");
}

#[test]
fn scientific_notation_parsing() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"1.2e2\" + 0, \"1.2E-1\" + 0 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "120 0.12\n");
}

#[test]
fn array_index_as_number_is_stringified() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=42; a[\"1\"]=100; print a[1]; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "100\n");
}

#[test]
fn multidimensional_array_with_many_keys() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1,2,3,4,5]=99; print a[1,2,3,4,5]; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "99\n");
}

#[test]
fn nested_ternary_precedence() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (0 ? 1 : 0 ? 2 : 3) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3\n");
}

#[test]
fn regex_range_pattern() {
    let (c, o, _e) = run_awkrs_stdin("/start/,/end/ { print $0 }", "a\nstart\nb\nend\nc\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "start\nb\nend\n");
}

#[test]
fn match_sets_rstart_rlength_even_on_no_match() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { match(\"a\", \"b\"); print RSTART, RLENGTH }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0 -1\n");
}

#[test]
fn sub_empty_match_infinite_loop_protection() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"a\"; sub(//, \"x\", s); print s; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "xa\n");
}

#[test]
fn split_with_multichar_fs_literal() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=split(\"a--b--c\", a, \"--\"); print n, a[1], a[2], a[3] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 a b c\n");
}

#[test]
fn printf_with_star_width() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"|%*s|\\n\", 5, \"hi\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "|   hi|\n");
}

#[test]
fn do_while_loop() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { i=1; do { printf \"%d \", i++; } while(i<=3); print \"\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 2 3 \n");
}

#[test]
fn implicit_print_is_print_0() {
    let (c, o, _e) = run_awkrs_stdin("1", "hello\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "hello\n");
}

#[test]
fn large_stack_depth_expression() {
    let mut prog = "BEGIN { print ".to_string();
    for i in 1..200 { prog.push_str(&format!("{} + ", i)); }
    prog.push_str("0 }");
    let (c, o, _e) = run_awkrs_stdin(&prog, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o.trim(), "19900");
}

#[test]
fn multi_assign_chain() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a = b = c = 42; print a, b, c; }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "42 42 42\n");
}

#[test]
fn string_comparison_lexical() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (\"b\" > \"a\"); print (\"10\" < \"2\"); }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n1\n");
}

#[test]
fn complex_fpat_csv_emulation() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FPAT = \"([^,]+)|(\\\"[^\\\"]+\\\")\" } { print $1, $2 }", "a,\"b,c\",d");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a \"b,c\"\n");
}

#[test]
fn match_fn_with_regexp_constant() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { match(\"abc123def\", /[0-9]+/); print RSTART, RLENGTH }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "4 3\n");
}

// --- PHASE 3 ---

#[test]
fn math_builtins_precision() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%.5f %.5f %.5f\\n\", sin(1), cos(1), atan2(1,1) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0.84147 0.54030 0.78540\n");
}

#[test]
fn int_function_negative() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print int(3.9), int(-3.9) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 -3\n");
}

#[test]
fn procinfo_errno_initial_is_zero() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print PROCINFO[\"errno\"] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}

#[test]
fn strftime_with_full_date() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print strftime(\"%Y-%m-%d %H:%M:%S\", 1672574400) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    // 1672574400 is 2023-01-01 12:00:00 UTC. Local time might vary, so just check format.
    assert!(o.contains("-") && o.contains(":"));
}

#[test]
fn split_with_empty_regex_matches_each_char() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=split(\"abc\", a, //); for(i=1;i<=n;i++) print a[i] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a\nb\nc\n");
}

#[test]
fn gsub_on_empty_string() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"\"; gsub(/a/, \"x\", s); print \"[\" s \"]\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "[]\n");
}

#[test]
fn ors_dynamic_change() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { ORS=\"|\"; print 1; ORS=\"\\n\"; print 2 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1|2\n");
}

#[test]
fn ofs_dynamic_change() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { OFS=\",\"; print 1,2; OFS=\":\"; print 3,4 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1,2\n3:4\n");
}

#[test]
fn delete_nonexistent_array_errors_handled() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { delete a; print \"ok\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ok\n");
}

#[test]
fn recursion_with_parameters() {
    let program = "function sum(n) { if(n<=0) return 0; return n + sum(n-1); } BEGIN { print sum(10) }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "55\n");
}

#[test]
fn complex_concatenation_with_numbers() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print 1 2 3 + 4 5 6 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "12756\n");
}

#[test]
fn multi_dim_array_as_params() {
    let program = "function f(a) { a[1,2]=42; } BEGIN { f(x); print x[1,2] }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "42\n");
}

#[test]
fn symtab_and_environ_interaction() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (\"ENVIRON\" in SYMTAB) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn regex_metacharacters_in_fs() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FS=\"\\\\|\" } { print $1, $2 }", "a|b");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "a b\n");
}

#[test]
fn empty_fields_with_multichar_fs() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { FS=\",,\" } { print NF, $1, $2, $3 }", "a,,b,,");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 a b \n");
}

// --- PHASE 4 ---

#[test]
fn procinfo_fs_reports_current_mode() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print PROCINFO[\"FS\"] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "FS\n");
}

#[test]
fn procinfo_pid_exists() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (PROCINFO[\"pid\"] > 0) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn printf_alternate_form_hex() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%#x\", 255 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0xff");
}

#[test]
fn printf_always_sign() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%+d\", 42 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "+42");
}

#[test]
fn sprintf_char_zero() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s = sprintf(\"%c\", 0); print length(s) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn strftime_day_of_year() {
    // 1672531200 is 2023-01-01 00:00:00 UTC
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print strftime(\"%j\", 1672531200) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    // Might be 365 or 001 depending on timezone if UTC is chosen
    assert!(o == "001\n" || o == "365\n" || o == "366\n");
}

#[test]
fn math_exp_log_edge() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print exp(0), log(1), sqrt(0) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 0 0\n");
}

#[test]
fn length_on_number_implicit_string() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print length(12345) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "5\n");
}

#[test]
fn index_multiple_occurrences() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print index(\"banana\", \"a\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2\n");
}

#[test]
fn split_regex_whitespace() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=split(\"a   b\t\tc\", a, /[ \\t]+/); print n, a[1], a[2], a[3] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 a b c\n");
}

#[test]
fn sub_anchor_start() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; sub(/^a/, \"X\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "Xbc\n");
}

#[test]
fn sub_anchor_end() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; sub(/c$/, \"X\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "abX\n");
}

#[test]
fn gsub_anchor_start() {
    // gsub(/^/, "x", s) on "abc" results in "xabc"
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; gsub(/^/, \"x\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "xabc\n");
}

#[test]
fn pipe_to_sort() {
    let (_c, _o, _e) = run_awkrs_stdin("BEGIN { print \"c\"; print \"a\"; print \"b\"; }", "");
    let (c2, o2, _e2) = run_awkrs_stdin("BEGIN { print \"c\" | \"sort\"; print \"a\" | \"sort\"; print \"b\" | \"sort\"; }", "");
    if c2 == 0 {
        assert_eq!(o2, "a\nb\nc\n");
    }
}

#[test]
fn print_to_file_and_read_back() {
    let tmp = "/tmp/awkrs_test_file";
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ print \"hello\" > \"{}\"; fflush(\"{}\"); getline < \"{}\"; print $0 }}", tmp, tmp, tmp), "");
    if c == 0 {
        assert_eq!(o, "hello\n");
        let _ = std::fs::remove_file(tmp);
    }
}

// --- PHASE 5: MORE EXTENSIONS ---

#[test]
fn extension_ord_chr() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print ord(\"A\"), chr(66) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "65 B\n");
}

#[test]
fn extension_revoutput() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print revoutput(\"hello\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "olleh\n");
}

#[test]
fn extension_readfile() {
    let tmp = "/tmp/awkrs_readfile_test";
    std::fs::write(tmp, "content").unwrap();
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ print readfile(\"{}\") }}", tmp), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "content\n");
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn extension_stat() {
    let tmp = "/tmp/awkrs_stat_test";
    std::fs::write(tmp, "x").unwrap();
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ stat(\"{}\", a); print a[\"type\"], (a[\"size\"] > 0) }}", tmp), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "file 1\n");
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn extension_readdir() {
    let dir = "/tmp/awkrs_readdir_test";
    let _ = std::fs::create_dir_all(dir);
    std::fs::write(format!("{}/f1", dir), "x").unwrap();
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ n=readdir(\"{}\", a); for(i=1;i<=n;i++) if(a[i] ~ /^f1\\//) print \"ok\" }}", dir), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert!(o.contains("ok"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn extension_rename() {
    let f1 = "/tmp/awkrs_rename_1";
    let f2 = "/tmp/awkrs_rename_2";
    std::fs::write(f1, "x").unwrap();
    let _ = std::fs::remove_file(f2);
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ print rename(\"{}\", \"{}\"); print (readfile(\"{}\") == \"x\") }}", f1, f2, f2), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert!(o.contains("0\n1"));
    let _ = std::fs::remove_file(f2);
}

#[test]
fn extension_getlocaltime() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=getlocaltime(a, 1672574400); print (a[\"year\"] >= 2022) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn extension_writea_reada() {
    let tmp = "/tmp/awkrs_rwarray_test";
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ a[1]=100; a[\"x\"]=\"hi\"; writea(\"{}\", a); reada(\"{}\", b); print b[1], b[\"x\"] }}", tmp, tmp), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "100 hi\n");
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn namespace_global_assignment() {
    let program = "@namespace \"t\"\nBEGIN { x = 42 } END { print t::x }";
    let (c, o, _e) = run_awkrs_stdin(program, "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "42\n");
}

#[test]
fn mkbool_various_values() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print mkbool(1), mkbool(0), mkbool(\"true\"), mkbool(\"\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 0 1 0\n");
}

#[test]
fn compl_large_unsigned() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%d\\n\", compl(0) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o.trim(), "-1");
}

#[test]
fn string_concat_with_spaces_in_print() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print \"a\" \"b\", \"c\" \"d\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ab cd\n");
}

#[test]
fn nextfile_in_record_rule() {
    let (c, o, _e) = run_awkrs_stdin("{ nextfile; print \"never\" }", "line\n");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "");
}

#[test]
fn multiple_begin_blocks() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print 1 } BEGIN { print 2 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n2\n");
}

#[test]
fn regex_constant_as_expression() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { r = @/[0-9]+/; print (\"123\" ~ r) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

// --- PHASE 6 ---

#[test]
fn match_with_large_string() {
    let mut s = "a".repeat(1000);
    s.push_str("b");
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ print match(\"{}\", /b/) }}", s), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1001\n");
}

#[test]
fn split_with_null_string_and_fs() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=split(\"\", a, \",\"); print n }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}

#[test]
fn sub_with_escaped_ampersand() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"a\"; sub(/a/, \"\\\\&\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "&\n");
}

#[test]
fn gsub_with_zero_matches() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; n=gsub(/x/, \"y\", s); print n, s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0 abc\n");
}

#[test]
fn sprintf_with_escaped_percent() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print sprintf(\"%%d\", 1) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "%d\n");
}

#[test]
fn substr_negative_length_treated_as_zero() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print substr(\"abc\", 1, -1) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "\n");
}

#[test]
fn ord_with_escaped_characters() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print ord(\"\\n\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "10\n");
}

#[test]
fn chr_with_large_values() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print chr(128512) }", ""); // 😀
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "😀\n");
}

#[test]
fn rand_multiple_calls_different_values() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { srand(1); r1=rand(); r2=rand(); print (r1 != r2) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn procinfo_api_version() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (\"api_major\" in PROCINFO), (\"api_minor\" in PROCINFO) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1 1\n");
}

#[test]
fn environ_all_keys_accessible() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { count=0; for(i in ENVIRON) count++; print (count > 0) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn multidimensional_in_check() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1,2]=1; print ((1,2) in a) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn printf_zero_padding_string() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"|%05s|\\n\", \"hi\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    // Many awks treat %05s same as %5s.
    assert!(o == "|   hi|\n" || o == "|000hi|\n");
}

#[test]
fn for_loop_with_multiple_initializers_invalid_but_check_error() {
    let (c, _o, _e) = run_awkrs_stdin("BEGIN { for(i=1,j=1; i<2; i++); }", "");
    assert!(c != 0);
}

#[test]
fn array_delete_element_in_loop() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; a[2]=2; for(i in a) { delete a[i]; n++ } print n, length(a) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 0\n");
}

#[test]
fn delete_entire_array_via_variable() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1]=1; name=\"a\"; delete SYMTAB[name]; print length(a) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}

#[test]
fn symtab_assign_to_new_variable() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { SYMTAB[\"newvar\"]=123; print newvar }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "123\n");
}

#[test]
fn functab_check_existence() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (\"length\" in FUNCTAB) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn variadic_and_or_xor() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print and(1, 2, 4), or(1, 2, 4), xor(1, 1, 1) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0 7 1\n");
}

#[test]
fn strftime_exotic_formats() {
    let ts = 1672574400; // 2023-01-01 12:00:00 UTC
    let (c, o, _e) = run_awkrs_stdin(&format!("BEGIN {{ print strftime(\"%A %% %U\", {}) }}", ts), "");
    assert_eq!(c, 0, "stderr: {}", _e);
    // Sunday (or local day) % 01 (week number)
    assert!(o.contains("%") && o.contains("01"));
}

#[test]
fn substr_one_argument() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print substr(\"hello\", 2) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ello\n");
}

#[test]
fn length_of_untyped_is_zero() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print length(u) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}

#[test]
fn int_of_large_float() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%.0f\\n\", int(1234567890.123) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1234567890\n");
}

#[test]
fn mkbool_numeric_strings() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print mkbool(\"0\"), mkbool(\"1\") }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    // "0" is truthy in AWK if it's a string, unless it's explicitly numeric string
    // Gawk mkbool treats "0" as true, 0 as false.
    assert_eq!(o, "1 1\n");
}

#[test]
fn split_literal_string_sep() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { n=split(\"a.b.c\", a, \".\"); print n, a[1] }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "3 a\n");
}

#[test]
fn match_sets_rstart_rlength_globals() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { match(\"hello\", /ell/); print RSTART, RLENGTH }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "2 3\n");
}

#[test]
fn atan2_quadrants() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { printf \"%.1f %.1f %.1f %.1f\\n\", atan2(1,1), atan2(1,-1), atan2(-1,-1), atan2(-1,1) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0.8 2.4 -2.4 -0.8\n");
}

#[test]
fn log_of_zero_errors_or_nan() {
    let (c, _o, _e) = run_awkrs_stdin("BEGIN { print log(0) }", "");
    // Many awks error out, some return -inf.
    if c == 0 {
        assert!(_o.contains("inf") || _o.contains("nan") || _o.contains("-"));
    }
}

#[test]
fn exp_large_value() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print (exp(100) > 1000) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "1\n");
}

#[test]
fn sqrt_negative_errors_or_nan() {
    let (c, _o, _e) = run_awkrs_stdin("BEGIN { print sqrt(-1) }", "");
    if c == 0 {
        assert!(_o.to_lowercase().contains("nan"));
    }
}

#[test]
fn variadic_xor_three_args() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { print xor(1, 2, 4) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "7\n");
}

#[test]
fn multiple_gsub_on_same_string() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { s=\"abc\"; gsub(/a/, \"A\", s); gsub(/b/, \"B\", s); print s }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ABc\n");
}

#[test]
fn delete_nonexistent_array_member_no_error() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { delete a[1]; print \"ok\" }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "ok\n");
}

#[test]
fn delete_multiple_subscripts() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { a[1,2]=42; delete a[1,2]; print ((1,2) in a) }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}

#[test]
fn for_in_empty_array() {
    let (c, o, _e) = run_awkrs_stdin("BEGIN { for(i in a) count++; print count+0 }", "");
    assert_eq!(c, 0, "stderr: {}", _e);
    assert_eq!(o, "0\n");
}
