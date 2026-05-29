//! Contract tests for previously-uncovered printf/getline/lint surfaces.
//!
//! Targets:
//! - printf `%*.*f` (both width AND precision via `*` from arglist)
//! - printf negative dynamic width (`%*s` with negative arg) gives left-align
//! - printf zero-padded float with precision (`%08.3f`) preserves both flags
//! - printf mixed flag combos (`%5.2f|%-10s|%+5d|%05d`)
//! - getline-into-var from file: does NOT advance NR (POSIX), but does set the var
//! - getline-into-var + split: split sees the line value, NF is unchanged
//! - split with regex separator splits on runs of separator chars
//! - --lint=invalid emits the lint banner to stderr without exit failure

mod common;

use common::{run_awkrs_stdin, run_awkrs_stdin_args};
use std::fs;

#[test]
fn test_printf_dynamic_width_and_precision_both_asterisk() {
    // `%*.*f` consumes TWO args (width, precision) before the value.
    let (code, stdout, stderr) =
        run_awkrs_stdin(r#"BEGIN { printf "<%*.*f>\n", 8, 3, 3.14159 }"#, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "<   3.142>\n",
        "%*.*f with width=8 prec=3 should right-align to 8 chars with 3 decimals"
    );
}

#[test]
fn test_printf_negative_dynamic_width_left_aligns() {
    // POSIX: negative width via `%*s` left-aligns (equivalent to `%-Ns`).
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN { printf "<%*s>\n", -10, "hi" }"#, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "<hi        >\n",
        "negative dynamic width should left-align string in 10-char field"
    );
}

#[test]
fn test_printf_zero_pad_with_precision_for_float() {
    // `%08.3f` — zero-pad to width 8 with precision 3. Effective output is `0003.140`.
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN { printf "<%08.3f>\n", 3.14 }"#, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "<0003.140>\n",
        "%08.3f should zero-pad to width 8 with 3 decimals"
    );
}

#[test]
fn test_printf_mixed_flag_combinations_in_one_call() {
    // Exercises four format specs in one printf: %5.2f, %-10s, %+5d, %05d.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"BEGIN { printf "<%5.2f|%-10s|%+5d|%05d>\n", 3.14159, "ab", 7, 7 }"#,
        "",
    );
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "< 3.14|ab        |   +7|00007>\n",
        "mixed format flags should all apply independently in a single call"
    );
}

#[test]
fn test_getline_into_var_from_file_does_not_advance_nr() {
    // POSIX: `getline var < file` reads into var; NR remains at its prior value.
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_contract_getline_nr_{id}.txt"));
    fs::write(&path, "line1\nline2\n").expect("write tmp");
    let p = path.display().to_string();
    let prog = format!(r#"BEGIN {{ while ((getline line < "{p}") > 0) {{ print NR, line }} }}"#);
    let (code, stdout, stderr) = run_awkrs_stdin(&prog, "");
    let _ = fs::remove_file(&path);
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    // BEGIN-time NR is 0; getline-into-var does NOT advance NR.
    assert_eq!(
        stdout, "0 line1\n0 line2\n",
        "getline-into-var must NOT advance NR (POSIX); got {stdout:?}"
    );
}

#[test]
fn test_getline_into_var_combined_with_split_does_not_alter_nf() {
    // `getline line < file` then `split(line, parts, " ")` should populate parts
    // without touching $0 / NF.
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_contract_getline_split_{id}.txt"));
    fs::write(&path, "alpha beta gamma\n").expect("write tmp");
    let p = path.display().to_string();
    let prog = format!(
        r#"BEGIN {{ NF_before = NF; while ((getline line < "{p}") > 0) {{ n = split(line, parts, " "); print n, parts[1], parts[n], NF, NF_before }} }}"#
    );
    let (code, stdout, stderr) = run_awkrs_stdin(&prog, "");
    let _ = fs::remove_file(&path);
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    // split returns 3, parts[1]=alpha, parts[3]=gamma; NF unchanged from BEGIN (0).
    assert_eq!(
        stdout, "3 alpha gamma 0 0\n",
        "getline-into-var + split must preserve NF; got {stdout:?}"
    );
}

#[test]
fn test_split_with_regex_separator_collapses_runs() {
    // split($0, a, /[, ]+/) on "a, b ,  c,d" should yield 4 fields.
    let (code, stdout, stderr) = run_awkrs_stdin(
        r#"{ n = split($0, a, /[, ]+/); for (i=1;i<=n;i++) print i":"a[i] }"#,
        "a, b ,  c,d\n",
    );
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "1:a\n2:b\n3:c\n4:d\n",
        "regex separator should collapse runs of [, ]+ into single splits"
    );
}

#[test]
fn test_lint_mode_emits_banner_on_stderr() {
    // `--lint=invalid` must surface the lint banner to stderr.
    let (code, _stdout, stderr) =
        run_awkrs_stdin_args(["--lint=invalid"], r#"BEGIN { print "ok" }"#, "");
    assert_eq!(
        code, 0,
        "lint mode shouldn't fail valid programs; stderr={stderr:?}"
    );
    assert!(
        stderr.contains("lint"),
        "expected lint banner on stderr, got {stderr:?}"
    );
}
