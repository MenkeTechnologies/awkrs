//! Language Server Protocol (stdio) for editors — `awkrs --lsp`.
//!
//! A self-contained LSP that reuses awkrs's own parser and language model:
//! diagnostics come from `crate::parser::parse_program`, and completion /
//! hover / signature help draw on `crate::namespace::BUILTIN_NAMES`,
//! `crate::namespace::SPECIAL_GLOBAL_NAMES`, and the AWK keyword set plus a
//! hand-authored builtin-signature table sourced from the POSIX awk spec and
//! the gawk extensions awkrs accepts.
//!
//! Capabilities: full-sync text documents, publish-diagnostics on open/change,
//! completion, hover, document symbols, signature help, goto-definition,
//! references, document highlight, and folding ranges. No output reaches the
//! terminal — the server speaks JSON-RPC on stdio only.

use std::collections::HashMap;

use lsp_server::{Connection, ErrorCode, ExtractError, Message, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::FoldingRangeRequest;
use lsp_types::request::{
    Completion, DocumentHighlightRequest, DocumentSymbolRequest, GotoDefinition, HoverRequest,
    References, Request as _, SignatureHelpRequest,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Documentation, FoldingRange,
    FoldingRangeParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, Location, MarkupContent, MarkupKind, OneOf, Position,
    PublishDiagnosticsParams, Range, ReferenceParams, ServerCapabilities, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SignatureInformation, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
    WorkDoneProgressOptions,
};

use crate::error::Error;
use crate::namespace::{BUILTIN_NAMES, SPECIAL_GLOBAL_NAMES};

/// AWK keyword set (fixed by the language; mirrors the lexer's keyword tokens).
///
/// Public so the offline `gen-docs` reference generator can enumerate the same
/// keyword corpus the LSP hover path uses (single source of truth).
pub const AWK_KEYWORDS: &[&str] = &[
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
    "getline",
    "print",
    "printf",
    "in",
    "switch",
    "case",
    "default",
];

/// Open-document store: uri string → full buffer text (FULL text sync).
type Docs = HashMap<String, String>;

/// Entry point for `awkrs --lsp`. Blocks serving JSON-RPC on stdio until the
/// client sends `shutdown`/`exit`. Returns once the io threads have joined.
pub fn run_stdio() -> crate::Result<()> {
    let (conn, io_threads) = Connection::stdio();

    let (init_id, _init_params) = conn
        .initialize_start()
        .map_err(|e| Error::Runtime(format!("lsp initialize: {e}")))?;

    let caps = server_capabilities();
    let init_result = serde_json::json!({
        "capabilities": caps,
        "serverInfo": { "name": "awkrs", "version": env!("CARGO_PKG_VERSION") },
    });
    // Respond to `initialize` manually rather than `initialize_finish`: VS Code's
    // language client sends `$/setTrace` / `workspace/didChangeConfiguration`
    // before `initialized`, and `initialize_finish` treats anything other than
    // `initialized` as a protocol error (killing the server). rust-analyzer takes
    // this same manual-reply approach; the main loop then absorbs `initialized`
    // like any other notification.
    conn.sender
        .send(Response::new_ok(init_id, init_result).into())
        .map_err(|e| Error::Runtime(format!("lsp send: {e}")))?;

    let mut docs: Docs = HashMap::new();

    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn
                    .handle_shutdown(&req)
                    .map_err(|e| Error::Runtime(format!("lsp shutdown: {e}")))?
                {
                    break;
                }
                dispatch_request(&conn, &docs, req);
            }
            Message::Notification(not) => dispatch_notification(&conn, &mut docs, not),
            Message::Response(_) => {}
        }
    }

    // Drop the connection before joining: the writer thread only exits once all
    // `Sender` clones are gone, so joining with `conn` still in scope would block
    // forever. After this, the reader thread ends on stdin EOF (client disconnect).
    drop(conn);
    io_threads
        .join()
        .map_err(|e| Error::Runtime(format!("lsp io join: {e}")))?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(lsp_types::FoldingRangeProviderCapability::Simple(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        ..Default::default()
    }
}

