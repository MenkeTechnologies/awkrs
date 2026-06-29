//! Debug Adapter Protocol (DAP) server for awkrs.
//!
//! Started via `awkrs --dap [HOST:PORT]`. Speaks DAP over stdio (or a TCP
//! socket) using `Content-Length`-framed JSON-RPC, the same framing as the LSP.
//! Wraps [`crate::debugger::Debugger`] so the breakpoint / step / inspection
//! logic is shared.
//!
//! Ported from strykelang's `dap.rs`. The protocol layer (threading model,
//! request dispatch, `stopped`/`output`/`terminated` events) is essentially
//! unchanged; the variable-capture path is rewritten for awk's value model —
//! only scalars and (flat) associative arrays exist, so the deep struct / class
//! / sketch drill-down strykelang needs is gone.
//!
//! ## Threading model
//!
//! * **Main thread** spawns a reader thread, waits for `launch`, then runs the
//!   VM in-place.
//! * **Reader thread** parses incoming DAP messages, mutates the shared
//!   [`DapShared`] / [`BreakpointState`], and signals the VM thread via condvar.
//! * **VM thread** (= main thread after `launch`) on each line stop captures a
//!   snapshot, emits a `stopped` event, and condvar-waits for a resume command.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::debugger::DebugAction;
use crate::runtime::Value;

const MAX_VAR_REPR: usize = 200;

/// Lightweight tracing to stderr, gated on `AWKRS_DAP_LOG` so it doesn't corrupt
/// the protocol stream by default (in stdio mode stdout carries DAP traffic).
macro_rules! dlog {
    ($($arg:tt)*) => {
        if std::env::var_os("AWKRS_DAP_LOG").is_some() {
            eprintln!("[awkrs-dap] {}", format!($($arg)*));
        }
    };
}

// ─── DAP wire types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct DapRequest {
    seq: u64,
    #[serde(rename = "type")]
    msg_type: String,
    command: String,
    #[serde(default)]
    arguments: Json,
}

// ─── Shared state ────────────────────────────────────────────────────────────

/// Snapshot of where the VM is paused, captured by the VM thread before it
/// blocks so the reader thread can answer `stackTrace` / `scopes` / `variables`
/// without touching the VM.
#[derive(Default, Clone)]
pub struct PauseSnapshot {
    /// `file` field.
    pub file: String,
    /// `line` field.
    pub line: usize,
    /// "breakpoint" | "step" | "pause" | "entry".
    pub reason: String,
    /// `frames` field.
    pub frames: Vec<FrameSnap>,
    /// `locals` field.
    pub locals: Vec<VarSnap>,
    /// `globals` field.
    pub globals: Vec<VarSnap>,
    /// Container varRef → child rows (for expandable arrays).
    pub var_ref_map: HashMap<u32, Vec<VarChild>>,
}

/// One call-stack frame.
#[derive(Clone)]
pub struct FrameSnap {
    /// `name` field.
    pub name: String,
    /// `file` field.
    pub file: String,
    /// `line` field.
    pub line: usize,
}

/// One row in the Locals / Globals scope.
#[derive(Clone)]
pub struct VarSnap {
    /// `name` field.
    pub name: String,
    /// `repr` field.
    pub repr: String,
    /// "scalar" | "array".
    pub kind: String,
    /// 0 = leaf; non-zero = a `variablesReference` to expand (array elements).
    pub var_ref: u32,
}

/// One element row inside an expanded array.
#[derive(Clone)]
pub struct VarChild {
    /// `name` field.
    pub name: String,
    /// `repr` field.
    pub repr: String,
    /// Always 0 for awk (array elements are scalars — no nesting).
    pub var_ref: u32,
}

struct SharedInner {
    pending_action: Option<DebugAction>,
    is_paused: bool,
    snapshot: PauseSnapshot,
    pause_request: bool,
}

/// Shared protocol state: the message writer, the sequence counter, the pause
/// handshake, and the disconnect/config flags.
pub struct DapShared {
    inner: Mutex<SharedInner>,
    cv: Condvar,
    seq: AtomicU64,
    writer: Mutex<Box<dyn Write + Send>>,
    /// Set when the client sends `configurationDone`.
    pub configuration_done: AtomicBool,
    /// Set when the client disconnects / terminates.
    pub disconnected: AtomicBool,
}

