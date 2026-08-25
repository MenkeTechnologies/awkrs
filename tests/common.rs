//! Shared helpers for integration test binaries (`mod common` from each `tests/*.rs`).

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A temp path unique to this test process.
///
/// Several test binaries can be running at once — 16 concurrent editor
/// instances share this worktree — and a fixed `/tmp/awkrs_<name>` is written
/// and deleted by each of them, so one run's cleanup deletes another run's
/// fixture mid-assertion. `std::env::temp_dir()` also honours `TMPDIR`, which a
/// sandboxed runner may point somewhere writable when `/tmp` is not.
#[allow(dead_code)] // Used by `massive_integration` and `posix_parity_regressions`; unused when `common` is built for the others.
pub fn unique_tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{name}_{}", std::process::id()))
}

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

/// Run `awkrs PROGRAM OPERAND…` with `stdin` piped in.
///
/// Distinct from [`run_awkrs_stdin_args`], which places its extra arguments
/// *before* the program text — those are option flags. These go *after* it,
/// where awk's operands live, which is the only position that can carry a
/// `var=value` assignment or a file name. Stdin is still supplied because a
/// command line whose operands are all assignments reads standard input.
#[allow(dead_code)] // Only `posix_parity_regressions` needs the operand form.
pub fn run_awkrs_operands<I, S>(program: &str, operands: I, stdin: &str) -> (i32, String, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut cmd = Command::new(bin);
    cmd.arg(program);
    for operand in operands {
        cmd.arg(operand.as_ref());
    }
    cmd.stdin(Stdio::piped())
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

/// Like [`run_awkrs_stdin`] but kills the child after `secs` and reports that it
/// had to. Tests for programs every reference awk *rejects* need this: a
/// regression that made awkrs accept and loop on one would otherwise wedge the
/// whole test binary instead of failing, and a wedged run reads as "still
/// running" rather than as a bug. Returns `None` on timeout.
#[allow(dead_code)] // Only `posix_parity_regressions` needs the bounded form.
pub fn run_awkrs_stdin_bounded(
    program: &str,
    stdin: &str,
    secs: u64,
) -> Option<(i32, String, String)> {
    use std::sync::mpsc;
    use std::time::Duration;

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

    // `wait_with_output` consumes the child, so the wait happens on a helper
    // thread and the timeout is enforced by giving up on the channel.
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(Ok(out)) => {
            let _ = handle.join();
            Some((
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ))
        }
        Ok(Err(e)) => panic!("wait awkrs: {e}"),
        Err(_) => None,
    }
}