/// Generic request handler: extract typed params, run `f`, reply with the JSON result.
fn handle<P, R>(conn: &Connection, req: Request, f: impl FnOnce(P) -> R)
where
    P: serde::de::DeserializeOwned,
    R: serde::Serialize,
{
    let method = req.method.clone();
    let id = req.id.clone();
    match req.extract::<P>(&method) {
        Ok((id, params)) => {
            let value = serde_json::to_value(f(params)).unwrap_or(serde_json::Value::Null);
            let _ = conn.sender.send(Response::new_ok(id, value).into());
        }
        Err(ExtractError::JsonError { error, .. }) => {
            let _ = conn.sender.send(
                Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string()).into(),
            );
        }
        Err(ExtractError::MethodMismatch(_)) => unreachable!("method matched before extract"),
    }
}

fn dispatch_request(conn: &Connection, docs: &Docs, req: Request) {
    match req.method.as_str() {
        Completion::METHOD => handle(conn, req, |p: CompletionParams| completions(docs, p)),
        HoverRequest::METHOD => handle(conn, req, |p: HoverParams| hover(docs, p)),
        DocumentSymbolRequest::METHOD => handle(conn, req, |p: DocumentSymbolParams| {
            document_symbols(docs, p)
        }),
        SignatureHelpRequest::METHOD => {
            handle(conn, req, |p: SignatureHelpParams| signature_help(docs, p))
        }
        GotoDefinition::METHOD => handle(conn, req, |p: GotoDefinitionParams| definition(docs, p)),
        References::METHOD => handle(conn, req, |p: ReferenceParams| references(docs, p)),
        DocumentHighlightRequest::METHOD => {
            handle(conn, req, |p: DocumentHighlightParams| highlights(docs, p))
        }
        FoldingRangeRequest::METHOD => handle(conn, req, |p: FoldingRangeParams| folding(docs, p)),
        _ => {
            let _ = conn.sender.send(
                Response::new_err(req.id, ErrorCode::MethodNotFound as i32, "unhandled".into())
                    .into(),
            );
        }
    }
}

fn dispatch_notification(conn: &Connection, docs: &mut Docs, not: lsp_server::Notification) {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.insert(uri_key(&uri), p.text_document.text.clone());
                publish_diagnostics(conn, &uri, &p.text_document.text);
            }
        }
        DidChangeTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(not.params) {
                if let Some(change) = p.content_changes.into_iter().last() {
                    let uri = p.text_document.uri;
                    docs.insert(uri_key(&uri), change.text.clone());
                    publish_diagnostics(conn, &uri, &change.text);
                }
            }
        }
        DidCloseTextDocument::METHOD => {
            if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(not.params) {
                let uri = p.text_document.uri;
                docs.remove(&uri_key(&uri));
                publish_diagnostics(conn, &uri, "");
            }
        }
        _ => {}
    }
}

fn uri_key(uri: &Uri) -> String {
    uri.as_str().to_string()
}

// ─────────────────────────── diagnostics ───────────────────────────

fn publish_diagnostics(conn: &Connection, uri: &Uri, text: &str) {
    let diagnostics = compute_diagnostics(text);
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics,
        version: None,
    };
    let not = lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = conn.sender.send(not.into());
}

