//! Command-line surface compatible with POSIX awk, GNU gawk, and mawk-style `-W` options.
//!
//! Extension flags are accepted for script compatibility; some only affect diagnostics.

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

    #[arg(short = 'd', long = "dump-variables", value_name = "FILE")]
    pub dump_variables: Option<String>,

    #[arg(short = 'D', long = "debug", value_name = "FILE")]
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

    #[arg(short = 'N', long = "use-lc-numeric")]
    pub use_lc_numeric: bool,

    #[arg(short = 'n', long = "non-decimal-data")]
    pub non_decimal_data: bool,

    #[arg(short = 'o', long = "pretty-print", value_name = "FILE")]
    pub pretty_print: Option<String>,

    #[arg(short = 'O', long = "optimize")]
    pub optimize: bool,

    #[arg(short = 'p', long = "profile", value_name = "FILE")]
    pub profile: Option<String>,

    #[arg(short = 'P', long = "posix")]
    pub posix: bool,

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
                    _ => {}
                }
                if let Some(rest) = part.strip_prefix("exec=") {
                    self.exec_file = Some(PathBuf::from(rest));
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
}

#[cfg(test)]
mod tests {
    use super::{Args, MawkWAction};
    use clap::Parser;

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
        let a = Args::try_parse_from([
            "awkrs",
            "-j",
            "2",
            "--read-ahead",
            "16",
            "{ print $1 }",
        ])
        .unwrap();
        assert_eq!(a.threads, Some(2));
        assert_eq!(a.read_ahead, 16);
    }
}
