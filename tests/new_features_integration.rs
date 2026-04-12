//! End-to-end coverage for recently added gawk-style features: **PROCINFO** (argv, FUNCTAB,
//! composite READ_TIMEOUT / RETRY keys), **`-L`/`--lint`**, **`-S`/`--sandbox`**, **`-k`/`--csv`**,
//! **`-d`/`-D`/`-o`/`-p`** CLI effects, extension builtins (**`chdir`**, **`stat`**, **`readfile`**,
//! **`ord`/`chr`**, **`gettimeofday`**, **`sleep`**, **`revoutput`**, **`fts`**, **`writea`/`reada`**),
//! **`@/regex/`** regexp values, **`SYMTAB`**, **`FUNCTAB`**, **`IGNORECASE`**, **`FIELDWIDTHS`**, **`RS=""`**
//! paragraph mode, **`-v`**, **`-M`** / **`-n`** / **`-b`**, **`@namespace`**, **`gsub`** with regexp values,
//! **BEGINFILE** / **ENDFILE**, failed **`stat`** / **`readfile`** + **`ERRNO`**, and **`-N`** **`printf`** **`%'`**
//! grouping.

mod common;

use common::{run_awkrs_file, run_awkrs_stdin, run_awkrs_stdin_args, run_awkrs_stdin_args_env};
use std::ffi::OsString;
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
fn functab_lists_two_distinct_user_functions() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function foo() { return 1 }
function bar(x, y) { return x + y }
BEGIN {
  fa = FUNCTAB["foo"]
  fb = FUNCTAB["bar"]
  print fa["arity"], fb["arity"]
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "0 2");
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

#[test]
fn procinfo_retry_composite_key_exists_for_stdin_placeholder() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  sep = SUBSEP
  k = "-" sep "RETRY"
  print (k in PROCINFO)
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

// ── PROCINFO sorted_in: additional @… modes and unknown-token warning ───────

#[test]
fn procinfo_sorted_in_val_num_asc_orders_by_numeric_value() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  a["x"] = 3
  a["y"] = 1
  a["z"] = 2
  PROCINFO["sorted_in"] = "@val_num_asc"
  for (k in a) print k
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "y\nz\nx\n");
}

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
fn procinfo_sorted_in_val_str_asc_orders_by_string_value() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  a["x"] = "b"
  a["y"] = "a"
  PROCINFO["sorted_in"] = "@val_str_asc"
  for (k in a) print k
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "y\nx\n");
}

#[test]
fn procinfo_sorted_in_ind_str_desc_reverse_lexicographic() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  a["a"] = 1
  a["b"] = 1
  a["c"] = 1
  PROCINFO["sorted_in"] = "@ind_str_desc"
  for (k in a) print k
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "c\nb\na\n");
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

#[test]
fn sandbox_rejects_chdir_builtin() {
    let (c, _, e) = run_awkrs_stdin_args(["-S"], r#"BEGIN { print chdir("/") }"#, "");
    assert_ne!(c, 0);
    assert!(e.contains("sandbox"), "stderr={e:?}");
}

#[test]
fn sandbox_rejects_file_redirect() {
    let (c, _, e) = run_awkrs_stdin_args(["-S"], r#"BEGIN { print "x" > "/dev/null" }"#, "");
    assert_ne!(c, 0);
    assert!(e.contains("sandbox"), "stderr={e:?}");
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
fn sleep_zero_returns_zero() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print sleep(0) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "0");
}

#[test]
fn revoutput_reverses_string() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print revoutput("ab") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "ba");
}

#[test]
fn revtwoway_matches_revoutput() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print (revtwoway("x") == revoutput("x")) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn chdir_then_readfile_relative_path() {
    let dir = std::env::temp_dir().join(format!("awkrs_chdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let inner = dir.join("inner.txt");
    std::fs::write(&inner, "inside").unwrap();
    let d = dir.to_string_lossy().replace('\\', "\\\\");
    let prog = format!(
        r#"BEGIN {{
  if (chdir("{d}") != 0) {{ print "cdfail"; exit 1 }}
  print readfile("inner.txt")
}}"#
    );
    let (c, o, err) = run_awkrs_stdin(&prog, "");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(c, 0, "stderr={err}");
    assert_eq!(o.trim(), "inside");
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
        .stdin(Stdio::null())
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

#[test]
fn profile_flag_emits_wall_time_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args(["-p", "-", "-e", "BEGIN { x = 1 }"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(
        e.contains("wall_seconds:") || e.contains("wall time"),
        "stderr={e:?}"
    );
}

#[test]
fn pretty_print_flag_emits_disclaimer_on_stdout() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args(["-o", "-", "-e", "BEGIN { z = 3 }"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("pretty-print") && s.contains("NOT gawk"),
        "stdout={s:?}"
    );
}

#[test]
fn dump_variables_flag_includes_user_global() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args(["-d", "-", "-e", "BEGIN { mydumpvar = 99 }"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("mydumpvar") && s.contains("99"), "stdout={s:?}");
}

// ── `match` with `@/regex/` regexp value ───────────────────────────────────

#[test]
fn typeof_regexp_literal_is_regexp() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { r = @/foo/; print typeof(r) }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "regexp");
}

