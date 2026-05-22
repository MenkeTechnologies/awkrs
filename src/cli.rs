//! Command-line surface compatible with POSIX awk, GNU gawk, and mawk-style `-W` options.
//!
//! Extension flags are accepted for script compatibility; some only affect diagnostics.
//!
//! **Implemented behaviors (see `awkrs::run` and `cli_effects`):**
//! `-d`/`--dump-variables` (dump after run), `-D`/`--debug` (rule/function listing to file or stderr),
//! `-p`/`--profile` (awkrs-specific wall-clock summary; not gawk’s profiler format), `-o`/`--pretty-print`
//! (AST-derived listing; not gawk’s `--pretty-print` output), `-g`/`--gen-pot`, `-L`/`--lint`,
//! `-t`/`--lint-old`, `-S`/`--sandbox`, `-l`/`--load` (AWKPATH), `-b`/`--characters-as-bytes`,
//! `-c`/`--traditional`, `-P`/`--posix` (reserved flags on [`crate::runtime::Runtime`]), `-n`/`--non-decimal-data`,
//! `-s`/`--no-optimize` (disables JIT). `-O`/`--optimize` is accepted alongside gawk; JIT is on unless `-s` is set.
//! `-r`/`--re-interval` is accepted as a no-op (interval regex syntax is always available). `-N`/`--use-lc-numeric`
//! applies `LC_NUMERIC` to sprintf/printf and print/CONVFMT/OFMT formatting; string→number parsing still uses `.`.

use clap::{ArgAction, Parser, ValueHint};
use std::path::PathBuf;

/// Union of common awk implementations' CLI flags (POSIX `awk`, GNU `gawk`, `mawk` `-W`, BusyBox).
#[derive(Debug, Clone, Parser)]
#[command(
    name = "awkrs",
    about = "Pattern-directed scanning and processing (awk-compatible CLI; parallel record engine when the program is parallel-safe, else sequential).",
    trailing_var_arg = true,
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Args {
    // --- POSIX ---
    #[arg(short = 'f', long = "file", value_name = "PROGFILE", action = ArgAction::Append, value_hint = ValueHint::FilePath)]
    pub progfiles: Vec<PathBuf>,

    #[arg(short = 'F', long = "field-separator", value_name = "FS")]
    pub field_sep: Option<String>,

    #[arg(short = 'v', long = "assign", value_name = "var=val", action = ArgAction::Append)]
    pub assigns: Vec<String>,

    // --- GNU: program sources ---
    #[arg(short = 'e', long = "source", value_name = "PROGRAM", action = ArgAction::Append)]
    pub source: Vec<String>,

    #[arg(short = 'i', long = "include", value_name = "FILE", action = ArgAction::Append, value_hint = ValueHint::FilePath)]
    pub include: Vec<PathBuf>,

    // --- gawk extensions ---
    #[arg(short = 'b', long = "characters-as-bytes")]
    pub characters_as_bytes: bool,

    #[arg(short = 'c', long = "traditional")]
    pub traditional: bool,

    #[arg(short = 'C', long = "copyright")]
    pub copyright: bool,

    /// Dump variable state after execution (stdout, `-`, or a file path).
    #[arg(
        short = 'd',
        long = "dump-variables",
        value_name = "FILE",
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub dump_variables: Option<String>,

    #[arg(
        short = 'D',
        long = "debug",
        value_name = "FILE",
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub debug: Option<String>,

    #[arg(short = 'E', long = "exec", value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub exec_file: Option<PathBuf>,

    #[arg(short = 'g', long = "gen-pot")]
    pub gen_pot: bool,

    #[arg(short = 'I', long = "trace")]
    pub trace: bool,

    /// CSV mode (gawk-style): set `FS` to comma and `FPAT` for quoted fields (`""` escape).
    #[arg(short = 'k', long = "csv")]
    pub csv: bool,

    #[arg(short = 'l', long = "load", value_name = "LIB", action = ArgAction::Append)]
    pub load: Vec<String>,

    #[arg(short = 'L', long = "lint", value_name = "fatal|invalid|no-ext")]
    pub lint: Option<String>,

    #[arg(short = 'M', long = "bignum")]
    pub bignum: bool,

    /// Apply `LC_NUMERIC` to `sprintf`/`printf`/`print` and `%'` grouping; `$n` / `$0` string→number still uses `.`.
    #[arg(short = 'N', long = "use-lc-numeric")]
    pub use_lc_numeric: bool,

    #[arg(short = 'n', long = "non-decimal-data")]
    pub non_decimal_data: bool,

    /// Awk-like listing from the AST (awkrs format; not gawk `--pretty-print` output).
    #[arg(
        short = 'o',
        long = "pretty-print",
        value_name = "FILE",
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub pretty_print: Option<String>,

    #[arg(short = 'O', long = "optimize")]
    pub optimize: bool,

    /// Wall-clock summary and per-record-rule hits (awkrs format; not gawk `--profile` output).
    #[arg(
        short = 'p',
        long = "profile",
        value_name = "FILE",
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub profile: Option<String>,

    #[arg(short = 'P', long = "posix")]
    pub posix: bool,

    /// Accepted for script compatibility; no-op (`{m,n}` intervals are always enabled).
    #[arg(short = 'r', long = "re-interval")]
    pub re_interval: bool,

    #[arg(short = 's', long = "no-optimize")]
    pub no_optimize: bool,

    #[arg(short = 'S', long = "sandbox")]
    pub sandbox: bool,

    #[arg(short = 't', long = "lint-old")]
    pub lint_old: bool,

    // --- mawk / BusyBox `-W` ---
    #[arg(short = 'W', value_name = "OPT", action = ArgAction::Append)]
    pub mawk_w: Vec<String>,

    /// Threads for internal pools (default: 1).
    #[arg(short = 'j', long = "threads", value_name = "N")]
    pub threads: Option<usize>,

    /// Stdin chunk size (lines per batch) when using **`-j`** parallel record mode without input files;
    /// each batch is processed in parallel and printed in order before the next batch is read.
    #[arg(long = "read-ahead", default_value_t = 1024usize)]
    pub read_ahead: usize,

    /// Print help (cyberpunk HUD).
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    pub show_help: bool,

    /// Print version.
    #[arg(short = 'V', long = "version", action = ArgAction::SetTrue)]
    pub show_version: bool,

    /// Inline program and input files (use `--` before files if program starts with `-`).
    #[arg(value_name = "program [file ...]")]
    pub rest: Vec<String>,
}

