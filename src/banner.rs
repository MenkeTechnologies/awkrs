//! AWKRS ASCII logo + live-stats box banner. Single source of truth shared by:
//!   - REPL startup (`repl::run`)
//!   - `awkrs --help` output (`cyber_help` sources the logo from here)
//!
//! Every count is pulled from the language reflection tables at call time
//! (`crate::lsp::AWK_KEYWORDS`, `crate::namespace::BUILTIN_NAMES`,
//! `crate::namespace::SPECIAL_GLOBAL_NAMES`) so the banner never goes stale
//! after a build adds keywords / builtins / special variables. System stats
//! (cores, memory) come from `sysinfo` at render time.

use crate::lsp::AWK_KEYWORDS;
use crate::namespace::{BUILTIN_NAMES, SPECIAL_GLOBAL_NAMES};

/// The AWKRS ASCII-shadow logo (six rows). Shared by the banner and the
/// `--help` HUD so the glyphs live in exactly one place.
pub const AWKRS_LOGO: &str = r" █████╗ ██╗    ██╗██╗  ██╗██████╗ ███████╗
██╔══██╗██║    ██║██║ ██╔╝██╔══██╗██╔════╝
███████║██║ █╗ ██║█████╔╝ ██████╔╝███████╗
██╔══██║██║███╗██║██╔═██╗ ██╔══██╗╚════██║
██║  ██║╚███╔███╔╝██║  ██╗██║  ██║███████║
╚═╝  ╚═╝ ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝";

/// Count of visible columns in `s`, ignoring ANSI SGR escape sequences.
/// Multi-byte UTF-8 is counted as one column per char — sufficient for the
/// box-drawing glyphs and Latin labels in the banner; East-Asian-Wide chars
/// would need a wcwidth-style lookup that we deliberately skip.
pub fn visible_width(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut w = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                i += 1;
            }
            i += 1;
        } else {
            let step = std::str::from_utf8(&bytes[i..])
                .ok()
                .and_then(|s| s.chars().next())
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            w += 1;
            i += step;
        }
    }
    w
}

/// Render the AWKRS logo + live-stats box + tagline into a string.
/// `colored=true` emits ANSI SGR escapes; `false` returns plain text.
pub fn render_banner(colored: bool) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let n_keywords = AWK_KEYWORDS.len();
    let n_builtins = BUILTIN_NAMES.len();
    let n_special = SPECIAL_GLOBAL_NAMES.len();

    let (mem_total_gib, mem_avail_gib) = {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let avail = sys.available_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        (total, avail)
    };

    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let pid = std::process::id();

    let (c, m, y, g, n) = if colored {
        ("\x1b[36m", "\x1b[35m", "\x1b[33m", "\x1b[32m", "\x1b[0m")
    } else {
        ("", "", "", "", "")
    };

    const INNER: usize = 64;
    let mut out = String::with_capacity(2048);

    let row = |out: &mut String, body: &str| {
        let pad = INNER.saturating_sub(visible_width(body));
        out.push_str(&format!("{c} │{n}{body}{:pad$}{c}│{n}\n", "", pad = pad));
    };

    // Logo (cyan → magenta → cyan gradient across the six rows).
    let logo_colors = [c, c, m, m, c, c];
    for (i, line) in AWKRS_LOGO.lines().enumerate() {
        let col = logo_colors.get(i).copied().unwrap_or(c);
        out.push_str(&format!("{col} {line}{n}\n"));
    }

    out.push_str(&format!(
        "{c} ┌────────────────────────────────────────────────────────────────┐{n}\n"
    ));
    row(
        &mut out,
        &format!(
            " {y}SYSTEM{n}  status:{g} ONLINE {c}//{n} {y}os:{n} {os} {y}arch:{n} {arch} {y}pid:{n} {pid}"
        ),
    );
    row(
        &mut out,
        &format!(
            " {y}CORES{n}   {cores}    {y}MEM{n}  {mem_avail_gib:.1} {c}/{n} {mem_total_gib:.1} GiB available"
        ),
    );
    out.push_str(&format!(
        "{c} ├────────────────────────────────────────────────────────────────┤{n}\n"
    ));
    row(
        &mut out,
        &format!(
            " {y}keywords{n}  {n_keywords:<6}  {y}builtins{n}  {n_builtins:<6}  {y}special vars{n} {n_special:<5}"
        ),
    );
    out.push_str(&format!(
        "{c} └────────────────────────────────────────────────────────────────┘{n}\n"
    ));
    out.push_str(&format!(
        "{m}  >> WORLD'S FASTEST AWK BYTECODE ENGINE // RUST-POWERED v{version} <<{n}\n"
    ));
    out
}

/// Print the banner to stdout. Convenience wrapper around [`render_banner`].
pub fn print_banner(colored: bool) {
    print!("{}", render_banner(colored));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_ignores_csi_sequences() {
        assert_eq!(visible_width("\x1b[31mabc\x1b[0m"), 3);
        assert_eq!(visible_width("\x1b[1;38;5;202mok"), 2);
    }

    #[test]
    fn visible_width_counts_each_char_once_for_multibyte() {
        // 3 box-drawing glyphs, each 3 bytes UTF-8, but one column each.
        assert_eq!(visible_width("─├┤"), 3);
        assert_eq!(visible_width("aé你"), 3);
    }

    #[test]
    fn visible_width_handles_empty_and_lone_escape() {
        assert_eq!(visible_width(""), 0);
        // Lone ESC with no `[` does not start a CSI; counts as 1 char.
        assert_eq!(visible_width("\x1bz"), 2);
    }

    #[test]
    fn render_banner_plain_has_no_ansi_escapes() {
        let s = render_banner(false);
        assert!(!s.contains('\x1b'), "plain banner must not contain ESC");
        assert!(s.contains("WORLD'S FASTEST AWK BYTECODE ENGINE"));
        assert!(s.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn render_banner_colored_contains_ansi_escapes() {
        let s = render_banner(true);
        assert!(s.contains("\x1b["));
        assert!(s.contains("\x1b[0m"));
    }

    #[test]
    fn render_banner_stats_reflect_live_source_tables() {
        // The numbers on the banner are pulled from the reflection tables, so
        // they must equal the current `.len()` — never hardcoded.
        let s = render_banner(false);
        assert!(s.contains(&AWK_KEYWORDS.len().to_string()));
        assert!(s.contains(&BUILTIN_NAMES.len().to_string()));
        assert!(s.contains(&SPECIAL_GLOBAL_NAMES.len().to_string()));
    }

    #[test]
    fn render_banner_rows_all_match_inner_width_after_strip() {
        // Anchor expected width to the top border, then prove every interior
        // row matches it. Catches drift in `row()` padding (or a stats row that
        // overflows the box) even if the box size is retuned later.
        let s = render_banner(false);
        let top = s
            .lines()
            .find(|l| l.starts_with(" ┌"))
            .expect("top border present");
        let want = visible_width(top);
        let mut box_rows = 0;
        for line in s.lines() {
            if line.starts_with(" │") && line.ends_with('│') {
                box_rows += 1;
                assert_eq!(
                    visible_width(line),
                    want,
                    "box row width drift on line: {line}"
                );
            }
        }
        assert!(box_rows >= 3, "expected the rendered box rows");
    }

    #[test]
    fn logo_has_six_rows_of_equal_visible_width() {
        let widths: Vec<usize> = AWKRS_LOGO.lines().map(visible_width).collect();
        assert_eq!(widths.len(), 6, "logo is six rows");
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "logo rows must share one visible width: {widths:?}"
        );
    }
}