impl DapShared {
    fn new(writer: Box<dyn Write + Send>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SharedInner {
                pending_action: None,
                is_paused: false,
                snapshot: PauseSnapshot::default(),
                pause_request: false,
            }),
            cv: Condvar::new(),
            seq: AtomicU64::new(1),
            writer: Mutex::new(writer),
            configuration_done: AtomicBool::new(false),
            disconnected: AtomicBool::new(false),
        })
    }

    /// Called by the VM thread on a stop: store the snapshot, emit `stopped`,
    /// then condvar-wait for the next resume command.
    pub fn pause(&self, snap: PauseSnapshot) -> DebugAction {
        // Flush program output so anything printed since the last stop is
        // visible before the suspend UI appears.
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        {
            let mut s = self.inner.lock().expect("dap lock");
            s.snapshot = snap.clone();
            s.is_paused = true;
            s.pending_action = None;
            s.pause_request = false;
        }
        self.emit_event(
            "stopped",
            json!({
                "reason": snap.reason,
                "threadId": 1,
                "allThreadsStopped": true,
                "preserveFocusHint": false,
                "description": snap.reason,
                "text": format!("{}:{}", snap.file, snap.line),
            }),
        );
        let mut guard = self.inner.lock().expect("dap lock");
        while guard.pending_action.is_none() && !self.disconnected.load(Ordering::SeqCst) {
            guard = self.cv.wait(guard).expect("dap cv");
        }
        let action = guard.pending_action.take().unwrap_or(DebugAction::Continue);
        guard.is_paused = false;
        action
    }

    /// True once the client has disconnected.
    pub fn was_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::SeqCst)
    }

    /// True when the client asked us to pause as soon as possible.
    pub fn want_pause(&self) -> bool {
        self.inner.lock().map(|g| g.pause_request).unwrap_or(false)
    }

    fn resume_with(&self, action: DebugAction) {
        let mut g = self.inner.lock().expect("dap lock");
        g.pending_action = Some(action);
        self.cv.notify_all();
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    fn write_message(&self, body: Json) {
        let s = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
        let mut w = self.writer.lock().expect("dap writer");
        let _ = write!(w, "Content-Length: {}\r\n\r\n{}", s.len(), s);
        let _ = w.flush();
    }

    fn emit_response(&self, req: &DapRequest, success: bool, body: Json) {
        let seq = self.next_seq();
        self.write_message(json!({
            "seq": seq,
            "type": "response",
            "request_seq": req.seq,
            "success": success,
            "command": req.command,
            "body": body,
        }));
    }

    /// Emit a DAP event (`stopped`, `output`, `terminated`, …).
    pub fn emit_event(&self, event: &str, body: Json) {
        let seq = self.next_seq();
        dlog!("→ event {} seq={}", event, seq);
        self.write_message(json!({
            "seq": seq,
            "type": "event",
            "event": event,
            "body": body,
        }));
    }
}

// ─── Reader / dispatch ───────────────────────────────────────────────────────

/// Spawn the DAP reader thread on an arbitrary input source (stdin for stdio
/// mode, a TCP socket for socket mode). Returns the join handle and a channel
/// the main thread blocks on for the `launch` parameters.
pub fn spawn_reader_with_input(
    shared: Arc<DapShared>,
    bp_state: Arc<Mutex<BreakpointState>>,
    input: Box<dyn Read + Send>,
) -> (
    thread::JoinHandle<()>,
    std::sync::mpsc::Receiver<LaunchParams>,
) {
    let (tx, rx) = std::sync::mpsc::channel::<LaunchParams>();
    let h = thread::spawn(move || {
        let mut reader = BufReader::new(input);
        loop {
            let body = match read_message(&mut reader) {
                Ok(Some(b)) => b,
                Ok(None) => break,
                Err(_) => break,
            };
            let req: DapRequest = match serde_json::from_slice(&body) {
                Ok(r) => r,
                Err(_) => continue,
            };
            handle_request(&shared, &bp_state, &tx, &req);
            if shared.was_disconnected() {
                break;
            }
        }
        // Stream closed → release any waiting VM thread.
        shared.resume_with(DebugAction::Quit);
        shared.disconnected.store(true, Ordering::SeqCst);
    });
    (h, rx)
}

