//! Interactive REPL for `awkrs` / `aw` — a reedline line editor over the awk
//! engine.
//!
//! Layout per turn:
//!
//! ```text
//! ─( HH:MM:SS )──< command N >─────────────────────────────{ awkrs 0.4.16 }─
//! awkrs❯ <buffer>
//!         BEGIN         END           and           asort         atan2   …
//! ```
//!
//! awk has no cross-program persistent interpreter, so the REPL fakes a session
//! by accumulating user `function` definitions in a string and prepending them
//! to every subsequent line before handing the whole thing to the compiler.
//! Per entered line:
//!   * a `function NAME(...){...}` definition is parsed and, if valid, appended
//!     to the accumulator (nothing is printed);
//!   * a complete rule — one with an explicit action block (`{ ... }`) or a
//!     `BEGIN`/`END` pattern — runs as typed against empty input, like a script;
//!   * anything else is a bare statement or expression: it is first tried as a
//!     printed expression (`BEGIN { print (<line>) }`) so `1 + 2` shows `3`, and
//!     failing that as a statement body (`BEGIN { <line> }`) so `print "hi"` and
//!     `for (i=0;i<3;i++) print i` work.
//!
//! Completion words come from [`crate::lsp::completion_words`] — the same
//! keyword/builtin/special-var vocabulary the LSP serves — plus the names of
//! functions defined this session. History is `~/.awkrs/history`; edit mode
//! (emacs/vi) is read from `~/.awkrs/config.toml` or `AWKRS_REPL_MODE`.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use nu_ansi_term::{Color as NuColor, Style};
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    ColumnarMenu, Completer, EditMode, Emacs, FileBackedHistory, KeyCode, KeyModifiers,
    Keybindings, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion,
    ValidationResult, Validator, Vi,
};

use crate::parser::parse_program;

const AWKRS_VERSION: &str = env!("CARGO_PKG_VERSION");

