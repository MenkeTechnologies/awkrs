//! Cross-feature integration tests: interactions between features that each have
//! their own focused suite already, but whose *combinations* are easy to regress.
//! These exist to catch bugs where two features pass in isolation but break together
//! (e.g. CSV mode + `nextfile`, paragraph `RS=""` + `getline`, `FIELDWIDTHS` + NF reassignment,
//! `-M` bignum + sprintf, `IGNORECASE` + regex split, asort + SUBSEP keys, ...).

mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_args};
use std::process::{Command, Stdio};

// ── CSV mode interactions ───────────────────────────────────────────────────

#[test]
fn csv_mode_keeps_quoted_comma_field_intact_across_records() {
    // Multi-record CSV with embedded commas — earlier records must not poison NF for later ones.
    let (c, o, _) = run_awkrs_stdin_args(
        ["-k"],
        r#"{ print NR, NF, $2 }"#,
        "a,\"b,c\",d\np,\"q,r,s\",t\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1 3 b,c\n2 3 q,r,s\n");
}

#[test]
fn csv_mode_with_escaped_quote_inside_field_keeps_quote() {
    // gawk-style `""` escapes a literal quote inside the quoted field.
    let (c, o, _) = run_awkrs_stdin_args(["-k"], r#"{ print $2 }"#, "x,\"he said \"\"hi\"\"\",y\n");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "he said \"hi\"");
}

// ── FIELDWIDTHS edge cases ──────────────────────────────────────────────────

#[test]
fn fieldwidths_with_input_shorter_than_total_width_truncates_last() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { FIELDWIDTHS = "2 2 2" } { print NF, "[" $1 "][" $2 "][" $3 "]" }"#,
        "abcde\n",
    );
    assert_eq!(c, 0);
    // 5 chars consumed across widths 2/2/2 — third field has only 1 char available.
    assert_eq!(o.trim(), "3 [ab][cd][e]");
}

#[test]
fn fieldwidths_reverts_when_set_to_empty_string() {
    // After FIELDWIDTHS goes back to "", default whitespace split resumes.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { FIELDWIDTHS = "3 3" }
           NR == 1 { print "fw:", NF, $1, $2 }
           NR == 2 { FIELDWIDTHS = ""; }
           NR == 3 { print "ws:", NF, $1, $2 }"#,
        "ABCDEF\nxx yy\nfoo bar\n",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[0], "fw: 2 ABC DEF");
    assert_eq!(lines[1], "ws: 2 foo bar");
}

// ── Paragraph mode (RS="") interactions ─────────────────────────────────────

#[test]
fn paragraph_mode_strips_leading_and_trailing_blank_runs() {
    // Multiple leading + trailing blank lines collapse; paragraphs are records.
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { RS = "" } { print NR ":" $0 ":" }"#,
        "\n\na\nb\n\n\nc\nd\n\n\n",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines, vec!["1:a", "b:", "2:c", "d:"]);
}

#[test]
fn paragraph_mode_default_fs_splits_on_newline_or_whitespace() {
    // In RS="" mode, FS still defaults to whitespace (incl. newlines).
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { RS = "" } { print NR, NF }"#, "a b\nc\n\nd e f\n");
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines, vec!["1 3", "2 3"]);
}

// ── RS regex + RT ──────────────────────────────────────────────────────────

#[test]
fn rs_regex_records_emit_each_separator_via_rt() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { RS = "X+|Y+" } { print NR, "[" $0 "]", "<" RT ">" }"#,
        "aXXbYYYcXd\n",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[0], "1 [a] <XX>");
    assert_eq!(lines[1], "2 [b] <YYY>");
    assert_eq!(lines[2], "3 [c] <X>");
    assert!(lines[3].starts_with("4 [d"));
}

// ── Field rebuild semantics ────────────────────────────────────────────────

#[test]
fn assigning_high_field_inserts_empty_intermediate_fields() {
    // `$5 = "e"` on a record with NF=1 must pad $2..$4 with empty strings.
    let (c, o, _) = run_awkrs_stdin(r#"{ $5 = "e"; print NF, "[" $0 "]" }"#, "x\n");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "5 [x    e]");
}

#[test]
fn nf_increase_pads_fields_and_dollar_zero() {
    let (c, o, _) = run_awkrs_stdin(
        r#"{ NF = 5; print "[" $0 "]"; for (i = 1; i <= NF; i++) printf "<%s>", $i; print "" }"#,
        "a b c\n",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[0], "[a b c  ]");
    assert_eq!(lines[1], "<a><b><c><><>");
}

#[test]
fn ofs_rebuild_triggered_by_self_assignment_of_field() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { OFS = "|" } { $1 = $1; print }"#, "a b c\n");
    assert_eq!(c, 0);
    assert_eq!(o, "a|b|c\n");
}