fn read_message<R: Read>(reader: &mut BufReader<R>) -> io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    let Some(len) = content_length else {
        return Ok(Some(Vec::new()));
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

// ─── Launch + breakpoint state ───────────────────────────────────────────────

/// Parameters from the `launch` request.
#[derive(Debug, Clone, Default)]
pub struct LaunchParams {
    /// Path to the awk program file.
    pub program: String,
    /// Input data files (positional args after the program).
    pub args: Vec<String>,
    /// Working directory to switch to before running.
    pub cwd: Option<String>,
    /// `noDebug` — run without stopping (currently still routes through the VM).
    pub no_debug: bool,
    /// `stopOnEntry` — halt on the first line.
    pub stop_on_entry: bool,
}

/// Breakpoints and queued step requests, shared between the reader thread and
/// the debugger running on the VM thread.
#[derive(Debug, Default)]
pub struct BreakpointState {
    /// Line breakpoints keyed by source path → lines.
    pub line_breakpoints: HashMap<String, Vec<usize>>,
    /// Function breakpoints (function names).
    pub function_breakpoints: Vec<String>,
    /// Step request queued by the reader thread; consumed after the VM wakes.
    pub pending_step: Option<StepKind>,
}

/// Flatten every source's breakpoints into one sorted, de-duplicated line list.
/// awk debugging is single-file, so we don't path-match.
fn all_breakpoint_lines(bp_state: &Arc<Mutex<BreakpointState>>) -> Vec<usize> {
    let mut v: Vec<usize> = bp_state
        .lock()
        .map(|g| g.line_breakpoints.values().flatten().copied().collect())
        .unwrap_or_default();
    v.sort_unstable();
    v.dedup();
    v
}

// ─── Request handlers ────────────────────────────────────────────────────────

fn handle_request(
    shared: &Arc<DapShared>,
    bp_state: &Arc<Mutex<BreakpointState>>,
    launch_tx: &std::sync::mpsc::Sender<LaunchParams>,
    req: &DapRequest,
) {
    dlog!("← {} seq={}", req.command, req.seq);
    match req.command.as_str() {
        "initialize" => {
            shared.emit_response(
                req,
                true,
                json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsFunctionBreakpoints": true,
                    "supportsConditionalBreakpoints": false,
                    "supportsHitConditionalBreakpoints": false,
                    "supportsEvaluateForHovers": true,
                    "supportsTerminateRequest": true,
                    "supportsRestartRequest": false,
                    "supportsStepInTargetsRequest": false,
                    "supportsSetVariable": false,
                    "supportsCompletionsRequest": false,
                    "supportsLoadedSourcesRequest": false,
                    "supportsExceptionInfoRequest": false,
                    "supportsLogPoints": false,
                    "supportsModulesRequest": false,
                    "supportsRestartFrame": false,
                    "supportsGotoTargetsRequest": false,
                    "supportsStepBack": false,
                }),
            );
            shared.emit_event("initialized", json!({}));
        }
        "setBreakpoints" => {
            let path = req
                .arguments
                .get("source")
                .and_then(|s| s.get("path"))
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let bps = req
                .arguments
                .get("breakpoints")
                .and_then(|b| b.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| b.get("line").and_then(|l| l.as_u64()))
                        .map(|l| l as usize)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            dlog!("setBreakpoints path={} lines={:?}", path, bps);
            {
                let mut bp = bp_state.lock().expect("bp lock");
                bp.line_breakpoints.insert(path.clone(), bps.clone());
            }
            let verified: Vec<Json> = bps
                .iter()
                .map(|l| json!({ "verified": true, "line": *l, "source": { "path": path } }))
                .collect();
            shared.emit_response(req, true, json!({ "breakpoints": verified }));
        }
        "setFunctionBreakpoints" => {
            let fbps: Vec<String> = req
                .arguments
                .get("breakpoints")
                .and_then(|b| b.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| b.get("name").and_then(|n| n.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            {
                let mut bp = bp_state.lock().expect("bp lock");
                bp.function_breakpoints = fbps.clone();
            }
            let body: Vec<Json> = fbps.iter().map(|_| json!({ "verified": true })).collect();
            shared.emit_response(req, true, json!({ "breakpoints": body }));
        }
        "setExceptionBreakpoints" => {
            shared.emit_response(req, true, json!({ "breakpoints": [] }));
        }
        "configurationDone" => {
            shared.configuration_done.store(true, Ordering::SeqCst);
            shared.emit_response(req, true, json!({}));
        }
        "launch" => {
            let lp = LaunchParams {
                program: req
                    .arguments
                    .get("program")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                args: req
                    .arguments
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                cwd: req
                    .arguments
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                no_debug: req
                    .arguments
                    .get("noDebug")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                stop_on_entry: req
                    .arguments
                    .get("stopOnEntry")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            };
            dlog!(
                "launch program={} stopOnEntry={} noDebug={}",
                lp.program,
                lp.stop_on_entry,
                lp.no_debug
            );
            let _ = launch_tx.send(lp);
            shared.emit_response(req, true, json!({}));
        }
        "threads" => {
            shared.emit_response(
                req,
                true,
                json!({ "threads": [ { "id": 1, "name": "main" } ] }),
            );
        }
        "stackTrace" => {
            let snap = shared.inner.lock().expect("dap lock").snapshot.clone();
            let frames: Vec<Json> = snap
                .frames
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    json!({
                        "id": i + 1,
                        "name": f.name,
                        "line": f.line,
                        "column": 1,
                        "source": { "name": leaf(&f.file), "path": f.file }
                    })
                })
                .collect();
            shared.emit_response(
                req,
                true,
                json!({ "stackFrames": frames, "totalFrames": frames.len() }),
            );
        }
        "scopes" => {
            shared.emit_response(
                req,
                true,
                json!({
                    "scopes": [
                        { "name": "Locals",  "variablesReference": 1000, "expensive": false },
                        { "name": "Globals", "variablesReference": 2000, "expensive": false }
                    ]
                }),
            );
        }
        "variables" => {
            let var_ref = req
                .arguments
                .get("variablesReference")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let snap = shared.inner.lock().expect("dap lock").snapshot.clone();
            let to_json = |v: &VarSnap| {
                json!({
                    "name": v.name,
                    "value": v.repr,
                    "type": v.kind,
                    "variablesReference": v.var_ref,
                })
            };
            let vars: Vec<Json> = match var_ref {
                1000 => snap.locals.iter().map(to_json).collect(),
                2000 => snap.globals.iter().map(to_json).collect(),
                _ => snap
                    .var_ref_map
                    .get(&var_ref)
                    .map(|children| {
                        children
                            .iter()
                            .map(|c| {
                                json!({
                                    "name": c.name,
                                    "value": c.repr,
                                    "type": "",
                                    "variablesReference": c.var_ref,
                                })
                            })
                            .collect::<Vec<Json>>()
                    })
                    .unwrap_or_default(),
            };
            shared.emit_response(req, true, json!({ "variables": vars }));
        }
        "continue" => {
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({ "allThreadsContinued": true }));
        }
        "next" => {
            // Set the step kind BEFORE resuming: the VM reads pending_step right
            // after the condvar wakes, so it must be in place first.
            request_step(bp_state, StepKind::Over);
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({}));
        }
        "stepIn" => {
            request_step(bp_state, StepKind::Into);
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({}));
        }
        "stepOut" => {
            request_step(bp_state, StepKind::Out);
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({}));
        }
        "pause" => {
            let mut g = shared.inner.lock().expect("dap lock");
            g.pause_request = true;
            shared.emit_response(req, true, json!({}));
        }
        "evaluate" => {
            let expr = req
                .arguments
                .get("expression")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let snap = shared.inner.lock().expect("dap lock").snapshot.clone();
            let result = evaluate_expression(&expr, &snap);
            shared.emit_response(
                req,
                true,
                json!({ "result": result, "variablesReference": 0 }),
            );
        }
        "terminate" | "disconnect" => {
            dlog!("{} — tearing down", req.command);
            shared.disconnected.store(true, Ordering::SeqCst);
            shared.resume_with(DebugAction::Quit);
            shared.emit_response(req, true, json!({}));
            shared.emit_event("terminated", json!({}));
        }
        other => {
            dlog!("unknown command {}", other);
            shared.emit_response(req, true, json!({}));
        }
    }
}

