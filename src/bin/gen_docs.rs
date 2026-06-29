//! Offline generator for `docs/reference.html` — the full awkrs language
//! reference rendered as a static HTML page using the same cyberpunk styling
//! as `docs/index.html`. Run with `cargo run --bin gen-docs` before pushing to
//! GitHub Pages.
//!
//! Source of truth: the LSP documentation corpus in `awkrs::lsp`
//! (`builtin_signature`, `special_doc`, `keyword_doc`, `AWK_KEYWORDS`). The
//! generator and the LSP hover path render from the exact same data, so the
//! static page never drifts from what editors show on hover.
//!
//! Chapters are explicit ordered name lists (the corpus is stored as `match`
//! arms, which aren't enumerable), and each name is resolved through the shared
//! lookups so a builtin, special variable, or keyword renders identically to
//! its hover markdown.
//!
//! The markdown → HTML converter is intentionally minimal (in-house, no crate
//! dependency): it handles what the corpus actually uses — fenced `awk` code
//! blocks, inline backticks, `**bold**`, paragraph breaks, `###` headings, and
//! bullet lists.

use std::fs;
use std::path::PathBuf;

use awkrs::lsp::{builtin_signature, keyword_doc, special_doc};

fn main() {
    let out_path = PathBuf::from("docs/reference.html");
    let html = build_page();
    fs::write(&out_path, html).expect("write docs/reference.html");
    println!("wrote {}", out_path.display());
}

/// One reference chapter: a heading plus an explicit ordered list of corpus
/// names to render under it.
struct Chapter {
    title: &'static str,
    names: &'static [&'static str],
}

/// Ordered chapters covering the whole corpus. Names are resolved through the
/// shared `awkrs::lsp` lookups at render time.
const CHAPTERS: &[Chapter] = &[
    Chapter {
        title: "String Functions",
        names: &[
            "length", "substr", "index", "split", "sub", "gsub", "match", "sprintf", "tolower",
            "toupper", "gensub",
        ],
    },
    Chapter {
        title: "Arithmetic Functions",
        names: &[
            "sin", "cos", "atan2", "exp", "log", "sqrt", "int", "rand", "srand",
        ],
    },
    Chapter {
        title: "I/O & General Functions",
        names: &["print", "printf", "getline", "close", "system", "fflush"],
    },
    Chapter {
        title: "Time Functions (gawk)",
        names: &["systime", "strftime", "mktime"],
    },
    Chapter {
        title: "Bitwise Functions (gawk)",
        names: &["and", "or", "xor", "compl", "lshift", "rshift"],
    },
    Chapter {
        title: "Array & Type Functions (gawk)",
        names: &["typeof", "isarray", "patsplit", "asort", "asorti"],
    },
    Chapter {
        title: "Special Variables",
        names: &[
            "NR",
            "NF",
            "FS",
            "OFS",
            "ORS",
            "RS",
            "FILENAME",
            "FNR",
            "RSTART",
            "RLENGTH",
            "SUBSEP",
            "ARGC",
            "ARGV",
            "ENVIRON",
            "CONVFMT",
            "OFMT",
            "IGNORECASE",
            "FIELDWIDTHS",
            "FPAT",
            "PROCINFO",
            "RT",
            "ERRNO",
            "TEXTDOMAIN",
            "BINMODE",
            "LINT",
        ],
    },
    Chapter {
        title: "Keywords & Control Flow",
        names: &[
            "BEGIN",
            "END",
            "BEGINFILE",
            "ENDFILE",
            "function",
            "if",
            "else",
            "while",
            "for",
            "do",
            "break",
            "continue",
            "next",
            "nextfile",
            "exit",
            "return",
            "delete",
            "switch",
            "case",
            "default",
            "in",
        ],
    },
];

/// Resolve a corpus name to its hover-identical markdown, trying builtins
/// first, then special variables, then keywords. Returns `None` if the name
/// is undocumented (so the generator can warn rather than emit a blank entry).
fn doc_markdown(name: &str) -> Option<String> {
    if let Some((sig, desc)) = builtin_signature(name) {
        Some(format!("```awk\n{sig}\n```\n\n{desc}"))
    } else if let Some(desc) = special_doc(name) {
        Some(format!("**`{name}`** — special variable\n\n{desc}"))
    } else {
        keyword_doc(name).map(|desc| format!("**`{name}`** — keyword\n\n{desc}"))
    }
}