// ── String/number coercion edges ───────────────────────────────────────────

#[test]
fn leading_zeros_in_input_string_treated_as_decimal_in_default_mode() {
    // Without -n, "00042" coerces to the number 42, not 34 (octal).
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { x = "00042" + 0; printf "%d\n", x }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "42");
}

#[test]
fn non_decimal_data_flag_parses_octal_literal_in_string_coercion() {
    let (c, o, _) = run_awkrs_stdin_args(["-n"], r#"BEGIN { print "017" + 0 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "15");
}

#[test]
fn comparison_of_two_string_literals_is_lexical_even_when_both_look_numeric() {
    // String *literals* "10" and "9" compare lexically: "10" < "9".
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { if ("10" > "9") print "GT"; else print "LT" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "LT");
}

#[test]
fn comparison_of_numeric_strnum_input_uses_numeric_order() {
    // Input fields ARE strnum; "10" > "9" must hold numerically here.
    let (c, o, _) = run_awkrs_stdin(r#"{ if ($1 > $2) print "GT"; else print "LT" }"#, "10 9\n");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "GT");
}

// ── Builtin string functions ───────────────────────────────────────────────

#[test]
fn substr_with_negative_start_clamps_to_one() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "[" substr("hello", -1, 3) "]" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "[hel]");
}

#[test]
fn substr_with_zero_length_returns_empty_string() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "[" substr("hello", 2, 0) "]" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "[]");
}

#[test]
fn substr_negative_length_returns_empty_string() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "[" substr("hello", 2, -3) "]" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "[]");
}

#[test]
fn substr_start_past_end_returns_empty_string() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "[" substr("hello", 99) "]" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "[]");
}

#[test]
fn gensub_global_with_backreferences_and_ampersand() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print gensub(/([a-z]+)@([a-z]+)/, "\\2 [at] \\1", "g", "alice@example bob@test") }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "example [at] alice test [at] bob");
}

#[test]
fn split_with_literal_multichar_separator_treats_as_string_not_regex() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { n = split("a::b::c", a, "::"); print n, a[1], a[2], a[3] }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "3 a b c");
}

#[test]
fn split_empty_string_input_returns_zero_and_empties_array() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { a["dummy"] = 1; n = split("", a, ","); print n, length(a) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "0 0");
}

#[test]
fn match_three_arg_populates_capture_subarray_with_start_and_length() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
            if (match("hello world", /w(or)ld/, m))
                printf "%s %s %d %d\n", m[0], m[1], m[1,"start"], m[1,"length"]
        }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "world or 8 2");
}

// ── User functions ─────────────────────────────────────────────────────────

#[test]
fn user_function_extra_params_act_as_locals_isolated_per_call() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function f(x,    tmp) { tmp = x * 2; return tmp }
           BEGIN {
             tmp = 99   # global tmp must NOT change
             print f(5), f(7), tmp
           }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "10 14 99");
}

#[test]
fn user_function_recursion_through_one_hundred_frames() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function down(n) { return n == 0 ? 0 : down(n - 1) + 1 }
           BEGIN { print down(100) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "100");
}

#[test]
fn user_function_array_parameter_mutates_caller_array() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function fill(a, n,    i) { for (i = 1; i <= n; i++) a[i] = i * i }
           BEGIN { fill(b, 4); for (i = 1; i <= 4; i++) printf "%s ", b[i]; print "" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1 4 9 16");
}

// ── Multidimensional arrays via SUBSEP ─────────────────────────────────────

#[test]
fn multidim_subscript_roundtrip_through_subsep_split() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             a[1,2,3] = "v"
             for (k in a) {
                 n = split(k, p, SUBSEP)
                 print n, p[1], p[2], p[3], a[k]
             }
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "3 1 2 3 v");
}

// ── Sorted iteration via PROCINFO["sorted_in"] ─────────────────────────────

#[test]
fn procinfo_sorted_in_ind_str_asc_orders_iteration() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             PROCINFO["sorted_in"] = "@ind_str_asc"
             a["c"] = 3; a["a"] = 1; a["b"] = 2
             for (k in a) printf "%s=%s ", k, a[k]
             print ""
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "a=1 b=2 c=3");
}

#[test]
fn procinfo_sorted_in_val_num_desc_orders_by_value() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             PROCINFO["sorted_in"] = "@val_num_desc"
             a["x"] = 10; a["y"] = 30; a["z"] = 20
             for (k in a) printf "%s ", k
             print ""
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "y z x");
}

// ── nextfile + ENDFILE across two real files ───────────────────────────────