/// How to step after a stop.
#[derive(Debug, Clone, Copy)]
pub enum StepKind {
    /// `next` — step over.
    Over,
    /// `stepIn` — step into.
    Into,
    /// `stepOut` — step out.
    Out,
}

fn request_step(bp_state: &Arc<Mutex<BreakpointState>>, kind: StepKind) {
    if let Ok(mut g) = bp_state.lock() {
        g.pending_step = Some(kind);
    }
}

fn leaf(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(_, t)| t.to_string())
        .unwrap_or_else(|| path.to_string())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

/// Evaluate a debugger expression. v1 supports direct variable-name lookup
/// against the paused snapshot (Locals first, then Globals). Anything else
/// returns a hint rather than spawning a sub-interpreter.
fn evaluate_expression(expr: &str, snap: &PauseSnapshot) -> String {
    let needle = expr.trim();
    if needle.is_empty() {
        return String::new();
    }
    for src in [&snap.locals, &snap.globals] {
        for v in src.iter() {
            if v.name == needle {
                return v.repr.clone();
            }
        }
    }
    format!("<cannot evaluate `{needle}`>")
}

// ─── Snapshot capture (awk values) ───────────────────────────────────────────

/// Base `variablesReference` for expandable arrays. Scopes use 1000/2000, so
/// array containers start at 10_000 and increment per array. Stable within one
/// pause; reset on each [`build_snapshot`] call.
const CONTAINER_REF_BASE: u32 = 10_000;

