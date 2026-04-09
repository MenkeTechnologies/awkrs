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
