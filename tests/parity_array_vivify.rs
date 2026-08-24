//! Reading a missing key auto-creates it (POSIX), but only where the array
//! actually lives. Pinned against awk/gawk/mawk, which all leave the global
//! untouched when the array came in as a function parameter.

use std::process::Command;

fn awkrs(prog: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .arg(prog)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run awkrs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A missing key read through a function parameter must vivify in the caller's
/// array, not under the parameter's name in the globals. awkrs used to test and
/// create the entry in the globals and only then read frame-aware, so the stray
/// global survived the call and showed up in `in` and `length`.
#[test]
fn param_array_read_does_not_vivify_a_global() {
    let out = awkrs(
        r#"function f(p) { z = p["missing"] }
BEGIN { q["a"] = 1; f(q); print ("missing" in p), length(p), length(q) }"#,
    );
    // `0 0 2`, verbatim from gawk 5 and one-true-awk: nothing under the
    // parameter's name in the globals, and the caller's array grew to two.
    assert_eq!(out, "0 0 2\n", "reading p[\"missing\"] leaked a global");
}

/// The caller's array is the one that grows, and the read still vivifies there:
/// `"missing" in q` is true afterwards and the element is unassigned, not "".
#[test]
fn param_array_read_vivifies_in_the_caller() {
    let out = awkrs(
        r#"function f(p) { z = p["missing"] }
BEGIN { q["a"] = 1; f(q); print ("missing" in q), length(q), typeof(q["missing"]) }"#,
    );
    assert_eq!(out, "1 2 unassigned\n"); // gawk 5, verbatim
}

/// The plain global case is unchanged: a read creates the entry as unassigned.
#[test]
fn global_array_read_vivifies_untyped() {
    let out = awkrs(r#"BEGIN { x = a["k"]; print ("k" in a), length(a), typeof(a["k"]) }"#);
    assert_eq!(out, "1 1 unassigned\n"); // gawk 5, verbatim
}
