//! Contract tests for previously-uncovered surfaces:
//!   - sprintf/%.*f rounding mode at the .5 boundary (banker's / half-to-even
//!     vs naive half-up) — pinning current behavior so downstream `OFMT`/`CONVFMT`
//!     migrations don't silently change numeric output.
//!   - `split()` with a multi-char regex FS (literal regex `/-[xy]-/`) splits on
//!     each *match*, not on character classes inside the match — different from
//!     `split(s, a, /[, ]+/)` which collapses runs.
//!   - `split()` with a multi-char *string* FS treats the whole string as the
//!     literal separator (no regex semantics) — distinct from regex-FS.
//!   - `next` skips the remainder of the per-record action chain *but* still
//!     increments NR/FNR for the skipped record (POSIX: `next` re-enters the
//!     main loop, it does not "discard" the record).
//!   - `nextfile` jumps to the next file argument; FNR resets, NR continues.
//!   - `getline < file` inside BEGIN does NOT advance NR or FNR; the main-loop
//!     NR continues from 1 once stdin is read.
//!
//! These pin behavior that earlier rounds did not cover:
//!   * Earlier rounds pinned `printf` width/precision *layout* and lint-banner
//!     emission — not rounding-mode at the .5 boundary.
//!   * Earlier rounds pinned `split()` with `/[, ]+/` (run-collapse) — not the
//!     multi-char regex vs multi-char string distinction.
//!   * Earlier rounds did not pin `next` semantics around NR continuation, nor
//!     `nextfile` cross-file FNR reset.

mod common;

use common::{run_awkrs_stdin, run_awkrs_stdin_args};
use std::fs;

/// `%.0f` at .5 must round to nearest even (banker's), not half-up:
/// 2.5 -> 2, 3.5 -> 4, 4.5 -> 4, -2.5 -> -2.
#[test]
fn test_sprintf_dot_zero_f_uses_bankers_rounding_at_half() {
    let prog = r#"BEGIN { printf "%.0f|%.0f|%.0f|%.0f\n", 2.5, 3.5, 4.5, -2.5 }"#;
    let (code, stdout, stderr) = run_awkrs_stdin(prog, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "2|4|4|-2\n",
        "%.0f at .5 must use round-half-to-even (banker's); got {stdout:?}"
    );
}

/// `%.3f` at the .5 ulp must follow the same banker's rule for digits beyond
/// the decimal point. 0.0005 -> 0.001 (next even is 0), but IEEE 754 stores
/// 0.0005 as 0.00050000...something, so rounding goes UP. 1.0005 has a true
/// .5 boundary that should round down to even (0 is even).
#[test]
fn test_sprintf_dot_3_f_rounding_pins_current_behavior() {
    let prog = r#"BEGIN { printf "%.3f|%.3f|%.3f\n", 0.0005, 0.0015, 1.0005 }"#;
    let (code, stdout, stderr) = run_awkrs_stdin(prog, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    // Pin the observed output from awkrs's current %.3f path; matches
    // glibc printf for these IEEE-754 representations.
    assert_eq!(
        stdout, "0.001|0.002|1.000\n",
        "%.3f rounding behavior changed; got {stdout:?}"
    );
}

/// `split(s, a, /-[xy]-/)` must split on each regex match — not on the
/// character class alone. Input `a-x-b-y-c` should yield exactly 3 pieces.
#[test]
fn test_split_with_multichar_regex_fs_matches_whole_pattern() {
    let prog = r#"BEGIN { n=split("a-x-b-y-c", a, /-[xy]-/); printf "%d|%s|%s|%s\n", n, a[1], a[2], a[3] }"#;
    let (code, stdout, stderr) = run_awkrs_stdin(prog, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "3|a|b|c\n",
        "multi-char regex FS must split on each match, not on char-class chars; got {stdout:?}"
    );
}

/// `split(s, a, "::")` (a multi-char STRING) treats `::` as the literal
/// separator. Distinct from the regex form above.
#[test]
fn test_split_with_multichar_string_fs_treats_as_literal() {
    let prog =
        r#"BEGIN { n=split("a::b::c", a, "::"); printf "%d|%s|%s|%s\n", n, a[1], a[2], a[3] }"#;
    let (code, stdout, stderr) = run_awkrs_stdin(prog, "");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "3|a|b|c\n",
        "multi-char string FS must be literal separator; got {stdout:?}"
    );
}

