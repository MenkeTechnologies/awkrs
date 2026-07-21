//! End-to-end inline Rust FFI for awk: a `rust { ... }` block is desugared to a
//! `BEGIN { __rust_compile(...) }` rule, compiled to a cdylib via `rustc`,
//! dlopened, and its exports called by bareword from a later `BEGIN` rule.
//! Requires `rustc` on PATH (always present in a Rust CI); skips cleanly
//! otherwise so a toolchain-less environment never reports a false failure.

use std::process::{Command, Stdio};

fn rustc_available() -> bool {
    Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `awkrs PROGRAM` with no input (BEGIN-only, so it exits without reading
/// stdin). Returns `(exit_code, stdout, stderr)`.
fn run_awkrs(program: &str) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .arg(program)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn awkrs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn rust_block_exports_are_callable_across_all_v1_signatures() {
    if !rustc_available() {
        eprintln!("skipping FFI test: rustc not on PATH");
        return;
    }
    // Distinct names so this test's registry entries never collide with another
    // test's. Exercises int-arity, float-arity, and string→int marshalling, each
    // called by bareword from a `BEGIN` rule after the `rust { }` block's own
    // desugared `BEGIN { __rust_compile(...) }` has registered them.
    let program = r#"
rust {
    pub extern "C" fn awk_ffi_addi(a: i64, b: i64) -> i64 { a + b }
    pub extern "C" fn awk_ffi_mulf(x: f64, y: f64, z: f64) -> f64 { x * y * z }
    pub extern "C" fn awk_ffi_slen(s: *const c_char) -> i64 {
        unsafe { CStr::from_ptr(s).to_bytes().len() as i64 }
    }
}
BEGIN { print awk_ffi_addi(21, 21) "|" awk_ffi_mulf(1.5, 2.0, 3.0) "|" awk_ffi_slen("hello world") }
"#;
    let (code, stdout, stderr) = run_awkrs(program);
    assert_eq!(code, 0, "awkrs exited nonzero; stderr:\n{stderr}");
    assert_eq!(
        stdout, "42|9|11\n",
        "unexpected FFI output; stderr:\n{stderr}"
    );
}

#[test]
fn rust_block_with_no_exports_errors() {
    if !rustc_available() {
        return;
    }
    // A block with no `pub extern "C" fn` is a hard error — v1 requires at least
    // one exported function. The error surfaces from the BEGIN-phase
    // `__rust_compile` call.
    let program = "rust { fn helper() -> i64 { 1 } }\nBEGIN { print 1 }\n";
    let (code, _stdout, stderr) = run_awkrs(program);
    assert_ne!(code, 0, "empty-export block must fail");
    assert!(
        stderr.contains("rust FFI"),
        "unexpected error text: {stderr}"
    );
}
