mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_args, run_awkrs_stdin_args_env};
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn prints_second_field() {
    let (code, stdout, _) = run_awkrs_stdin("{print $2}", "one two three\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "two\n");
}

#[test]
fn parallel_record_mode_preserves_output_order() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .args(["-j", "8", "{ print $1 }"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"a\nb\nc\nd\ne\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\nc\nd\ne\n");
}

#[test]
fn stdin_parallel_chunked_read_ahead_preserves_order() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .args(["-j", "4", "--read-ahead", "2", "{ print $1 }"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"a\nb\nc\nd\ne\nf\ng\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a\nb\nc\nd\ne\nf\ng\n"
    );
}

#[test]
fn begin_end_sum() {
    let (code, stdout, _) =
        run_awkrs_stdin("BEGIN { s=0 } { s += $1 } END { print s }", "1\n2\n3\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

#[test]
fn compound_mod_assign_runs_in_begin() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { x = 17; x %= 5; print x }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n");
}

#[test]
fn sprintf_percent_x_lower_and_left_pad_s() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { printf \"<%x><%-4s>\\n\", 10, \"ab\" }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "<a><ab  >\n");
}

#[test]
fn begin_for_c_loop_runs_expected_iterations() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { s = 0; for (i = 1; i <= 4; i++) s += i; print s }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n");
}

#[test]
fn begin_delete_whole_array_then_scalar_reuse() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { a[1] = 1; delete a; a = 9; print a }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "9\n");
}

#[test]
fn builtin_index_substr_and_string_concat() {
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN { s = "foo" "bar"; print s, index(s, "bar"), substr(s, 4, 3) }"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "foobar 4 bar\n");
}

#[test]
fn split_fs_and_asort_end_to_end() {
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN {
 n = split("3,1,2", v, ",");
  for (i = 1; i <= n; i++) a[i] = v[i] + 0;
  asort(a);
  print n, a[1], a[2], a[3];
}"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3 1 2 3\n");
}

#[test]
fn mktime_positive_and_strftime_year_four_chars() {
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN {
  t = mktime("2020 06 15 12 00 00");
  print (t > 0), length(strftime("%Y", t));
}"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 4\n");
}

#[test]
fn bitwise_and_or_xor_builtins() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { print and(3, 1), or(2, 1), xor(5, 3) }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 3 6\n");
}