#[test]
fn nextfile_jumps_remaining_records_and_endfile_runs_for_skipped_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f1 = dir.join(format!("awkrs_nf1_{id}.txt"));
    let f2 = dir.join(format!("awkrs_nf2_{id}.txt"));
    std::fs::write(&f1, "a\nb\nc\n").unwrap();
    std::fs::write(&f2, "d\ne\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(r#"FNR == 2 { nextfile } { print FNR, $0 } ENDFILE { print "END" }"#)
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
    let lines: Vec<&str> = stdout.lines().collect();
    // f1: prints "1 a", hits nextfile on FNR==2, runs ENDFILE
    // f2: prints "1 d", hits nextfile on FNR==2, runs ENDFILE
    assert_eq!(lines, vec!["1 a", "END", "1 d", "END"]);
}

// ── Pipe to a long-lived sort process ──────────────────────────────────────

#[test]
fn print_into_sort_pipe_sorts_three_lines() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             print "delta" | "sort"
             print "alpha" | "sort"
             print "charlie" | "sort"
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "alpha\ncharlie\ndelta\n");
}

// ── getline forms ──────────────────────────────────────────────────────────

#[test]
fn getline_from_file_into_var_does_not_alter_dollar_zero() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f = dir.join(format!("awkrs_glv_{id}.txt"));
    std::fs::write(&f, "from-file\n").unwrap();
    let prog = format!(
        r#"{{
              (getline line < "{path}")
              print "0=" $0 " line=" line
           }}"#,
        path = f.display()
    );
    let (c, o, _) = run_awkrs_stdin(&prog, "from-stdin\n");
    let _ = std::fs::remove_file(&f);
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "0=from-stdin line=from-file");
}

#[test]
fn getline_command_pipe_yields_command_stdout_lines() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             cmd = "printf 'one\ntwo\nthree\n'"
             while ((cmd | getline line) > 0) print "L:" line
             close(cmd)
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "L:one\nL:two\nL:three\n");
}

// ── ENVIRON ────────────────────────────────────────────────────────────────

#[test]
fn environ_array_reflects_inherited_variable() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .env("AWKRS_TEST_VAR", "needle-42")
        .arg(r#"BEGIN { print ENVIRON["AWKRS_TEST_VAR"] }"#)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "needle-42");
}

// ── Range patterns ─────────────────────────────────────────────────────────

#[test]
fn range_pattern_includes_both_endpoints_inclusive() {
    let (c, o, _) = run_awkrs_stdin(r#"/start/,/end/ { print }"#, "x\nstart\nmid\nend\ny\n");
    assert_eq!(c, 0);
    assert_eq!(o, "start\nmid\nend\n");
}

#[test]
fn range_pattern_reactivates_after_close() {
    let (c, o, _) = run_awkrs_stdin(r#"/a/,/b/ { print NR ":" $0 }"#, "x\na\nm\nb\ny\na\nz\nb\n");
    assert_eq!(c, 0);
    // First range NR=2..4, second NR=6..8.
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines, vec!["2:a", "3:m", "4:b", "6:a", "7:z", "8:b"]);
}

// ── printf POSIX positional args ───────────────────────────────────────────

#[test]
fn printf_positional_argument_specifier_reorders_args() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { printf "%2$s %1$s\n", "world", "hello" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o, "hello world\n");
}

// ── Bitwise builtins ───────────────────────────────────────────────────────

#[test]
fn bitwise_builtins_match_gawk_semantics() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             printf "%d %d %d %d %d\n",
                and(12, 10), or(12, 10), xor(12, 10),
                lshift(1, 4), rshift(16, 2)
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "8 14 6 16 4");
}

// ── typeof / isarray ───────────────────────────────────────────────────────

#[test]
fn typeof_distinguishes_number_string_array_and_untyped() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             a = 5; b = "hi"; d[1] = 1
             printf "%s %s %s %s\n", typeof(a), typeof(b), typeof(d), typeof(unset)
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "number string array untyped");
}

// ── Line continuation, comments, semicolons ────────────────────────────────