fn build_page() -> String {
    // Collect (chapter, [(name, markdown)]) in declaration order, warning on
    // any name the corpus doesn't document so nothing silently vanishes.
    let mut chapters: Vec<(&'static str, Vec<(&'static str, String)>)> = Vec::new();
    for ch in CHAPTERS {
        let mut rows: Vec<(&'static str, String)> = Vec::new();
        for &name in ch.names {
            match doc_markdown(name) {
                Some(md) => rows.push((name, md)),
                None => eprintln!("warning: no doc for `{name}` (chapter `{}`)", ch.title),
            }
        }
        if !rows.is_empty() {
            chapters.push((ch.title, rows));
        }
    }

    let total_topics: usize = chapters.iter().map(|(_, r)| r.len()).sum();
    let chapter_count = chapters.len();

    // ── render ──────────────────────────────────────────────────────────
    let mut out = String::with_capacity(256 * 1024);
    out.push_str(HEAD);
    out.push_str(&format!(
        r#"  <header class="tutorial-header">
    <div class="tutorial-header-inner">
      <div>
        <h1 class="tutorial-brand">// AWKRS — FULL REFERENCE</h1>
        <nav class="tutorial-crumbs" aria-label="Breadcrumb">
          <a href="index.html">Docs</a>
          <span class="sep">/</span>
          <span class="current">Reference</span>
          <span class="sep">/</span>
          <a href="https://github.com/MenkeTechnologies/awkrs" target="_blank" rel="noopener noreferrer">GitHub</a>
        </nav>
        <p class="docs-build-line">awkrs v{version} · {total_topics} topics · {chapter_count} chapters · generated from <code>awkrs/src/lsp.rs</code></p>
      </div>
      <div class="tutorial-toolbar">
        <button type="button" class="btn btn-secondary" id="btnTheme" title="Toggle light/dark">Theme</button>
        <button type="button" class="btn btn-secondary active" id="btnCrt" title="CRT scanline overlay">CRT</button>
        <button type="button" class="btn btn-secondary active" id="btnNeon" title="Neon border pulse">Neon</button>
        <a class="btn btn-secondary" href="index.html">Hub</a>
        <a class="btn btn-secondary" href="https://github.com/MenkeTechnologies/awkrs" target="_blank" rel="noopener noreferrer">GitHub</a>
      </div>
    </div>
  </header>

  <div class="hub-scheme-strip">
    <div class="hub-scheme-strip-inner">
      <span class="hud-scheme-label">// Color scheme</span>
      <div class="scheme-grid" id="hudSchemeGrid"></div>
    </div>
  </div>

  <main class="tutorial-main">
    <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>LANGUAGE REFERENCE</h2>
    <p class="tutorial-subtitle">Every builtin, special variable, and keyword awkrs documents, rendered from the exact markdown that the awkrs LSP shows on hover. Jump via the chapter index, or <kbd>Ctrl+F</kbd> for a specific name.</p>
"#,
        version = env!("CARGO_PKG_VERSION"),
        total_topics = total_topics,
        chapter_count = chapter_count,
    ));

    // Chapter index
    out.push_str(
        r#"    <section class="tutorial-section">
      <h2>Chapters</h2>
      <ul class="chapter-index">
"#,
    );
    for (chapter, rows) in &chapters {
        let slug = slugify(chapter);
        out.push_str(&format!(
            "        <li><a href=\"#ch-{slug}\">{chapter}</a> <span class=\"chapter-count\">{n}</span></li>\n",
            slug = slug,
            chapter = html_escape(chapter),
            n = rows.len(),
        ));
    }
    out.push_str("      </ul>\n    </section>\n");

    // Chapters and their topics
    for (chapter, rows) in &chapters {
        let slug = slugify(chapter);
        out.push_str(&format!(
            r#"    <section class="tutorial-section" id="ch-{slug}">
      <h2>{chapter}</h2>
      <p class="chapter-meta">{n} topics</p>
"#,
            slug = slug,
            chapter = html_escape(chapter),
            n = rows.len(),
        ));
        for (topic, md) in rows {
            let topic_slug = slugify(topic);
            let topic_escaped = html_escape(topic);
            out.push_str("      <article class=\"doc-entry\" id=\"doc-");
            out.push_str(&topic_slug);
            out.push_str("\">\n        <h3><a class=\"doc-anchor\" href=\"#doc-");
            out.push_str(&topic_slug);
            out.push_str("\">#</a> <code>");
            out.push_str(&topic_escaped);
            out.push_str("</code></h3>\n");
            out.push_str(&markdown_to_html(md));
            out.push_str("      </article>\n");
        }
        out.push_str("    </section>\n");
    }

    out.push_str(FOOT);
    out
}