#[test]
fn length_sin_cos_int_in_begin() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { print length(\"ab\"), sin(0), cos(0), int(-2.1) }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "2 0 1 -2\n");
}

#[test]
fn sprintf_positional_reorders_arguments() {
    let (code, stdout, _) = run_awkrs_stdin(r#"BEGIN { printf "%2$d %1$s\n", "last", 7 }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "7 last\n");
}

#[test]
fn ofs_ors_and_multidim_array_in_begin() {
    let (code, stdout, _) = run_awkrs_stdin(
        r#"BEGIN {
  OFS = ":";
  ORS = "|";
  print "x", "y";
  a[9,8] = 3;
  print a[9,8];
}"#,
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "x:y|3|");
}

#[test]
fn regex_pattern_matches_line() {
    let (code, stdout, _) = run_awkrs_stdin(r#"/hello/ { print "yes" }"#, "hello\nworld\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "yes\n");
}

#[test]
fn unknown_function_errors() {
    let (code, _, stderr) = run_awkrs_stdin("{nosuch()}", "a\n");
    assert_ne!(code, 0);
    assert!(stderr.contains("unknown function"), "stderr={stderr:?}");
}

#[test]
fn split_populates_array() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { n = split(\"a:b:c\", parts, \":\"); print n, parts[1], parts[2], parts[3] }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3 a b c\n");
}

#[test]
fn user_function() {
    let (code, stdout, _) = run_awkrs_stdin(
        "function add(x,y){ return x+y } { print add($1, $2) }",
        "3 4\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n");
}

#[test]
fn next_skips_following_rules() {
    let (code, stdout, _) = run_awkrs_stdin("{ print \"a\"; next } { print \"b\" }", "x\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "a\n");
}

#[test]
fn exit_runs_end_before_process_exit() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { exit 2 } END { print \"done\" }", "");
    assert_eq!(code, 2);
    assert_eq!(stdout, "done\n");
}

#[test]
fn getline_primary_advances_record() {
    let (code, stdout, _) = run_awkrs_stdin("{ print $0; getline; print $0 }", "a\nb\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "a\nb\n");
}

#[test]
fn cyberpunk_help_banner() {
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("--help")
        .output()
        .expect("spawn awkrs --help");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("STATUS: ONLINE") && s.contains("SIGNAL:"),
        "expected HUD status line, got: {s:?}"
    );
    assert!(
        s.contains("TEXT HAX") || s.contains("████"),
        "expected tagline or logo, got: {s:?}"
    );
}

#[test]
fn mawk_w_help_matches_long_help_flag() {
    let w = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .args(["-W", "help"])
        .output()
        .expect("spawn awkrs -W help");
    let long = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("--help")
        .output()
        .expect("spawn awkrs --help");
    assert_eq!(w.status, long.status);
    assert_eq!(w.stdout, long.stdout);
}

#[test]
fn gsub_on_record() {
    let (code, stdout, _) = run_awkrs_stdin("{ gsub(\"o\", \"x\"); print }", "hello\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hellx\n");
}

#[test]
fn gsub_literal_print_slurped_file_no_match() {
    let path: PathBuf =
        std::env::temp_dir().join(format!("awkrs-gsub-slurp-{}.txt", std::process::id()));
    fs::write(&path, "1\n2\n3\n").expect("write temp");
    let (code, stdout, _) = run_awkrs_file(r#"{ gsub("alpha", "ALPHA"); print }"#, &path);
    let _ = fs::remove_file(&path);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n3\n");
}

#[test]
fn gsub_literal_print_slurped_file_with_match() {
    let path: PathBuf =
        std::env::temp_dir().join(format!("awkrs-gsub-slurp-m-{}.txt", std::process::id()));
    fs::write(&path, "x alphay\n").expect("write temp");
    let (code, stdout, _) = run_awkrs_file(r#"{ gsub("alpha", "ALPHA"); print }"#, &path);
    let _ = fs::remove_file(&path);
    assert_eq!(code, 0);
    assert_eq!(stdout, "x ALPHAy\n");
}

#[test]
fn multidimensional_array_subsep() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { a[1,2] = 42; print a[1,2] }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn beginfile_endfile_stdin() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGINFILE { s = s \"B\" } ENDFILE { s = s \"E\" } END { print s }",
        "x\n",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "BE\n");
}

#[test]
fn sprintf_width_zero_pad() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%05d\", 7) }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "00007\n");
}

#[test]
fn patsplit_with_pattern() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { n = patsplit(\"a b c\", p, \"[^ ]+\"); print n, p[1], p[2], p[3] }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3 a b c\n");
}

#[test]
fn patsplit_seps_between_fields() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { n = patsplit(\"aa  bb\", a, \"[a-z]+\", seps); print n, length(seps[1]) }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "2 2\n");
}

#[test]
fn sprintf_star_width() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%*d\", 4, 7) }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "   7\n");
}

#[test]
fn sprintf_positional_args() {
    let (code, stdout, _) =
        run_awkrs_stdin("BEGIN { print sprintf(\"%2$s %1$s\", \"a\", \"b\") }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "b a\n");
}

#[test]
fn printf_statement_no_parens() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { printf \"%d\\n\", 42 }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn sprintf_star_positional_width() {
    let (code, stdout, _) = run_awkrs_stdin("BEGIN { print sprintf(\"%*1$d\", 4, 9) }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "   9\n");
}

#[test]
fn print_pipe_to_cat() {
    let (code, stdout, stderr) = run_awkrs_stdin("BEGIN { print \"hello\" | \"cat\" }", "");
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "hello\n");
}

#[test]
fn print_coproc_getline_cat() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        "BEGIN { print \"hi\" |& \"cat\"; fflush(\"cat\"); getline x <& \"cat\"; print x }",
        "",
    );
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "hi\n");
}

#[test]
fn printf_coproc_getline_cat() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        "BEGIN { printf \"%s\\n\", \"q\" |& \"cat\"; fflush(\"cat\"); getline x <& \"cat\"; print x }",
        "",
    );
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "q\n");
}

#[test]
fn coproc_two_lines_roundtrip() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN {
  print "first" |& "cat"
  print "second" |& "cat"
  fflush("cat")
  getline a <& "cat"
  getline b <& "cat"
  print a
  print b
}"#,
        "",
    );
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "first\nsecond\n");
}