impl Args {
    /// Hook for merging duplicate long/short flags if we add aliases later.
    pub fn normalize(&mut self) {}

    /// Parse `-W` tokens (mawk: comma-separated; `help` / `version` signal early exit).
    pub fn apply_mawk_w(&mut self) -> Result<(), MawkWAction> {
        for w in &self.mawk_w {
            for part in w.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                match part {
                    "help" | "usage" => return Err(MawkWAction::Help),
                    "version" | "v" => return Err(MawkWAction::Version),
                    "dump" => return Err(MawkWAction::Dump),
                    "posix_space" => { /* mawk: makes RS ignore \\n in \\s class — accepted silently */
                    }
                    "interactive" => { /* mawk: unbuffered stdout — accepted silently */ }
                    "random" => { /* mawk: seed from pid — accepted silently */ }
                    _ => {}
                }
                if let Some(rest) = part.strip_prefix("exec=") {
                    self.exec_file = Some(PathBuf::from(rest));
                }
                if let Some(rest) = part.strip_prefix("sprintf=") {
                    // mawk: set sprintf buffer size (silently accept any value)
                    let _ = rest;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MawkWAction {
    Help,
    Version,
    Dump,
}

#[cfg(test)]
mod tests {
    use super::{Args, MawkWAction};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn mawk_w_help_returns_action() {
        let mut a = Args::try_parse_from(["awkrs", "-W", "help"]).unwrap();
        assert!(matches!(a.apply_mawk_w(), Err(MawkWAction::Help)));
    }

    #[test]
    fn mawk_w_version_alias_v() {
        let mut a = Args::try_parse_from(["awkrs", "-W", "version"]).unwrap();
        assert!(matches!(a.apply_mawk_w(), Err(MawkWAction::Version)));
        let mut b = Args::try_parse_from(["awkrs", "-W", "v"]).unwrap();
        assert!(matches!(b.apply_mawk_w(), Err(MawkWAction::Version)));
    }

    #[test]
    fn mawk_w_comma_separated_merged() {
        let mut a = Args::try_parse_from(["awkrs", "-W", "help,version"]).unwrap();
        assert!(matches!(a.apply_mawk_w(), Err(MawkWAction::Help)));
    }

    #[test]
    fn mawk_w_exec_sets_exec_file() {
        let mut a = Args::try_parse_from(["awkrs", "-W", "exec=/tmp/x.awk"]).unwrap();
        assert!(a.apply_mawk_w().is_ok());
        assert_eq!(
            a.exec_file.as_deref(),
            Some(std::path::Path::new("/tmp/x.awk"))
        );
    }

    #[test]
    fn threads_and_field_sep_parse() {
        let a = Args::try_parse_from(["awkrs", "-j", "4", "-F", ",", "{print $1}"]).unwrap();
        assert_eq!(a.threads, Some(4));
        assert_eq!(a.field_sep.as_deref(), Some(","));
    }

    #[test]
    fn csv_flag_k_parses() {
        let a = Args::try_parse_from(["awkrs", "-k", "{print $1}"]).unwrap();
        assert!(a.csv);
    }

    #[test]
    fn assign_flag_collects() {
        let a = Args::try_parse_from(["awkrs", "-v", "a=1", "-v", "b=two", "{print a,b}"]).unwrap();
        assert_eq!(a.assigns, vec!["a=1".to_string(), "b=two".to_string()]);
    }

    #[test]
    fn read_ahead_parses_alongside_parallel_threads() {
        let a = Args::try_parse_from(["awkrs", "-j", "2", "--read-ahead", "16", "{ print $1 }"])
            .unwrap();
        assert_eq!(a.threads, Some(2));
        assert_eq!(a.read_ahead, 16);
    }

    #[test]
    fn gawk_compat_flags_parse_to_expected_fields() {
        let a = Args::try_parse_from([
            "awkrs", "-d", "-D", "-p", "-o", "-g", "-L", "fatal", "-t", "-S", "-l", "foo", "-b",
            "-c", "-P", "-n", "-O", "-s", "{print}",
        ])
        .unwrap();
        assert_eq!(a.dump_variables.as_deref(), Some(""));
        assert_eq!(a.debug.as_deref(), Some(""));
        assert_eq!(a.profile.as_deref(), Some(""));
        assert_eq!(a.pretty_print.as_deref(), Some(""));
        assert!(a.gen_pot);
        assert_eq!(a.lint.as_deref(), Some("fatal"));
        assert!(a.lint_old);
        assert!(a.sandbox);
        assert_eq!(a.load, vec!["foo".to_string()]);
        assert!(a.characters_as_bytes);
        assert!(a.traditional);
        assert!(a.posix);
        assert!(a.non_decimal_data);
        assert!(a.optimize);
        assert!(a.no_optimize);
    }

    #[test]
    fn dump_debug_pretty_profile_accept_optional_file() {
        let a = Args::try_parse_from([
            "awkrs", "-d", "/tmp/v", "-D", "/tmp/d", "-o", "/tmp/o", "-p", "/tmp/p", "1",
        ])
        .unwrap();
        assert_eq!(a.dump_variables.as_deref(), Some("/tmp/v"));
        assert_eq!(a.debug.as_deref(), Some("/tmp/d"));
        assert_eq!(a.pretty_print.as_deref(), Some("/tmp/o"));
        assert_eq!(a.profile.as_deref(), Some("/tmp/p"));
    }

    #[test]
    fn bignum_and_lc_numeric_flags_parse() {
        let a = Args::try_parse_from(["awkrs", "-M", "-N", "1"]).unwrap();
        assert!(a.bignum);
        assert!(a.use_lc_numeric);
    }

    #[test]
    fn copyright_and_trace_flags_parse() {
        let a = Args::try_parse_from(["awkrs", "-C", "-I", "{ }"]).unwrap();
        assert!(a.copyright);
        assert!(a.trace);
    }

    #[test]
    fn exec_short_flag_parses() {
        let a = Args::try_parse_from(["awkrs", "-E", "/tmp/prog.awk", "{ }"]).unwrap();
        assert_eq!(
            a.exec_file.as_deref(),
            Some(std::path::Path::new("/tmp/prog.awk"))
        );
    }

    #[test]
    fn include_short_flag_collects_paths() {
        let a = Args::try_parse_from(["awkrs", "-i", "/a.awk", "-i", "/b.awk", "{ }"]).unwrap();
        assert_eq!(a.include.len(), 2);
    }

    #[test]
    fn rest_positional_args_collect_program_and_files() {
        let a = Args::try_parse_from(["awkrs", "BEGIN{}", "file1", "file2"]).unwrap();
        assert_eq!(
            a.rest,
            vec![
                "BEGIN{}".to_string(),
                "file1".to_string(),
                "file2".to_string()
            ]
        );
    }

    #[test]
    fn trailing_var_args_after_double_dash() {
        let a = Args::try_parse_from(["awkrs", "--", "-v", "not-an-assign", "file"]).unwrap();
        assert!(a.assigns.is_empty());
        assert_eq!(
            a.rest,
            vec![
                "-v".to_string(),
                "not-an-assign".to_string(),
                "file".to_string()
            ]
        );
    }

    #[test]
    fn multiple_source_flags_collect() {
        let a = Args::try_parse_from(["awkrs", "-e", "rule1", "-e", "rule2"]).unwrap();
        assert_eq!(a.source, vec!["rule1".to_string(), "rule2".to_string()]);
    }

    #[test]
    fn mixed_file_and_source_flags() {
        let a = Args::try_parse_from(["awkrs", "-f", "p1.awk", "-e", "rule1"]).unwrap();
        assert_eq!(a.progfiles.len(), 1);
        assert_eq!(a.source, vec!["rule1".to_string()]);
    }

    #[test]
    fn fs_flag_sets_field_sep_v3() {
        let a = Args::try_parse_from(["awkrs", "-F", ":", "1"]).unwrap();
        assert_eq!(a.field_sep, Some(":".to_string()));
    }

    #[test]
    fn csv_flag_sets_csv_mode_v3() {
        let a = Args::try_parse_from(["awkrs", "--csv", "1"]).unwrap();
        assert!(a.csv);
    }

    #[test]
    fn jobs_flag_sets_threads_v3() {
        let a = Args::try_parse_from(["awkrs", "-j", "8", "1"]).unwrap();
        assert_eq!(a.threads, Some(8));
    }

    #[test]
    fn copyright_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-C"]).unwrap();
        assert!(a.copyright);
    }

    #[test]
    fn traditional_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-c"]).unwrap();
        assert!(a.traditional);
    }

    #[test]
    fn bignum_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-M"]).unwrap();
        assert!(a.bignum);
    }

    #[test]
    fn lint_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-L", "1"]).unwrap();
        assert!(a.lint.is_some());
    }