#[test]
fn ignorecase_makes_regex_match_case_insensitive() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  IGNORECASE = 1
  print ("abc" ~ /A/)
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

#[test]
fn csv_mode_splits_quoted_comma_into_single_field() {
    let (c, o, _) = run_awkrs_stdin_args(["-k"], r#"{ print NF, $2 }"#, "a,\"b,c\"\n");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "2 b,c");
}

#[test]
fn multiple_dash_e_sources_concatenate() {
    let bin = env!("CARGO_BIN_EXE_awkrs");
    let out = Command::new(bin)
        .args(["-e", "BEGIN { q = 41 }", "-e", "BEGIN { print q + 1 }"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "42");
}

#[test]
fn at_load_time_extension_name_noop() {
    let (c, o, _) = run_awkrs_stdin(
        r#"@load "time"
BEGIN { print "t" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "t");
}

#[test]
fn at_load_readdir_extension_name_noop() {
    let (c, o, _) = run_awkrs_stdin(
        r#"@load "readdir"
BEGIN { print "r" }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "r");
}

#[test]
fn symtab_two_globals_roundtrip() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  SYMTAB["aa"] = 10
  SYMTAB["bb"] = 20
  print aa, bb
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "10 20");
}

#[test]
fn exponentiation_unary_minus_binds_looser_than_pow() {
    let (c, o, _) = run_awkrs_stdin(r#"BEGIN { print -2^2, (-2)^2 }"#, "");
    assert_eq!(c, 0);
    let parts: Vec<&str> = o.split_whitespace().collect();
    assert_eq!(parts, vec!["-4", "4"]);
}

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

// ── FIELDWIDTHS, RS paragraph, `-v`, **`-M`**, **`-n`**, **`-b`** ───────────

#[test]
fn fieldwidths_splits_fields_by_widths() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { FIELDWIDTHS = "2 3 1" }
{ print $1, $2, NF }"#,
        "abcdef\n",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "ab cde 3");
}

#[test]
fn rs_empty_string_is_paragraph_mode() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { RS = "" }
{ print NF, $1 }"#,
        "a b\n\nc d\n",
    );
    assert_eq!(c, 0);
    let lines: Vec<&str> = o.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].trim(), "2 a");
    assert_eq!(lines[1].trim(), "2 c");
}

#[test]
fn assign_flag_v_visible_in_begin() {
    let (c, o, _) = run_awkrs_stdin_args(["-v", "qq=42"], "BEGIN { print qq }", "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "42");
}

#[test]
fn bignum_sprintf_integer_beyond_i64_without_double_rounding() {
    // Decimal integer literals without `.` are kept as digit strings so **`-M`** does not round through `f64`.
    let (c, o, _) = run_awkrs_stdin_args(
        ["-M"],
        r#"BEGIN { print sprintf("%d", 9223372036854775807 + 1) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "9223372036854775808");
}

#[test]
fn non_decimal_flag_n_coerces_hex_in_numeric_context() {
    let (c, o, _) = run_awkrs_stdin_args(["-n"], r#"BEGIN { x = "0xFF"; print x + 0 }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "255");
}

#[test]
fn characters_as_bytes_flag_length_counts_utf8_bytes() {
    let (c, o, _) = run_awkrs_stdin_args(["-b"], r#"BEGIN { print length("α") }"#, "");
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "2");
}

// ── gawk: `@namespace`, regexp **gsub**, **BEGINFILE** / **ENDFILE** ─────────

#[test]
fn namespace_prefixes_unqualified_identifier() {
    let (c, o, _) = run_awkrs_stdin(
        "@namespace \"ns\"\nBEGIN { unqual = 5; print unqual }\n",
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "5");
}

#[test]
fn gsub_regexp_constant_replaces_matches() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  s = "abc"
  r = @/[a-z]/
  n = gsub(r, "X", s)
  print n, s
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "3 XXX");
}

#[test]
fn beginfile_endfile_run_around_slurped_file() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("awkrs_bef_{}.txt", std::process::id()));
    std::fs::write(&path, "row\n").unwrap();
    let (c, o, e) = run_awkrs_file(
        r#"BEGINFILE { print "BF" } { print $1 } ENDFILE { print "EF" }"#,
        &path,
    );
    let _ = std::fs::remove_file(&path);
    assert_eq!(c, 0, "stderr={e}");
    assert_eq!(o, "BF\nrow\nEF\n");
}