#[test]
fn coproc_close_after_roundtrip() {
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN {
  print "x" |& "cat"
  fflush("cat")
  getline v <& "cat"
  print v
  close("cat")
}"#,
        "",
    );
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "x\n");
}

#[test]
fn pipe_then_coproc_same_cmd_errors() {
    let (code, _, stderr) =
        run_awkrs_stdin(r#"BEGIN { print "a" | "cat"; print "b" |& "cat" }"#, "");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("two-way pipe") && stderr.contains("conflicts"),
        "stderr={stderr:?}"
    );
}

#[test]
fn coproc_then_pipe_same_cmd_errors() {
    let (code, _, stderr) =
        run_awkrs_stdin(r#"BEGIN { print "a" |& "cat"; print "b" | "cat" }"#, "");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("one-way pipe") && stderr.contains("conflicts"),
        "stderr={stderr:?}"
    );
}

#[test]
fn fflush_unknown_target_errors() {
    let (code, _, stderr) = run_awkrs_stdin(r#"BEGIN { fflush("not_an_open_redirection") }"#, "");
    assert_ne!(code, 0);
    assert!(stderr.contains("fflush"), "stderr={stderr:?}");
}

#[test]
fn sprintf_star_width_second_arg_end_to_end() {
    let (code, stdout, _) = run_awkrs_stdin(r#"BEGIN { print sprintf("%*2$d", 5, 4, 9) }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "   9\n");
}

#[test]
fn threads_with_print_coproc_warns_not_parallel_safe() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .args(["-j", "8", r#"{ print $1 |& "cat" }"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"a\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not parallel-safe"),
        "expected warning, stderr={stderr:?}"
    );
}

#[test]
fn print_redirect_and_fflush() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_out_{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_string_lossy().replace('\\', "/");
    let prog = format!("BEGIN {{ print \"hello\" > \"{p}\" ; fflush(\"{p}\") }}");
    let (code, stdout, stderr) = run_awkrs_stdin(&prog, "");
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert!(stdout.is_empty());
    let contents = std::fs::read_to_string(&path).expect("read redirected output");
    assert_eq!(contents, "hello\n");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn use_lc_numeric_short_flag_c_locale_printf() {
    let (code, stdout, stderr) = run_awkrs_stdin_args_env(
        ["-N"],
        r#"BEGIN { printf "%f\n", 1.5 }"#,
        "",
        [(OsString::from("LC_NUMERIC"), OsString::from("C"))],
    );
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert_eq!(stdout, "1.500000\n");
}

#[test]
fn use_lc_numeric_long_flag_c_locale_sprintf() {
    let (code, stdout, stderr) = run_awkrs_stdin_args_env(
        ["--use-lc-numeric"],
        r#"BEGIN { print sprintf("%e", 0.25) }"#,
        "",
        [(OsString::from("LC_NUMERIC"), OsString::from("C"))],
    );
    assert_eq!(code, 0, "stderr={stderr:?}");
    assert!(
        stdout.starts_with("2.5") && stdout.contains('e') && stdout.ends_with('\n'),
        "unexpected scientific output: {stdout:?}"
    );
}

#[test]
fn stdin_parallel_safe_high_threads_no_parallel_unsafe_warning() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .args(["-j", "8", "{ print $1 }"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"a\nb\nc\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("not parallel-safe"),
        "stdin should not hit parallel-unsafe path; stderr={stderr:?}"
    );
}

#[test]
fn version_flag_prints_name_and_semver_line() {
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .args(["--version"])
        .output()
        .expect("spawn awkrs --version");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("awkrs") && s.lines().next().is_some_and(|l| l.contains('.')),
        "unexpected version output: {s:?}"
    );
}

#[test]
fn short_version_flag_matches_long() {
    let long = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("--version")
        .output()
        .expect("spawn");
    let short = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("-V")
        .output()
        .expect("spawn");
    assert_eq!(long.status, short.status);
    assert_eq!(long.stdout, short.stdout);
}

#[test]
fn copyright_flag_mentions_license() {
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg("-C")
        .output()
        .expect("spawn awkrs -C");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("Copyright") && s.contains("MIT"),
        "unexpected -C output: {s:?}"
    );
}

#[test]
fn assign_flag_sets_variable_before_begin() {
    let (code, stdout, _) = run_awkrs_stdin_args(["-v", "x=99"], "BEGIN { print x }", "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "99\n");
}

#[test]
fn field_separator_flag_splits_columns() {
    let (code, stdout, _) = run_awkrs_stdin_args(["-F", ","], "{ print $2 }", "a,b,c\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "b\n");
}

#[test]
fn long_field_separator_flag() {
    let (code, stdout, _) =
        run_awkrs_stdin_args(["--field-separator", ":"], "{ print $3 }", "p:q:r\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "r\n");
}

#[test]
fn progfile_option_reads_script_from_file() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_progfile_{id}.awk"));
    std::fs::write(&path, "{ print $2 }\n").expect("write awk program");
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .args(["-f", path.to_str().expect("path utf-8")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs -f");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"one two three\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "two\n");
}

#[test]
fn parallel_mode_with_input_file_preserves_line_order() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_par_order_{id}.txt"));
    std::fs::write(&path, "a\nb\nc\nd\n").expect("write temp file");
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args(["-j", "8", "{ print $1 }"])
        .arg(&path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("not parallel-safe"), "stderr={stderr:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\nc\nd\n");
}

#[test]
fn index_returns_zero_when_substring_not_present() {
    let (code, stdout, _) = run_awkrs_stdin(r#"BEGIN { print index("abc", "z") }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

#[test]
fn substr_start_past_end_prints_blank_line() {
    let (code, stdout, _) = run_awkrs_stdin(r#"BEGIN { print substr("hi", 99, 2) }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n");
}

#[test]
fn begin_argc_is_one_for_program_plus_implicit_stdin() {
    let (code, stdout, _) = run_awkrs_stdin(r#"BEGIN { print ARGC }"#, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn next_in_end_rule_is_runtime_error_nonzero_exit() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"END { next }"#, "");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("next") && stderr.contains("END"),
        "stderr={stderr:?}"
    );
}

#[test]
fn next_in_begin_rule_is_runtime_error_nonzero_exit() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN { next }"#, "");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("next") && stderr.contains("BEGIN"),
        "stderr={stderr:?}"
    );
}

#[test]
fn nextfile_in_begin_rule_is_runtime_error_nonzero_exit() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN { nextfile }"#, "");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("nextfile") && stderr.contains("BEGIN"),
        "stderr={stderr:?}"
    );
}

#[test]
fn next_in_beginfile_rule_is_runtime_error_nonzero_exit() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGINFILE { next }"#, "x\n");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("next") && stderr.contains("BEGINFILE"),
        "stderr={stderr:?}"
    );
}