fn awkrs_dir() -> std::path::PathBuf {
    let dir = dirs::home_dir()
        .map(|h| h.join(".awkrs"))
        .unwrap_or_else(|| std::path::PathBuf::from(".awkrs"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn history_path() -> std::path::PathBuf {
    awkrs_dir().join("history")
}

fn config_path() -> std::path::PathBuf {
    awkrs_dir().join("config.toml")
}

/// Contents of the auto-seeded `~/.awkrs/config.toml`. Every setting is
/// commented out so the seeded file documents the schema without changing
/// behavior — uncomment + edit a line to override the in-code default.
const DEFAULT_CONFIG_TOML: &str = r#"# awkrs runtime config — auto-generated on first REPL launch.
# Lines starting with `#` are comments. Uncomment + edit a line to
# override the in-code default. Delete this file and awkrs will
# regenerate it on the next run.

[repl]
# Edit mode for the interactive REPL. Defaults to emacs.
#
#   "emacs" — Ctrl-A/Ctrl-E/Ctrl-K/etc., readline-style (default)
#   "vi"    — modal editing; Esc → normal mode, i/a → insert,
#             h/j/k/l navigation, dd/cc/yy/x, /-search, etc.
#
# Tab + Shift+Tab cycle the completion menu in either mode.
# Override per-session with `AWKRS_REPL_MODE=vi awkrs`.
# mode = "emacs"
"#;

/// First-run seed: write `~/.awkrs/config.toml` if it does not exist. Safe to
/// call on every REPL launch — no-op when the file is already there (and silent
/// if the home directory is read-only). Honors `AWKRS_NO_CONFIG=1` for CI /
/// sandbox environments that should not touch the user's home dir.
fn ensure_default_config_seeded() {
    if std::env::var_os("AWKRS_NO_CONFIG").is_some() {
        return;
    }
    let path = config_path();
    if path.exists() {
        return;
    }
    let _ = std::fs::write(&path, DEFAULT_CONFIG_TOML);
}

/// REPL edit-mode selector. `Emacs` is the default; `Vi` enables reedline's
/// two-mode insert/normal keybinding set with the standard `Esc` toggle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ReplMode {
    Emacs,
    Vi,
}

/// Resolve the REPL edit mode in this precedence:
/// 1. `AWKRS_REPL_MODE=emacs|vi` env var (overrides everything).
/// 2. `~/.awkrs/config.toml` `[repl] mode = "vi"`.
/// 3. Default `Emacs`.
fn resolve_repl_mode() -> ReplMode {
    if let Some(env) = std::env::var_os("AWKRS_REPL_MODE") {
        let s = env.to_string_lossy().to_ascii_lowercase();
        if s == "vi" || s == "vim" {
            return ReplMode::Vi;
        }
        if s == "emacs" {
            return ReplMode::Emacs;
        }
    }
    let raw = match std::fs::read_to_string(config_path()) {
        Ok(s) => s,
        Err(_) => return ReplMode::Emacs,
    };
    let parsed: toml::Value = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return ReplMode::Emacs,
    };
    let mode = parsed
        .get("repl")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("emacs");
    match mode.to_ascii_lowercase().as_str() {
        "vi" | "vim" => ReplMode::Vi,
        _ => ReplMode::Emacs,
    }
}

/// Apply the completion-menu Tab / Shift+Tab bindings to a keybinding set —
/// shared so the bindings live on the emacs map AND the vi insert map.
fn install_menu_bindings(keybindings: &mut Keybindings) {
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
}

/// True when `c` can appear inside an awk identifier (`[A-Za-z0-9_]`).
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Byte index `start` and the incomplete word before the cursor (for prefix
/// matching). awk identifiers are `[A-Za-z_][A-Za-z0-9_]*`; everything else
/// (whitespace and awk punctuation `$`, `(`, `{`, operators, etc.) is a word
/// boundary. Unlike stryke there is no sigil-prefixed completion — awk field
/// refs `$1` / `$NF` complete on the bare tail after `$`.
fn completion_word_start(line: &str, pos: usize) -> (usize, &str) {
    let pos = pos.min(line.len());
    let before = line.get(..pos).unwrap_or("");
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !is_ident_char(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    (start, line.get(start..pos).unwrap_or(""))
}

struct AwkCompleter {
    static_words: Vec<String>,
    /// Names of functions defined this session — refreshed each turn.
    dynamic: Arc<Mutex<Vec<String>>>,
}

impl Completer for AwkCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let (start, prefix) = completion_word_start(line, pos);
        let span = Span::new(start, pos);

        let dyn_list = match self.dynamic.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut out: Vec<Suggestion> = Vec::new();
        for w in self.static_words.iter().chain(dyn_list.iter()) {
            if !w.starts_with(prefix) || !seen.insert(w.as_str()) {
                continue;
            }
            out.push(Suggestion {
                value: w.clone(),
                description: None,
                style: None,
                extra: None,
                span,
                append_whitespace: false,
                display_override: None,
                match_indices: None,
            });
        }
        out.sort_by(|a, b| a.value.cmp(&b.value));
        out
    }
}

/// Multi-line continuation: keep reading while there are more open `{` than
/// close `}` outside of string/regex-ish quoting, so a `function`/action block
/// spread over several lines is submitted as one unit.
struct BraceValidator;

impl Validator for BraceValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if brace_balanced(line) {
            ValidationResult::Complete
        } else {
            ValidationResult::Incomplete
        }
    }
}