// ── I/O errors: **stat**, **readfile**, **ERRNO** ───────────────────────────

#[test]
fn stat_missing_file_returns_minus_one() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN { print stat("/no/such/awkrs_stat_missing", a) }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "-1");
}

#[test]
fn readfile_missing_file_sets_errno_nonempty() {
    let (c, o, _) = run_awkrs_stdin(
        r#"BEGIN {
  readfile("/no/such/awkrs_rf_missing")
  print (ERRNO != "")
}"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o.trim(), "1");
}

// ── Locale: **`-N`** + **printf** `'` grouping (needs `LC_NUMERIC` set) ──────

#[test]
fn use_lc_numeric_printf_apostrophe_groups_integer() {
    let (c, o, e) = run_awkrs_stdin_args_env(
        ["-N"],
        r#"BEGIN { printf "%'d\n", 1234567 }"#,
        "",
        [(OsString::from("LC_ALL"), OsString::from("C"))],
    );
    assert_eq!(c, 0, "stderr={e}");
    assert!(
        o.contains("1") && o.contains("234") && o.contains("567"),
        "stdout={o:?}"
    );
}

// ── Variadic and/or/xor ──────────────────────────────────────────────────

#[test]
fn variadic_and_three_args() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { print and(0xFF, 0x0F, 0x03) }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    assert_eq!(o.trim(), "3");
}

#[test]
fn variadic_or_three_args() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { print or(1, 2, 4) }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    assert_eq!(o.trim(), "7");
}

#[test]
fn variadic_xor_three_args() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { print xor(7, 3, 1) }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    // 7 xor 3 = 4, 4 xor 1 = 5
    assert_eq!(o.trim(), "5");
}

// ── readdir builtin ──────────────────────────────────────────────────────

#[test]
fn readdir_populates_array_with_dir_entries() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN {
  n = readdir("/tmp", a)
  print (n >= 0 ? "ok" : "fail")
}"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    assert_eq!(o.trim(), "ok");
}

// ── getlocaltime builtin ─────────────────────────────────────────────────

#[test]
fn getlocaltime_populates_time_array() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN {
  ts = getlocaltime(a)
  print (ts > 0 ? "ok" : "fail")
  print ("year" in a ? "has_year" : "no_year")
}"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    let lines: Vec<&str> = o.trim().lines().collect();
    assert_eq!(lines[0], "ok");
    assert_eq!(lines[1], "has_year");
}

// ── printf %a hex float ──────────────────────────────────────────────────

#[test]
fn printf_hex_float_a() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { printf "%a\n", 1.5 }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    let t = o.trim().to_lowercase();
    assert!(t.contains("0x") && t.contains("p"), "stdout={o:?}");
}

// ── close() pipe exit status ─────────────────────────────────────────────

#[test]
fn close_pipe_returns_exit_status() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN {
  cmd = "sh -c 'exit 42'"
  print "hi" | cmd
  r = close(cmd)
  print r
}"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    // Shell "exit 42" should yield 42
    assert_eq!(o.trim().lines().last().unwrap().trim(), "42");
}

// ── PROCINFO nproc key ───────────────────────────────────────────────────

#[test]
fn procinfo_has_nproc_key() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { print (PROCINFO["nproc"] >= 1 ? "ok" : "fail") }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    assert_eq!(o.trim(), "ok");
}

// ── FUNCTAB includes builtins ────────────────────────────────────────────

#[test]
fn functab_includes_builtin_length() {
    let (c, o, e) = run_awkrs_stdin(
        r#"BEGIN { print ("length" in FUNCTAB ? "ok" : "fail") }"#,
        "",
    );
    assert_eq!(c, 0, "stderr={e}");
    assert_eq!(o.trim(), "ok");
}

// ── posix mode rejects gawk extensions ───────────────────────────────────

#[test]
fn posix_mode_rejects_gawk_builtin() {
    let (c, _o, _e) = run_awkrs_stdin_args(
        ["-P"],
        r#"BEGIN { print typeof(1) }"#,
        "",
    );
    assert_ne!(c, 0, "posix mode should reject typeof()");
}