/// `next` skips action body for that record but the record itself was consumed
/// and NR advances. After 3 records with `next` on record 2, NR at END is 3.
#[test]
fn test_next_increments_nr_for_skipped_record_and_reenters_main_loop() {
    let prog = r#"NR==2 { next } { print "saw:", NR, $0 } END { print "end_nr:", NR }"#;
    let (code, stdout, stderr) = run_awkrs_stdin(prog, "a\nb\nc\n");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "saw: 1 a\nsaw: 3 c\nend_nr: 3\n",
        "`next` must advance NR for the skipped record; got {stdout:?}"
    );
}

/// `nextfile` jumps to next file argument. With two files of 3 lines each and
/// `nextfile` on FNR==2, file1 yields rec 1 (FNR=1), file2 yields rec 1 (FNR=1).
/// NR is cumulative — file2's first line is global NR=2.
#[test]
fn test_nextfile_resets_fnr_but_continues_nr_across_files() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let p1 = dir.join(format!("awkrs_nextfile_a_{id}.txt"));
    let p2 = dir.join(format!("awkrs_nextfile_b_{id}.txt"));
    fs::write(&p1, "a\nb\nc\n").expect("write tmp1");
    fs::write(&p2, "x\ny\nz\n").expect("write tmp2");
    let prog = r#"FNR==2 { nextfile } { print NR, FNR, $0 }"#;
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = std::process::Command::new(bin)
        .arg(prog)
        .arg(&p1)
        .arg(&p2)
        .output()
        .expect("spawn");
    let _ = fs::remove_file(&p1);
    let _ = fs::remove_file(&p2);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code(),
        Some(0),
        "exit non-zero, stderr={stderr:?}"
    );
    // Record 1 from file1 (NR=1, FNR=1) -> printed; FNR==2 triggers nextfile
    // (the FNR==2 record IS read — NR still increments for it); record 1 from
    // file2 (NR=3, FNR=1) -> printed.
    assert_eq!(
        stdout, "1 1 a\n3 1 x\n",
        "nextfile must reset FNR but continue NR (incl. skipped record); got {stdout:?}"
    );
}

/// `getline line < file` inside BEGIN must NOT advance NR or FNR. After the
/// BEGIN block finishes, the main loop should see NR=1 for the first stdin
/// record.
#[test]
fn test_getline_lt_file_in_begin_does_not_advance_main_nr_fnr() {
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let path = dir.join(format!("awkrs_getline_nr_fnr_{id}.txt"));
    fs::write(&path, "ext1\next2\n").expect("write tmp");
    let p = path.display().to_string();
    let prog = format!(
        r#"BEGIN {{ while ((getline line < "{p}") > 0) print "be:", NR, FNR, line }} {{ print "mn:", NR, FNR, $0 }}"#
    );
    let (code, stdout, stderr) = run_awkrs_stdin(&prog, "stdin1\nstdin2\n");
    let _ = fs::remove_file(&path);
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    // BEGIN reads 2 ext lines: NR/FNR stay at 0. Main loop reads 2 stdin lines:
    // NR/FNR=1 then 2.
    assert_eq!(
        stdout, "be: 0 0 ext1\nbe: 0 0 ext2\nmn: 1 1 stdin1\nmn: 2 2 stdin2\n",
        "getline-from-file in BEGIN must not bleed into main NR/FNR; got {stdout:?}"
    );
}

/// `--csv` mode must keep quoted fields containing a comma intact across the
/// whole record. This pins CSV-mode field parsing for awkrs (gawk 5.3+ semantics).
#[test]
fn test_csv_mode_keeps_quoted_comma_field_intact() {
    let prog = r#"{ printf "%d|%s|%s|%s\n", NF, $1, $2, $3 }"#;
    let (code, stdout, stderr) = run_awkrs_stdin_args(["--csv"], prog, "a,\"b,c\",d\n");
    assert_eq!(code, 0, "exit non-zero, stderr={stderr:?}");
    assert_eq!(
        stdout, "3|a|b,c|d\n",
        "CSV mode must keep quoted comma inside field; got {stdout:?}"
    );
}
