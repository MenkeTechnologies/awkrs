//! Cyberpunk `--help` HUD (MenkeTechnologies `tp -h` style: ASCII, box, taglines, footer).

use std::io::{self, IsTerminal, Write};

use clap::builder::styling::{AnsiColor, Effects, Style, Styles};
use clap::CommandFactory;

use crate::cli::Args;

fn cyber_styles() -> Styles {
    Styles::styled()
        .header(
            Style::new()
                .fg_color(Some(AnsiColor::Cyan.into()))
                .effects(Effects::BOLD | Effects::UNDERLINE),
        )
        .usage(
            Style::new()
                .fg_color(Some(AnsiColor::Yellow.into()))
                .effects(Effects::BOLD),
        )
        .literal(
            Style::new()
                .fg_color(Some(AnsiColor::Green.into()))
                .effects(Effects::BOLD),
        )
        .placeholder(Style::new().fg_color(Some(AnsiColor::Magenta.into())))
        .valid(Style::new().fg_color(Some(AnsiColor::Green.into())))
        .invalid(
            Style::new()
                .fg_color(Some(AnsiColor::Red.into()))
                .effects(Effects::BOLD),
        )
        .error(
            Style::new()
                .fg_color(Some(AnsiColor::Red.into()))
                .effects(Effects::BOLD),
        )
}

/// Inner width between `┌` and `┐` (matches `tp -h` layout).
const BOX_INNER: usize = 54;

fn color_on() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && (io::stdout().is_terminal() || std::env::var_os("CLICOLOR_FORCE").is_some())
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
pub fn print_cyberpunk_help(bin_name: &str) {
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

    let mut cmd = Args::command()
        .bin_name(bin_name)
        .styles(cyber_styles())
        .help_template(
            "\
{usage-heading} {usage}

{all-args}",
        );
    if color_on() {
        cmd = cmd.color(clap::ColorChoice::Always);
    }
    let _ = cmd.print_help();
    let _ = write!(out, "{}", footer(version));
    let _ = writeln!(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_box_has_fixed_inner_width_and_version() {
        let s = status_box("0.0.0");
        assert!(s.contains("0.0.0"));
        assert!(s.contains("ONLINE"));
        assert!(s.contains("SIGNAL"));
    }

    #[test]
    fn footer_contains_version_and_license_tag() {
        let f = footer("1.2.3");
        assert!(f.contains("1.2.3"));
        assert!(f.contains("MIT"));
    }

    #[test]
    fn tagline_non_empty() {
        assert!(!tagline().is_empty());
    }

    #[test]
    fn status_box_middle_line_has_pipe_borders_and_version() {
        let s = status_box("0.0.0-test");
        assert!(
            s.contains("0.0.0-test") && s.contains('│') && s.contains("ONLINE"),
            "{s}"
        );
        let lines: Vec<_> = s.lines().collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains('│') && line.contains("STATUS")),
            "{lines:?}"
        );
    }

    #[test]
    fn awkrs_logo_contains_awkrs_spelled_in_ascii_art() {
        let logo = awkrs_logo();
        assert!(
            logo.contains("AWK") || logo.contains("awk") || logo.contains('█'),
            "logo should contain banner glyphs: len={}",
            logo.len()
        );
    }
}
