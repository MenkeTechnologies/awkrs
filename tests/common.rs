//! Shared helpers for integration test binaries (`mod common` from each `tests/*.rs`).

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn run_awkrs_stdin(program: &str, stdin: &str) -> (i32, String, String) {
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

#[allow(dead_code)] // Used by `more_integration`; unused when `common` is built for `integration` only.
pub fn run_awkrs_stdin_args<I, S>(
    extra_args: I,
    program: &str,
    stdin: &str,
) -> (i32, String, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut cmd = Command::new(bin);
    for a in extra_args {
        cmd.arg(a.as_ref());
    }
    cmd.arg(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn awkrs");
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

/// Like [`run_awkrs_stdin_args`], but sets environment variables (e.g. `LC_NUMERIC`).
#[allow(dead_code)] // Used by `integration`; unused when only `more_integration` is built.
pub fn run_awkrs_stdin_args_env<I, S, E>(
    extra_args: I,
    program: &str,
    stdin: &str,
    env_pairs: E,
) -> (i32, String, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    E: IntoIterator<Item = (OsString, OsString)>,
{
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut cmd = Command::new(bin);
    for (k, v) in env_pairs {
        cmd.env(k, v);
    }
    for a in extra_args {
        cmd.arg(a.as_ref());
    }
    cmd.arg(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn awkrs");
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

/// Run `awkrs PROGRAM FILE` (no stdin) — exercises slurped-file fast paths.
#[allow(dead_code)] // Used by `integration` only; `more_integration` shares this crate.
pub fn run_awkrs_file(program: &str, path: &Path) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(program)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs with file");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}
