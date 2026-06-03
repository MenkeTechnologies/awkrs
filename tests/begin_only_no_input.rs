//! Regression: a program consisting solely of `BEGIN` actions (no main/record
//! rules, no `END`, no `BEGINFILE`/`ENDFILE`) must exit after `BEGIN` without
//! reading input. Before the fix, `awkrs 'BEGIN { print 1 }'` blocked forever on
//! an open stdin that never reached EOF (e.g. an interactive terminal).

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Spawn awkrs with stdin held open (never written, never closed) and require the
/// process to exit on its own within the deadline.
fn run_holding_stdin_open(program: &str, deadline: Duration) -> (i32, String) {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let mut child = Command::new(bin)
        .arg(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn awkrs");

    // Keep the write end of the stdin pipe alive for the whole test so the child
    // sees an open-but-idle stdin (the condition that used to wedge it).
    let _stdin = child.stdin.take().expect("stdin");

    let mut stdout = child.stdout.take().expect("stdout");
    let reader = thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });

    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = reader.join().expect("join reader");
                return (status.code().unwrap_or(-1), out);
            }
            None => {
                if start.elapsed() > deadline {
                    let _ = child.kill();
                    let _ = reader.join();
                    panic!(
                        "BEGIN-only program did not exit within {:?}; it is blocking on stdin",
                        deadline
                    );
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

#[test]
fn begin_only_exits_without_reading_stdin() {
    let (code, out) = run_holding_stdin_open("BEGIN { print 1 + 1 }", Duration::from_secs(10));
    assert_eq!(code, 0, "expected clean exit");
    assert_eq!(out, "2\n");
}

#[test]
fn begin_chain_only_exits_without_reading_stdin() {
    let (code, out) = run_holding_stdin_open(
        "BEGIN { x = 3 } BEGIN { print x * x }",
        Duration::from_secs(10),
    );
    assert_eq!(code, 0);
    assert_eq!(out, "9\n");
}
