//! End-to-end coverage for recently added gawk-style features: **PROCINFO** (argv, FUNCTAB,
//! composite READ_TIMEOUT keys), **`-L`/`--lint`**, **`-S`/`--sandbox`**, extension builtins
//! (**`stat`**, **`readfile`**, **`ord`/`chr`**, **`gettimeofday`**, **`fts`**, **`writea`/`reada`**),
//! **`@/regex/`** regexp values, and additional **`PROCINFO["sorted_in"]`** modes.

mod common;

use common::{run_awkrs_stdin, run_awkrs_stdin_args};
use std::process::{Command, Stdio};

// ── PROCINFO: argv mirror, FUNCTAB, composite keys ──────────────────────────

#[test]
fn procinfo_argv_subarray_matches_process_args() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  a = PROCINFO["argv"]
  print (a["0"] ~ /awkrs/)
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn procinfo_api_and_program_identify_runtime() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  print PROCINFO["api"]
  print (PROCINFO["program"] != "")
  print (PROCINFO["api_major"] + 0 >= 4)
}"#,
        "",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines[0].trim(), "awkrs");
    assert_eq!(lines[1].trim(), "1");
    assert_eq!(lines[2].trim(), "1");
}

#[test]
fn functab_lists_user_function_metadata() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function f(x) { return x }
BEGIN { m = FUNCTAB["f"]; print m["type"], m["arity"] }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "user 1");
}

#[test]
fn procinfo_read_timeout_composite_key_exists_for_stdin_placeholder() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  sep = SUBSEP
  k = "-" sep "READ_TIMEOUT"
  print (k in PROCINFO)
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

// ── PROCINFO sorted_in: additional @… modes and unknown-token warning ───────

#[test]
fn procinfo_sorted_in_val_num_desc_orders_by_numeric_value() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  a["x"] = 3
  a["y"] = 1
  a["z"] = 2
  PROCINFO["sorted_in"] = "@val_num_desc"
  for (k in a) print k
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "x\nz\ny\n");
}

#[test]
fn procinfo_sorted_in_ind_num_asc_orders_indices_numerically() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  a["10"] = 1
  a["2"] = 1
  a["1"] = 1
  PROCINFO["sorted_in"] = "@ind_num_asc"
  for (k in a) print k
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "1\n2\n10\n");
}

#[test]
fn procinfo_sorted_in_unknown_at_token_warns_once_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args([
            "-e",
            r#"BEGIN {
  PROCINFO["sorted_in"] = "@not_a_real_mode"
  a["b"] = 1
  a["a"] = 1
  for (k in a) print k
}"#,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(
        e.contains("unknown @") || e.contains("sorted_in"),
        "stderr should mention unknown sorted_in token: {e:?}"
    );
}

// ── Gawk regexp constants `@/…/` ────────────────────────────────────────────

#[test]
fn regexp_constant_split_behaves_like_regex_string() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  r = @/[;,]/
  n = split("a,b;c", arr, r)
  print n, arr[1], arr[2], arr[3]
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "3 a b c");
}

#[test]
fn regexp_constant_gensub_replacement() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  r = @/[0-9]+/
  print gensub(r, "N", "g", "a1b2c")
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "aNbNc");
}

// ── CLI: `-L` lint (stderr diagnostics after BEGIN) ─────────────────────────

#[test]
fn lint_flag_warns_uninitialized_read_in_begin() {
    // Use a non-fatal level: `-L fatal` exits on the *first* lint line (the static header),
    // before uninitialized-variable checks run.
    let (c, o, e) = run_awkrs_stdin_args(["-L", "invalid"], "BEGIN { print u }", "");
    assert_eq!(c, 0);
    assert_eq!(o, "\n");
    assert!(
        e.contains("uninitialized") && e.contains('u'),
        "expected uninit lint on stderr, got: {e:?}"
    );
}

// ── CLI: `-S` sandbox blocks `system()` ─────────────────────────────────────

#[test]
fn sandbox_rejects_system_call_with_runtime_error() {
    let (c, _, e) = run_awkrs_stdin_args(["-S"], "BEGIN { system(\"true\") }", "");
    assert_ne!(c, 0);
    assert!(
        e.contains("sandbox") && e.contains("system"),
        "stderr={e:?}"
    );
}

// ── Extension builtins (native; no `@load` required) ─────────────────────────

#[test]
fn stat_builtin_reports_regular_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_stat_{}.txt", std::process::id()));
    std::fs::write(&path, "x").unwrap();
    let ps = path.to_string_lossy().replace('\\', "\\\\");
    let prog = format!(
        r#"BEGIN {{ if (stat("{ps}", st) == 0) print st["type"], (st["size"] + 0 == 1) }}"#
    );
    let (c, o, err) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={err}");
    assert_eq!(o.trim(), "file 1");
}

#[test]
fn readfile_reads_entire_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_rf_{}.txt", std::process::id()));
    std::fs::write(&path, "hello\nworld").unwrap();
    let ps = path.to_string_lossy().replace('\\', "\\\\");
    let prog = format!(r#"BEGIN {{ print readfile("{ps}") }}"#);
    let (c, o, err) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={err}");
    assert_eq!(o, "hello\nworld\n");
}