/// True when `{`/`}` are balanced, ignoring braces inside `"..."` strings
/// (respecting `\` escapes) and `#` line comments. Returns true on excess
/// closers too — only a positive open-brace surplus means "keep typing".
fn brace_balanced(src: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    for c in src.chars() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '#' => break, // rest of the line is a comment
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

struct AwkPrompt {
    label: String,
    cmd_count: Arc<Mutex<u64>>,
}

fn now_hms() -> String {
    // Local time via `libc::localtime_r` — no chrono in the interactive path.
    // Falls back to UTC modulo math so the status bar always shows something.
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&secs, &mut tm).is_null() };
    if ok {
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    } else {
        let s = (secs as u64) % 86_400;
        format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

fn term_cols() -> usize {
    use std::os::unix::io::AsRawFd;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let fd = std::io::stdout().as_raw_fd();
    let cols = if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 {
        ws.ws_col as usize
    } else {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(80)
    };
    cols.max(40)
}

fn render_status_bar(label: &str, cmd_count: u64) -> String {
    let cols = term_cols();
    let dim = NuColor::DarkGray;
    let accent = NuColor::Cyan;
    let label_c = NuColor::LightYellow;

    let left = format!(" {} ", now_hms());
    let mid = format!(" command {} ", cmd_count);
    let right = format!(" {} {} ", label, AWKRS_VERSION);

    // Plain-text widths for layout math; `frame_chars` = display width of every
    // literal frame glyph emitted below (`─(`, `)──<`, `>`, `{`, `}─`).
    let frame_chars = "─()──<>{}─".chars().count();
    let visible = left.chars().count() + mid.chars().count() + right.chars().count() + frame_chars;
    let dashes = cols.saturating_sub(visible);
    if dashes < 2 {
        return format!(
            "{lp}{l}{rp}{ml}{m}{mr}",
            lp = Style::new().fg(dim).paint("─("),
            l = Style::new().fg(accent).paint(left),
            rp = Style::new().fg(dim).paint(")"),
            ml = Style::new().fg(dim).paint("──<"),
            m = Style::new().fg(label_c).bold().paint(mid),
            mr = Style::new().fg(dim).paint(">"),
        );
    }
    let left_dash = dashes / 2;
    let right_dash = dashes - left_dash;
    let bar_l = "─".repeat(left_dash);
    let bar_r = "─".repeat(right_dash);

    format!(
        "{lp}{l}{rp}{ml}{m}{mr}{bar}{rl}{r}{rr}",
        lp = Style::new().fg(dim).paint("─("),
        l = Style::new().fg(accent).paint(left),
        rp = Style::new().fg(dim).paint(")"),
        ml = Style::new().fg(dim).paint("──<"),
        m = Style::new().fg(label_c).bold().paint(mid),
        mr = Style::new().fg(dim).paint(">"),
        bar = Style::new().fg(dim).paint(format!("{}{}", bar_l, bar_r)),
        rl = Style::new().fg(dim).paint("{"),
        r = Style::new().fg(NuColor::Magenta).paint(right),
        rr = Style::new().fg(dim).paint("}─"),
    )
}

impl Prompt for AwkPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let count = self.cmd_count.lock().map(|g| *g).unwrap_or(0);
        let bar = render_status_bar(&self.label, count);
        let prompt = Style::new()
            .fg(NuColor::Cyan)
            .bold()
            .paint(&self.label)
            .to_string();
        Cow::Owned(format!("{}\n{}", bar, prompt))
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Owned(
            Style::new()
                .fg(NuColor::LightCyan)
                .bold()
                .paint("❯ ")
                .to_string(),
        )
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Owned(
            Style::new()
                .fg(NuColor::DarkGray)
                .paint("····❯ ")
                .to_string(),
        )
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}

/// Extract the function name from a `function NAME(...)` definition line, if the
/// trimmed line is one. Used to feed defined-this-session names to completion.
fn defined_function_name(src: &str) -> Option<String> {
    let rest = src.trim_start().strip_prefix("function")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| is_ident_char(*c) || *c == ':')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Evaluate one submitted REPL entry against the accumulated function
