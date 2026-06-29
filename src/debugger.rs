//! Breakpoint / single-step state machine for the awk debugger.
//!
//! Ported from strykelang's `debugger.rs`, trimmed to the pieces awkrs needs and
//! retargeted from strykelang's `Scope`/`StrykeValue` to awkrs's
//! [`crate::runtime::Value`]. There is only one front-end here — the Debug
//! Adapter Protocol server in [`crate::dap`]. (strykelang additionally has a
//! `perl -d`-style TTY REPL; that path is intentionally omitted.)
//!
//! The VM (`src/vm.rs`) drives this on each [`crate::bytecode::Op::DebugLine`]
//! marker: it asks [`Debugger::should_stop`] whether the current source line is
//! a stop point, and if so builds a [`crate::dap::PauseSnapshot`] and calls
//! [`Debugger::handle_pause`], which emits a `stopped` event and blocks until the
//! client resumes. Function entry/return are reported via
//! [`Debugger::enter_sub`] / [`Debugger::leave_sub`] so step-over / step-out can
//! reason about call depth.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::runtime::Value;

/// Debugger state shared with the VM (lives on [`crate::runtime::Runtime`]).
pub struct Debugger {
    /// Breakpoints by source line.
    breakpoints: HashSet<usize>,
    /// Breakpoints by function name.
    sub_breakpoints: HashSet<String>,
    /// Single-step mode: stop at every line.
    step_mode: bool,
    /// Step-over: stop at the next line at the same or a shallower call depth.
    step_over_depth: Option<usize>,
    /// Step-out: stop when the call depth drops below this.
    step_out_depth: Option<usize>,
    /// Current call depth (bumped by [`Self::enter_sub`] / [`Self::leave_sub`]).
    call_depth: usize,
    /// Last line we stopped at — avoids re-stopping on the same line.
    last_stop_line: Option<usize>,
    /// Call depth at the last stop; paired with [`Self::last_stop_line`] so a
    /// depth change (entered/returned a function) counts as forward progress.
    last_stop_depth: usize,
    /// Source file path (for the `stopped` snapshot).
    pub file: String,
    /// Source lines (currently informational; reserved for future listing).
    source_lines: Vec<String>,
    /// Master enable flag.
    enabled: bool,
    /// DAP backend handle. Always `Some` in practice (set right after construction).
    dap_backend: Option<DapBackendHandle>,
}

