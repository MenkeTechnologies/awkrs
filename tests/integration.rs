use std::io::Write;
use std::process::{Command, Stdio};

fn run_awkrs_stdin(program: &str, stdin: &str) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .arg(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

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
fn multidimensional_array_subsep() {
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { a[1,2] = 42; print a[1,2] }",
        "",
    );
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
    let (code, stdout, _) = run_awkrs_stdin(
        "BEGIN { print sprintf(\"%2$s %1$s\", \"a\", \"b\") }",
        "",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "b a\n");
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