#[test]
fn ord_chr_roundtrip_ascii() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print chr(ord("A")) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "A");
}

#[test]
fn gettimeofday_sets_sec_and_usec() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  gettimeofday(t)
  print (t["sec"] + 0 > 1000000000), (t["usec"] + 0 >= 0)
}"#,
        "",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines.len(), 1);
    let parts: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(parts, vec!["1", "1"]);
}

#[test]
fn fts_lists_sorted_paths_under_temp_directory() {
    let dir = std::env::temp_dir().join(format!("awkrs_fts_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    std::fs::write(&a, "1").unwrap();
    std::fs::write(&b, "2").unwrap();
    let ds = dir.to_string_lossy().replace('\\', "\\\\");
    let prog = format!(
        r#"BEGIN {{
  n = fts("{ds}", paths)
  print n
  print paths["2"]
  print paths["3"]
}}"#
    );
    let (c, o, err) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    let _ = std::fs::remove_dir(&dir);
    assert_eq!(c, 0, "stderr={err}");
    let lines: Vec<&str> = o.lines().collect();
    assert!(lines.len() >= 3);
    assert_eq!(lines[0].trim(), "3"); // directory + two files (sorted)
    assert!(lines[1].contains("a.txt"), "paths[2]={:?}", lines[1]);
    assert!(lines[2].contains("b.txt"), "paths[3]={:?}", lines[2]);
    assert!(lines[1] <= lines[2], "fts sorts paths");
}

#[test]
fn rename_builtin_moves_temp_file() {
    let dir = std::env::temp_dir();
    let old = dir.join(format!("awkrs_rn_old_{}.txt", std::process::id()));
    let new = dir.join(format!("awkrs_rn_new_{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new);
    std::fs::write(&old, "z").unwrap();
    let o = old.to_string_lossy().replace('\\', "\\\\");
    let n = new.to_string_lossy().replace('\\', "\\\\");
    let prog = format!(
        r#"BEGIN {{
  if (rename("{o}", "{n}") != 0) {{ print "fail"; exit 1 }}
  print readfile("{n}")
}}"#
    );
    let (c, out, err) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&new);
    assert_eq!(c, 0, "stderr={err}");
    assert_eq!(out.trim(), "z");
}

#[test]
fn writea_reada_roundtrip_preserves_array() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_wa_{}.dat", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "\\\\");
    let prog = format!(
        r#"BEGIN {{
  src["x"] = 1
  src["y"] = 2
  if (writea("{ps}", src) != 0) {{ print "writea_fail"; exit 1 }}
  delete dst
  if (reada("{ps}", dst) != 0) {{ print "reada_fail"; exit 1 }}
  print dst["x"], dst["y"]
}}"#
    );
    let (c, o, err) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={err}");
    assert_eq!(o.trim(), "1 2");
}

// ── `-M` MPFR: four-arg sorted_in still orders by value ──────────────────────

#[test]
fn bignum_sorted_in_four_arg_prefers_lower_value() {
    let prog = r#"function vcmp(i1,v1,i2,v2) {
  if (v1 < v2) return -1
  if (v1 > v2) return 1
  return 0
}
BEGIN {
  a["low"] = 1
  a["high"] = 9
  PROCINFO["sorted_in"] = "vcmp"
  for (k in a) print k
}"#;
    let (c, o, _) = run_awkrs_stdin_args(["-M"], prog, "");
    assert_eq!(c, 0);
    assert_eq!(o, "low\nhigh\n");
}

// ── `statvfs` on Unix root (best-effort; skips if unsupported) ───────────────

#[test]
#[cfg(unix)]
fn statvfs_root_populates_f_bsize() {
    let (c, o, err) = run_awkrs_stdin(
        r#"BEGIN {
  if (statvfs("/", v) == 0) print (v["f_bsize"] + 0 > 0)
  else print 0
}"#,
        "",
    );
    assert_eq!(c, 0, "stderr={err}");
    assert_eq!(o.trim(), "1");
}

#[test]
fn fts_missing_path_returns_minus_one() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  n = fts("/no/such/path/awkrs_fts_x", a)
  print n
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "-1");
}

// ── `@load` bundled name is accepted as no-op (parse + run) ─────────────────

#[test]
fn at_load_filefuncs_noop_runs_begin() {
    let (c, o, _) = run_awkrs_stdin(
        r#"@load "filefuncs"
BEGIN { print "ok" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "ok");
}

// ── `-D` / `--debug`: static listing to stderr (not gawk’s interactive debugger) ──

#[test]
fn debug_flag_emits_static_listing_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args(["-D", "-", "-e", "BEGIN { print 1 }"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(
        e.contains("awkrs --debug") && e.contains("rules:"),
        "stderr={e:?}"
    );
}

// ── `match` with `@/regex/` regexp value ───────────────────────────────────

#[test]
fn match_accepts_regexp_constant_as_second_argument() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  r = @/[a-z]+/
  print match("123abc", r), RSTART, RLENGTH
}"#,
        "",
    );
    assert_eq!(c, 0);
    let parts: Vec<&str> = o.split_whitespace().collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "4");
    assert_eq!(parts[1], "4");
    assert_eq!(parts[2], "3");
}