/// Allocator for per-pause container var-refs.
struct RefAlloc {
    next: u32,
}
impl RefAlloc {
    fn alloc(&mut self) -> u32 {
        let r = self.next;
        self.next += 1;
        r
    }
}

/// AWK's SUBSEP (0x1c) joins multidimensional array subscripts. Render it as a
/// comma so `a[1,2]` keys read as `1,2` instead of showing a control char.
fn display_key(k: &str) -> String {
    k.replace('\u{1c}', ",")
}

/// Build one scope row for `(name, value)`. Scalars are leaves; arrays get an
/// expandable var-ref whose children are their (sorted) element rows.
fn snap_one(
    name: &str,
    v: &Value,
    refs: &mut RefAlloc,
    map: &mut HashMap<u32, Vec<VarChild>>,
) -> VarSnap {
    match v {
        Value::Array(arr) => {
            let len = arr.len();
            let var_ref = if len == 0 { 0 } else { refs.alloc() };
            if var_ref != 0 {
                let mut children: Vec<VarChild> = arr
                    .iter()
                    .take(5000)
                    .map(|(k, val)| VarChild {
                        name: display_key(k),
                        repr: truncate(&crate::debugger::format_value(val), MAX_VAR_REPR),
                        var_ref: 0,
                    })
                    .collect();
                children.sort_by(|a, b| a.name.cmp(&b.name));
                map.insert(var_ref, children);
            }
            VarSnap {
                name: name.to_string(),
                repr: format!("array ({} element{})", len, if len == 1 { "" } else { "s" }),
                kind: "array".to_string(),
                var_ref,
            }
        }
        _ => VarSnap {
            name: name.to_string(),
            repr: truncate(&crate::debugger::format_value(v), MAX_VAR_REPR),
            kind: "scalar".to_string(),
            var_ref: 0,
        },
    }
}