#[test]
fn line_continuation_with_backslash_joins_expression() {
    let (c, o, _) = run_awkrs_stdin("BEGIN { x = 1 + \\\n2 + \\\n3 + \\\n4; print x }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "10");
}

#[test]
fn inline_and_trailing_comments_do_not_disrupt_statements() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { # leading
             print 1 # trailing
             print 2
             ; print 3   # extra semicolon
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1\n2\n3\n");
}

// ── End-to-end pipelines ───────────────────────────────────────────────────

#[test]
fn end_to_end_email_extraction_via_match_three_arg() {
    let (c, o, _) = run_awkrs_stdin(
        r#"match($0, /[a-z]+@[a-z.]+/, m) { print m[0] }"#,
        "contact: alice@example.com\nnope\ncontact: bob@test.org\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "alice@example.com\nbob@test.org\n");
}

#[test]
fn end_to_end_word_count_per_input_field() {
    let (c, o, _) = run_awkrs_stdin(
        r#"{ for (i = 1; i <= NF; i++) c[$i]++ }
           END { for (w in c) print w, c[w] }"#,
        "the quick brown fox\nthe lazy dog\n",
    );
    assert_eq!(c, 0);
    let mut got: Vec<&str> = o.lines().collect();
    got.sort_unstable();
    assert_eq!(
        got,
        vec!["brown 1", "dog 1", "fox 1", "lazy 1", "quick 1", "the 2",]
    );
}

#[test]
fn end_to_end_two_pass_compute_average_with_asort() {
    let (c, o, _) = run_awkrs_stdin(
        r#"{ s += $1; n++; a[NR] = $1 }
           END {
              k = asort(a)
              printf "mean=%.2f median=%s\n", s / n, a[int((k + 1) / 2)]
           }"#,
        "10\n20\n30\n40\n50\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "mean=30.00 median=30");
}

// ── -v assignment and -F field separator interactions ──────────────────────

#[test]
fn dash_capital_f_with_regex_class_splits_correctly() {
    let (c, o, _) = run_awkrs_stdin_args(["-F", "[:;]"], r#"{ print $1, $2, $3 }"#, "a:b;c\n");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "a b c");
}

#[test]
fn three_dash_v_assignments_combine_into_arithmetic_expression() {
    let (c, o, _) = run_awkrs_stdin_args(
        ["-v", "a=10", "-v", "b=20", "-v", "c=12"],
        r#"BEGIN { print a + b + c }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "42");
}

// ── ARGV introspection in BEGIN ────────────────────────────────────────────

#[test]
fn begin_sees_argv_zero_naming_the_binary() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print ARGV[0] }"#, "");
    assert_eq!(c, 0);
    assert!(
        o.trim().ends_with("awkrs"),
        "expected ARGV[0] to be the awkrs binary path, got {:?}",
        o.trim()
    );
}

// ── srand returns the previous seed ────────────────────────────────────────

#[test]
fn srand_returns_previous_seed_value() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { srand(7); print srand(42) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "7");
}

// ── int() and math intrinsics ──────────────────────────────────────────────

#[test]
fn int_truncates_toward_zero_for_negative_and_positive() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { printf "%d %d %d %d\n", int(3.9), int(-3.9), int(0.5), int(-0.5) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "3 -3 0 0");
}

// ── File-driven tests via run_awkrs_file (slurped fast path) ───────────────

#[test]
fn slurped_file_sum_and_count_via_file_arg() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let f = dir.join(format!("awkrs_sum_{id}.txt"));
    std::fs::write(&f, "10\n20\n30\n40\n").unwrap();
    let (c, o, _) = run_awkrs_file(r#"{ s += $1 } END { print NR, s }"#, &f);
    let _ = std::fs::remove_file(&f);
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "4 100");
}

// ── exit code propagation ──────────────────────────────────────────────────

#[test]
fn exit_with_explicit_code_propagates_to_process_status() {
    let (c, _, _) = run_awkrs_stdin(r#"BEGIN { exit 3 }"#, "");
    assert_eq!(c, 3);
}

#[test]
fn end_rule_can_override_exit_code_set_by_main_block() {
    let (c, _, _) = run_awkrs_stdin(r#"{ exit 5 } END { exit 7 }"#, "a\n");
    assert_eq!(c, 7);
}

// ── String concatenation precedence vs comparison ──────────────────────────

#[test]
fn concat_binds_tighter_than_comparison_in_print_arg() {
    // `"a" "b" == "ab"` must parse as `("a" "b") == "ab"` → true (1).
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print "a" "b" == "ab" }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

// ── delete loop safety ─────────────────────────────────────────────────────

#[test]
fn delete_inside_for_in_does_not_skip_remaining_keys() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
             a["a"] = 1; a["b"] = 2; a["c"] = 3
             for (k in a) if (a[k] == 2) delete a[k]
             PROCINFO["sorted_in"] = "@ind_str_asc"
             for (k in a) printf "%s=%s ", k, a[k]
             print ""
         }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "a=1 c=3");
}

// ── Heredoc-style multiline program ────────────────────────────────────────

#[test]
fn multiline_program_with_indented_statements_runs_cleanly() {
    let prog = r#"
        BEGIN {
            for (i = 1; i <= 3; i++) {
                print i, i * i
            }
        }
    "#;
    let (c, o, _) = run_awkrs_stdin(prog, "");
    assert_eq!(c, 0);
    assert_eq!(o, "1 1\n2 4\n3 9\n");
}