fn compute_diagnostics(text: &str) -> Vec<Diagnostic> {
    match crate::parser::parse_program(text) {
        Ok(_) => Vec::new(),
        Err(Error::Parse { line, msg }) => {
            let l = line.saturating_sub(1) as u32;
            let end_char = line_len_chars(text, l);
            vec![Diagnostic {
                range: Range {
                    start: Position {
                        line: l,
                        character: 0,
                    },
                    end: Position {
                        line: l,
                        character: end_char,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("awkrs".into()),
                message: msg,
                ..Default::default()
            }]
        }
        // @include / source-expand IO errors and the like: surface at the top of
        // the buffer rather than dropping them silently.
        Err(e) => vec![Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("awkrs".into()),
            message: e.to_string(),
            ..Default::default()
        }],
    }
}

// ─────────────────────────── completion ───────────────────────────

fn completions(docs: &Docs, params: CompletionParams) -> CompletionResponse {
    let uri = uri_key(&params.text_document_position.text_document.uri);
    let mut items: Vec<CompletionItem> = Vec::new();

    for kw in AWK_KEYWORDS {
        items.push(simple_item(kw, CompletionItemKind::KEYWORD, None));
    }
    for b in BUILTIN_NAMES {
        let detail = builtin_signature(b).map(|(sig, _)| sig.to_string());
        items.push(simple_item(b, CompletionItemKind::FUNCTION, detail));
    }
    for v in SPECIAL_GLOBAL_NAMES {
        let detail = special_doc(v).map(|d| d.to_string());
        items.push(simple_item(v, CompletionItemKind::VARIABLE, detail));
    }
    if let Some(text) = docs.get(&uri) {
        for name in user_function_names(text) {
            items.push(simple_item(
                &name,
                CompletionItemKind::FUNCTION,
                Some("user-defined function".into()),
            ));
        }
    }
    CompletionResponse::Array(items)
}

fn simple_item(label: &str, kind: CompletionItemKind, detail: Option<String>) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail,
        ..Default::default()
    }
}

// ─────────────────────────── hover ───────────────────────────

fn hover(docs: &Docs, params: HoverParams) -> Option<Hover> {
    let pos = params.text_document_position_params.position;
    let uri = uri_key(&params.text_document_position_params.text_document.uri);
    let text = docs.get(&uri)?;
    let (word, range) = word_at(text, pos)?;

    let md = if let Some((sig, doc)) = builtin_signature(&word) {
        format!("```awk\n{sig}\n```\n\n{doc}")
    } else if let Some(doc) = special_doc(&word) {
        format!("**`{word}`** — special variable\n\n{doc}")
    } else if let Some(doc) = keyword_doc(&word) {
        format!("**`{word}`** — keyword\n\n{doc}")
    } else {
        return None;
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(range),
    })
}

// ─────────────────────────── document symbols ───────────────────────────

fn document_symbols(docs: &Docs, params: DocumentSymbolParams) -> DocumentSymbolResponse {
    let uri = uri_key(&params.text_document.uri);
    let mut syms: Vec<DocumentSymbol> = Vec::new();
    let Some(text) = docs.get(&uri) else {
        return DocumentSymbolResponse::Nested(syms);
    };

    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = (line.len() - trimmed.len()) as u32;
        let l = i as u32;
        let line_range = Range {
            start: Position {
                line: l,
                character: 0,
            },
            end: Position {
                line: l,
                character: line.chars().count() as u32,
            },
        };
        if let Some(name) = function_name_on_line(trimmed) {
            let name_start = indent + "function ".len() as u32;
            #[allow(deprecated)]
            syms.push(DocumentSymbol {
                name,
                detail: None,
                kind: SymbolKind::FUNCTION,
                tags: None,
                deprecated: None,
                range: line_range,
                selection_range: Range {
                    start: Position {
                        line: l,
                        character: name_start,
                    },
                    end: Position {
                        line: l,
                        character: name_start,
                    },
                },
                children: None,
            });
        } else if let Some(special) = special_rule_on_line(trimmed) {
            #[allow(deprecated)]
            syms.push(DocumentSymbol {
                name: special.to_string(),
                detail: None,
                kind: SymbolKind::EVENT,
                tags: None,
                deprecated: None,
                range: line_range,
                selection_range: line_range,
                children: None,
            });
        }
    }
    DocumentSymbolResponse::Nested(syms)
}

/// `BEGIN` / `END` / `BEGINFILE` / `ENDFILE` if the trimmed line starts with one.
fn special_rule_on_line(trimmed: &str) -> Option<&'static str> {
    for kw in ["BEGINFILE", "ENDFILE", "BEGIN", "END"] {
        if trimmed == kw
            || trimmed.starts_with(&format!("{kw} "))
            || trimmed.starts_with(&format!("{kw}{{"))
        {
            return Some(match kw {
                "BEGINFILE" => "BEGINFILE",
                "ENDFILE" => "ENDFILE",
                "BEGIN" => "BEGIN",
                _ => "END",
            });
        }
    }
    None
}