/// definitions. Returns the string to write to stdout (already carrying awk's
/// own newlines) on the success path, or the error string to write to stderr.
/// Kept free of I/O so it can be unit-tested. On a `function` definition the
/// definition is appended to `funcs` and `Ok(String::new())` is returned.
fn eval_entry(funcs: &mut String, line: &str) -> Result<String, String> {
    let trimmed = line.trim();

    // (a) `function ...` definition: parse in isolation; on success accumulate.
    if trimmed.starts_with("function ") || trimmed.starts_with("function\t") {
        return match parse_program(line) {
            Ok(_) => {
                funcs.push_str(line);
                funcs.push('\n');
                Ok(String::new())
            }
            Err(e) => Err(e.to_string()),
        };
    }

    // (b) A complete rule/program — one with an explicit action block (`{ ... }`)
    //     or a BEGIN/END special pattern — runs as typed, with the accumulated
    //     functions prepended. Interactive `{ print $1 }` / `/re/{ ... }` behave
    //     like a script fed empty input.
    let is_program =
        has_action_block(trimmed) || trimmed.starts_with("BEGIN") || trimmed.starts_with("END");
    if is_program {
        let candidate = format!("{}\n{}", funcs, line);
        return match parse_program(&candidate) {
            Ok(_) => crate::run_program(&candidate, "").map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
    }

    // (c) A bare statement or expression. Try it as a printed expression first
    //     (`BEGIN { print (<line>) }`) so `1 + 2` shows `3` and `substr(...)`
    //     shows its result — a bare expression is otherwise a no-output pattern
    //     over the REPL's empty input. Fall back to a statement body
    //     (`BEGIN { <line> }`) so `print "hi"` / `for (...) ...` work. On a
    //     double failure, surface the statement-form error (the more general).
    let expr_candidate = format!("{}\nBEGIN {{ print ({}) }}", funcs, line);
    if parse_program(&expr_candidate).is_ok() {
        return crate::run_program(&expr_candidate, "").map_err(|e| e.to_string());
    }
    let stmt_candidate = format!("{}\nBEGIN {{ {} }}", funcs, line);
    match parse_program(&stmt_candidate) {
        Ok(_) => crate::run_program(&stmt_candidate, "").map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Does `src` contain a top-level `{` (the start of an awk action block),
/// ignoring any `{` inside a double-quoted string? Such a line is a complete
/// rule and is run as typed; a line without one is a bare statement/expression
/// that `eval_entry` wraps in a `BEGIN { ... }` block.
fn has_action_block(src: &str) -> bool {
    let mut in_str = false;
    let mut escaped = false;
    for c in src.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => return true,
            _ => {}
        }
    }
    false
}

/// Launch the interactive REPL. `bin_name` (`awkrs` / `aw`) labels the prompt
/// status bar. Returns once the user leaves the REPL (`exit`, `quit`, Ctrl-D).
pub fn run(bin_name: &str) -> crate::Result<()> {
    ensure_default_config_seeded();

    // Startup banner (same logo the `--help` HUD shows) + a single hint line.
    crate::banner::print_banner(std::io::IsTerminal::is_terminal(&std::io::stdout()));
    println!();
    println!("\x1b[2m  type `exit` or Ctrl-D to leave the REPL — Tab for completion\x1b[0m");
    println!();

    let static_words = crate::lsp::completion_words();
    let dynamic: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let cmd_count = Arc::new(Mutex::new(0u64));

    let completer = AwkCompleter {
        static_words,
        dynamic: Arc::clone(&dynamic),
    };

    let menu = ColumnarMenu::default()
        .with_name("completion_menu")
        .with_columns(4)
        .with_column_padding(2);

    let edit_mode: Box<dyn EditMode> = match resolve_repl_mode() {
        ReplMode::Emacs => {
            let mut kb = default_emacs_keybindings();
            install_menu_bindings(&mut kb);
            Box::new(Emacs::new(kb))
        }
        ReplMode::Vi => {
            let mut insert_kb = default_vi_insert_keybindings();
            install_menu_bindings(&mut insert_kb);
            let normal_kb = default_vi_normal_keybindings();
            Box::new(Vi::new(insert_kb, normal_kb))
        }
    };

    let history = match FileBackedHistory::with_file(5_000, history_path()) {
        Ok(h) => Box::new(h) as Box<dyn reedline::History>,
        Err(e) => {
            eprintln!("{}: repl: history unavailable: {}", bin_name, e);
            match FileBackedHistory::new(5_000) {
                Ok(h) => Box::new(h) as Box<dyn reedline::History>,
                Err(e2) => {
                    return Err(crate::Error::Runtime(format!(
                        "repl: cannot create history: {e2}"
                    )));
                }
            }
        }
    };

    let mut line_editor = Reedline::create()
        .with_completer(Box::new(completer))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_edit_mode(edit_mode)
        .with_validator(Box::new(BraceValidator))
        .with_history(history);

    let prompt = AwkPrompt {
        label: bin_name.to_string(),
        cmd_count: Arc::clone(&cmd_count),
    };

    // Accumulated `function` definitions typed this session.
    let mut funcs = String::new();

    loop {
        let sig = match line_editor.read_line(&prompt) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: repl: {}", bin_name, e);
                break;
            }
        };

        match sig {
            Signal::Success(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let low = trimmed.to_lowercase();
                if low == "exit" || low == "quit" {
                    break;
                }

                if let Ok(mut g) = cmd_count.lock() {
                    *g += 1;
                }

                match eval_entry(&mut funcs, &line) {
                    // Result already carries awk's own newlines — `print!`, not `println!`.
                    Ok(out) => print!("{}", out),
                    Err(e) => eprintln!("{}", e),
                }

                // Refresh the completion word list with any newly-defined funcs.
                if let Ok(mut g) = dynamic.lock() {
                    *g = funcs.lines().filter_map(defined_function_name).collect();
                }
            }
            Signal::CtrlC => continue,
            Signal::CtrlD => break,
            _ => break,
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_word_at_cursor_bare_ident() {
        let s = "print subst";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(pre, "subst");
        assert_eq!(st, 6);
    }

    #[test]
    fn completion_word_after_dollar_takes_bare_tail() {
        // awk field ref `$NF` — the identifier tail after `$` is the prefix.
        let s = "print $NF";
        let (_, pre) = completion_word_start(s, s.len());
        assert_eq!(pre, "NF");
    }

    #[test]
    fn completion_word_empty_after_boundary() {
        let s = "length(";
        let (st, pre) = completion_word_start(s, s.len());
        assert_eq!(pre, "");
        assert_eq!(st, s.len());
    }

    #[test]
    fn brace_balance_tracks_open_close() {
        assert!(brace_balanced("BEGIN { print 1 }"));
        assert!(!brace_balanced("function f(x) {"));
        assert!(!brace_balanced("BEGIN {"));
        assert!(brace_balanced("1 + 1"));
    }

    #[test]
    fn brace_balance_ignores_braces_in_strings_and_comments() {
        assert!(brace_balanced("BEGIN { s = \"{\" }"));
        assert!(!brace_balanced("BEGIN { print \"}\"  # }"));
    }

    #[test]
    fn defined_function_name_parsed() {
        assert_eq!(
            defined_function_name("function foo(a, b) { return a }"),
            Some("foo".to_string())
        );
        assert_eq!(defined_function_name("functional()"), None);
        assert_eq!(defined_function_name("x = 1"), None);
    }

    #[test]
    fn has_action_block_ignores_braces_in_strings() {
        assert!(has_action_block("{ print $1 }"));
        assert!(has_action_block("/re/ { print }"));
        assert!(!has_action_block("1 + 2"));
        assert!(!has_action_block("print \"a{b}c\""));
        assert!(!has_action_block("length(\"{\")"));
    }

    #[test]
    fn eval_entry_statement_runs_as_begin_body() {
        // A bare statement that is not a valid printed expression falls back to
        // the `BEGIN { <line> }` statement wrap.
        let mut funcs = String::new();
        let out = eval_entry(&mut funcs, "print \"hi\"").expect("statement runs");
        assert_eq!(out.trim(), "hi");
        let out2 = eval_entry(&mut funcs, "for (i = 0; i < 3; i++) print i").expect("loop runs");
        assert_eq!(out2.split_whitespace().collect::<Vec<_>>(), ["0", "1", "2"]);
    }

    #[test]
    fn eval_entry_begin_block_prints() {
        let mut funcs = String::new();
        let out = eval_entry(&mut funcs, "BEGIN { print \"hi\" }").expect("begin runs");
        assert_eq!(out.trim(), "hi");
    }

    #[test]
    fn eval_entry_begin_prints_expression() {
        // The usable spelling for interactive arithmetic: wrap in BEGIN.
        let mut funcs = String::new();
        let out = eval_entry(&mut funcs, "BEGIN { print 1 + 2 }").expect("begin runs");
        assert_eq!(out.trim(), "3");
    }

    #[test]
    fn eval_entry_bare_expression_prints_value() {
        // A bare expression is wrapped as `BEGIN { print (<line>) }` so it shows
        // its value interactively instead of being a silent no-record pattern.
        let mut funcs = String::new();
        let out = eval_entry(&mut funcs, "1 + 2").expect("expression prints");
        assert_eq!(out.trim(), "3");
        let out2 = eval_entry(&mut funcs, "substr(\"abcdef\", 2, 3)").expect("builtin prints");
        assert_eq!(out2.trim(), "bcd");
        assert!(
            funcs.is_empty(),
            "expression must not touch the func accumulator"
        );
    }

    #[test]
    fn eval_entry_accumulates_function_callable_from_begin() {
        let mut funcs = String::new();
        // Defining a function prints nothing and stashes it in the accumulator.
        let out = eval_entry(&mut funcs, "function dbl(x) { return x * 2 }").expect("def ok");
        assert!(out.is_empty());
        assert!(funcs.contains("function dbl"));
        // The accumulated function is prepended to every later program, so a
        // BEGIN block can call it.
        let out2 = eval_entry(&mut funcs, "BEGIN { print dbl(21) }").expect("call ok");
        assert_eq!(out2.trim(), "42");
    }

    #[test]
    fn eval_entry_reports_original_error_on_double_parse_failure() {
        let mut funcs = String::new();
        // Neither a valid program nor a valid parenthesized expression.
        let err = eval_entry(&mut funcs, "for for for").expect_err("should fail");
        assert!(!err.is_empty());
    }
}
