//! Cyberpunk `--help` HUD (MenkeTechnologies `tp -h` style: ASCII, box, taglines, footer).

use std::io::{self, IsTerminal, Write};

use clap::CommandFactory;

use crate::cli::Args;

/// Inner width between `┌` and `┐` (matches `tp -h` layout).
const BOX_INNER: usize = 54;

fn color_on() -> bool {
    std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}

fn c_cyan(s: &str) -> String {
    if color_on() {
        format!("\x1b[36m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn c_magenta(s: &str) -> String {
    if color_on() {
        format!("\x1b[35m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn c_yellow(s: &str) -> String {
    if color_on() {
        format!("\x1b[33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn c_dim(s: &str) -> String {
    if color_on() {
        format!("\x1b[2m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn awkrs_logo() -> String {
    let art = r" █████╗ ██╗    ██╗██╗  ██╗██████╗ ███████╗
██╔══██╗██║    ██║██║ ██╔╝██╔══██╗██╔════╝
███████║██║ █╗ ██║█████╔╝ ██████╔╝███████╗
██╔══██║██║███╗██║██╔═██╗ ██╔══██╗╚════██║
██║  ██║╚███╔███╔╝██║  ██╗██║  ██║███████║
╚═╝  ╚═╝ ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝";
    c_cyan(art)
}

fn status_box(version: &str) -> String {
    let top = format!(" ┌{}┐", "─".repeat(BOX_INNER));
    let bottom = format!(" └{}┘", "─".repeat(BOX_INNER));
    let mut inner = format!(" STATUS: ONLINE  // SIGNAL: ████████░░ // v{version}");
    while inner.chars().count() < BOX_INNER {
        inner.push(' ');
    }
    if inner.chars().count() > BOX_INNER {
        inner = inner.chars().take(BOX_INNER).collect();
    }
    let mid = format!(" │{inner}│");
    format!("{}\n{}\n{}", c_cyan(&top), c_magenta(&mid), c_cyan(&bottom))
}

fn tagline() -> String {
    let s = " >> PATTERN / ACTION ENGINE // FIELD RECORDS & TEXT HAX <<";
    c_yellow(s)
}

fn footer(version: &str) -> String {
    let rule = "─".repeat(58);
    let line1 = format!("  {rule}");
    let line2 = format!("  {version} // (c) MenkeTechnologies // MIT");
    let line3 = "  >>> PARSE THE STREAM. SPLIT THE FIELDS. JACK IN. <<<";
    let dots = "░".repeat(55);
    format!(
        "\n{}\n{}\n{}\n{}\n",
        c_dim(&line1),
        c_dim(&line2),
        c_yellow(line3),
        c_dim(&dots)
    )
}

/// Full cyberpunk help: banner + clap usage/args + footer.
pub fn print_cyberpunk_help() {
    let version = env!("CARGO_PKG_VERSION");
    let mut out = io::stdout();
    let _ = writeln!(out, "{}", awkrs_logo());
    let _ = writeln!(out, "{}", status_box(version));
    let _ = writeln!(out, " {}", tagline());
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{}",
        c_dim(
            "Pattern-directed scanning: union CLI (POSIX / gawk / mawk-style). Sequential engine."
        )
    );
    let _ = writeln!(out);

    let _ = Args::command()
        .help_template(
            "\
{usage-heading} {usage}

{all-args}",
        )
        .print_help();
    let _ = write!(out, "{}", footer(version));
    let _ = writeln!(out);
}