/// Function name if the trimmed line is a `function NAME(...)` definition.
fn function_name_on_line(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("function")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn user_function_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| function_name_on_line(l.trim_start()))
        .collect()
}

// ─────────────────────────── signature help ───────────────────────────

fn signature_help(docs: &Docs, params: SignatureHelpParams) -> Option<SignatureHelp> {
    let pos = params.text_document_position_params.position;
    let uri = uri_key(&params.text_document_position_params.text_document.uri);
    let text = docs.get(&uri)?;
    let offset = position_to_offset(text, pos)?;
    let chars: Vec<char> = text.chars().collect();

    let mut depth = 0i32;
    let mut commas = 0u32;
    let mut i = offset.min(chars.len());
    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    let mut j = i;
                    while j > 0 && is_word_char(chars[j - 1]) {
                        j -= 1;
                    }
                    let name: String = chars[j..i].iter().collect();
                    let (sig, doc) = builtin_signature(&name)?;
                    return Some(SignatureHelp {
                        signatures: vec![SignatureInformation {
                            label: sig.to_string(),
                            documentation: Some(Documentation::String(doc.to_string())),
                            parameters: None,
                            active_parameter: None,
                        }],
                        active_signature: Some(0),
                        active_parameter: Some(commas),
                    });
                }
                depth -= 1;
            }
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    None
}

// ─────────────────────────── definition / references / highlight ───────────────────────────

fn definition(docs: &Docs, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
    let pos = params.text_document_position_params.position;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .clone();
    let text = docs.get(&uri_key(&uri))?;
    let (word, _) = word_at(text, pos)?;

    for (i, line) in text.lines().enumerate() {
        if function_name_on_line(line.trim_start()).as_deref() == Some(word.as_str()) {
            let l = i as u32;
            let range = Range {
                start: Position {
                    line: l,
                    character: 0,
                },
                end: Position {
                    line: l,
                    character: line.chars().count() as u32,
                },
            };
            return Some(GotoDefinitionResponse::Scalar(Location { uri, range }));
        }
    }
    None
}

fn references(docs: &Docs, params: ReferenceParams) -> Option<Vec<Location>> {
    let pos = params.text_document_position.position;
    let uri = params.text_document_position.text_document.uri.clone();
    let text = docs.get(&uri_key(&uri))?;
    let (word, _) = word_at(text, pos)?;
    Some(
        word_occurrences(text, &word)
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect(),
    )
}

fn highlights(docs: &Docs, params: DocumentHighlightParams) -> Option<Vec<DocumentHighlight>> {
    let pos = params.text_document_position_params.position;
    let uri = uri_key(&params.text_document_position_params.text_document.uri);
    let text = docs.get(&uri)?;
    let (word, _) = word_at(text, pos)?;
    Some(
        word_occurrences(text, &word)
            .into_iter()
            .map(|range| DocumentHighlight {
                range,
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect(),
    )
}

/// All whole-word occurrences of `word`, as ranges, scanning line by line.
fn word_occurrences(text: &str, word: &str) -> Vec<Range> {
    let mut out = Vec::new();
    let wchars: Vec<char> = word.chars().collect();
    let wlen = wchars.len();
    if wlen == 0 {
        return out;
    }
    for (li, line) in text.lines().enumerate() {
        let cs: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + wlen <= cs.len() {
            if cs[i..i + wlen] == wchars[..]
                && (i == 0 || !is_word_char(cs[i - 1]))
                && (i + wlen == cs.len() || !is_word_char(cs[i + wlen]))
            {
                out.push(Range {
                    start: Position {
                        line: li as u32,
                        character: i as u32,
                    },
                    end: Position {
                        line: li as u32,
                        character: (i + wlen) as u32,
                    },
                });
                i += wlen;
            } else {
                i += 1;
            }
        }
    }
    out
}

// ─────────────────────────── folding ───────────────────────────

fn folding(docs: &Docs, params: FoldingRangeParams) -> Option<Vec<FoldingRange>> {
    let uri = uri_key(&params.text_document.uri);
    let text = docs.get(&uri)?;
    let mut stack: Vec<u32> = Vec::new();
    let mut out: Vec<FoldingRange> = Vec::new();

    for (li, line) in text.lines().enumerate() {
        let cs: Vec<char> = line.chars().collect();
        let mut in_str = false;
        let mut esc = false;
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if in_str {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    in_str = false;
                }
            } else if c == '#' {
                break;
            } else if c == '"' {
                in_str = true;
            } else if c == '{' {
                stack.push(li as u32);
            } else if c == '}' {
                if let Some(open) = stack.pop() {
                    if (li as u32) > open {
                        out.push(FoldingRange {
                            start_line: open,
                            end_line: li as u32,
                            ..Default::default()
                        });
                    }
                }
            }
            i += 1;
        }
    }
    Some(out)
}