#[test]
fn nextfile_in_end_rule_is_runtime_error_nonzero_exit() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"END { nextfile }"#, "");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("nextfile") && stderr.contains("END"),
        "stderr={stderr:?}"
    );
}

#[test]
fn nextfile_in_beginfile_rule_is_runtime_error_nonzero_exit() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGINFILE { nextfile }"#, "x\n");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("nextfile") && stderr.contains("BEGINFILE"),
        "stderr={stderr:?}"
    );
}

#[test]
fn next_in_endfile_rule_is_runtime_error_nonzero_exit() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_endfile_next_{id}.txt"));
    fs::write(&path, "x\n").expect("temp");
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg(r#"ENDFILE { next }"#)
        .arg(&path)
        .output()
        .expect("spawn awkrs");
    let _ = fs::remove_file(&path);
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("next") && stderr.contains("ENDFILE"),
        "stderr={stderr:?}"
    );
}

#[test]
fn nextfile_in_endfile_rule_is_runtime_error_nonzero_exit() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_endfile_nextfile_{id}.txt"));
    fs::write(&path, "y\n").expect("temp");
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg(r#"ENDFILE { nextfile }"#)
        .arg(&path)
        .output()
        .expect("spawn awkrs");
    let _ = fs::remove_file(&path);
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nextfile") && stderr.contains("ENDFILE"),
        "stderr={stderr:?}"
    );
}

#[test]
fn intdiv_division_by_zero_is_runtime_error() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN { print intdiv(1, 0) }"#, "");
    assert_ne!(code, 0);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("intdiv") && stderr.contains("zero"),
        "stderr={stderr:?}"
    );
}
