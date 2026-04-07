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
    assert!(
        stderr.contains("unknown function"),
        "stderr={stderr:?}"
    );
}