/// Capture a sorted scope from `(name, &Value)` pairs.
fn capture_scope(
    pairs: &[(String, &Value)],
    refs: &mut RefAlloc,
    map: &mut HashMap<u32, Vec<VarChild>>,
) -> Vec<VarSnap> {
    let mut out: Vec<VarSnap> = pairs
        .iter()
        .map(|(name, v)| snap_one(name, v, refs, map))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Assemble a full [`PauseSnapshot`] from the captured locals/globals and the
/// call-stack frames. Called by the VM debug hook (which holds the runtime).
pub(crate) fn build_snapshot(
    file: String,
    line: usize,
    reason: &str,
    frames: Vec<FrameSnap>,
    locals: &[(String, &Value)],
    globals: &[(String, &Value)],
) -> PauseSnapshot {
    let mut refs = RefAlloc {
        next: CONTAINER_REF_BASE,
    };
    let mut var_ref_map = HashMap::new();
    let local_snaps = capture_scope(locals, &mut refs, &mut var_ref_map);
    let global_snaps = capture_scope(globals, &mut refs, &mut var_ref_map);
    PauseSnapshot {
        file,
        line,
        reason: reason.to_string(),
        frames,
        locals: local_snaps,
        globals: global_snaps,
        var_ref_map,
    }
}

// ─── Public entrypoint ───────────────────────────────────────────────────────

/// Run `awkrs --dap [HOST:PORT]`.
///
/// * **Stdio mode** (`--dap`) — DAP traffic on stdio. Fine for manual testing;
///   the program's own `print` output is interleaved on stdout.
/// * **TCP mode** (`--dap HOST:PORT`) — DAP on the socket; stdout is left free
///   for program output. This is the mode IDE plugins use.
///
/// Returns the process exit code.
pub fn run_with_args(connect_addr: Option<String>) -> i32 {
    dlog!(
        "starting --dap pid={} addr={:?} version={}",
        std::process::id(),
        connect_addr,
        env!("CARGO_PKG_VERSION")
    );
    let (reader, writer): (Box<dyn Read + Send>, Box<dyn Write + Send>) = match connect_addr {
        Some(addr) => match std::net::TcpStream::connect(&addr) {
            Ok(s) => {
                dlog!("connected tcp {}", addr);
                let r = s.try_clone().expect("dap: tcp clone");
                (Box::new(r), Box::new(s))
            }
            Err(e) => {
                eprintln!("awkrs --dap: connect {addr}: {e}");
                return 2;
            }
        },
        None => (Box::new(io::stdin()), Box::new(io::stdout())),
    };

    let shared = DapShared::new(writer);
    let bp_state = Arc::new(Mutex::new(BreakpointState::default()));
    let (_reader_handle, launch_rx) =
        spawn_reader_with_input(shared.clone(), bp_state.clone(), reader);

    // Block until the client sends `launch`.
    let lp = match launch_rx.recv() {
        Ok(p) => p,
        Err(_) => return 1,
    };

    shared.emit_event(
        "process",
        json!({ "name": lp.program, "isLocalProcess": true, "startMethod": "launch" }),
    );
    shared.emit_event("thread", json!({ "reason": "started", "threadId": 1 }));

    let code = launch_and_run(&shared, &bp_state, &lp);

    shared.emit_event("exited", json!({ "exitCode": code }));
    shared.emit_event("terminated", json!({}));
    code
}

/// Parse, compile (with line markers), install the debugger, and run the program
/// over its input. Mirrors the sequential core of [`crate::run`].
fn launch_and_run(
    shared: &Arc<DapShared>,
    bp_state: &Arc<Mutex<BreakpointState>>,
    lp: &LaunchParams,
) -> i32 {
    use crate::runtime::Runtime;

    if let Some(cwd) = &lp.cwd {
        let _ = std::env::set_current_dir(cwd);
    }

    let source = match std::fs::read_to_string(&lp.program) {
        Ok(s) => s,
        Err(e) => {
            shared.emit_event(
                "output",
                json!({ "category": "stderr", "output": format!("awkrs --dap: cannot read {}: {}\n", lp.program, e) }),
            );
            return 1;
        }
    };

    let prog = match crate::parser::parse_program_debug(&source) {
        Ok(p) => p,
        Err(e) => {
            shared.emit_event(
                "output",
                json!({ "category": "stderr", "output": format!("awkrs: {}\n", e) }),
            );
            return 1;
        }
    };
    let cp = match crate::compiler::Compiler::compile_program_debug(&prog) {
        Ok(c) => c,
        Err(e) => {
            shared.emit_event(
                "output",
                json!({ "category": "stderr", "output": format!("awkrs: {}\n", e) }),
            );
            return 1;
        }
    };

    let files: Vec<std::path::PathBuf> = lp.args.iter().map(std::path::PathBuf::from).collect();

    let mut rt = Runtime::new();
    rt.init_argv(&files);
    rt.slots = cp.init_slots(&rt.vars);
    rt.symtab_slot_map = cp.slot_map.clone();

    // Install the debugger with the pre-set breakpoints and DAP backend.
    let mut dbg = crate::debugger::Debugger::new();
    dbg.set_file(&lp.program);
    dbg.load_source(&source);
    for line in all_breakpoint_lines(bp_state) {
        dbg.add_breakpoint_line(line);
    }
    {
        let bp = bp_state.lock().expect("bp lock");
        for name in &bp.function_breakpoints {
            dbg.add_breakpoint_sub(name);
        }
    }
    dbg.set_dap_backend(shared.clone(), bp_state.clone());
    // stopOnEntry / noDebug: when not stopping on entry (and not in noDebug),
    // start with step mode off so we run to the first breakpoint.
    if lp.stop_on_entry && !lp.no_debug {
        dbg.set_step_mode(true);
    } else {
        dbg.set_step_mode(false);
    }
    rt.debugger = Some(dbg);

    rt.refresh_special_arrays(&cp, "awkrs");
    if let Err(e) = crate::attach_primary_input_before_begin_for_getline(&cp, &files, &mut rt) {
        return finish_with_error(shared, &mut rt, e);
    }

    // BEGIN.
    if let Err(e) = crate::vm::vm_run_begin(&cp, &mut rt) {
        return finish_with_error(shared, &mut rt, e);
    }
    rt.refresh_special_arrays(&cp, "awkrs");
    let _ = crate::vm::flush_print_buf(&mut rt.print_buf);

    // Record loop (unless `exit` already fired in BEGIN).
    let program_reads_input = !cp.record_rules.is_empty()
        || !cp.end_chunks.is_empty()
        || !cp.beginfile_chunks.is_empty()
        || !cp.endfile_chunks.is_empty();
    if !rt.exit_pending && program_reads_input {
        let mut range_state: Vec<bool> = vec![false; cp.prog_rules_len];
        if files.is_empty() {
            rt.filename = "-".into();
            if let Err(e) = run_one_input(shared, &cp, &mut rt, None, &mut range_state) {
                return finish_with_error(shared, &mut rt, e);
            }
        } else {
            for (i, p) in files.iter().enumerate() {
                rt.vars.insert("ARGIND".into(), Value::Num((i + 1) as f64));
                rt.filename = p.to_string_lossy().into_owned();
                rt.fnr = 0.0;
                if let Err(e) =
                    run_one_input(shared, &cp, &mut rt, Some(p.as_path()), &mut range_state)
                {
                    return finish_with_error(shared, &mut rt, e);
                }
                if rt.exit_pending {
                    break;
                }
            }
        }
    }

    // END.
    rt.detach_input_reader();
    if let Err(e) = crate::vm::vm_run_end(&cp, &mut rt) {
        return finish_with_error(shared, &mut rt, e);
    }
    let _ = crate::vm::flush_print_buf(&mut rt.print_buf);
    rt.exit_code
}

/// Run BEGINFILE → records → ENDFILE for one input source.
fn run_one_input(
    shared: &Arc<DapShared>,
    cp: &crate::bytecode::CompiledProgram,
    rt: &mut crate::runtime::Runtime,
    path: Option<&std::path::Path>,
    range_state: &mut [bool],
) -> crate::Result<()> {
    crate::vm::vm_run_beginfile(cp, rt)?;
    if rt.exit_pending {
        crate::vm::vm_run_endfile(cp, rt)?;
        return Ok(());
    }
    crate::process_file(path, cp, range_state, rt)?;
    crate::vm::vm_run_endfile(cp, rt)?;
    let _ = crate::vm::flush_print_buf(&mut rt.print_buf);
    let _ = shared; // (reserved for future per-file output events)
    Ok(())
}

/// Map a VM error to an exit code, emitting it as a DAP `output` event unless it
/// is the expected debugger-quit / client-disconnect path.
fn finish_with_error(
    shared: &Arc<DapShared>,
    rt: &mut crate::runtime::Runtime,
    e: crate::Error,
) -> i32 {
    let _ = crate::vm::flush_print_buf(&mut rt.print_buf);
    match e {
        crate::Error::Exit(code) => code,
        crate::Error::Runtime(ref msg) if msg == "debugger: quit" || shared.was_disconnected() => 0,
        other => {
            shared.emit_event(
                "output",
                json!({ "category": "stderr", "output": format!("awkrs: {}\n", other) }),
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{AwkMap, Value};

    fn refs() -> (RefAlloc, HashMap<u32, Vec<VarChild>>) {
        (
            RefAlloc {
                next: CONTAINER_REF_BASE,
            },
            HashMap::new(),
        )
    }

    #[test]
    fn scalar_row_is_leaf() {
        let (mut r, mut m) = refs();
        let row = snap_one("x", &Value::Num(42.0), &mut r, &mut m);
        assert_eq!(row.name, "x");
        assert_eq!(row.repr, "42");
        assert_eq!(row.kind, "scalar");
        assert_eq!(row.var_ref, 0);
        assert!(m.is_empty());
    }

    #[test]
    fn array_row_expands_to_sorted_children() {
        let mut arr = AwkMap::default();
        arr.insert("b".to_string(), Value::Num(2.0));
        arr.insert("a".to_string(), Value::StrLit("hi".to_string()));
        let (mut r, mut m) = refs();
        let row = snap_one("arr", &Value::Array(arr), &mut r, &mut m);
        assert_eq!(row.kind, "array");
        assert!(row.var_ref >= CONTAINER_REF_BASE);
        assert_eq!(row.repr, "array (2 elements)");
        let kids = m.get(&row.var_ref).expect("expandable");
        assert_eq!(kids.len(), 2);
        // Sorted by key.
        assert_eq!(kids[0].name, "a");
        assert_eq!(kids[0].repr, "\"hi\"");
        assert_eq!(kids[1].name, "b");
        assert_eq!(kids[1].repr, "2");
    }

    #[test]
    fn empty_array_is_not_expandable() {
        let (mut r, mut m) = refs();
        let row = snap_one("e", &Value::Array(AwkMap::default()), &mut r, &mut m);
        assert_eq!(row.var_ref, 0);
        assert_eq!(row.repr, "array (0 elements)");
    }

    #[test]
    fn subsep_keys_render_with_comma() {
        let mut arr = AwkMap::default();
        arr.insert("1\u{1c}2".to_string(), Value::Num(9.0));
        let (mut r, mut m) = refs();
        let row = snap_one("m", &Value::Array(arr), &mut r, &mut m);
        let kids = m.get(&row.var_ref).unwrap();
        assert_eq!(kids[0].name, "1,2");
    }

    #[test]
    fn build_snapshot_separates_locals_and_globals_with_unique_refs() {
        let mut a1 = AwkMap::default();
        a1.insert("k".to_string(), Value::Num(1.0));
        let a1v = Value::Array(a1);
        let mut a2 = AwkMap::default();
        a2.insert("k".to_string(), Value::Num(2.0));
        let a2v = Value::Array(a2);
        let locals = vec![("la".to_string(), &a1v)];
        let globals = vec![("ga".to_string(), &a2v)];
        let snap = build_snapshot("f.awk".into(), 3, "step", vec![], &locals, &globals);
        assert_eq!(snap.locals.len(), 1);
        assert_eq!(snap.globals.len(), 1);
        // Distinct container refs for the two arrays.
        assert_ne!(snap.locals[0].var_ref, snap.globals[0].var_ref);
        assert!(snap.var_ref_map.contains_key(&snap.locals[0].var_ref));
        assert!(snap.var_ref_map.contains_key(&snap.globals[0].var_ref));
    }

    #[test]
    fn evaluate_resolves_names_from_snapshot() {
        let snap = PauseSnapshot {
            locals: vec![VarSnap {
                name: "x".into(),
                repr: "7".into(),
                kind: "scalar".into(),
                var_ref: 0,
            }],
            ..Default::default()
        };
        assert_eq!(evaluate_expression("x", &snap), "7");
        assert!(evaluate_expression("nope", &snap).contains("cannot evaluate"));
    }
}