// ─────────────────────────── text helpers ───────────────────────────

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Number of characters on (0-based) `line`, or 0 if out of range.
fn line_len_chars(text: &str, line: u32) -> u32 {
    text.lines()
        .nth(line as usize)
        .map(|l| l.chars().count() as u32)
        .unwrap_or(0)
}

/// Identifier under `pos` and its range, treating `[A-Za-z0-9_]` as word chars.
fn word_at(text: &str, pos: Position) -> Option<(String, Range)> {
    let line = text.lines().nth(pos.line as usize)?;
    let cs: Vec<char> = line.chars().collect();
    let i = (pos.character as usize).min(cs.len());
    let mut start = i;
    while start > 0 && is_word_char(cs[start - 1]) {
        start -= 1;
    }
    let mut end = i;
    while end < cs.len() && is_word_char(cs[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    let word: String = cs[start..end].iter().collect();
    let range = Range {
        start: Position {
            line: pos.line,
            character: start as u32,
        },
        end: Position {
            line: pos.line,
            character: end as u32,
        },
    };
    Some((word, range))
}

/// Character offset (into `text.chars()`) for an LSP position, clamped to the doc.
fn position_to_offset(text: &str, pos: Position) -> Option<usize> {
    let mut offset = 0usize;
    let mut line = 0u32;
    let mut chars = text.chars();
    if pos.line == 0 {
        // count chars on line 0 up to pos.character
        for c in chars.by_ref() {
            if line == 0 && (offset as u32) >= pos.character {
                return Some(offset);
            }
            if c == '\n' {
                return Some(offset);
            }
            offset += 1;
        }
        return Some(offset);
    }
    for c in chars.by_ref() {
        offset += 1;
        if c == '\n' {
            line += 1;
            if line == pos.line {
                break;
            }
        }
    }
    for (col, c) in chars.enumerate() {
        if col >= pos.character as usize || c == '\n' {
            break;
        }
        offset += 1;
    }
    Some(offset)
}

// ─────────────────────────── language data ───────────────────────────

/// Signature + one-line description for an AWK builtin, if known.
/// Sourced from the POSIX awk spec and the gawk extensions awkrs accepts.
///
/// Public so the offline `gen-docs` reference generator renders the exact same
/// signatures/descriptions the LSP hover and signature-help paths use.
pub fn builtin_signature(name: &str) -> Option<(&'static str, &'static str)> {
    let v = match name {
        "length" => (
            "length([s])",
            "Length of string `s` (or `$0`), or element count of an array.",
        ),
        "substr" => (
            "substr(s, m [, n])",
            "`n`-char substring of `s` starting at position `m` (1-based).",
        ),
        "index" => (
            "index(s, t)",
            "1-based position of `t` in `s`, or 0 if not found.",
        ),
        "split" => (
            "split(s, arr [, fs [, seps]])",
            "Split `s` into `arr` on `fs`; returns the field count.",
        ),
        "sub" => (
            "sub(re, repl [, target])",
            "Replace first match of `re` in `target` (`$0`); returns 1/0.",
        ),
        "gsub" => (
            "gsub(re, repl [, target])",
            "Replace all matches of `re` in `target` (`$0`); returns count.",
        ),
        "gensub" => (
            "gensub(re, repl, how [, target])",
            "Non-destructive global/Nth substitution returning the new string.",
        ),
        "match" => (
            "match(s, re [, arr])",
            "Set RSTART/RLENGTH to the match of `re` in `s`; returns position or 0.",
        ),
        "sprintf" => (
            "sprintf(fmt, ...)",
            "Format the arguments per `fmt` and return the string.",
        ),
        "printf" => (
            "printf(fmt, ...)",
            "Format and print the arguments per `fmt`.",
        ),
        "sin" => ("sin(x)", "Sine of `x` (radians)."),
        "cos" => ("cos(x)", "Cosine of `x` (radians)."),
        "atan2" => ("atan2(y, x)", "Arctangent of `y/x` in radians."),
        "exp" => ("exp(x)", "e raised to the power `x`."),
        "log" => ("log(x)", "Natural logarithm of `x`."),
        "sqrt" => ("sqrt(x)", "Square root of `x`."),
        "int" => ("int(x)", "Integer part of `x`, truncated toward zero."),
        "rand" => ("rand()", "Pseudo-random number in [0, 1)."),
        "srand" => (
            "srand([x])",
            "Seed the RNG with `x` (or time); returns the previous seed.",
        ),
        "tolower" => (
            "tolower(s)",
            "Copy of `s` with uppercase letters lowercased.",
        ),
        "toupper" => (
            "toupper(s)",
            "Copy of `s` with lowercase letters uppercased.",
        ),
        "system" => (
            "system(cmd)",
            "Run `cmd` via the shell; returns its exit status.",
        ),
        "close" => (
            "close(file [, how])",
            "Close an open file/pipe; returns its status.",
        ),
        "fflush" => (
            "fflush([file])",
            "Flush buffers for `file`, or all outputs if omitted.",
        ),
        "strftime" => (
            "strftime([fmt [, ts [, utc]]])",
            "Format timestamp `ts` per `fmt` (gawk).",
        ),
        "systime" => (
            "systime()",
            "Current time as seconds since the epoch (gawk).",
        ),
        "mktime" => (
            "mktime(spec [, utc])",
            "Convert a \"YYYY MM DD HH MM SS [DST]\" spec to a timestamp (gawk).",
        ),
        "strtonum" => (
            "strtonum(s)",
            "Numeric value of `s`, honoring 0x/0 prefixes (gawk).",
        ),
        "and" => ("and(v1, v2, ...)", "Bitwise AND of the arguments (gawk)."),
        "or" => ("or(v1, v2, ...)", "Bitwise OR of the arguments (gawk)."),
        "xor" => ("xor(v1, v2, ...)", "Bitwise XOR of the arguments (gawk)."),
        "compl" => ("compl(v)", "Bitwise complement of `v` (gawk)."),
        "lshift" => ("lshift(v, n)", "`v` left-shifted by `n` bits (gawk)."),
        "rshift" => ("rshift(v, n)", "`v` right-shifted by `n` bits (gawk)."),
        "patsplit" => (
            "patsplit(s, arr [, fpat [, seps]])",
            "Split `s` into `arr` by the pattern `fpat` (gawk).",
        ),
        "asort" => (
            "asort(src [, dst [, how]])",
            "Sort array values; returns the element count (gawk).",
        ),
        "asorti" => (
            "asorti(src [, dst [, how]])",
            "Sort array indices; returns the element count (gawk).",
        ),
        "typeof" => (
            "typeof(x)",
            "Type of `x`: \"scalar\", \"array\", \"untyped\", … (gawk).",
        ),
        "isarray" => ("isarray(x)", "1 if `x` is an array, else 0 (gawk)."),
        "mkbool" => (
            "mkbool(expr)",
            "Boolean-typed value from the truth of `expr` (gawk).",
        ),
        "chr" => ("chr(n)", "Character for code point `n`."),
        "ord" => ("ord(s)", "Code point of the first character of `s`."),
        _ => return None,
    };
    Some(v)
}

/// One-line description for an AWK special variable, if known.
///
/// Public so the offline `gen-docs` reference generator shares this corpus
/// with the LSP hover path (single source of truth).
pub fn special_doc(name: &str) -> Option<&'static str> {
    let d = match name {
        "NR" => "Total number of input records read so far.",
        "NF" => "Number of fields in the current record.",
        "FNR" => "Record number within the current input file.",
        "FS" => "Input field separator (default \" \").",
        "OFS" => "Output field separator (default \" \").",
        "ORS" => "Output record separator (default \"\\n\").",
        "RS" => "Input record separator (default \"\\n\").",
        "RT" => "Text matched by RS for the current record (gawk).",
        "FILENAME" => "Name of the current input file.",
        "SUBSEP" => "Subscript separator for multi-dimensional array keys.",
        "RSTART" => "Start position of the last match() (1-based), or 0.",
        "RLENGTH" => "Length of the last match(), or -1.",
        "CONVFMT" => "Conversion format for numbers used as strings (default \"%.6g\").",
        "OFMT" => "Output format for numbers in print (default \"%.6g\").",
        "FPAT" => "Regexp describing field contents, as an alternative to FS (gawk).",
        "FIELDWIDTHS" => "Space-separated fixed field widths for parsing (gawk).",
        "IGNORECASE" => "When non-zero, regex and string comparisons ignore case (gawk).",
        "ARGC" => "Count of command-line arguments in ARGV.",
        "ARGV" => "Array of command-line arguments.",
        "ARGIND" => "Index in ARGV of the current input file (gawk).",
        "ENVIRON" => "Array of the process environment variables.",
        "ERRNO" => "Description of the last getline/close/system error (gawk).",
        "PROCINFO" => "Array of process/runtime information (gawk).",
        "SYMTAB" => "Array aliasing the program's global variables (gawk).",
        "FUNCTAB" => "Array of the program's function names (gawk).",
        "BINMODE" => "Binary I/O mode control (gawk).",
        "LINT" => "Dynamic control of lint warnings (gawk).",
        "TEXTDOMAIN" => "Text domain for string translation (gawk).",
        _ => return None,
    };
    Some(d)
}

/// One-line description for an AWK keyword / control-flow construct, if known.
/// Covers every entry in [`AWK_KEYWORDS`]. Sourced from the POSIX awk grammar
/// and the gawk manual; gawk-only constructs are tagged `(gawk)`.
///
/// Public so the offline `gen-docs` reference generator and the LSP hover path
/// render keyword docs from one source of truth.
pub fn keyword_doc(name: &str) -> Option<&'static str> {
    let d = match name {
        "BEGIN" => "Special pattern whose action runs once before any input is read.",
        "END" => "Special pattern whose action runs once after all input is consumed.",
        "BEGINFILE" => "Pattern whose action runs before each input file is read (gawk).",
        "ENDFILE" => "Pattern whose action runs after each input file is processed (gawk).",
        "function" => "Define a user function: `function name(params) { body }`.",
        "if" => "Conditional statement: `if (cond) stmt` with an optional `else` branch.",
        "else" => "The alternative branch taken when an `if` condition is false.",
        "while" => "Loop that repeats `stmt` while `cond` is true: `while (cond) stmt`.",
        "for" => "Loop: `for (init; cond; incr) stmt`, or `for (key in array) stmt`.",
        "do" => "Do-while loop: `do stmt while (cond)`; the body runs at least once.",
        "break" => "Exit the innermost `for`, `while`, or `do` loop immediately.",
        "continue" => "Skip to the next iteration of the innermost loop.",
        "next" => "Stop processing the current record and read the next one.",
        "nextfile" => "Stop processing the current input file and advance to the next.",
        "exit" => "Stop reading input and run `END`; `exit [expr]` sets the exit status.",
        "return" => "Return from a user function, optionally with a value: `return [expr]`.",
        "delete" => "Remove an array element (`delete arr[k]`) or clear it (`delete arr`).",
        "getline" => {
            "Read the next record into `$0` or a variable from input, a file, or a command."
        }
        "print" => "Write its arguments to output, separated by `OFS` and terminated by `ORS`.",
        "printf" => "Write formatted output using a C-style format string: `printf fmt, args`.",
        "in" => "Array membership test (`key in arr`) or iteration (`for (key in arr)`).",
        "switch" => "Multi-way branch on a value: `switch (expr) { case ...: ... }` (gawk).",
        "case" => "A labeled branch inside a `switch` statement (gawk).",
        "default" => "The fallback branch taken when no `case` matches in a `switch` (gawk).",
        _ => return None,
    };
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_empty_for_valid_program() {
        let diags = compute_diagnostics("BEGIN { print \"hi\" }\n");
        assert!(diags.is_empty(), "valid program should have no diagnostics");
    }

    #[test]
    fn diagnostics_report_parse_error_with_line() {
        // Unterminated block — the parser should fault.
        let diags = compute_diagnostics("BEGIN { print \n");
        assert!(!diags.is_empty(), "expected a parse diagnostic");
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diags[0].source.as_deref(), Some("awkrs"));
    }

    #[test]
    fn word_at_finds_identifier_under_cursor() {
        let text = "BEGIN { length(x) }";
        let (w, _) = word_at(
            text,
            Position {
                line: 0,
                character: 10,
            },
        )
        .unwrap();
        assert_eq!(w, "length");
    }

    #[test]
    fn function_name_parsed_from_definition_line() {
        assert_eq!(
            function_name_on_line("function foo(a, b) {"),
            Some("foo".to_string())
        );
        assert_eq!(function_name_on_line("functional()"), None);
        assert_eq!(function_name_on_line("x = 1"), None);
    }

    #[test]
    fn user_function_names_collected() {
        let text = "function a() {}\nfunction b(x) { return x }\n";
        assert_eq!(
            user_function_names(text),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn word_occurrences_are_whole_word_only() {
        let text = "x = x + xx\n";
        let occ = word_occurrences(text, "x");
        assert_eq!(
            occ.len(),
            2,
            "should match the two standalone `x`, not `xx`"
        );
    }

    #[test]
    fn builtin_and_special_lookups_resolve() {
        assert!(builtin_signature("gsub").is_some());
        assert!(builtin_signature("not_a_builtin").is_none());
        assert!(special_doc("NR").is_some());
        assert!(special_doc("nope").is_none());
    }

    #[test]
    fn folding_pairs_braces_across_lines() {
        let text = "BEGIN {\n  print 1\n}\n";
        let ranges = folding_default(text);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 2);
    }

    #[test]
    fn folding_ignores_braces_in_strings_and_comments() {
        let text = "BEGIN {\n  s = \"{\"  # }\n  print s\n}\n";
        let ranges = folding_default(text);
        assert_eq!(ranges.len(), 1, "string/comment braces must not nest");
        assert_eq!(ranges[0].end_line, 3);
    }

    #[test]
    fn special_rule_detected_at_line_start() {
        assert_eq!(special_rule_on_line("BEGIN {"), Some("BEGIN"));
        assert_eq!(special_rule_on_line("END{print}"), Some("END"));
        assert_eq!(special_rule_on_line("x == 1 {"), None);
    }

    #[test]
    fn position_to_offset_handles_multiline() {
        let text = "abc\ndef\n";
        // line 1, char 1 → 'e' which is the 5th char (index 5)
        let off = position_to_offset(
            text,
            Position {
                line: 1,
                character: 1,
            },
        )
        .unwrap();
        assert_eq!(text.chars().nth(off), Some('e'));
    }

    /// Test-only folding over raw text (mirrors `folding`, sans the Docs/uri plumbing).
    fn folding_default(text: &str) -> Vec<FoldingRange> {
        let mut docs = Docs::new();
        let uri: Uri = "file:///t.awk".parse().unwrap();
        docs.insert(uri_key(&uri), text.to_string());
        folding(
            &docs,
            FoldingRangeParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        )
        .unwrap()
    }
}
