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

    /// Read-ahead queue depth between reader thread and engine (lines).
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