    #[test]
    fn profile_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-p", "1"]).unwrap();
        assert!(a.profile.is_some());
    }

    #[test]
    fn sandbox_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-S", "1"]).unwrap();
        assert!(a.sandbox);
    }

    #[test]
    fn include_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-i", "lib.awk", "1"]).unwrap();
        assert_eq!(a.include[0], PathBuf::from("lib.awk"));
    }

    #[test]
    fn dump_variables_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-d", "vars.out", "1"]).unwrap();
        assert_eq!(a.dump_variables, Some("vars.out".into()));
    }

    #[test]
    fn debug_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-D", "debug.out", "1"]).unwrap();
        assert_eq!(a.debug, Some("debug.out".into()));
    }

    #[test]
    fn optimize_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-O", "1"]).unwrap();
        assert!(a.optimize);
    }

    #[test]
    fn posix_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-P", "1"]).unwrap();
        assert!(a.posix);
    }

    #[test]
    fn re_interval_flag_v2() {
        let a = Args::try_parse_from(["awkrs", "-r", "1"]).unwrap();
        assert!(a.re_interval);
    }

    #[test]
    fn cli_no_args_v16() {
        assert!(Args::try_parse_from(["awkrs"]).is_ok());
    } // Defaults to stdin
    #[test]
    fn cli_version_long_v16() {
        assert!(
            Args::try_parse_from(["awkrs", "--version"])
                .unwrap()
                .show_version
        );
    }
    #[test]
    fn cli_file_v16() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-f", "f.awk"])
                .unwrap()
                .progfiles
                .len(),
            1
        );
    }
    #[test]
    fn cli_assign_v16() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-v", "x=1", "1"])
                .unwrap()
                .assigns
                .len(),
            1
        );
    }
    #[test]
    fn cli_source_v16() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-e", "BEGIN{print 1}"])
                .unwrap()
                .source
                .len(),
            1
        );
    }
    #[test]
    fn cli_input_file_v16() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "1", "in.txt"])
                .unwrap()
                .rest
                .len(),
            2
        );
    }
    #[test]
    fn cli_multiple_input_files_v16() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "1", "f1", "f2"])
                .unwrap()
                .rest
                .len(),
            3
        );
    }
    #[test]
    fn cli_csv_v16() {
        assert!(Args::try_parse_from(["awkrs", "--csv", "1"]).unwrap().csv);
    }
    #[test]
    fn cli_threads_v16() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-j", "4", "1"])
                .unwrap()
                .threads,
            Some(4)
        );
    }

    #[test]
    fn cli_sandbox_v22() {
        assert!(Args::try_parse_from(["awkrs", "-S", "1"]).unwrap().sandbox);
    }
    #[test]
    fn cli_lint_v22() {
        assert!(Args::try_parse_from(["awkrs", "-L", "1"])
            .unwrap()
            .lint
            .is_some());
    }
    #[test]
    fn cli_lint_old_v22() {
        assert!(Args::try_parse_from(["awkrs", "-t", "1"]).unwrap().lint_old);
    }
    #[test]
    fn cli_trace_v22() {
        assert!(
            Args::try_parse_from(["awkrs", "--trace", "1"])
                .unwrap()
                .trace
        );
    }
    #[test]
    fn cli_gen_pot_v22() {
        assert!(
            Args::try_parse_from(["awkrs", "--gen-pot", "1"])
                .unwrap()
                .gen_pot
        );
    }
    #[test]
    fn cli_traditional_v22() {
        assert!(
            Args::try_parse_from(["awkrs", "-c", "1"])
                .unwrap()
                .traditional
        );
    }
    #[test]
    fn cli_copyright_v22() {
        assert!(
            Args::try_parse_from(["awkrs", "-C", "1"])
                .unwrap()
                .copyright
        );
    }
    #[test]
    fn cli_bignum_v22() {
        assert!(Args::try_parse_from(["awkrs", "-M", "1"]).unwrap().bignum);
    }
    #[test]
    fn cli_use_lc_numeric_v22() {
        assert!(
            Args::try_parse_from(["awkrs", "-N", "1"])
                .unwrap()
                .use_lc_numeric
        );
    }
    #[test]
    fn cli_non_decimal_data_v22() {
        assert!(
            Args::try_parse_from(["awkrs", "-n", "1"])
                .unwrap()
                .non_decimal_data
        );
    }

    #[test]
    fn cli_f_v55_0() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-F", ":", "1"])
                .unwrap()
                .field_sep,
            Some(":".into())
        );
    }
    #[test]
    fn cli_f_v55_1() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-F:", "1"])
                .unwrap()
                .field_sep,
            Some(":".into())
        );
    }
    #[test]
    fn cli_v_v55_0() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-v", "x=1", "1"])
                .unwrap()
                .assigns[0],
            "x=1"
        );
    }
    #[test]
    fn cli_v_v55_1() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-vx=1", "1"])
                .unwrap()
                .assigns[0],
            "x=1"
        );
    }
    #[test]
    fn cli_f_file_v55() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-f", "a.awk", "1"])
                .unwrap()
                .progfiles[0]
                .to_str()
                .unwrap(),
            "a.awk"
        );
    }
    #[test]
    fn cli_e_source_v55() {
        assert_eq!(
            Args::try_parse_from(["awkrs", "-e", "1", "1"])
                .unwrap()
                .source[0],
            "1"
        );
    }

    #[test]
    fn cli_posix_v55() {
        assert!(Args::try_parse_from(["awkrs", "-P", "1"]).unwrap().posix);
    }
    #[test]
    fn cli_traditional_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "-c", "1"])
                .unwrap()
                .traditional
        );
    }
    #[test]
    fn cli_bignum_v55() {
        assert!(Args::try_parse_from(["awkrs", "-M", "1"]).unwrap().bignum);
    }
    #[test]
    fn cli_sandbox_v55() {
        assert!(Args::try_parse_from(["awkrs", "-S", "1"]).unwrap().sandbox);
    }
    #[test]
    fn cli_lint_v55() {
        assert!(Args::try_parse_from(["awkrs", "-L", "1"])
            .unwrap()
            .lint
            .is_some());
    }
    #[test]
    fn cli_lint_old_v55() {
        assert!(Args::try_parse_from(["awkrs", "-t", "1"]).unwrap().lint_old);
    }
    #[test]
    fn cli_non_decimal_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "-n", "1"])
                .unwrap()
                .non_decimal_data
        );
    }
    #[test]
    fn cli_use_lc_numeric_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "-N", "1"])
                .unwrap()
                .use_lc_numeric
        );
    }
    #[test]
    fn cli_re_interval_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "-r", "1"])
                .unwrap()
                .re_interval
        );
    }
    #[test]
    fn cli_csv_v55() {
        assert!(Args::try_parse_from(["awkrs", "--csv", "1"]).unwrap().csv);
    }
    #[test]
    fn cli_trace_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "--trace", "1"])
                .unwrap()
                .trace
        );
    }
    #[test]
    fn cli_gen_pot_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "--gen-pot", "1"])
                .unwrap()
                .gen_pot
        );
    }
    #[test]
    fn cli_copyright_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "-C", "1"])
                .unwrap()
                .copyright
        );
    }
    #[test]
    fn cli_version_v55() {
        assert!(
            Args::try_parse_from(["awkrs", "-V", "1"])
                .unwrap()
                .show_version
        );
    }
}