// ─────────────────────────────────────────────────────────────────────────
// Minimal markdown → HTML converter. Scope: what the LSP corpus actually
// uses. Blocks: fenced code, `### heading`, blank-line-separated paragraphs,
// `-`/`*` bullet lists. Inlines: `backtick code` and `**bold**`. Everything
// else is HTML-escaped and passes through as plain text.
// ─────────────────────────────────────────────────────────────────────────
fn markdown_to_html(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + md.len() / 4);
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Fenced code block: ```LANG … ```
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            let lang = rest.trim().to_string();
            let lang_attr = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"lang-{}\"", html_escape(&lang))
            };
            out.push_str(&format!("        <pre><code{lang_attr}>"));
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                out.push_str(&html_escape(l));
                out.push('\n');
                i += 1;
            }
            out.push_str("</code></pre>\n");
            continue;
        }

        // Heading: `###` (the only level the corpus uses).
        if let Some(body) = line.strip_prefix("### ") {
            out.push_str(&format!("        <h4>{}</h4>\n", inline(body)));
            i += 1;
            continue;
        }
        if let Some(body) = line.strip_prefix("## ") {
            out.push_str(&format!("        <h4>{}</h4>\n", inline(body)));
            i += 1;
            continue;
        }

        // Bullet list.
        if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
            out.push_str("        <ul>\n");
            while i < lines.len() {
                let l = lines[i];
                let t = l.trim_start();
                let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
                    break;
                };
                out.push_str(&format!("          <li>{}</li>\n", inline(item)));
                i += 1;
            }
            out.push_str("        </ul>\n");
            continue;
        }

        // Blank line → paragraph boundary.
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // Paragraph: accumulate contiguous non-blank, non-block lines.
        let mut para = String::new();
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim_start();
            if l.trim().is_empty()
                || t.starts_with("```")
                || t.starts_with("### ")
                || t.starts_with("## ")
                || t.starts_with("- ")
                || t.starts_with("* ")
            {
                break;
            }
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(l.trim());
            i += 1;
        }
        if !para.is_empty() {
            out.push_str(&format!("        <p>{}</p>\n", inline(&para)));
        }
    }
    out
}

/// Inline pass: `backtick code` spans and `**bold**` spans, otherwise
/// HTML-escape. Single `*em*` is intentionally not supported because awk
/// docs contain `*` in expressions; bold uses doubled `**`.
fn inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if bytes[i] == b'`' {
            // Find matching backtick.
            let start = i + 1;
            let mut j = start;
            while j < s.len() && bytes[j] != b'`' {
                j += 1;
            }
            if j < s.len() {
                out.push_str("<code>");
                out.push_str(&html_escape(&s[start..j]));
                out.push_str("</code>");
                i = j + 1;
                continue;
            }
        }
        // `**bold**`. Require the closing `**` to also exist; otherwise fall
        // through and treat the literal `**` as text.
        if i + 1 < s.len() && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < s.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    break;
                }
                j += 1;
            }
            if j + 1 < s.len() && bytes[j] == b'*' && bytes[j + 1] == b'*' {
                out.push_str("<strong>");
                out.push_str(&inline(&s[start..j]));
                out.push_str("</strong>");
                i = j + 2;
                continue;
            }
        }
        // Default: html-escape this one char.
        let c = &s[i..i + char_len(bytes, i)];
        out.push_str(&html_escape(c));
        i += c.len();
    }
    out
}