/// Handle into the DAP server: the shared protocol state plus the breakpoint
/// channel the reader thread writes step/breakpoint requests into.
pub struct DapBackendHandle {
    /// `shared` field.
    pub shared: Arc<crate::dap::DapShared>,
    /// `bp_state` field.
    pub bp_state: Arc<Mutex<crate::dap::BreakpointState>>,
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

impl Debugger {
    /// `new` — see implementation.
    pub fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
            sub_breakpoints: HashSet::new(),
            // Start in step mode so a `stopOnEntry` launch halts on the first
            // line; the launcher clears it when stopOnEntry is false.
            step_mode: true,
            step_over_depth: None,
            step_out_depth: None,
            call_depth: 0,
            last_stop_line: None,
            last_stop_depth: 0,
            file: String::new(),
            source_lines: Vec::new(),
            enabled: true,
            dap_backend: None,
        }
    }

    /// Add a line breakpoint (the DAP server sets these before the VM starts).
    pub fn add_breakpoint_line(&mut self, line: usize) {
        self.breakpoints.insert(line);
    }

    /// Add a function breakpoint.
    pub fn add_breakpoint_sub(&mut self, name: &str) {
        self.sub_breakpoints.insert(name.to_string());
    }

    /// Replace the entire line-breakpoint set (DAP re-sends the full set each
    /// `setBreakpoints`).
    pub fn set_line_breakpoints(&mut self, lines: &[usize]) {
        self.breakpoints = lines.iter().copied().collect();
    }

    /// Toggle step mode (DAP flips this for `stepIn` / `stopOnEntry`).
    pub fn set_step_mode(&mut self, on: bool) {
        self.step_mode = on;
    }

    /// Install the DAP backend. After this, [`Self::handle_pause`] routes through
    /// the DAP server.
    pub fn set_dap_backend(
        &mut self,
        shared: Arc<crate::dap::DapShared>,
        bp_state: Arc<Mutex<crate::dap::BreakpointState>>,
    ) {
        self.dap_backend = Some(DapBackendHandle { shared, bp_state });
    }

    /// Load source for display (informational).
    pub fn load_source(&mut self, source: &str) {
        self.source_lines = source.lines().map(String::from).collect();
    }

    /// Set the source file path.
    pub fn set_file(&mut self, file: &str) {
        self.file = file.to_string();
    }

    /// True if `line` carries a line breakpoint (drives the `stopped` reason).
    pub fn is_line_breakpoint(&self, line: usize) -> bool {
        self.breakpoints.contains(&line)
    }

    /// True when the client has requested an asap pause (the `pause` button).
    /// Routed through the DAP backend's shared state.
    pub fn pause_requested(&self) -> bool {
        self.dap_backend
            .as_ref()
            .is_some_and(|b| b.shared.want_pause())
    }

    /// Whether the VM should stop at this source line.
    pub fn should_stop(&mut self, line: usize) -> bool {
        if !self.enabled {
            return false;
        }
        // Line 0 is the "no source mapping" sentinel — never user-visible.
        if line == 0 {
            return false;
        }
        // Honor an async pause request before any same-line/step filtering so
        // the `pause` button always halts at the next line.
        if self.pause_requested() {
            return true;
        }
        // Same-line guard: skip when we haven't made progress since the last
        // stop. Progress = the line moved OR the call depth changed. This keeps
        // a `next` from stopping twice when one source line holds several
        // statements (`a=1; b=2`).
        if self.last_stop_line == Some(line) && self.call_depth == self.last_stop_depth {
            return false;
        }
        // We've moved off the last-stopped line, so clear the guard. Without
        // this, a breakpoint inside a loop / per-record rule would fire only on
        // the first iteration (control returns to the same line at the same
        // depth and the guard above would keep suppressing it).
        self.last_stop_line = None;
        if self.breakpoints.contains(&line) {
            return true;
        }
        if self.step_mode {
            return true;
        }
        if let Some(depth) = self.step_over_depth {
            if self.call_depth <= depth {
                self.step_over_depth = None;
                return true;
            }
        }
        if let Some(depth) = self.step_out_depth {
            if self.call_depth < depth {
                self.step_out_depth = None;
                return true;
            }
        }
        false
    }

    /// Whether to stop at entry to function `name`.
    pub fn should_stop_at_sub(&self, name: &str) -> bool {
        self.enabled && self.sub_breakpoints.contains(name)
    }

    /// Notify of a function call (depth bookkeeping for step-over / step-out).
    pub fn enter_sub(&mut self, _name: &str) {
        self.call_depth += 1;
    }

    /// Notify of a function return.
    pub fn leave_sub(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    /// Handle a stop: emit a `stopped` event with `snap`, block on the DAP
    /// condvar until the client resumes, then apply any step request / breakpoint
    /// resync the reader thread queued while we were paused. Returns the resume
    /// action (Continue or Quit).
    pub fn handle_pause(&mut self, snap: crate::dap::PauseSnapshot) -> DebugAction {
        self.last_stop_line = Some(snap.line);
        self.last_stop_depth = self.call_depth;
        self.step_mode = false;

        let Some(backend) = self.dap_backend.as_ref() else {
            return DebugAction::Continue;
        };
        let shared = backend.shared.clone();
        let bp = backend.bp_state.clone();
        let action = shared.pause(snap);

        if let Ok(mut g) = bp.lock() {
            if let Some(kind) = g.pending_step.take() {
                match kind {
                    crate::dap::StepKind::Over => self.step_over_depth = Some(self.call_depth),
                    crate::dap::StepKind::Into => self.step_mode = true,
                    crate::dap::StepKind::Out => self.step_out_depth = Some(self.call_depth),
                }
            }
            // The client may have changed breakpoints while we were paused.
            // awk debugging is effectively single-file, so flatten every
            // source's breakpoints rather than path-matching on `self.file`
            // (the client's absolute path may not equal the launched path).
            let lines: Vec<usize> = g.line_breakpoints.values().flatten().copied().collect();
            self.set_line_breakpoints(&lines);
        }
        action
    }
}

/// What the VM should do after a debugger stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugAction {
    /// Resume execution.
    Continue,
    /// Tear the program down (client disconnected / terminated).
    Quit,
}

/// Render an awk [`Value`] for the Variables / hover panels. Strings are quoted
/// so empty vs `"0"` vs `0` are distinguishable; arrays are summarised (their
/// elements are shown as expandable child rows by [`crate::dap`]).
pub(crate) fn format_value(v: &Value) -> String {
    match v {
        Value::Uninit => "uninitialized".to_string(),
        Value::Str(s) | Value::StrLit(s) => format!("\"{}\"", s.escape_default()),
        Value::Regexp(s) => format!("@/{}/", s),
        Value::Array(a) => format!(
            "array ({} element{})",
            a.len(),
            if a.len() == 1 { "" } else { "s" }
        ),
        // Num / Mpfr — use awk's own number→string coercion.
        _ => v.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults() {
        let d = Debugger::new();
        assert!(d.breakpoints.is_empty());
        assert!(d.step_mode);
        assert!(d.enabled);
        assert_eq!(d.call_depth, 0);
    }

    #[test]
    fn stops_at_breakpoint_not_elsewhere() {
        let mut d = Debugger::new();
        d.step_mode = false;
        d.breakpoints.insert(10);
        assert!(d.should_stop(10));
        assert!(!d.should_stop(11));
    }

    #[test]
    fn step_mode_stops_everywhere() {
        let mut d = Debugger::new();
        d.step_mode = true;
        assert!(d.should_stop(1));
        assert!(d.should_stop(999));
    }

    #[test]
    fn disabled_never_stops() {
        let mut d = Debugger::new();
        d.enabled = false;
        d.step_mode = true;
        assert!(!d.should_stop(1));
    }

    #[test]
    fn line_zero_sentinel_never_stops() {
        let mut d = Debugger::new();
        d.step_mode = true;
        assert!(!d.should_stop(0));
    }

    #[test]
    fn enter_leave_sub_tracks_depth() {
        let mut d = Debugger::new();
        d.enter_sub("f");
        d.enter_sub("g");
        assert_eq!(d.call_depth, 2);
        d.leave_sub();
        assert_eq!(d.call_depth, 1);
        d.leave_sub();
        d.leave_sub();
        assert_eq!(d.call_depth, 0);
    }

    #[test]
    fn step_over_skips_nested_frame_then_resumes() {
        let mut d = Debugger::new();
        d.step_mode = false;
        d.step_over_depth = Some(0);
        d.enter_sub("callee");
        assert!(!d.should_stop(20));
        d.leave_sub();
        assert!(d.should_stop(11));
        assert!(d.step_over_depth.is_none());
    }

    #[test]
    fn step_out_fires_on_return() {
        let mut d = Debugger::new();
        d.step_mode = false;
        d.enter_sub("callee");
        d.last_stop_line = Some(5);
        d.last_stop_depth = 1;
        d.step_out_depth = Some(1);
        assert!(!d.should_stop(5));
        d.leave_sub();
        assert!(d.should_stop(5));
    }

    #[test]
    fn same_line_guard_yields_to_depth_change() {
        let mut d = Debugger::new();
        d.last_stop_line = Some(10);
        d.last_stop_depth = 0;
        d.step_mode = true;
        assert!(!d.should_stop(10));
        d.enter_sub("callee");
        assert!(d.should_stop(10));
    }

    #[test]
    fn format_value_quotes_strings_and_summarises_arrays() {
        assert_eq!(format_value(&Value::StrLit("hi".into())), "\"hi\"");
        assert_eq!(format_value(&Value::Num(42.0)), "42");
        assert_eq!(format_value(&Value::Uninit), "uninitialized");
        let mut m = crate::runtime::AwkMap::default();
        m.insert("a".to_string(), Value::Num(1.0));
        assert_eq!(format_value(&Value::Array(m)), "array (1 element)");
    }
}