fn char_len(bytes: &[u8], i: usize) -> usize {
    let b = bytes[i];
    if b < 0x80 {
        1
    } else if b & 0xE0 == 0xC0 {
        2
    } else if b & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

const HEAD: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark light">
  <meta name="description" content="awkrs full reference — every builtin, special variable, and keyword with its LSP hover doc rendered as a static page.">
  <title>awkrs — Reference</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;700;900&amp;family=Share+Tech+Mono&amp;display=swap" rel="stylesheet">
  <link rel="stylesheet" href="hud-static.css">
  <link rel="stylesheet" href="tutorial.css">
  <style>
    .tutorial-main { max-width: 72rem; }
    .docs-build-line {
      margin: 0.35rem 0 0;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 11px; color: var(--text-dim);
      letter-spacing: 0.03em; max-width: 42rem; opacity: 0.75;
    }
    .hub-scheme-strip {
      border-bottom: 1px dashed var(--border);
      background: color-mix(in srgb, var(--bg-secondary) 85%, transparent);
      padding: 0.55rem 1.5rem 0.65rem; position: relative;
    }
    .hub-scheme-strip-inner {
      max-width: 72rem; margin: 0 auto;
      display: flex; align-items: center; gap: 0.85rem;
    }
    .hub-scheme-strip .hud-scheme-label {
      flex: 0 0 auto;
      font-family: 'Orbitron', sans-serif; font-size: 9px; font-weight: 700;
      letter-spacing: 2px; text-transform: uppercase; color: var(--accent);
    }
    .hub-scheme-strip .scheme-grid {
      flex: 1 1 auto;
      display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 6px;
    }
    @media (max-width: 720px) {
      .hub-scheme-strip-inner { flex-direction: column; align-items: stretch; }
      .hub-scheme-strip .scheme-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    }

    .chapter-index {
      list-style: none; padding: 0; margin: 0;
      display: grid; grid-template-columns: repeat(auto-fill, minmax(18rem, 1fr));
      gap: 0.3rem;
    }
    .chapter-index li {
      border: 1px solid var(--border); padding: 0.45rem 0.65rem; border-radius: 2px;
      background: color-mix(in srgb, var(--bg-card) 92%, transparent);
      display: flex; justify-content: space-between; align-items: baseline;
    }
    .chapter-index li a {
      color: var(--cyan); text-decoration: none; font-size: 13px;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }
    .chapter-index li a:hover { color: var(--accent-light); }
    .chapter-count {
      font-size: 10px; color: var(--text-muted);
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }
    .chapter-meta {
      font-size: 11px; color: var(--text-muted); margin: -0.3rem 0 0.8rem;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }

    .doc-entry {
      margin: 1rem 0 1.4rem;
      padding: 0.75rem 0.9rem 0.5rem;
      border-left: 2px solid var(--cyan);
      background: color-mix(in srgb, var(--bg) 94%, transparent);
      border-radius: 2px;
    }
    .doc-entry h3 {
      margin: 0 0 0.45rem;
      font-family: 'Orbitron', sans-serif;
      font-size: 13px; font-weight: 700; letter-spacing: 1.5px;
      text-transform: uppercase; color: var(--cyan);
    }
    .doc-entry h3 code {
      color: var(--accent-light); background: transparent; border: none;
      padding: 0; font-size: 1em; letter-spacing: 0.5px;
    }
    .doc-entry .doc-anchor {
      color: var(--text-muted); font-size: 0.85em; margin-right: 0.25rem;
      text-decoration: none;
    }
    .doc-entry .doc-anchor:hover { color: var(--accent); }
    .doc-entry h4 {
      font-family: 'Orbitron', sans-serif;
      font-size: 11px; font-weight: 700; letter-spacing: 1.5px;
      text-transform: uppercase; color: var(--accent-light);
      margin: 0.8rem 0 0.3rem;
    }
    .doc-entry p {
      font-size: 13px; line-height: 1.6; color: var(--text-dim);
      margin: 0.35rem 0;
    }
    .doc-entry p code, .doc-entry li code {
      color: var(--accent-light); font-size: 12px;
    }
    .doc-entry ul { margin: 0.3rem 0 0.5rem; padding-left: 1.25rem; }
    .doc-entry li { font-size: 13px; color: var(--text-dim); line-height: 1.55; margin: 0.2rem 0; }
    .doc-entry pre {
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 12px;
      background: var(--bg); border: 1px solid var(--border);
      border-radius: 2px;
      padding: 0.7rem 0.9rem; overflow-x: auto;
      color: var(--text); margin: 0.5rem 0;
      box-shadow: inset 0 0 18px rgba(0, 0, 0, 0.35);
    }
    .doc-entry pre code { color: var(--text); background: transparent; border: none; padding: 0; }
    [data-theme="light"] .doc-entry pre { box-shadow: inset 0 0 10px rgba(0, 0, 0, 0.05); }

    kbd {
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 11px;
      padding: 1px 6px;
      background: var(--bg-secondary);
      border: 1px solid var(--border);
      border-bottom-width: 2px;
      border-radius: 3px;
      color: var(--cyan);
    }
  </style>
</head>
<body>
  <div class="app tutorial-app" id="docsApp">
    <div class="crt-scanline" id="crtH" aria-hidden="true"></div>
    <div class="crt-scanline-v" id="crtV" aria-hidden="true"></div>
"##;

const FOOT: &str = r#"  </main>
  </div>
  <script src="hud-theme.js"></script>
</body>
</html>
"#;
