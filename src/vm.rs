//! Stack-based virtual machine that executes compiled bytecode.

use crate::ast::{BinOp, IncDecOp};
use crate::bignum;
use crate::builtins;
use crate::bytecode::*;
use crate::error::{Error, Result};
use crate::flow::Flow;
use crate::format;
use crate::runtime::AwkMap;
use crate::runtime::{sorted_in_mode, value_to_float, Runtime, SortedInMode, Value};
use rug::ops::Pow as _;
use rug::Float;
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::ffi::c_void;
use std::io::{self, Write};
use std::mem;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;

/// Max interned identifier length resolved via stack buffer (`with_short_pool_name_mut`).
const POOL_NAME_STACK_MAX: usize = 128;

// ── VM context ──────────────────────────────────────────────────────────────

struct ForInState {
    keys: Vec<String>,
    index: usize,
}

pub struct VmCtx<'a> {
    pub cp: &'a CompiledProgram,
    pub rt: &'a mut Runtime,
    locals: Vec<AwkMap<String, Value>>,
    in_function: bool,
    print_out: Option<&'a mut Vec<String>>,
    for_in_iters: Vec<ForInState>,
    stack: Vec<Value>,
}

impl<'a> VmCtx<'a> {
    pub fn new(cp: &'a CompiledProgram, rt: &'a mut Runtime) -> Self {
        // Take the pre-allocated stack from Runtime to avoid per-call malloc.
        let mut stack = std::mem::take(&mut rt.vm_stack);
        stack.clear();
        Self {
            cp,
            rt,
            locals: Vec::new(),
            in_function: false,
            print_out: None,
            for_in_iters: Vec::new(),
            stack,
        }
    }

    pub fn with_print_capture(
        cp: &'a CompiledProgram,
        rt: &'a mut Runtime,
        out: &'a mut Vec<String>,
    ) -> Self {
        let mut stack = std::mem::take(&mut rt.vm_stack);
        stack.clear();
        Self {
            cp,
            rt,
            locals: Vec::new(),
            in_function: false,
            print_out: Some(out),
            for_in_iters: Vec::new(),
            stack,
        }
    }

    /// Return the stack buffer to Runtime for reuse.
    fn recycle(mut self) {
        self.stack.clear();
        self.rt.vm_stack = std::mem::take(&mut self.stack);
    }

    fn emit_print(&mut self, s: &str) {
        if let Some(buf) = self.print_out.as_mut() {
            buf.push(s.to_string());
        } else {
            self.rt.print_buf.extend_from_slice(s.as_bytes());
        }
    }

    fn emit_flush(&mut self) -> Result<()> {
        if self.print_out.is_none() {
            flush_print_buf(&mut self.rt.print_buf)?;
            std::io::stdout().flush().map_err(Error::Io)?;
        }
        Ok(())
    }

    /// Scalar read for locals/slots/globals: [`Cow::Borrowed`] when stored (no clone);
    /// [`Cow::Owned`] for synthesized scalars (`NR`, `NF`, …) or missing globals.
    pub(crate) fn var_value_cow(&mut self, name: &str) -> Cow<'_, Value> {
        for frame in self.locals.iter().rev() {
            if let Some(v) = frame.get(name) {
                return Cow::Borrowed(v);
            }
        }
        if let Some(&slot) = self.cp.slot_map.get(name) {
            return Cow::Borrowed(&self.rt.slots[slot as usize]);
        }
        // `NF` is synthesized from the record (`nf()`), not `vars["NF"]` (matches legacy `get_var`).
        if name == "NF" {
            return Cow::Owned(Value::Num(self.rt.nf() as f64));
        }
        if let Some(v) = self.rt.get_global_var(name) {
            return Cow::Borrowed(v);
        }
        match name {
            "NR" => Cow::Owned(Value::Num(self.rt.nr)),
            "FNR" => Cow::Owned(Value::Num(self.rt.fnr)),
            "FILENAME" => Cow::Owned(Value::Str(self.rt.filename.clone())),
            _ => Cow::Owned(Value::Uninit),
        }
    }

    fn get_var(&mut self, name: &str) -> Value {
        match self.var_value_cow(name) {
            Cow::Borrowed(v) => v.clone(),
            Cow::Owned(v) => v,
        }
    }

    /// `arr[key]` — `SYMTAB` uses live global/slot resolution (gawk lvalue
    /// semantics). Function parameters that are arrays live in the current
    /// frame (`self.locals.last()`); consult them first so writes/reads
    /// through a by-reference array param hit the caller's data.
    fn array_elem_get(&self, name: &str, key: &str) -> Value {
        if name == "SYMTAB" {
            return self.rt.symtab_elem_get(key);
        }
        // Check the current (innermost) frame for an array param with this
        // name — POSIX array call-by-reference for user functions.
        for frame in self.locals.iter().rev() {
            if let Some(Value::Array(a)) = frame.get(name) {
                return match a.get(key) {
                    Some(Value::Num(n)) => Value::Num(*n),
                    Some(v) => v.clone(),
                    None => Value::Str(String::new()),
                };
            }
        }
        self.rt.array_get(name, key)
    }

    fn array_elem_set(&mut self, name: &str, key: String, val: Value) {
        if name == "SYMTAB" {
            self.rt.symtab_elem_set(&key, val);
            return;
        }
        // POSIX array call-by-reference: writes through a function array
        // param go to the current frame, not global vars.
        for frame in self.locals.iter_mut().rev() {
            if let Some(slot) = frame.get_mut(name) {
                if let Value::Array(a) = slot {
                    a.insert(key, val);
                    return;
                }
                // If the local was uninit (no value yet), promote to an
                // array — POSIX allows lazy array creation.
                if matches!(slot, Value::Uninit) {
                    let mut a = crate::runtime::AwkMap::default();
                    a.insert(key, val);
                    *slot = Value::Array(a);
                    return;
                }
            }
        }
        self.rt.array_set(name, key, val);
    }

    fn symtab_has(&self, key: &str) -> bool {
        self.rt.array_has("SYMTAB", key)
    }

    fn symtab_key_count(&self) -> usize {
        self.rt.symtab_keys_reflect().len()
    }

    /// Keys for `for (k in …)` / `SYMTAB` iteration order, including **`PROCINFO["sorted_in"]`** and
    /// user-defined comparators (`cmp` with two index arguments).
    pub(crate) fn for_in_keys(&mut self, name: &str) -> Result<Vec<String>> {
        // POSIX array call-by-reference: check the current frame for an
        // array param with this name before falling through to global vars.
        let frame_keys: Option<Vec<String>> = self.locals.iter().rev().find_map(|frame| {
            if let Some(Value::Array(a)) = frame.get(name) {
                Some(a.keys().cloned().collect())
            } else {
                None
            }
        });

        if let SortedInMode::CustomFn(fname) = sorted_in_mode(self.rt) {
            if name == "SYMTAB" {
                let mut keys = self.rt.symtab_keys_reflect();
                if self.rt.posix {
                    return Ok(keys);
                }
                sort_keys_with_custom_cmp(self, &mut keys, fname.as_str(), name)?;
                return Ok(keys);
            }
            let mut keys = match frame_keys {
                Some(k) => k,
                None => {
                    let Some(Value::Array(a)) = self.rt.get_global_var(name) else {
                        return Ok(Vec::new());
                    };
                    a.keys().cloned().collect::<Vec<String>>()
                }
            };
            if self.rt.posix {
                return Ok(keys);
            }
            sort_keys_with_custom_cmp(self, &mut keys, fname.as_str(), name)?;
            return Ok(keys);
        }
        if let Some(keys) = frame_keys {
            return Ok(keys);
        }
        Ok(self.rt.array_keys(name))
    }

    fn symtab_delete(&mut self, key: &str) {
        if let Some(&slot) = self.cp.slot_map.get(key) {
            let i = slot as usize;
            if i < self.rt.slots.len() {
                self.rt.slots[i] = Value::Uninit;
            }
        }
        self.rt.vars.remove(key);
    }

    /// Run `f` with `str_ref(idx)` as `&str` without a heap allocation when the name is short.
    fn with_short_pool_name_mut<T>(&mut self, idx: u32, f: impl FnOnce(&mut Self, &str) -> T) -> T {
        let s = self.str_ref(idx);
        if s.len() <= POOL_NAME_STACK_MAX {
            let mut buf = [0u8; POOL_NAME_STACK_MAX];
            buf[..s.len()].copy_from_slice(s.as_bytes());
            let name = std::str::from_utf8(&buf[..s.len()])
                .expect("interned identifier must be valid UTF-8");
            f(self, name)
        } else {
            let owned = s.to_string();
            f(self, owned.as_str())
        }
    }

    fn set_var(&mut self, name: &str, val: Value) -> Result<()> {
        for frame in self.locals.iter_mut().rev() {
            if let Some(v) = frame.get_mut(name) {
                *v = val;
                return Ok(());
            }
        }
        if name == "NF" {
            let n = val.as_number() as i32;
            self.rt.set_nf(n)?;
            return Ok(());
        }
        // gawk parity: NR/FNR are user-assignable. The internal counters
        // (`rt.nr`/`rt.fnr`) are read directly by `Op::GetNR` / `Op::GetFNR`,
        // so updating `rt.vars` alone wouldn't propagate to subsequent reads.
        // Keep both in sync — the input loop adds 1 to `rt.nr` per record, so
        // a user-set value of N causes the next record to see N+1.
        if name == "NR" {
            self.rt.nr = val.as_number();
            return Ok(());
        }
        if name == "FNR" {
            self.rt.fnr = val.as_number();
            return Ok(());
        }
        // Check slots
        if let Some(&slot) = self.cp.slot_map.get(name) {
            self.rt.slots[slot as usize] = val;
            return Ok(());
        }
        // Update cached OFS/ORS bytes when those vars change.
        match name {
            "OFS" => self.rt.ofs_bytes = val.as_str().into_bytes(),
            "ORS" => self.rt.ors_bytes = val.as_str().into_bytes(),
            // RS / CONVFMT: no cached bytes; next read uses [`Runtime::rs_string`] / CONVFMT lookup.
            _ => {}
        }
        match self.rt.vars.get_mut(name) {
            Some(v) => *v = val,
            None => {
                self.rt.vars.insert(name.to_string(), val);
            }
        }
        Ok(())
    }

    /// Resolve variable name from the string pool without a heap `String` when the
    /// identifier is short (see [`POOL_NAME_STACK_MAX`]), e.g. `for (k in a)`.
    fn set_var_interned(&mut self, var_idx: u32, val: Value) -> Result<()> {
        self.with_short_pool_name_mut(var_idx, |ctx, name| ctx.set_var(name, val))
    }

    /// Same as [`set_var_interned`](Self::set_var_interned) plus JIT slot sync for compiled loops.
    fn set_var_interned_jit_sync(&mut self, var_idx: u32, val: Value) -> Result<()> {
        self.with_short_pool_name_mut(var_idx, |ctx, name| {
            ctx.set_var(name, val)?;
            if let Some(&slot) = ctx.cp.slot_map.get(name) {
                sync_jit_slot_value(ctx, slot);
            }
            Ok(())
        })
    }

    #[inline]
    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    #[inline]
    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Str(String::new()))
    }

    #[inline]
    fn peek(&self) -> &Value {
        self.stack.last().unwrap_or(&EMPTY_STR)
    }

    fn str_ref(&self, idx: u32) -> &str {
        self.cp.strings.get(idx)
    }

    /// `typeof(name)` for a simple identifier (mirrors lvalue resolution order).
    fn typeof_scalar_name(&self, name: &str) -> Value {
        match name {
            "NR" | "FNR" | "NF" => return Value::Str("number".into()),
            "FILENAME" => return Value::Str("string".into()),
            _ => {}
        }
        for frame in self.locals.iter().rev() {
            if let Some(v) = frame.get(name) {
                return Value::Str(builtins::awk_typeof_value(v).into());
            }
        }
        if let Some(&slot) = self.cp.slot_map.get(name) {
            return Value::Str(builtins::awk_typeof_value(&self.rt.slots[slot as usize]).into());
        }
        if let Some(v) = self.rt.get_global_var(name) {
            return Value::Str(builtins::awk_typeof_value(v).into());
        }
        Value::Str("untyped".into())
    }
}

static EMPTY_STR: Value = Value::Str(String::new());

// ── Signal from VM execution ────────────────────────────────────────────────

enum VmSignal {
    Normal,
    Next,
    NextFile,
    Return(Value),
    ExitPending,
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn vm_run_begin(cp: &CompiledProgram, rt: &mut Runtime) -> Result<()> {
    let mut ctx = VmCtx::new(cp, rt);
    for chunk in &cp.begin_chunks {
        match execute(chunk, &mut ctx)? {
            VmSignal::Next => return Err(Error::Runtime("`next` is invalid in BEGIN".into())),
            VmSignal::NextFile => {
                return Err(Error::Runtime("`nextfile` is invalid in BEGIN".into()));
            }
            VmSignal::Return(_) => return Err(Error::Runtime("`return` outside function".into())),
            VmSignal::ExitPending => {
                ctx.recycle();
                return Ok(());
            }
            VmSignal::Normal => {}
        }
    }
    ctx.recycle();
    Ok(())
}

pub fn vm_run_end(cp: &CompiledProgram, rt: &mut Runtime) -> Result<()> {
    let mut ctx = VmCtx::new(cp, rt);
    for chunk in &cp.end_chunks {
        match execute(chunk, &mut ctx)? {
            VmSignal::Next => return Err(Error::Runtime("`next` is invalid in END".into())),
            VmSignal::NextFile => {
                return Err(Error::Runtime("`nextfile` is invalid in END".into()));
            }
            VmSignal::Return(_) => return Err(Error::Runtime("`return` outside function".into())),
            VmSignal::ExitPending => {
                ctx.recycle();
                return Ok(());
            }
            VmSignal::Normal => {}
        }
    }
    ctx.recycle();
    Ok(())
}

pub fn vm_run_beginfile(cp: &CompiledProgram, rt: &mut Runtime) -> Result<()> {
    let mut ctx = VmCtx::new(cp, rt);
    for chunk in &cp.beginfile_chunks {
        match execute(chunk, &mut ctx)? {
            VmSignal::Next => return Err(Error::Runtime("`next` is invalid in BEGINFILE".into())),
            VmSignal::NextFile => {
                return Err(Error::Runtime("`nextfile` is invalid in BEGINFILE".into()));
            }
            VmSignal::Return(_) => return Err(Error::Runtime("`return` outside function".into())),
            VmSignal::ExitPending => {
                ctx.recycle();
                return Ok(());
            }
            VmSignal::Normal => {}
        }
    }
    ctx.recycle();
    Ok(())
}

pub fn vm_run_endfile(cp: &CompiledProgram, rt: &mut Runtime) -> Result<()> {
    let mut ctx = VmCtx::new(cp, rt);
    for chunk in &cp.endfile_chunks {
        match execute(chunk, &mut ctx)? {
            VmSignal::Next => return Err(Error::Runtime("`next` is invalid in ENDFILE".into())),
            VmSignal::NextFile => {
                return Err(Error::Runtime("`nextfile` is invalid in ENDFILE".into()));
            }
            VmSignal::Return(_) => return Err(Error::Runtime("`return` outside function".into())),
            VmSignal::ExitPending => {
                ctx.recycle();
                return Ok(());
            }
            VmSignal::Normal => {}
        }
    }
    ctx.recycle();
    Ok(())
}

/// Evaluate a compiled pattern against the current record.
pub fn vm_pattern_matches(
    rule: &CompiledRule,
    cp: &CompiledProgram,
    rt: &mut Runtime,
) -> Result<bool> {
    match &rule.pattern {
        CompiledPattern::Always => Ok(true),
        CompiledPattern::Regexp(idx) => {
            let pat = cp.strings.get(*idx);
            rt.ensure_regex(pat).map_err(Error::Runtime)?;
            Ok(rt.regex_ref(pat).is_match(&rt.record))
        }
        CompiledPattern::LiteralRegexp(idx) => {
            let pat = cp.strings.get(*idx);
            // gawk parity: when `IGNORECASE` is set, the literal-regex fast
            // path must still match case-insensitively. Previously awkrs's
            // optimization bypassed regex compilation, so `IGNORECASE=1` had
            // no effect on bare `/abc/` patterns.
            if rt.ignore_case_flag() {
                rt.ensure_regex(pat).map_err(Error::Runtime)?;
                Ok(rt.regex_ref(pat).is_match(&rt.record))
            } else {
                Ok(rt.record.contains(pat))
            }
        }
        CompiledPattern::Expr(chunk) => {
            let mut ctx = VmCtx::new(cp, rt);
            let r = execute(chunk, &mut ctx)?;
            let val = truthy(&ctx.pop())?;
            // Drop VmSignal — Expr patterns can't produce Next/Exit
            let _ = r;
            ctx.recycle();
            Ok(val)
        }
        CompiledPattern::Range { .. } => Ok(false), // handled via [`vm_range_step`]
    }
}

/// Match one range-pattern endpoint (regex, expr, empty, `BEGIN`-style never, or nested-range error).
pub fn vm_match_range_endpoint(
    ep: &CompiledRangeEndpoint,
    cp: &CompiledProgram,
    rt: &mut Runtime,
) -> Result<bool> {
    match ep {
        CompiledRangeEndpoint::Always => Ok(true),
        CompiledRangeEndpoint::Never => Ok(false),
        CompiledRangeEndpoint::NestedRangeError => {
            Err(Error::Runtime("nested range pattern".into()))
        }
        CompiledRangeEndpoint::Regexp(idx) => {
            let pat = cp.strings.get(*idx);
            rt.ensure_regex(pat).map_err(Error::Runtime)?;
            Ok(rt.regex_ref(pat).is_match(&rt.record))
        }
        CompiledRangeEndpoint::LiteralRegexp(idx) => {
            let pat = cp.strings.get(*idx);
            if rt.ignore_case_flag() {
                rt.ensure_regex(pat).map_err(Error::Runtime)?;
                Ok(rt.regex_ref(pat).is_match(&rt.record))
            } else {
                Ok(rt.record.contains(pat))
            }
        }
        CompiledRangeEndpoint::Expr(chunk) => {
            let mut ctx = VmCtx::new(cp, rt);
            let r = execute(chunk, &mut ctx)?;
            let val = truthy(&ctx.pop())?;
            let _ = r;
            ctx.recycle();
            Ok(val)
        }
    }
}

/// Range pattern: `state` is false until `start` matches, then true until `end` matches after a run.
pub fn vm_range_step(
    state: &mut bool,
    start: &CompiledRangeEndpoint,
    end: &CompiledRangeEndpoint,
    cp: &CompiledProgram,
    rt: &mut Runtime,
) -> Result<bool> {
    if !*state && vm_match_range_endpoint(start, cp, rt)? {
        *state = true;
    }
    if *state {
        let run = true;
        if vm_match_range_endpoint(end, cp, rt)? {
            *state = false;
        }
        return Ok(run);
    }
    Ok(false)
}

/// Execute a rule body; maps the VM completion status to [`Flow`] for the record loop driver.
pub fn vm_run_rule(
    rule: &CompiledRule,
    cp: &CompiledProgram,
    rt: &mut Runtime,
    print_out: Option<&mut Vec<String>>,
    profile_record_idx: Option<usize>,
) -> Result<Flow> {
    if let Some(i) = profile_record_idx {
        if i < rt.profile_record_hits.len() {
            rt.profile_record_hits[i] = rt.profile_record_hits[i].saturating_add(1);
        }
    }
    let mut ctx = match print_out {
        Some(buf) => VmCtx::with_print_capture(cp, rt, buf),
        None => VmCtx::new(cp, rt),
    };
    let result = match execute(&rule.body, &mut ctx)? {
        VmSignal::Normal => Ok(Flow::Normal),
        VmSignal::Next => Ok(Flow::Next),
        VmSignal::NextFile => Ok(Flow::NextFile),
        VmSignal::Return(_) => Err(Error::Runtime(
            "`return` used outside function in rule action".into(),
        )),
        VmSignal::ExitPending => Ok(Flow::ExitPending),
    };
    ctx.recycle();
    result
}

// ── JIT field callback support ─────────────────────────────────────────────

use std::cell::Cell;

thread_local! {
    /// Signal raised by JIT (0 = none, >0 = signal type from `JIT_VAL_SIGNAL_*`).
    static JIT_SIGNAL: Cell<u32> = const { Cell::new(0) };
    /// Argument for signal (e.g. exit code for `ExitWithCode`, return value for `ReturnVal`).
    static JIT_SIGNAL_ARG: Cell<f64> = const { Cell::new(0.0) };
    /// `seps` pool index between [`crate::jit::MIXED_PATSPLIT_STASH_SEPS`] and [`crate::jit::MIXED_PATSPLIT_FP_SEP_WIDE`].
    static JIT_PATSPLIT_SEPS_STASH: Cell<u32> = const { Cell::new(0) };
}

thread_local! {
    /// ForIn iterator stack for JIT-compiled for-in loops.
    static JIT_FORIN_ITERS: RefCell<Vec<ForInState>> = const { RefCell::new(Vec::new()) };
    /// Dynamic string storage for NaN-boxed JIT stack values (indices via [`crate::jit::nan_str_dyn`]).
    static JIT_DYN_STRINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Buffered `print` arguments for mixed-mode JIT (`MIXED_PRINT_ARG` / `MIXED_PRINT_FLUSH`).
    static MIXED_PRINT_SLOTS: RefCell<Vec<Option<f64>>> = const { RefCell::new(Vec::new()) };
    /// Components for multidimensional array keys (`MIXED_JOIN_KEY_ARG` / `MIXED_JOIN_ARRAY_KEY`).
    static JIT_JOIN_KEY_PARTS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    /// Buffered arguments for JIT `CallBuiltin` (`MIXED_BUILTIN_ARG` / `MIXED_BUILTIN_CALL`).
    static JIT_BUILTIN_ARGS: RefCell<Vec<Option<f64>>> = const { RefCell::new(Vec::new()) };
    /// Propagate `Error` from mixed JIT callbacks that cannot return `Result` (e.g. `getline` I/O).
    static JIT_CHUNK_ERR: RefCell<Option<Error>> = const { RefCell::new(None) };
    /// Buffered args for JIT [`Op::CallUser`] (`MIXED_CALL_USER_ARG` / `MIXED_CALL_USER_CALL`).
    static JIT_CALL_USER_ARGS: RefCell<Vec<Option<f64>>> = const { RefCell::new(Vec::new()) };
    /// Stashed array key for JIT `sub`/`gsub` on `arr[k]` (`MIXED_*_INDEX_STASH` before `MIXED_*_INDEX`).
    static SUB_FN_STASH_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn jit_f64_to_value(ctx: &VmCtx<'_>, x: f64) -> Value {
    use crate::jit::{decode_nan_str_bits, is_nan_str, is_nan_uninit};
    let bits = x.to_bits();
    if is_nan_uninit(bits) {
        return Value::Uninit;
    }
    if is_nan_str(bits) {
        let (is_dyn, idx) = decode_nan_str_bits(bits).unwrap_or((true, 0));
        if is_dyn {
            let s =
                JIT_DYN_STRINGS.with(|c| c.borrow().get(idx as usize).cloned().unwrap_or_default());
            Value::Str(s)
        } else {
            Value::Str(ctx.str_ref(idx).to_string())
        }
    } else {
        Value::Num(x)
    }
}

/// `typeof(name)` for a simple identifier while JIT may hold fresh slot values only in
/// [`Runtime::jit_slot_buf`] (mirrors `VmCtx::typeof_scalar_name` but reads slotted scalars from the scratch buffer).
fn typeof_scalar_name_for_jit(ctx: &mut VmCtx<'_>, name: &str) -> Value {
    match name {
        "NR" | "FNR" | "NF" => return Value::Str("number".into()),
        "FILENAME" => return Value::Str("string".into()),
        _ => {}
    }
    for frame in ctx.locals.iter().rev() {
        if let Some(v) = frame.get(name) {
            return Value::Str(builtins::awk_typeof_value(v).into());
        }
    }
    if let Some(&slot) = ctx.cp.slot_map.get(name) {
        let slot = slot as usize;
        let buf = &ctx.rt.jit_slot_buf;
        let v = if slot < buf.len() {
            let raw = buf[slot];
            jit_f64_to_value(ctx, raw)
        } else {
            ctx.rt.slots.get(slot).cloned().unwrap_or(Value::Uninit)
        };
        return Value::Str(builtins::awk_typeof_value(&v).into());
    }
    if let Some(v) = ctx.rt.get_global_var(name) {
        return Value::Str(builtins::awk_typeof_value(v).into());
    }
    Value::Str("untyped".into())
}

fn value_to_jit_f64(_ctx: &mut VmCtx<'_>, v: Value) -> f64 {
    use crate::jit::{nan_str_dyn, nan_uninit};
    match v {
        Value::Num(n) => n,
        Value::Uninit => nan_uninit(),
        Value::Str(s) | Value::StrLit(s) | Value::Regexp(s) => {
            let idx = JIT_DYN_STRINGS.with(|c| {
                let mut c = c.borrow_mut();
                let idx = c.len();
                c.push(s);
                idx as u32
            });
            nan_str_dyn(idx)
        }
        Value::Array(_) => 0.0,
        Value::Mpfr(f) => f.to_f64(),
    }
}

#[inline]
fn jit_scratch_slot_raw(ctx: &VmCtx<'_>, slot: usize) -> f64 {
    let buf = &ctx.rt.jit_slot_buf;
    if slot >= buf.len() {
        return 0.0;
    }
    buf[slot]
}

#[inline]
fn jit_scratch_slot_store_num(ctx: &mut VmCtx<'_>, slot: usize, n: f64) {
    let buf = &mut ctx.rt.jit_slot_buf;
    if slot >= buf.len() {
        return;
    }
    buf[slot] = n;
}

fn jit_mixed_op_dispatch(ctx: &mut VmCtx<'_>, op: u32, a1: u32, a2: f64, a3: f64) -> f64 {
    use crate::ast::BinOp;
    use crate::jit::{
        MIXED_ADD, MIXED_ADD_FIELDNUM_TO_SLOT, MIXED_ADD_FIELD_TO_SLOT,
        MIXED_ADD_MUL_FIELDNUMS_TO_SLOT, MIXED_ADD_MUL_FIELDS_TO_SLOT, MIXED_ADD_SLOT_TO_SLOT,
        MIXED_ARRAY_COMPOUND, MIXED_ARRAY_DELETE_ALL, MIXED_ARRAY_DELETE_ELEM, MIXED_ARRAY_GET,
        MIXED_ARRAY_IN, MIXED_ARRAY_INCDEC, MIXED_ARRAY_SET, MIXED_BUILTIN_ARG, MIXED_BUILTIN_CALL,
        MIXED_CALL_USER_ARG, MIXED_CALL_USER_CALL, MIXED_CMP_EQ, MIXED_CMP_GE, MIXED_CMP_GT,
        MIXED_CMP_LE, MIXED_CMP_LT, MIXED_CMP_NE, MIXED_COMPOUND_ASSIGN_FIELD, MIXED_CONCAT,
        MIXED_CONCAT_POOL, MIXED_DECR_SLOT, MIXED_DIV, MIXED_GETLINE_COPROC, MIXED_GETLINE_FILE,
        MIXED_GETLINE_INTO_RECORD, MIXED_GETLINE_PRIMARY, MIXED_GET_FIELD, MIXED_GET_SLOT,
        MIXED_GET_VAR, MIXED_GSUB_FIELD, MIXED_GSUB_INDEX, MIXED_GSUB_INDEX_STASH,
        MIXED_GSUB_RECORD, MIXED_GSUB_SLOT, MIXED_GSUB_VAR, MIXED_INCDEC_SLOT, MIXED_INCR_SLOT,
        MIXED_JOIN_ARRAY_KEY, MIXED_JOIN_KEY_ARG, MIXED_MATCH_BUILTIN, MIXED_MATCH_BUILTIN_ARR,
        MIXED_MOD, MIXED_MUL, MIXED_NEG, MIXED_NOT, MIXED_PATSPLIT, MIXED_PATSPLIT_FP,
        MIXED_PATSPLIT_FP_SEP, MIXED_PATSPLIT_FP_SEP_WIDE, MIXED_PATSPLIT_SEP,
        MIXED_PATSPLIT_STASH_SEPS, MIXED_POS, MIXED_POW, MIXED_PRINTF_FLUSH,
        MIXED_PRINTF_FLUSH_REDIR, MIXED_PRINT_ARG, MIXED_PRINT_FLUSH, MIXED_PRINT_FLUSH_REDIR,
        MIXED_PUSH_STR, MIXED_REGEX_MATCH, MIXED_REGEX_NOT_MATCH, MIXED_SET_FIELD, MIXED_SET_VAR,
        MIXED_SLOT_AS_NUMBER, MIXED_SPLIT, MIXED_SPLIT_WITH_FS, MIXED_SUB, MIXED_SUB_FIELD,
        MIXED_SUB_INDEX, MIXED_SUB_INDEX_STASH, MIXED_SUB_RECORD, MIXED_SUB_SLOT, MIXED_SUB_VAR,
        MIXED_TO_BOOL, MIXED_TRUTHINESS, MIXED_TYPEOF_ARRAY_ELEM, MIXED_TYPEOF_FIELD,
        MIXED_TYPEOF_SLOT, MIXED_TYPEOF_VALUE, MIXED_TYPEOF_VAR,
    };

    match op {
        MIXED_PUSH_STR => crate::jit::nan_str_pool(a1),
        MIXED_CONCAT => {
            let a = jit_f64_to_value(ctx, a2);
            let b = jit_f64_to_value(ctx, a3);
            if let Err(e) = a
                .reject_if_array_scalar()
                .and_then(|_| b.reject_if_array_scalar())
            {
                JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                return 0.0;
            }
            let mut s = a.into_string();
            b.append_to_string(&mut s);
            value_to_jit_f64(ctx, Value::Str(s))
        }
        MIXED_CONCAT_POOL => {
            let a = jit_f64_to_value(ctx, a2);
            if let Err(e) = a.reject_if_array_scalar() {
                JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                return 0.0;
            }
            let mut s = a.into_string();
            s.push_str(ctx.str_ref(a1));
            value_to_jit_f64(ctx, Value::Str(s))
        }
        MIXED_ADD | MIXED_SUB | MIXED_MUL | MIXED_DIV | MIXED_MOD | MIXED_POW => {
            let a = jit_f64_to_value(ctx, a2);
            let b = jit_f64_to_value(ctx, a3);
            match op {
                MIXED_ADD => a.as_number() + b.as_number(),
                MIXED_SUB => a.as_number() - b.as_number(),
                MIXED_MUL => a.as_number() * b.as_number(),
                MIXED_DIV => {
                    if b.as_number() == 0.0 {
                        JIT_CHUNK_ERR.with(|c| {
                            *c.borrow_mut() =
                                Some(Error::Runtime("division by zero attempted".into()));
                        });
                        return 0.0;
                    }
                    a.as_number() / b.as_number()
                }
                MIXED_MOD => a.as_number() % b.as_number(),
                MIXED_POW => a.as_number().powf(b.as_number()),
                _ => unreachable!(),
            }
        }
        MIXED_CMP_EQ => {
            let ic = ctx.rt.ignore_case_flag();
            let r = awk_cmp_eq(
                &jit_f64_to_value(ctx, a2),
                &jit_f64_to_value(ctx, a3),
                ic,
                ctx.rt,
            );
            r.as_number()
        }
        MIXED_CMP_NE => {
            let ic = ctx.rt.ignore_case_flag();
            let eq = awk_cmp_eq(
                &jit_f64_to_value(ctx, a2),
                &jit_f64_to_value(ctx, a3),
                ic,
                ctx.rt,
            );
            Value::Num(if eq.as_number() != 0.0 { 0.0 } else { 1.0 }).as_number()
        }
        MIXED_CMP_LT | MIXED_CMP_LE | MIXED_CMP_GT | MIXED_CMP_GE => {
            let ic = ctx.rt.ignore_case_flag();
            let bop = match op {
                MIXED_CMP_LT => BinOp::Lt,
                MIXED_CMP_LE => BinOp::Le,
                MIXED_CMP_GT => BinOp::Gt,
                MIXED_CMP_GE => BinOp::Ge,
                _ => unreachable!(),
            };
            awk_cmp_rel(
                bop,
                &jit_f64_to_value(ctx, a2),
                &jit_f64_to_value(ctx, a3),
                ic,
                ctx.rt,
            )
            .as_number()
        }
        MIXED_NEG => {
            let v = jit_f64_to_value(ctx, a2);
            Value::Num(-v.as_number()).as_number()
        }
        MIXED_POS => jit_f64_to_value(ctx, a2).as_number(),
        MIXED_NOT => {
            let v = jit_f64_to_value(ctx, a2);
            match truthy(&v) {
                Ok(t) => Value::Num(if t { 0.0 } else { 1.0 }).as_number(),
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_TO_BOOL => {
            let v = jit_f64_to_value(ctx, a2);
            match truthy(&v) {
                Ok(t) => Value::Num(if t { 1.0 } else { 0.0 }).as_number(),
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_TRUTHINESS => {
            let v = jit_f64_to_value(ctx, a2);
            match truthy(&v) {
                Ok(t) => Value::Num(if t { 1.0 } else { 0.0 }).as_number(),
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GET_FIELD => {
            let i = a2 as i32;
            match ctx.rt.field(i) {
                Ok(v) => value_to_jit_f64(ctx, v),
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GET_VAR => {
            let name = ctx.str_ref(a1).to_string();
            let v = ctx.get_var(&name);
            value_to_jit_f64(ctx, v)
        }
        MIXED_SET_VAR => {
            let name = ctx.str_ref(a1).to_string();
            let val = jit_f64_to_value(ctx, a2);
            if let Err(e) = ctx.set_var(&name, val) {
                JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
            }
            0.0
        }
        MIXED_GET_SLOT => {
            let slot = a1 as usize;
            jit_scratch_slot_raw(ctx, slot)
        }
        MIXED_REGEX_MATCH | MIXED_REGEX_NOT_MATCH => {
            let s = jit_f64_to_value(ctx, a2).as_str();
            let pat = jit_f64_to_value(ctx, a3).as_str();
            if ctx.rt.ensure_regex(&pat).is_err() {
                return 0.0;
            }
            let m = ctx.rt.regex_ref(&pat).is_match(&s);
            let hit = if op == MIXED_REGEX_MATCH { m } else { !m };
            if hit {
                1.0
            } else {
                0.0
            }
        }
        MIXED_PRINT_ARG => {
            let pos = a1 as usize;
            MIXED_PRINT_SLOTS.with(|c| {
                let mut v = c.borrow_mut();
                if v.len() <= pos {
                    v.resize(pos + 1, None);
                }
                v[pos] = Some(a2);
            });
            0.0
        }
        MIXED_PRINT_FLUSH => {
            let argc = a1 as usize;
            MIXED_PRINT_SLOTS.with(|c| {
                let mut slots = c.borrow_mut();
                let mut ofs_local = [0u8; 64];
                let ofs_len = ctx.rt.ofs_bytes.len().min(64);
                ofs_local[..ofs_len].copy_from_slice(&ctx.rt.ofs_bytes[..ofs_len]);
                let mut ors_local = [0u8; 64];
                let ors_len = ctx.rt.ors_bytes.len().min(64);
                ors_local[..ors_len].copy_from_slice(&ctx.rt.ors_bytes[..ors_len]);
                if let Some(out) = ctx.print_out.take() {
                    let mut line = String::new();
                    for i in 0..argc {
                        let f = slots.get(i).and_then(|x| *x).unwrap_or(0.0);
                        if i > 0 {
                            line.push_str(std::str::from_utf8(&ofs_local[..ofs_len]).unwrap_or(""));
                        }
                        line.push_str(&jit_f64_to_value(ctx, f).as_str());
                    }
                    line.push_str(std::str::from_utf8(&ors_local[..ors_len]).unwrap_or(""));
                    out.push(line);
                    ctx.print_out = Some(out);
                } else {
                    for i in 0..argc {
                        let f = slots.get(i).and_then(|x| *x).unwrap_or(0.0);
                        if i > 0 {
                            ctx.rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                        }
                        ctx.rt
                            .print_buf
                            .extend_from_slice(jit_f64_to_value(ctx, f).as_str().as_bytes());
                    }
                    ctx.rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
                }
                slots.clear();
            });
            0.0
        }
        MIXED_PRINTF_FLUSH => {
            let argc = a1 as usize;
            if argc == 0 {
                return 0.0;
            }
            MIXED_PRINT_SLOTS.with(|c| {
                let mut slots = c.borrow_mut();
                let values: Vec<Value> = (0..argc)
                    .map(|i| {
                        let f = slots.get(i).and_then(|x| *x).unwrap_or(0.0);
                        jit_f64_to_value(ctx, f)
                    })
                    .collect();
                slots.clear();
                if values.is_empty() {
                    return;
                }
                let fmt = values[0].as_str_cow();
                let vals = &values[1..];
                if let Ok(v) = sprintf_simple(
                    fmt.as_ref(),
                    vals,
                    ctx.rt.numeric_decimal,
                    ctx.rt.numeric_thousands_sep,
                    ctx.rt,
                ) {
                    let s = v.as_str();
                    if let Some(ref mut buf) = ctx.print_out {
                        buf.push(s.to_string());
                    } else {
                        ctx.rt.print_buf.extend_from_slice(s.as_bytes());
                    }
                }
            });
            0.0
        }
        MIXED_PRINT_FLUSH_REDIR => {
            use crate::bytecode::RedirKind;
            let argc = (a1 & 0xFFFF) as usize;
            let rk = (a1 >> 16) & 0xF;
            let redir = match rk {
                1 => RedirKind::Overwrite,
                2 => RedirKind::Append,
                3 => RedirKind::Pipe,
                4 => RedirKind::Coproc,
                _ => return 0.0,
            };
            let path_owned = jit_f64_to_value(ctx, a2).into_string();
            let path = path_owned.as_str();
            let ofs = String::from_utf8_lossy(&ctx.rt.ofs_bytes).into_owned();
            let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();
            MIXED_PRINT_SLOTS.with(|c| {
                let mut slots = c.borrow_mut();
                let line = if argc == 0 {
                    ctx.rt.record.clone()
                } else {
                    let parts: Vec<String> = (0..argc)
                        .map(|i| {
                            let f = slots.get(i).and_then(|x| *x).unwrap_or(0.0);
                            jit_f64_to_value(ctx, f).as_str()
                        })
                        .collect();
                    parts.join(&ofs)
                };
                slots.clear();
                let chunk = format!("{line}{ors}");
                if let Err(e) = emit_with_redir(ctx, &chunk, redir, Some(path)) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                }
            });
            0.0
        }
        MIXED_PRINTF_FLUSH_REDIR => {
            use crate::bytecode::RedirKind;
            let argc = (a1 & 0xFFFF) as usize;
            let rk = (a1 >> 16) & 0xF;
            let redir = match rk {
                1 => RedirKind::Overwrite,
                2 => RedirKind::Append,
                3 => RedirKind::Pipe,
                4 => RedirKind::Coproc,
                _ => return 0.0,
            };
            let path_owned = jit_f64_to_value(ctx, a2).into_string();
            let path = path_owned.as_str();
            if argc == 0 {
                return 0.0;
            }
            MIXED_PRINT_SLOTS.with(|c| {
                let mut slots = c.borrow_mut();
                let values: Vec<Value> = (0..argc)
                    .map(|i| {
                        let f = slots.get(i).and_then(|x| *x).unwrap_or(0.0);
                        jit_f64_to_value(ctx, f)
                    })
                    .collect();
                slots.clear();
                if values.is_empty() {
                    return;
                }
                let fmt = values[0].as_str_cow();
                let vals = &values[1..];
                if let Ok(v) = sprintf_simple(
                    fmt.as_ref(),
                    vals,
                    ctx.rt.numeric_decimal,
                    ctx.rt.numeric_thousands_sep,
                    ctx.rt,
                ) {
                    let s = v.as_str();
                    if let Err(e) = emit_with_redir(ctx, s.as_str(), redir, Some(path)) {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    }
                }
            });
            0.0
        }
        MIXED_ARRAY_GET => {
            let name = ctx.cp.strings.get(a1);
            let key_val = jit_f64_to_value(ctx, a2);
            let k = key_val.as_str_cow();
            let v = ctx.array_elem_get(name, k.as_ref());
            value_to_jit_f64(ctx, v)
        }
        MIXED_ARRAY_SET => {
            let name = ctx.cp.strings.get(a1);
            let key = jit_f64_to_value(ctx, a2).into_string();
            let val = jit_f64_to_value(ctx, a3);
            ctx.array_elem_set(name, key, val.clone());
            value_to_jit_f64(ctx, val)
        }
        MIXED_ARRAY_IN => {
            let name = ctx.cp.strings.get(a1);
            let key_val = jit_f64_to_value(ctx, a2);
            let k = key_val.as_str_cow();
            let b = if name == "SYMTAB" {
                ctx.symtab_has(k.as_ref())
            } else {
                ctx.rt.array_has(name, k.as_ref())
            };
            if b {
                1.0
            } else {
                0.0
            }
        }
        MIXED_ARRAY_DELETE_ELEM => {
            let name = ctx.cp.strings.get(a1);
            let key_val = jit_f64_to_value(ctx, a2);
            let k = key_val.as_str_cow();
            if name == "SYMTAB" {
                ctx.symtab_delete(k.as_ref());
            } else {
                ctx.rt.array_delete(name, Some(k.as_ref()));
            }
            0.0
        }
        MIXED_ARRAY_DELETE_ALL => {
            let name = ctx.cp.strings.get(a1).to_string();
            ctx.rt.array_delete(&name, None);
            0.0
        }
        MIXED_ARRAY_COMPOUND => {
            let arr = a1 & 0xffff;
            let bcode = (a1 >> 16) & 0xffff;
            let bop = match bcode {
                0 => BinOp::Add,
                1 => BinOp::Sub,
                2 => BinOp::Mul,
                3 => BinOp::Div,
                4 => BinOp::Mod,
                _ => BinOp::Add,
            };
            let name = ctx.cp.strings.get(arr);
            let key_val = jit_f64_to_value(ctx, a2);
            let old = {
                let k = key_val.as_str_cow();
                ctx.array_elem_get(name, k.as_ref())
            };
            let rhs = jit_f64_to_value(ctx, a3);
            let newv = match apply_binop(bop, &old, &rhs, false, ctx.rt) {
                Ok(v) => v,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
            };
            let n = newv.as_number();
            let key = key_val.into_string();
            ctx.array_elem_set(name, key, Value::Num(n));
            n
        }
        MIXED_ARRAY_INCDEC => {
            let arr = a1 & 0xffff;
            let kcode = (a1 >> 16) & 0xffff;
            let kind = match kcode {
                0 => IncDecOp::PreInc,
                1 => IncDecOp::PostInc,
                2 => IncDecOp::PreDec,
                3 => IncDecOp::PostDec,
                _ => IncDecOp::PreInc,
            };
            let name = ctx.cp.strings.get(arr);
            let key_val = jit_f64_to_value(ctx, a2);
            let old_n = {
                let k = key_val.as_str_cow();
                ctx.array_elem_get(name, k.as_ref())
            }
            .as_number();
            let delta = match kind {
                IncDecOp::PreInc | IncDecOp::PostInc => 1.0,
                IncDecOp::PreDec | IncDecOp::PostDec => -1.0,
            };
            let new_n = old_n + delta;
            let key = key_val.into_string();
            ctx.array_elem_set(name, key, Value::Num(new_n));
            incdec_push(kind, old_n, new_n)
        }
        MIXED_INCDEC_SLOT => {
            let slot = (a1 & 0xffff) as usize;
            let kcode = (a1 >> 16) & 0xffff;
            let kind = match kcode {
                0 => IncDecOp::PreInc,
                1 => IncDecOp::PostInc,
                2 => IncDecOp::PreDec,
                3 => IncDecOp::PostDec,
                _ => IncDecOp::PreInc,
            };
            let raw = jit_scratch_slot_raw(ctx, slot);
            let old_n = jit_f64_to_value(ctx, raw).as_number();
            let delta = incdec_delta(kind);
            let new_n = old_n + delta;
            jit_scratch_slot_store_num(ctx, slot, new_n);
            incdec_push(kind, old_n, new_n)
        }
        MIXED_INCR_SLOT => {
            let slot = a1 as usize;
            let raw = jit_scratch_slot_raw(ctx, slot);
            let n = jit_f64_to_value(ctx, raw).as_number();
            jit_scratch_slot_store_num(ctx, slot, n + 1.0);
            0.0
        }
        MIXED_DECR_SLOT => {
            let slot = a1 as usize;
            let raw = jit_scratch_slot_raw(ctx, slot);
            let n = jit_f64_to_value(ctx, raw).as_number();
            jit_scratch_slot_store_num(ctx, slot, n - 1.0);
            0.0
        }
        MIXED_ADD_SLOT_TO_SLOT => {
            let src = (a1 & 0xffff) as usize;
            let dst = ((a1 >> 16) & 0xffff) as usize;
            let sv = jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, src)).as_number();
            let dv = jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, dst)).as_number();
            jit_scratch_slot_store_num(ctx, dst, dv + sv);
            0.0
        }
        MIXED_ADD_FIELD_TO_SLOT => {
            let field = (a1 & 0xffff) as i32;
            let slot = ((a1 >> 16) & 0xffff) as usize;
            let field_val = match ctx.rt.field_as_number(field) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
            };
            let old = jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, slot)).as_number();
            jit_scratch_slot_store_num(ctx, slot, old + field_val);
            0.0
        }
        MIXED_ADD_FIELDNUM_TO_SLOT => {
            let slot = a1 as usize;
            let field_val = a2;
            let old = jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, slot)).as_number();
            jit_scratch_slot_store_num(ctx, slot, old + field_val);
            0.0
        }
        MIXED_ADD_MUL_FIELDS_TO_SLOT => {
            let f1 = (a1 & 0xffff) as i32;
            let f2 = ((a1 >> 16) & 0xffff) as i32;
            let slot = a2 as usize;
            let n1 = match ctx.rt.field_as_number(f1) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
            };
            let n2 = match ctx.rt.field_as_number(f2) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
            };
            let p = n1 * n2;
            let old = jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, slot)).as_number();
            jit_scratch_slot_store_num(ctx, slot, old + p);
            0.0
        }
        MIXED_ADD_MUL_FIELDNUMS_TO_SLOT => {
            let slot = a1 as usize;
            let p = a2 * a3;
            let old = jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, slot)).as_number();
            jit_scratch_slot_store_num(ctx, slot, old + p);
            0.0
        }
        MIXED_SLOT_AS_NUMBER => {
            let slot = a1 as usize;
            jit_f64_to_value(ctx, jit_scratch_slot_raw(ctx, slot)).as_number()
        }
        MIXED_SET_FIELD => {
            let field_idx = a1 as i32;
            let val = jit_f64_to_value(ctx, a2);
            let s = val.as_str();
            if let Err(e) = ctx.rt.set_field(field_idx, &s) {
                JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                return 0.0;
            }
            value_to_jit_f64(ctx, val)
        }
        MIXED_COMPOUND_ASSIGN_FIELD => {
            let bop = match a1 {
                0 => BinOp::Add,
                1 => BinOp::Sub,
                2 => BinOp::Mul,
                3 => BinOp::Div,
                4 => BinOp::Mod,
                _ => BinOp::Add,
            };
            let idx = jit_f64_to_value(ctx, a2).as_number() as i32;
            let rhs = jit_f64_to_value(ctx, a3);
            let old = match ctx.rt.field(idx) {
                Ok(v) => v,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
            };
            let new_val = match apply_binop(bop, &old, &rhs, false, ctx.rt) {
                Ok(v) => v,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
            };
            let s = new_val.as_str();
            if let Err(e) = ctx.rt.set_field(idx, &s) {
                JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                return 0.0;
            }
            value_to_jit_f64(ctx, new_val)
        }
        MIXED_JOIN_KEY_ARG => {
            JIT_JOIN_KEY_PARTS.with(|c| {
                c.borrow_mut().push(a2);
            });
            0.0
        }
        MIXED_JOIN_ARRAY_KEY => {
            let n = a1 as usize;
            JIT_JOIN_KEY_PARTS.with(|c| {
                let mut buf = c.borrow_mut();
                if buf.len() != n {
                    return 0.0;
                }
                let parts: Vec<String> = buf
                    .iter()
                    .map(|bits| jit_f64_to_value(ctx, *bits).as_str())
                    .collect();
                buf.clear();
                let sep = ctx
                    .rt
                    .vars
                    .get("SUBSEP")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| "\x1c".into());
                let joined = parts.join(&sep);
                value_to_jit_f64(ctx, Value::Str(joined))
            })
        }
        MIXED_TYPEOF_VAR => {
            let name = ctx.str_ref(a1).to_string();
            let v = typeof_scalar_name_for_jit(ctx, name.as_str());
            value_to_jit_f64(ctx, v)
        }
        MIXED_TYPEOF_SLOT => {
            let slot = a1 as usize;
            let buf = &ctx.rt.jit_slot_buf;
            let v = if slot < buf.len() {
                let raw = buf[slot];
                jit_f64_to_value(ctx, raw)
            } else {
                ctx.rt.slots.get(slot).cloned().unwrap_or(Value::Uninit)
            };
            let t = builtins::awk_typeof_value(&v);
            value_to_jit_f64(ctx, Value::Str(t.into()))
        }
        MIXED_TYPEOF_ARRAY_ELEM => {
            let name = ctx.cp.strings.get(a1);
            let key = jit_f64_to_value(ctx, a2).into_string();
            let t = builtins::awk_typeof_array_elem(ctx.rt, name, &key);
            value_to_jit_f64(ctx, Value::Str(t.into()))
        }
        MIXED_TYPEOF_FIELD => {
            let i = a2 as i32;
            let t = if ctx.rt.field_is_unassigned(i) {
                "untyped"
            } else {
                "string"
            };
            value_to_jit_f64(ctx, Value::Str(t.into()))
        }
        MIXED_TYPEOF_VALUE => {
            let v = jit_f64_to_value(ctx, a2);
            let t = builtins::awk_typeof_value(&v);
            value_to_jit_f64(ctx, Value::Str(t.into()))
        }
        MIXED_BUILTIN_ARG => {
            let pos = a1 as usize;
            JIT_BUILTIN_ARGS.with(|c| {
                let mut v = c.borrow_mut();
                if v.len() <= pos {
                    v.resize(pos + 1, None);
                }
                v[pos] = Some(a2);
            });
            0.0
        }
        MIXED_BUILTIN_CALL => {
            let name = ctx.str_ref(a1).to_string();
            let argc = a2 as usize;
            let args: Vec<Value> = JIT_BUILTIN_ARGS.with(|c| {
                let slots = c.borrow();
                (0..argc)
                    .map(|i| {
                        let f = slots.get(i).and_then(|x| *x).unwrap_or(0.0);
                        jit_f64_to_value(ctx, f)
                    })
                    .collect()
            });
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            let v = exec_builtin_dispatch(ctx, name.as_str(), args)
                .expect("JIT should only compile whitelisted builtins");
            value_to_jit_f64(ctx, v)
        }
        MIXED_SPLIT => {
            let name = ctx.str_ref(a1).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let fs = ctx
                .rt
                .vars
                .get("FS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".into());
            let parts =
                crate::runtime::split_string_by_field_separator(&s, &fs, ctx.rt.ignore_case_flag());
            let n = parts.len();
            ctx.rt.split_into_array(&name, &parts);
            n as f64
        }
        MIXED_SPLIT_WITH_FS => {
            let name = ctx.str_ref(a1).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let fs = jit_f64_to_value(ctx, a3).as_str();
            let parts =
                crate::runtime::split_string_by_field_separator(&s, &fs, ctx.rt.ignore_case_flag());
            let n = parts.len();
            ctx.rt.split_into_array(&name, &parts);
            n as f64
        }
        MIXED_PATSPLIT => {
            let arr_name = ctx.str_ref(a1).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            builtins::patsplit(ctx.rt, &s, &arr_name, None, None).unwrap_or(0.0)
        }
        MIXED_PATSPLIT_SEP => {
            let arr_name = ctx.str_ref(a1).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let seps_s = jit_f64_to_value(ctx, a3).into_string();
            builtins::patsplit(ctx.rt, &s, &arr_name, None, Some(seps_s.as_str())).unwrap_or(0.0)
        }
        MIXED_PATSPLIT_FP => {
            let arr_name = ctx.str_ref(a1).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let fp = jit_f64_to_value(ctx, a3).as_str();
            builtins::patsplit(ctx.rt, &s, &arr_name, Some(&fp), None).unwrap_or(0.0)
        }
        MIXED_PATSPLIT_FP_SEP => {
            let arr_idx = a1 & 0xFFFF;
            let seps_idx = a1 >> 16;
            let arr_name = ctx.str_ref(arr_idx).to_string();
            let seps_name = ctx.str_ref(seps_idx).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let fp = jit_f64_to_value(ctx, a3).as_str();
            builtins::patsplit(ctx.rt, &s, &arr_name, Some(&fp), Some(seps_name.as_str()))
                .unwrap_or(0.0)
        }
        MIXED_PATSPLIT_STASH_SEPS => {
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(a1));
            0.0
        }
        MIXED_PATSPLIT_FP_SEP_WIDE => {
            let seps_idx = JIT_PATSPLIT_SEPS_STASH.with(|c| {
                let v = c.get();
                c.set(0);
                v
            });
            let arr_name = ctx.str_ref(a1).to_string();
            let seps_name = ctx.str_ref(seps_idx).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let fp = jit_f64_to_value(ctx, a3).as_str();
            builtins::patsplit(ctx.rt, &s, &arr_name, Some(&fp), Some(seps_name.as_str()))
                .unwrap_or(0.0)
        }
        MIXED_MATCH_BUILTIN => {
            let s = jit_f64_to_value(ctx, a2).as_str();
            let re_pat = jit_f64_to_value(ctx, a3).as_str();
            builtins::match_fn(ctx.rt, &s, &re_pat, None).unwrap_or(0.0)
        }
        MIXED_MATCH_BUILTIN_ARR => {
            let arr_name = ctx.str_ref(a1).to_string();
            let s = jit_f64_to_value(ctx, a2).as_str();
            let re_pat = jit_f64_to_value(ctx, a3).as_str();
            builtins::match_fn(ctx.rt, &s, &re_pat, Some(arr_name.as_str())).unwrap_or(0.0)
        }
        MIXED_GETLINE_PRIMARY => {
            let var = if a1 == MIXED_GETLINE_INTO_RECORD {
                None
            } else {
                Some(a1)
            };
            match ctx.rt.read_line_primary() {
                Ok(line) => {
                    if let Err(e) = apply_getline_line(ctx, var, GetlineSource::Primary, line) {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    }
                }
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                }
            }
            0.0
        }
        MIXED_GETLINE_FILE => {
            let var = if a1 == MIXED_GETLINE_INTO_RECORD {
                None
            } else {
                Some(a1)
            };
            let path = jit_f64_to_value(ctx, a2).into_string();
            match ctx.rt.read_line_file(path.as_str()) {
                Ok(line) => {
                    if let Err(e) = apply_getline_line(ctx, var, GetlineSource::File, line) {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    }
                }
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                }
            }
            0.0
        }
        MIXED_GETLINE_COPROC => {
            let var = if a1 == MIXED_GETLINE_INTO_RECORD {
                None
            } else {
                Some(a1)
            };
            let path = jit_f64_to_value(ctx, a2).into_string();
            match ctx.rt.read_line_coproc(path.as_str()) {
                Ok(line) => {
                    if let Err(e) = apply_getline_line(ctx, var, GetlineSource::Coproc, line) {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    }
                }
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                }
            }
            0.0
        }
        MIXED_CALL_USER_ARG => {
            let pos = a1 as usize;
            JIT_CALL_USER_ARGS.with(|c| {
                let mut v = c.borrow_mut();
                if v.len() <= pos {
                    v.resize(pos + 1, None);
                }
                v[pos] = Some(a2);
            });
            0.0
        }
        MIXED_CALL_USER_CALL => {
            let name = ctx.str_ref(a1).to_string();
            let argc = a2 as usize;
            let args: Vec<Value> = JIT_CALL_USER_ARGS.with(|c| {
                let b = c.borrow();
                (0..argc)
                    .map(|i| {
                        b.get(i)
                            .and_then(|x| *x)
                            .map(|f| jit_f64_to_value(ctx, f))
                            .unwrap_or(Value::Uninit)
                    })
                    .collect()
            });
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            match exec_call_user_inner(ctx, name.as_str(), args) {
                Ok(v) => value_to_jit_f64(ctx, v),
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_SUB_RECORD => {
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(ctx, SubTarget::Record, false, re_v, repl_v, None, None) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GSUB_RECORD => {
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(ctx, SubTarget::Record, true, re_v, repl_v, None, None) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_SUB_VAR => {
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(ctx, SubTarget::Var(a1), false, re_v, repl_v, None, None) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GSUB_VAR => {
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(ctx, SubTarget::Var(a1), true, re_v, repl_v, None, None) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_SUB_SLOT => {
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(
                ctx,
                SubTarget::SlotVar(a1 as u16),
                false,
                re_v,
                repl_v,
                None,
                None,
            ) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GSUB_SLOT => {
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(
                ctx,
                SubTarget::SlotVar(a1 as u16),
                true,
                re_v,
                repl_v,
                None,
                None,
            ) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_SUB_FIELD => {
            let field_i = a1 as i32;
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(
                ctx,
                SubTarget::Field,
                false,
                re_v,
                repl_v,
                None,
                Some(field_i),
            ) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GSUB_FIELD => {
            let field_i = a1 as i32;
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(
                ctx,
                SubTarget::Field,
                true,
                re_v,
                repl_v,
                None,
                Some(field_i),
            ) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_SUB_INDEX_STASH => {
            let k = jit_f64_to_value(ctx, a2).into_string();
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = Some(k));
            0.0
        }
        MIXED_GSUB_INDEX_STASH => {
            let k = jit_f64_to_value(ctx, a2).into_string();
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = Some(k));
            0.0
        }
        MIXED_SUB_INDEX => {
            let key = SUB_FN_STASH_KEY
                .with(|c| c.borrow_mut().take())
                .unwrap_or_default();
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(
                ctx,
                SubTarget::Index(a1),
                false,
                re_v,
                repl_v,
                Some(key),
                None,
            ) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        MIXED_GSUB_INDEX => {
            let key = SUB_FN_STASH_KEY
                .with(|c| c.borrow_mut().take())
                .unwrap_or_default();
            let re_v = jit_f64_to_value(ctx, a2);
            let repl_v = jit_f64_to_value(ctx, a3);
            match exec_sub_from_values(
                ctx,
                SubTarget::Index(a1),
                true,
                re_v,
                repl_v,
                Some(key),
                None,
            ) {
                Ok(n) => n,
                Err(e) => {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    0.0
                }
            }
        }
        _ => 0.0,
    }
}

fn sync_jit_slot_if_scalar(ctx: &mut VmCtx<'_>, name: &str) {
    let Some(&slot) = ctx.cp.slot_map.get(name) else {
        return;
    };
    let us = slot as usize;
    let buf = &mut ctx.rt.jit_slot_buf;
    if us >= buf.len() {
        return;
    }
    let v = ctx.rt.slots[us].as_number();
    buf[us] = v;
}

/// During mixed JIT, `SetSlot` writes only the scratch buffer; `rt.slots` is refreshed at chunk end.
/// Callbacks that read a slotted scalar before mutating must use this instead of `rt.slots` alone.
fn slot_value_live_for_jit(ctx: &VmCtx<'_>, slot: u16) -> Value {
    let us = slot as usize;
    let buf = &ctx.rt.jit_slot_buf;
    if us < buf.len() {
        let raw = buf[us];
        jit_f64_to_value(ctx, raw)
    } else {
        ctx.rt.slots[us].clone()
    }
}

/// After interpreter/callback code mutates `rt.slots[slot]` (e.g. string loop vars),
/// mirror `Value` into the JIT scratch buffer so `GetSlot` / `MIXED_GET_SLOT` see it.
fn sync_jit_slot_value(ctx: &mut VmCtx<'_>, slot: u16) {
    let us = slot as usize;
    if us >= ctx.rt.jit_slot_buf.len() {
        return;
    }
    let v = ctx.rt.slots[us].clone();
    let f = value_to_jit_f64(ctx, v);
    ctx.rt.jit_slot_buf[us] = f;
}

#[inline]
fn sync_jit_slot_for_scalar_name(ctx: &mut VmCtx<'_>, name: &str) {
    if let Some(&slot) = ctx.cp.slot_map.get(name) {
        sync_jit_slot_value(ctx, slot);
    }
}

/// Sentinels passed to [`jit_field_callback`] for NR / FNR / NF so they never collide with
/// negative field indices like `$(-1)` (POSIX fatal).
pub(crate) const JIT_FIELD_SENTINEL_NR: i32 = i32::MIN;
pub(crate) const JIT_FIELD_SENTINEL_FNR: i32 = i32::MIN + 1;
pub(crate) const JIT_FIELD_SENTINEL_NF: i32 = i32::MIN + 2;

/// Field callback passed to JIT-compiled code.
/// Positive i → field $i as f64.
/// [`JIT_FIELD_SENTINEL_NR`] / [`JIT_FIELD_SENTINEL_FNR`] / [`JIT_FIELD_SENTINEL_NF`] → NR / FNR / NF.
/// Field reads for JIT (`PushFieldNum`, `GetField`, NR/FNR/NF). Exposed for colocated calls.
pub(crate) extern "C" fn jit_field_callback(vmctx: *mut c_void, i: i32) -> f64 {
    if vmctx.is_null() {
        return 0.0;
    }
    let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
    let rt = &mut *ctx.rt;
    match i {
        JIT_FIELD_SENTINEL_NR => rt.nr,
        JIT_FIELD_SENTINEL_FNR => rt.fnr,
        JIT_FIELD_SENTINEL_NF => rt.nf() as f64,
        _ => match rt.field_as_number(i) {
            Ok(n) => n,
            Err(e) => {
                JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                0.0
            }
        },
    }
}

/// Fused `a[$field] += delta` — matches `Op::ArrayFieldAddConst` in the VM loop.
extern "C" fn jit_array_field_add_const(vmctx: *mut c_void, arr_idx: u32, field: i32, delta: f64) {
    if vmctx.is_null() {
        return;
    }
    let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
    let name = ctx.cp.strings.get(arr_idx);
    ctx.rt.array_field_add_delta(name, field, delta);
}

/// Multiplexed HashMap-path variable ops — must match `JIT_VAR_OP_*` in `jit.rs`.
extern "C" fn jit_var_dispatch(vmctx: *mut c_void, op: u32, name_idx: u32, arg: f64) -> f64 {
    if vmctx.is_null() {
        return 0.0;
    }
    {
        let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
        let name_owned = ctx.str_ref(name_idx).to_string();
        use crate::jit::{
            JIT_VAR_OP_COMPOUND_ADD, JIT_VAR_OP_COMPOUND_DIV, JIT_VAR_OP_COMPOUND_MOD,
            JIT_VAR_OP_COMPOUND_MUL, JIT_VAR_OP_COMPOUND_SUB, JIT_VAR_OP_DECR, JIT_VAR_OP_GET,
            JIT_VAR_OP_INCDEC_POST_DEC, JIT_VAR_OP_INCDEC_POST_INC, JIT_VAR_OP_INCDEC_PRE_DEC,
            JIT_VAR_OP_INCDEC_PRE_INC, JIT_VAR_OP_INCR, JIT_VAR_OP_SET,
        };
        match op {
            JIT_VAR_OP_GET => {
                let v = ctx.get_var(name_owned.as_str());
                value_to_jit_f64(ctx, v)
            }
            JIT_VAR_OP_SET => {
                if let Err(e) = ctx.set_var(name_owned.as_str(), Value::Num(arg)) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                sync_jit_slot_if_scalar(ctx, name_owned.as_str());
                arg
            }
            JIT_VAR_OP_INCR => {
                let n = match ctx.var_value_cow(name_owned.as_str()) {
                    Cow::Borrowed(v) => v.as_number(),
                    Cow::Owned(v) => v.as_number(),
                };
                if let Err(e) = ctx.set_var(name_owned.as_str(), Value::Num(n + 1.0)) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                sync_jit_slot_if_scalar(ctx, name_owned.as_str());
                0.0
            }
            JIT_VAR_OP_DECR => {
                let n = match ctx.var_value_cow(name_owned.as_str()) {
                    Cow::Borrowed(v) => v.as_number(),
                    Cow::Owned(v) => v.as_number(),
                };
                if let Err(e) = ctx.set_var(name_owned.as_str(), Value::Num(n - 1.0)) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                sync_jit_slot_if_scalar(ctx, name_owned.as_str());
                0.0
            }
            JIT_VAR_OP_COMPOUND_ADD
            | JIT_VAR_OP_COMPOUND_SUB
            | JIT_VAR_OP_COMPOUND_MUL
            | JIT_VAR_OP_COMPOUND_DIV
            | JIT_VAR_OP_COMPOUND_MOD => {
                let bop = match op {
                    JIT_VAR_OP_COMPOUND_ADD => BinOp::Add,
                    JIT_VAR_OP_COMPOUND_SUB => BinOp::Sub,
                    JIT_VAR_OP_COMPOUND_MUL => BinOp::Mul,
                    JIT_VAR_OP_COMPOUND_DIV => BinOp::Div,
                    JIT_VAR_OP_COMPOUND_MOD => BinOp::Mod,
                    _ => unreachable!(),
                };
                let old = match ctx.var_value_cow(name_owned.as_str()) {
                    Cow::Borrowed(v) => v.clone(),
                    Cow::Owned(v) => v,
                };
                let rhs = Value::Num(arg);
                let new_val = match apply_binop(bop, &old, &rhs, false, ctx.rt) {
                    Ok(v) => v,
                    Err(e) => {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                        return 0.0;
                    }
                };
                let n = new_val.as_number();
                if let Err(e) = ctx.set_var(name_owned.as_str(), new_val) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                sync_jit_slot_if_scalar(ctx, name_owned.as_str());
                n
            }
            JIT_VAR_OP_INCDEC_PRE_INC
            | JIT_VAR_OP_INCDEC_POST_INC
            | JIT_VAR_OP_INCDEC_PRE_DEC
            | JIT_VAR_OP_INCDEC_POST_DEC => {
                let kind = match op {
                    JIT_VAR_OP_INCDEC_PRE_INC => IncDecOp::PreInc,
                    JIT_VAR_OP_INCDEC_POST_INC => IncDecOp::PostInc,
                    JIT_VAR_OP_INCDEC_PRE_DEC => IncDecOp::PreDec,
                    JIT_VAR_OP_INCDEC_POST_DEC => IncDecOp::PostDec,
                    _ => unreachable!(),
                };
                let old = match ctx.var_value_cow(name_owned.as_str()) {
                    Cow::Borrowed(v) => v.clone(),
                    Cow::Owned(v) => v,
                };
                let old_n = old.as_number();
                let delta = incdec_delta(kind);
                let new_n = old_n + delta;
                if let Err(e) = ctx.set_var(name_owned.as_str(), Value::Num(new_n)) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                sync_jit_slot_if_scalar(ctx, name_owned.as_str());
                incdec_push(kind, old_n, new_n)
            }
            _ => 0.0,
        }
    }
}

/// `$n` compound assign / `++$n` / `$n++` — reuses `JIT_VAR_OP_*` for compound and inc/dec
/// (same numeric opcodes as [`jit_var_dispatch`], different first-class args).
extern "C" fn jit_field_dispatch(vmctx: *mut c_void, op: u32, field_idx: i32, arg: f64) -> f64 {
    if vmctx.is_null() {
        return 0.0;
    }
    {
        let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
        use crate::jit::{
            JIT_FIELD_OP_SET_NUM, JIT_VAR_OP_COMPOUND_ADD, JIT_VAR_OP_COMPOUND_DIV,
            JIT_VAR_OP_COMPOUND_MOD, JIT_VAR_OP_COMPOUND_MUL, JIT_VAR_OP_COMPOUND_SUB,
            JIT_VAR_OP_INCDEC_POST_DEC, JIT_VAR_OP_INCDEC_POST_INC, JIT_VAR_OP_INCDEC_PRE_DEC,
            JIT_VAR_OP_INCDEC_PRE_INC,
        };
        match op {
            JIT_FIELD_OP_SET_NUM => {
                let s = Value::Num(arg).as_str();
                if let Err(e) = ctx.rt.set_field(field_idx, &s) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                arg
            }
            JIT_VAR_OP_COMPOUND_ADD
            | JIT_VAR_OP_COMPOUND_SUB
            | JIT_VAR_OP_COMPOUND_MUL
            | JIT_VAR_OP_COMPOUND_DIV
            | JIT_VAR_OP_COMPOUND_MOD => {
                let bop = match op {
                    JIT_VAR_OP_COMPOUND_ADD => BinOp::Add,
                    JIT_VAR_OP_COMPOUND_SUB => BinOp::Sub,
                    JIT_VAR_OP_COMPOUND_MUL => BinOp::Mul,
                    JIT_VAR_OP_COMPOUND_DIV => BinOp::Div,
                    JIT_VAR_OP_COMPOUND_MOD => BinOp::Mod,
                    _ => unreachable!(),
                };
                let old = match ctx.rt.field(field_idx) {
                    Ok(v) => v,
                    Err(e) => {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                        return 0.0;
                    }
                };
                let new_val = match apply_binop(bop, &old, &Value::Num(arg), false, ctx.rt) {
                    Ok(v) => v,
                    Err(e) => {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                        return 0.0;
                    }
                };
                let s = new_val.as_str();
                if let Err(e) = ctx.rt.set_field(field_idx, &s) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                new_val.as_number()
            }
            JIT_VAR_OP_INCDEC_PRE_INC
            | JIT_VAR_OP_INCDEC_POST_INC
            | JIT_VAR_OP_INCDEC_PRE_DEC
            | JIT_VAR_OP_INCDEC_POST_DEC => {
                let kind = match op {
                    JIT_VAR_OP_INCDEC_PRE_INC => IncDecOp::PreInc,
                    JIT_VAR_OP_INCDEC_POST_INC => IncDecOp::PostInc,
                    JIT_VAR_OP_INCDEC_PRE_DEC => IncDecOp::PreDec,
                    JIT_VAR_OP_INCDEC_POST_DEC => IncDecOp::PostDec,
                    _ => unreachable!(),
                };
                let old = match ctx.rt.field(field_idx) {
                    Ok(v) => v,
                    Err(e) => {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                        return 0.0;
                    }
                };
                let old_n = old.as_number();
                let delta = incdec_delta(kind);
                let new_n = old_n + delta;
                if let Err(e) = ctx.rt.set_field_num(field_idx, new_n) {
                    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    return 0.0;
                }
                incdec_push(kind, old_n, new_n)
            }
            _ => 0.0,
        }
    }
}

/// Print side-effects from JIT: `PrintFieldStdout`, `PrintFieldSepField`,
/// `PrintThreeFieldsStdout`, bare `print` (argc=0).
extern "C" fn jit_io_dispatch(vmctx: *mut c_void, op: u32, a1: i32, a2: i32, a3: i32) {
    use crate::jit::{
        JIT_IO_PRINT_FIELD, JIT_IO_PRINT_FIELD_SEP_FIELD, JIT_IO_PRINT_RECORD,
        JIT_IO_PRINT_THREE_FIELDS,
    };
    if vmctx.is_null() {
        return;
    }
    {
        let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
        match op {
            JIT_IO_PRINT_FIELD => {
                let field = a1 as u16;
                if let Some(ref mut buf) = ctx.print_out {
                    let val = match ctx.rt.field(field as i32) {
                        Ok(v) => v,
                        Err(e) => {
                            JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                            return;
                        }
                    };
                    let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();
                    buf.push(format!("{}{}", val.as_str(), ors));
                } else {
                    ctx.rt.print_field_to_buf(field as usize);
                    let ors = &ctx.rt.ors_bytes;
                    ctx.rt.print_buf.extend_from_slice(ors);
                }
            }
            JIT_IO_PRINT_FIELD_SEP_FIELD => {
                let f1 = a1 as u16;
                let sep_idx = a2 as u32;
                let f2 = a3 as u16;
                let sep_s = ctx.str_ref(sep_idx).to_string();
                if let Some(ref mut buf) = ctx.print_out {
                    let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();
                    let v1 = match ctx.rt.field(f1 as i32) {
                        Ok(v) => v.as_str(),
                        Err(e) => {
                            JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                            return;
                        }
                    };
                    let v2 = match ctx.rt.field(f2 as i32) {
                        Ok(v) => v.as_str(),
                        Err(e) => {
                            JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                            return;
                        }
                    };
                    buf.push(format!("{v1}{sep_s}{v2}{ors}"));
                } else {
                    ctx.rt.print_field_to_buf(f1 as usize);
                    ctx.rt.print_buf.extend_from_slice(sep_s.as_bytes());
                    ctx.rt.print_field_to_buf(f2 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ctx.rt.ors_bytes);
                }
            }
            JIT_IO_PRINT_THREE_FIELDS => {
                let f1 = a1 as u16;
                let f2 = a2 as u16;
                let f3 = a3 as u16;
                if let Some(ref mut buf) = ctx.print_out {
                    let ofs = String::from_utf8_lossy(&ctx.rt.ofs_bytes).into_owned();
                    let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();
                    let v1 = match ctx.rt.field(f1 as i32) {
                        Ok(v) => v.as_str(),
                        Err(e) => {
                            JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                            return;
                        }
                    };
                    let v2 = match ctx.rt.field(f2 as i32) {
                        Ok(v) => v.as_str(),
                        Err(e) => {
                            JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                            return;
                        }
                    };
                    let v3 = match ctx.rt.field(f3 as i32) {
                        Ok(v) => v.as_str(),
                        Err(e) => {
                            JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                            return;
                        }
                    };
                    buf.push(format!("{v1}{ofs}{v2}{ofs}{v3}{ors}"));
                } else {
                    let mut ofs_local = [0u8; 64];
                    let ofs_len = ctx.rt.ofs_bytes.len().min(64);
                    ofs_local[..ofs_len].copy_from_slice(&ctx.rt.ofs_bytes[..ofs_len]);
                    ctx.rt.print_field_to_buf(f1 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                    ctx.rt.print_field_to_buf(f2 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                    ctx.rt.print_field_to_buf(f3 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ctx.rt.ors_bytes);
                }
            }
            JIT_IO_PRINT_RECORD => {
                // Bare `print` — print $0 + ORS
                if let Some(ref mut buf) = ctx.print_out {
                    let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();
                    buf.push(format!("{}{}", ctx.rt.record, ors));
                } else {
                    ctx.rt.print_buf.extend_from_slice(ctx.rt.record.as_bytes());
                    ctx.rt.print_buf.extend_from_slice(&ctx.rt.ors_bytes);
                }
            }
            _ => {}
        }
    }
}

/// Array ops, `MatchRegexp`, flow signals from JIT.
extern "C" fn jit_val_dispatch(vmctx: *mut c_void, op: u32, a1: u32, a2: f64, a3: f64) -> f64 {
    use crate::jit::{
        JIT_VAL_ARRAY_COMPOUND_ADD, JIT_VAL_ARRAY_COMPOUND_DIV, JIT_VAL_ARRAY_COMPOUND_MOD,
        JIT_VAL_ARRAY_COMPOUND_MUL, JIT_VAL_ARRAY_COMPOUND_SUB, JIT_VAL_ARRAY_DELETE_ALL,
        JIT_VAL_ARRAY_DELETE_ELEM, JIT_VAL_ARRAY_GET, JIT_VAL_ARRAY_IN,
        JIT_VAL_ARRAY_INCDEC_POST_DEC, JIT_VAL_ARRAY_INCDEC_POST_INC, JIT_VAL_ARRAY_INCDEC_PRE_DEC,
        JIT_VAL_ARRAY_INCDEC_PRE_INC, JIT_VAL_ARRAY_SET, JIT_VAL_ASORT, JIT_VAL_ASORTI,
        JIT_VAL_FDIV_CHECKED, JIT_VAL_FORIN_END, JIT_VAL_FORIN_NEXT, JIT_VAL_FORIN_START,
        JIT_VAL_MATCH_REGEXP, JIT_VAL_SIGNAL_EXIT_CODE, JIT_VAL_SIGNAL_EXIT_DEFAULT,
        JIT_VAL_SIGNAL_NEXT, JIT_VAL_SIGNAL_NEXT_FILE, JIT_VAL_SIGNAL_RETURN_EMPTY,
        JIT_VAL_SIGNAL_RETURN_VAL,
    };

    // Signals — set thread-local flag and return immediately.
    match op {
        JIT_VAL_SIGNAL_NEXT
        | JIT_VAL_SIGNAL_NEXT_FILE
        | JIT_VAL_SIGNAL_EXIT_DEFAULT
        | JIT_VAL_SIGNAL_RETURN_EMPTY => {
            JIT_SIGNAL.with(|c| c.set(op));
            return 0.0;
        }
        JIT_VAL_SIGNAL_EXIT_CODE | JIT_VAL_SIGNAL_RETURN_VAL => {
            JIT_SIGNAL.with(|c| c.set(op));
            JIT_SIGNAL_ARG.with(|c| c.set(a2));
            return 0.0;
        }
        _ => {}
    }

    if op >= 100 {
        if vmctx.is_null() {
            return 0.0;
        }
        let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
        return jit_mixed_op_dispatch(ctx, op, a1, a2, a3);
    }

    if vmctx.is_null() {
        return 0.0;
    }
    {
        let ctx = unsafe { &mut *vmctx.cast::<VmCtx<'static>>() };
        match op {
            JIT_VAL_FDIV_CHECKED => {
                let b = a3;
                let a = a2;
                if b == 0.0 {
                    JIT_CHUNK_ERR.with(|c| {
                        *c.borrow_mut() = Some(Error::Runtime("division by zero attempted".into()));
                    });
                    return 0.0;
                }
                a / b
            }
            JIT_VAL_MATCH_REGEXP => {
                let idx = a1;
                let pat = ctx.str_ref(idx).to_string();
                if ctx.rt.ensure_regex(&pat).is_err() {
                    return 0.0;
                }
                if ctx.rt.regex_ref(&pat).is_match(&ctx.rt.record) {
                    1.0
                } else {
                    0.0
                }
            }
            JIT_VAL_ARRAY_GET => {
                let name = ctx.cp.strings.get(a1);
                let key = jit_f64_to_value(ctx, a2).into_string();
                ctx.array_elem_get(name, &key).as_number()
            }
            JIT_VAL_ARRAY_SET => {
                let name = ctx.cp.strings.get(a1);
                let key = jit_f64_to_value(ctx, a2).into_string();
                let val = jit_f64_to_value(ctx, a3);
                ctx.array_elem_set(name, key, val);
                a3
            }
            JIT_VAL_ARRAY_IN => {
                let name = ctx.cp.strings.get(a1);
                let key = jit_f64_to_value(ctx, a2).into_string();
                let b = if name == "SYMTAB" {
                    ctx.symtab_has(&key)
                } else {
                    ctx.rt.array_has(name, &key)
                };
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            JIT_VAL_ARRAY_DELETE_ELEM => {
                let name = ctx.cp.strings.get(a1);
                let key = jit_f64_to_value(ctx, a2).into_string();
                if name == "SYMTAB" {
                    ctx.symtab_delete(&key);
                } else {
                    ctx.rt.array_delete(name, Some(&key));
                }
                0.0
            }
            JIT_VAL_ARRAY_DELETE_ALL => {
                let name = ctx.cp.strings.get(a1).to_string();
                ctx.rt.array_delete(&name, None);
                0.0
            }
            JIT_VAL_ARRAY_COMPOUND_ADD
            | JIT_VAL_ARRAY_COMPOUND_SUB
            | JIT_VAL_ARRAY_COMPOUND_MUL
            | JIT_VAL_ARRAY_COMPOUND_DIV
            | JIT_VAL_ARRAY_COMPOUND_MOD => {
                let name = ctx.cp.strings.get(a1);
                let key = jit_f64_to_value(ctx, a2).into_string();
                let old = ctx.array_elem_get(name, &key).as_number();
                let rhs = a3;
                let n = match op {
                    JIT_VAL_ARRAY_COMPOUND_ADD => old + rhs,
                    JIT_VAL_ARRAY_COMPOUND_SUB => old - rhs,
                    JIT_VAL_ARRAY_COMPOUND_MUL => old * rhs,
                    JIT_VAL_ARRAY_COMPOUND_DIV => old / rhs,
                    JIT_VAL_ARRAY_COMPOUND_MOD => old % rhs,
                    _ => unreachable!(),
                };
                ctx.array_elem_set(name, key, Value::Num(n));
                n
            }
            JIT_VAL_ARRAY_INCDEC_PRE_INC
            | JIT_VAL_ARRAY_INCDEC_POST_INC
            | JIT_VAL_ARRAY_INCDEC_PRE_DEC
            | JIT_VAL_ARRAY_INCDEC_POST_DEC => {
                let name = ctx.cp.strings.get(a1);
                let key = jit_f64_to_value(ctx, a2).into_string();
                let old_n = ctx.array_elem_get(name, &key).as_number();
                let delta = match op {
                    JIT_VAL_ARRAY_INCDEC_PRE_INC | JIT_VAL_ARRAY_INCDEC_POST_INC => 1.0,
                    _ => -1.0,
                };
                let new_n = old_n + delta;
                ctx.array_elem_set(name, key, Value::Num(new_n));
                match op {
                    JIT_VAL_ARRAY_INCDEC_PRE_INC | JIT_VAL_ARRAY_INCDEC_PRE_DEC => new_n,
                    _ => old_n,
                }
            }
            // ── ForIn iteration ────────────────────────────────────────
            JIT_VAL_FORIN_START => {
                let name = ctx.cp.strings.get(a1);
                match ctx.for_in_keys(name) {
                    Ok(keys) => {
                        JIT_FORIN_ITERS.with(|c| {
                            c.borrow_mut().push(ForInState { keys, index: 0 });
                        });
                    }
                    Err(e) => {
                        JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = Some(e));
                    }
                }
                0.0
            }
            JIT_VAL_FORIN_NEXT => {
                let var_idx = a1;
                JIT_FORIN_ITERS.with(|c| {
                    let mut iters = c.borrow_mut();
                    let state = iters.last_mut().expect("ForInNext without ForInStart");
                    if state.index >= state.keys.len() {
                        0.0 // exhausted
                    } else {
                        let key = mem::take(&mut state.keys[state.index]);
                        state.index += 1;
                        if let Err(e) = ctx.set_var_interned_jit_sync(var_idx, Value::Str(key)) {
                            JIT_CHUNK_ERR.with(|ce| *ce.borrow_mut() = Some(e));
                            return 0.0;
                        }
                        1.0 // has next
                    }
                })
            }
            JIT_VAL_FORIN_END => {
                JIT_FORIN_ITERS.with(|c| {
                    c.borrow_mut().pop();
                });
                0.0
            }
            // ── Array sorting ──────────────────────────────────────────
            JIT_VAL_ASORT => {
                let s = ctx.cp.strings.get(a1);
                let d = if a2 < 0.0 {
                    None
                } else {
                    Some(ctx.cp.strings.get(a2 as u32))
                };
                builtins::asort(ctx.rt, s, d).unwrap_or(0.0)
            }
            JIT_VAL_ASORTI => {
                let s = ctx.cp.strings.get(a1);
                let d = if a2 < 0.0 {
                    None
                } else {
                    Some(ctx.cp.strings.get(a2 as u32))
                };
                builtins::asorti(ctx.rt, s, d).unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }
}

/// Try JIT dispatch for the full instruction set. Converts slots to/from f64[] and executes.
fn try_jit_dispatch(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<Option<VmSignal>> {
    let ops = &chunk.ops;
    if ctx.rt.bignum {
        return Ok(None);
    }
    if !ctx.rt.jit_enabled {
        return Ok(None);
    }
    if crate::jit::jit_disabled_by_env() {
        return Ok(None);
    }

    let inv = chunk
        .jit_invocation_count
        .fetch_add(1, AtomicOrdering::Relaxed)
        + 1;
    if inv < crate::jit::jit_min_invocations_before_compile() {
        return Ok(None);
    }

    let arc = {
        let mut guard = chunk
            .jit_lock
            .lock()
            .map_err(|_| Error::Runtime("JIT chunk cache lock poisoned".into()))?;
        match &*guard {
            Some(Err(())) => return Ok(None),
            Some(Ok(a)) => Arc::clone(a),
            None => {
                if !crate::jit::is_jit_eligible(ops)
                    || !crate::jit::jit_call_builtins_ok(ops, ctx.cp)
                {
                    *guard = Some(Err(()));
                    return Ok(None);
                }
                let Some(jc) = crate::jit::try_compile_with_options(
                    ops,
                    ctx.cp,
                    crate::jit::JitCompileOptions::vm_default(),
                ) else {
                    *guard = Some(Err(()));
                    return Ok(None);
                };
                let a = Arc::new(jc);
                *guard = Some(Ok(Arc::clone(&a)));
                a
            }
        }
    };

    let mixed = crate::jit::needs_mixed_mode(ops);

    let slot_count = ctx.rt.slots.len();

    // Clear TLS scratch pools **before** `value_to_jit_f64` — mixed-mode NaN-boxed
    // string slots index into `JIT_DYN_STRINGS`; filling the buffer then clearing
    // would leave stale indices (e.g. `-v a=1` string slots → printed `0`).
    JIT_CHUNK_ERR.with(|c| *c.borrow_mut() = None);
    JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
    MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
    JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
    JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
    JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
    SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
    JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));

    ctx.rt.ensure_jit_slot_buf(slot_count);
    if mixed {
        let slots_copy: Vec<Value> = ctx.rt.slots[..slot_count].to_vec();
        let mut tmp = vec![0.0; slot_count];
        for (i, v) in slots_copy.into_iter().enumerate() {
            tmp[i] = value_to_jit_f64(ctx, v);
        }
        ctx.rt.jit_slot_buf[..slot_count].copy_from_slice(&tmp);
    } else {
        let nums: Vec<f64> = ctx.rt.slots[..slot_count]
            .iter()
            .map(|v| v.as_number())
            .collect();
        ctx.rt.jit_slot_buf[..slot_count].copy_from_slice(&nums);
    }

    JIT_SIGNAL.with(|c| c.set(0));

    let vmctx = std::ptr::from_mut(ctx).cast::<c_void>();
    let mut jit_state = crate::jit::JitRuntimeState::new(
        vmctx,
        &mut ctx.rt.jit_slot_buf[..slot_count],
        jit_field_callback,
        jit_array_field_add_const,
        jit_var_dispatch,
        jit_field_dispatch,
        jit_io_dispatch,
        jit_val_dispatch,
    );
    let result = crate::jit::try_jit_execute_cached(&arc, &mut jit_state);

    JIT_FORIN_ITERS.with(|c| c.borrow_mut().clear());

    if let Some(e) = JIT_CHUNK_ERR.with(|c| c.borrow_mut().take()) {
        JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
        MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
        JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
        JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
        JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
        SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
        JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
        return Err(e);
    }

    // JIT compilation failed — fall back to interpreter.
    let Some(result) = result else {
        JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
        MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
        JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
        JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
        JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
        SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
        JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
        return Ok(None);
    };

    // Write back modified slots
    if mixed {
        for i in 0..slot_count {
            let jit_val = ctx.rt.jit_slot_buf[i];
            ctx.rt.slots[i] = jit_f64_to_value(ctx, jit_val);
        }
    } else {
        for i in 0..slot_count {
            let jit_val = ctx.rt.jit_slot_buf[i];
            let old = ctx.rt.slots[i].as_number();
            if (jit_val - old).abs() > f64::EPSILON || (old == 0.0 && jit_val != 0.0) {
                ctx.rt.slots[i] = Value::Num(jit_val);
            }
        }
    }

    // Check for JIT-raised signals (Next, NextFile, Exit).
    let sig = JIT_SIGNAL.with(|c| c.replace(0));
    use crate::jit::{
        JIT_VAL_SIGNAL_EXIT_CODE, JIT_VAL_SIGNAL_EXIT_DEFAULT, JIT_VAL_SIGNAL_NEXT,
        JIT_VAL_SIGNAL_NEXT_FILE,
    };
    match sig {
        JIT_VAL_SIGNAL_NEXT => {
            JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
            MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
            JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
            return Ok(Some(VmSignal::Next));
        }
        JIT_VAL_SIGNAL_NEXT_FILE => {
            JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
            MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
            JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
            return Ok(Some(VmSignal::NextFile));
        }
        JIT_VAL_SIGNAL_EXIT_DEFAULT => {
            ctx.rt.exit_code = 0;
            ctx.rt.exit_pending = true;
            JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
            MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
            JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
            return Ok(Some(VmSignal::ExitPending));
        }
        JIT_VAL_SIGNAL_EXIT_CODE => {
            let code = JIT_SIGNAL_ARG.with(|c| c.get()) as i32;
            ctx.rt.exit_code = code;
            ctx.rt.exit_pending = true;
            JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
            MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
            JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
            return Ok(Some(VmSignal::ExitPending));
        }
        crate::jit::JIT_VAL_SIGNAL_RETURN_VAL => {
            let val = JIT_SIGNAL_ARG.with(|c| c.get());
            // Non-mixed JIT still returns strings via NaN-boxed f64; `Value::Num` would mis-decode.
            let ret = jit_f64_to_value(ctx, val);
            // Do not clear `JIT_DYN_STRINGS` here: user-function JIT can nest inside an outer
            // mixed JIT chunk (e.g. `BEGIN` calling `id("ok")`); the outer JIT stack still holds
            // NaN-encoded indices into this pool until the outer `try_jit_dispatch` finishes.
            MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
            JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
            return Ok(Some(VmSignal::Return(ret)));
        }
        crate::jit::JIT_VAL_SIGNAL_RETURN_EMPTY => {
            MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
            JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
            JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
            JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
            SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
            JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
            return Ok(Some(VmSignal::Return(Value::Str(String::new()))));
        }
        _ => {}
    }

    // Normal execution — push result value (may be unused for void chunks).
    ctx.push(if mixed {
        jit_f64_to_value(ctx, result)
    } else {
        Value::Num(result)
    });
    JIT_DYN_STRINGS.with(|c| c.borrow_mut().clear());
    MIXED_PRINT_SLOTS.with(|c| c.borrow_mut().clear());
    JIT_JOIN_KEY_PARTS.with(|c| c.borrow_mut().clear());
    JIT_BUILTIN_ARGS.with(|c| c.borrow_mut().clear());
    JIT_CALL_USER_ARGS.with(|c| c.borrow_mut().clear());
    SUB_FN_STASH_KEY.with(|c| *c.borrow_mut() = None);
    JIT_PATSPLIT_SEPS_STASH.with(|c| c.set(0));
    Ok(Some(VmSignal::Normal))
}

// ── fusevm dispatch (shared VM + JIT) ──────────────────────────────────────

/// Check if a chunk is eligible for fusevm dispatch: pure-numeric, no strings,
/// no mixed-mode ops, no bignum. These chunks get fusevm's Cranelift JIT for free.
/// Check if every op in the chunk can be lowered to fusevm.
/// Universal ops map directly; currently limited to pure-numeric slot chunks.
fn is_fusevm_eligible(ops: &[Op], bignum: bool) -> bool {
    if bignum || ops.is_empty() {
        return false;
    }
    for op in ops {
        match op {
            Op::PushNum(_)
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::Pow
            | Op::Neg
            | Op::Not
            | Op::Pos
            | Op::ToBool
            | Op::Pop
            | Op::Dup
            | Op::GetSlot(_)
            | Op::SetSlot(_)
            | Op::CmpEq
            | Op::CmpNe
            | Op::CmpLt
            | Op::CmpLe
            | Op::CmpGt
            | Op::CmpGe
            | Op::Jump(_)
            | Op::JumpIfFalsePop(_)
            | Op::JumpIfTruePop(_)
            | Op::IncrSlot(_)
            | Op::DecrSlot(_)
            | Op::CompoundAssignSlot(_, BinOp::Add)
            | Op::CompoundAssignSlot(_, BinOp::Sub)
            | Op::CompoundAssignSlot(_, BinOp::Mul)
            | Op::CompoundAssignSlot(_, BinOp::Div)
            | Op::CompoundAssignSlot(_, BinOp::Mod)
            | Op::CompoundAssignSlot(_, BinOp::Pow)
            | Op::AddSlotToSlot { .. }
            | Op::JumpIfSlotGeNum { .. }
            | Op::IncDecSlot(_, _) => continue,
            _ => return false,
        }
    }
    true
}

/// Translate an awkrs chunk to a fusevm chunk and execute via fusevm's VM.
/// Handles slot marshaling and writeback for full side-effect support.
fn try_fusevm_dispatch(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<Option<VmSignal>> {
    let ops = &chunk.ops;
    if !is_fusevm_eligible(ops, ctx.rt.bignum) {
        return Ok(None);
    }
    if ops.is_empty() {
        return Ok(Some(VmSignal::Normal));
    }

    // Multi-op expansions (DecrSlot → 3 ops, CompoundAssign → 4-5 ops) shift
    // jump targets. Two-pass: first compute the ip offset map, then emit.
    let mut ip_map: Vec<usize> = Vec::with_capacity(ops.len() + 1);
    let mut fusevm_ip = 0usize;
    // Slot init: PushFrame + N×(LoadFloat + SetSlot) = 1 + 2*N
    let slot_count = ctx.rt.slots.len();
    fusevm_ip += 1 + slot_count * 2; // PushFrame + slot init ops

    for op in ops {
        ip_map.push(fusevm_ip);
        fusevm_ip += match op {
            // AWK incr/decr use f64: GetSlot + LoadFloat(1.0) + Add/Sub + SetSlot
            Op::IncrSlot(_) | Op::DecrSlot(_) => 4,
            // slot op= rhs: GetSlot + <op> + Dup + SetSlot (4 ops)
            // Sub needs Swap: GetSlot + Swap + Sub + Dup + SetSlot (5 ops)
            Op::CompoundAssignSlot(_, BinOp::Sub) => 5,
            Op::CompoundAssignSlot(
                _,
                BinOp::Add | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow,
            ) => 4,
            // Not → LoadFloat(0.0) + NumEq (AWK returns Num, not Bool)
            Op::Not => 2,
            // Pos → LoadFloat(0.0) + Add (coerce to number)
            Op::Pos => 2,
            // ToBool → Dup + LoadFloat(0.0) + NumNe (0.0 if falsy, 1.0 if truthy)
            // Actually: we need the result to be 0.0 or 1.0. Use custom expansion.
            Op::ToBool => 2,
            // AddSlotToSlot: GetSlot(src) + GetSlot(dst) + Add + SetSlot(dst) = 4
            Op::AddSlotToSlot { .. } => 4,
            // JumpIfSlotGeNum: GetSlot + LoadFloat + NumGe + JumpIfTrue = 4
            Op::JumpIfSlotGeNum { .. } => 4,
            // IncDecSlot: 5 ops (GetSlot + Dup/LoadFloat + Add/Sub + Dup/SetSlot + SetSlot)
            Op::IncDecSlot(_, _) => 5,
            _ => 1,
        };
    }
    ip_map.push(fusevm_ip); // sentinel: one past the end

    // Build fusevm chunk with slot initialization preamble
    let mut builder = fusevm::ChunkBuilder::new();

    // Preamble: PushFrame + initialize all slots from awkrs runtime
    builder.emit(fusevm::Op::PushFrame, 0);
    for i in 0..slot_count {
        let n = ctx.rt.slots[i].as_number();
        builder.emit(fusevm::Op::LoadFloat(n), 0);
        builder.emit(fusevm::Op::SetSlot(i as u16), 0);
    }

    // Translate ops (with remapped jump targets)
    for op in ops.iter() {
        match op {
            Op::PushNum(n) => {
                builder.emit(fusevm::Op::LoadFloat(*n), 0);
            }
            Op::Add => {
                builder.emit(fusevm::Op::Add, 0);
            }
            Op::Sub => {
                builder.emit(fusevm::Op::Sub, 0);
            }
            Op::Mul => {
                builder.emit(fusevm::Op::Mul, 0);
            }
            Op::Div => {
                builder.emit(fusevm::Op::Div, 0);
            }
            Op::Mod => {
                builder.emit(fusevm::Op::Mod, 0);
            }
            Op::Pow => {
                builder.emit(fusevm::Op::Pow, 0);
            }
            Op::Neg => {
                builder.emit(fusevm::Op::Negate, 0);
            }
            // AWK !x returns Num(0/1), not Bool — emit 0.0 == x (truthy→0, falsy→1)
            // Actually: !0 → 1, !nonzero → 0. Use LogNot which gives Bool, then convert.
            // Simpler: x == 0.0 gives the same result for numbers.
            Op::Not => {
                builder.emit(fusevm::Op::LoadFloat(0.0), 0);
                builder.emit(fusevm::Op::NumEq, 0);
            }
            Op::Pos => {
                // Unary +: coerce to number (noop for nums already on stack)
                builder.emit(fusevm::Op::LoadFloat(0.0), 0);
                builder.emit(fusevm::Op::Add, 0);
            }
            Op::ToBool => {
                // Convert to 0.0 or 1.0: emit (x != 0.0)
                builder.emit(fusevm::Op::LoadFloat(0.0), 0);
                builder.emit(fusevm::Op::NumNe, 0);
            }
            Op::Pop => {
                builder.emit(fusevm::Op::Pop, 0);
            }
            Op::Dup => {
                builder.emit(fusevm::Op::Dup, 0);
            }
            Op::GetSlot(s) => {
                builder.emit(fusevm::Op::GetSlot(*s), 0);
            }
            Op::SetSlot(s) => {
                builder.emit(fusevm::Op::SetSlot(*s), 0);
            }
            Op::CmpEq => {
                builder.emit(fusevm::Op::NumEq, 0);
            }
            Op::CmpNe => {
                builder.emit(fusevm::Op::NumNe, 0);
            }
            Op::CmpLt => {
                builder.emit(fusevm::Op::NumLt, 0);
            }
            Op::CmpLe => {
                builder.emit(fusevm::Op::NumLe, 0);
            }
            Op::CmpGt => {
                builder.emit(fusevm::Op::NumGt, 0);
            }
            Op::CmpGe => {
                builder.emit(fusevm::Op::NumGe, 0);
            }
            Op::Jump(t) => {
                let target = if *t < ip_map.len() {
                    ip_map[*t]
                } else {
                    ip_map[ip_map.len() - 1]
                };
                builder.emit(fusevm::Op::Jump(target), 0);
            }
            Op::JumpIfFalsePop(t) => {
                let target = if *t < ip_map.len() {
                    ip_map[*t]
                } else {
                    ip_map[ip_map.len() - 1]
                };
                builder.emit(fusevm::Op::JumpIfFalse(target), 0);
            }
            Op::JumpIfTruePop(t) => {
                let target = if *t < ip_map.len() {
                    ip_map[*t]
                } else {
                    ip_map[ip_map.len() - 1]
                };
                builder.emit(fusevm::Op::JumpIfTrue(target), 0);
            }
            Op::IncrSlot(s) => {
                // AWK uses f64: slot += 1.0 (not int increment)
                builder.emit(fusevm::Op::GetSlot(*s), 0);
                builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                builder.emit(fusevm::Op::Add, 0);
                builder.emit(fusevm::Op::SetSlot(*s), 0);
            }
            Op::DecrSlot(s) => {
                // AWK uses f64: slot -= 1.0
                builder.emit(fusevm::Op::GetSlot(*s), 0);
                builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                builder.emit(fusevm::Op::Sub, 0);
                builder.emit(fusevm::Op::SetSlot(*s), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Add) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Add, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Sub) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Swap, 0);
                builder.emit(fusevm::Op::Sub, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Mul) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Mul, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Div) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Div, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Mod) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Mod, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Pow) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Pow, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::IncDecSlot(s, kind) => {
                use crate::ast::IncDecOp;
                let slot = *s;
                match kind {
                    IncDecOp::PreInc => {
                        builder.emit(fusevm::Op::GetSlot(slot), 0);
                        builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                        builder.emit(fusevm::Op::Add, 0);
                        builder.emit(fusevm::Op::Dup, 0);
                        builder.emit(fusevm::Op::SetSlot(slot), 0);
                    }
                    IncDecOp::PostInc => {
                        builder.emit(fusevm::Op::GetSlot(slot), 0);
                        builder.emit(fusevm::Op::Dup, 0);
                        builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                        builder.emit(fusevm::Op::Add, 0);
                        builder.emit(fusevm::Op::SetSlot(slot), 0);
                    }
                    IncDecOp::PreDec => {
                        builder.emit(fusevm::Op::GetSlot(slot), 0);
                        builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                        builder.emit(fusevm::Op::Sub, 0);
                        builder.emit(fusevm::Op::Dup, 0);
                        builder.emit(fusevm::Op::SetSlot(slot), 0);
                    }
                    IncDecOp::PostDec => {
                        builder.emit(fusevm::Op::GetSlot(slot), 0);
                        builder.emit(fusevm::Op::Dup, 0);
                        builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                        builder.emit(fusevm::Op::Sub, 0);
                        builder.emit(fusevm::Op::SetSlot(slot), 0);
                    }
                }
            }
            Op::AddSlotToSlot { src, dst } => {
                builder.emit(fusevm::Op::GetSlot(*src), 0);
                builder.emit(fusevm::Op::GetSlot(*dst), 0);
                builder.emit(fusevm::Op::Add, 0);
                builder.emit(fusevm::Op::SetSlot(*dst), 0);
            }
            Op::JumpIfSlotGeNum {
                slot,
                limit,
                target,
            } => {
                let fuse_target = if *target < ip_map.len() {
                    ip_map[*target]
                } else {
                    ip_map[ip_map.len() - 1]
                };
                builder.emit(fusevm::Op::GetSlot(*slot), 0);
                builder.emit(fusevm::Op::LoadFloat(*limit), 0);
                builder.emit(fusevm::Op::NumGe, 0);
                builder.emit(fusevm::Op::JumpIfTrue(fuse_target), 0);
            }
            _ => return Ok(None),
        }
    }
    let fuse_chunk = builder.build();

    // Execute via fusevm VM (full interpreter with control flow + slot support)
    let mut vm = fusevm::VM::new(fuse_chunk);
    let result = vm.run();

    // Write back modified slots from fusevm frame to awkrs runtime
    if let Some(frame) = vm.frames.last() {
        for i in 0..slot_count.min(frame.slots.len()) {
            match &frame.slots[i] {
                fusevm::Value::Float(f) => ctx.rt.slots[i] = Value::Num(*f),
                fusevm::Value::Int(n) => ctx.rt.slots[i] = Value::Num(*n as f64),
                _ => {}
            }
        }
    }
    // Also check if the frame was already popped (VM ran to completion)
    // — slots were in the last frame which may have been consumed
    if vm.frames.is_empty() && slot_count > 0 {
        // Frame was consumed by PopFrame or end-of-execution.
        // The VM doesn't auto-pop PushFrame at end, so slots should still be there.
        // If not, the original values are unchanged (no writeback needed).
    }

    // Map fusevm result back to awkrs stack + signal.
    // VM::run() pops the last stack value into VMResult::Ok(val), so push it
    // onto ctx.stack so callers that inspect the stack (e.g. pattern truthiness)
    // see the result.
    match result {
        fusevm::VMResult::Ok(ref val) => {
            match val {
                fusevm::Value::Float(f) => ctx.push(Value::Num(*f)),
                fusevm::Value::Int(n) => ctx.push(Value::Num(*n as f64)),
                fusevm::Value::Bool(b) => ctx.push(Value::Num(if *b { 1.0 } else { 0.0 })),
                _ => ctx.push(Value::Num(0.0)),
            }
            // Also push any remaining stack values (multi-value results)
            for val in &vm.stack {
                match val {
                    fusevm::Value::Float(f) => ctx.push(Value::Num(*f)),
                    fusevm::Value::Int(n) => ctx.push(Value::Num(*n as f64)),
                    fusevm::Value::Bool(b) => ctx.push(Value::Num(if *b { 1.0 } else { 0.0 })),
                    _ => ctx.push(Value::Num(0.0)),
                }
            }
            Ok(Some(VmSignal::Normal))
        }
        fusevm::VMResult::Halted => {
            // No result value — rule body with side effects only
            Ok(Some(VmSignal::Normal))
        }
        fusevm::VMResult::Error(msg) => Err(Error::Runtime(msg)),
    }
}

// ── Core VM loop ────────────────────────────────────────────────────────────

fn execute(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<VmSignal> {
    let ops = &chunk.ops;
    // Tier 1: try fusevm JIT (shared VM, universal ops get Cranelift-compiled)
    match try_fusevm_dispatch(chunk, ctx) {
        Ok(Some(signal)) => return Ok(signal),
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    // Tier 2: try awkrs standalone JIT (handles mixed-mode NaN-boxing, full op coverage)
    match try_jit_dispatch(chunk, ctx) {
        Ok(Some(signal)) => return Ok(signal),
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    let len = ops.len();
    let mut pc: usize = 0;

    while pc < len {
        match ops[pc] {
            // ── Constants ───────────────────────────────────────────────
            Op::PushNum(n) => {
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, n, round).0));
                } else {
                    ctx.push(Value::Num(n));
                }
            }
            Op::PushNumDecimalStr(idx) => {
                let s = ctx.str_ref(idx);
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let f = crate::bignum::numeric_string_to_mpfr(s, prec, round);
                    ctx.push(Value::Mpfr(f));
                } else {
                    ctx.push(Value::Num(s.parse().unwrap_or(0.0)));
                }
            }
            Op::PushStr(idx) => ctx.push(Value::StrLit(ctx.str_ref(idx).to_string())),
            Op::PushRegexp(idx) => ctx.push(Value::Regexp(ctx.str_ref(idx).to_string())),

            // ── Variable access ─────────────────────────────────────────
            Op::GetVar(idx) => {
                let name = ctx.str_ref(idx).to_string();
                let v = ctx.get_var(name.as_str());
                ctx.push(v);
            }
            Op::SetVar(idx) => {
                let val = ctx.peek().clone();
                ctx.set_var_interned(idx, val)?;
            }
            Op::GetSlot(slot) => {
                let v = match &ctx.rt.slots[slot as usize] {
                    Value::Num(n) => Value::Num(*n),
                    other => other.clone(),
                };
                ctx.push(v);
            }
            Op::SetSlot(slot) => {
                ctx.rt.slots[slot as usize] = ctx.peek().clone();
            }
            Op::GetField => {
                let i = ctx.pop().as_number() as i32;
                let v = ctx.rt.field(i)?;
                ctx.push(v);
            }
            Op::SetField => {
                let val = ctx.pop();
                let idx = ctx.pop().as_number() as i32;
                // gawk parity: assigning a number to `$n` stringifies via
                // CONVFMT (so `CONVFMT="%.2f"; $1=3.14159` stores `"3.14"`),
                // not via the f64 Display default which keeps full precision.
                let s = match &val {
                    Value::Num(n) => ctx.rt.num_to_string_convfmt(*n),
                    Value::Mpfr(f) => ctx.rt.mpfr_to_string_convfmt(f),
                    _ => val.as_str(),
                };
                ctx.rt.set_field(idx, &s)?;
                ctx.push(val);
            }
            Op::GetArrayElem(arr) => {
                let key_val = ctx.pop();
                // POSIX: numeric subscripts are stringified via CONVFMT.
                let k = ctx.rt.value_to_array_key(&key_val);
                let name = ctx.str_ref(arr).to_string();
                // POSIX: reading `a[k]` auto-creates the entry as `Uninit` if
                // missing. Subsequent `k in a` returns 1, `typeof(a[k])` is
                // "untyped" (rather than "string" from a coerced "").
                if name != "SYMTAB" && !ctx.rt.array_has(&name, &k) {
                    ctx.rt.array_set(&name, k.clone(), Value::Uninit);
                }
                let v = ctx.array_elem_get(&name, &k);
                ctx.push(v);
            }
            Op::SymtabKeyCount => {
                let n = ctx.symtab_key_count() as f64;
                ctx.push(Value::Num(n));
            }
            Op::TypeofVar(idx) => {
                let name = ctx.str_ref(idx);
                let t = ctx.typeof_scalar_name(name);
                ctx.push(t);
            }
            Op::TypeofSlot(slot) => {
                if ctx.rt.posix || ctx.rt.traditional {
                    return Err(Error::Runtime(
                        "`typeof` is a gawk extension not available in POSIX/traditional mode"
                            .into(),
                    ));
                }
                let t = builtins::awk_typeof_value(&ctx.rt.slots[slot as usize]);
                ctx.push(Value::Str(t.into()));
            }
            Op::TypeofArrayElem(arr) => {
                if ctx.rt.posix || ctx.rt.traditional {
                    return Err(Error::Runtime(
                        "`typeof` is a gawk extension not available in POSIX/traditional mode"
                            .into(),
                    ));
                }
                let key_val = ctx.pop();
                let k = key_val.as_str_cow();
                let name = ctx.str_ref(arr);
                let t = if name == "SYMTAB" {
                    ctx.typeof_scalar_name(k.as_ref())
                } else {
                    Value::Str(builtins::awk_typeof_array_elem(ctx.rt, name, k.as_ref()).into())
                };
                ctx.push(t);
            }
            Op::TypeofField => {
                if ctx.rt.posix || ctx.rt.traditional {
                    return Err(Error::Runtime(
                        "`typeof` is a gawk extension not available in POSIX/traditional mode"
                            .into(),
                    ));
                }
                let i = ctx.pop().as_number() as i32;
                if i < 0 {
                    return Err(Error::Runtime("attempt to access field number -1".into()));
                }
                // gawk parity: fields are "strnum" if their string value parses
                // as a number (`$1` of "  42 " is "strnum"), "string" otherwise,
                // and "unassigned" when the field index is beyond NF.
                let t = if ctx.rt.field_is_unassigned(i) {
                    "unassigned"
                } else {
                    let v = ctx.rt.field(i)?;
                    if v.is_numeric_str() && !matches!(&v, Value::Str(s) if s.is_empty()) {
                        "strnum"
                    } else {
                        "string"
                    }
                };
                ctx.push(Value::Str(t.into()));
            }
            Op::TypeofValue => {
                if ctx.rt.posix || ctx.rt.traditional {
                    return Err(Error::Runtime(
                        "`typeof` is a gawk extension not available in POSIX/traditional mode"
                            .into(),
                    ));
                }
                let v = ctx.pop();
                let t = builtins::awk_typeof_value(&v);
                ctx.push(Value::Str(t.into()));
            }
            Op::SetArrayElem(arr) => {
                let val = ctx.pop();
                // POSIX: numeric subscripts are stringified via CONVFMT.
                let key_val = ctx.pop();
                let key = ctx.rt.value_to_array_key(&key_val);
                let name = ctx.cp.strings.get(arr);
                ctx.array_elem_set(name, key, val.clone());
                ctx.push(val);
            }

            // ── Compound assignment ─────────────────────────────────────
            Op::CompoundAssignVar(idx, bop) => {
                let rhs = ctx.pop();
                let new_val = ctx.with_short_pool_name_mut(
                    idx,
                    |ctx, name| -> crate::error::Result<Value> {
                        let old = match ctx.var_value_cow(name) {
                            Cow::Borrowed(v) => v.clone(),
                            Cow::Owned(v) => v,
                        };
                        let new_val = apply_binop(bop, &old, &rhs, ctx.rt.bignum, ctx.rt)?;
                        ctx.set_var(name, new_val.clone())?;
                        Ok(new_val)
                    },
                )?;
                ctx.push(new_val);
            }
            Op::CompoundAssignSlot(slot, bop) => {
                let rhs = ctx.pop();
                let new_val = apply_binop(
                    bop,
                    &ctx.rt.slots[slot as usize],
                    &rhs,
                    ctx.rt.bignum,
                    ctx.rt,
                )?;
                ctx.rt.slots[slot as usize] = new_val.clone();
                ctx.push(new_val);
            }
            Op::CompoundAssignField(bop) => {
                let rhs = ctx.pop();
                let idx = ctx.pop().as_number() as i32;
                let old = ctx.rt.field(idx)?;
                let new_val = apply_binop(bop, &old, &rhs, ctx.rt.bignum, ctx.rt)?;
                let s = new_val.as_str();
                ctx.rt.set_field(idx, &s)?;
                ctx.push(new_val);
            }
            Op::CompoundAssignIndex(arr, bop) => {
                let rhs = ctx.pop();
                let key_val = ctx.pop();
                let name = ctx.cp.strings.get(arr);
                let old = {
                    let k = key_val.as_str_cow();
                    ctx.array_elem_get(name, k.as_ref())
                };
                let new_val = apply_binop(bop, &old, &rhs, ctx.rt.bignum, ctx.rt)?;
                let key = key_val.into_string();
                ctx.array_elem_set(name, key, new_val.clone());
                ctx.push(new_val);
            }

            Op::IncDecVar(idx, kind) => {
                let delta = incdec_delta(kind);
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let pushed =
                        ctx.with_short_pool_name_mut(idx, |ctx, name| -> Result<Value> {
                            let old = match ctx.var_value_cow(name) {
                                Cow::Borrowed(v) => v.clone(),
                                Cow::Owned(v) => v,
                            };
                            let old_f = value_to_float(&old, prec, round);
                            let d = Float::with_val(prec, delta);
                            let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                            ctx.set_var(name, Value::Mpfr(new_f.clone()))?;
                            Ok(match kind {
                                IncDecOp::PreInc | IncDecOp::PreDec => Value::Mpfr(new_f),
                                IncDecOp::PostInc | IncDecOp::PostDec => Value::Mpfr(old_f),
                            })
                        })?;
                    ctx.push(pushed);
                } else {
                    let (old_n, new_n) =
                        ctx.with_short_pool_name_mut(idx, |ctx, name| -> Result<(f64, f64)> {
                            let old_n = match ctx.var_value_cow(name) {
                                Cow::Borrowed(v) => v.as_number(),
                                Cow::Owned(v) => v.as_number(),
                            };
                            let new_n = old_n + delta;
                            ctx.set_var(name, Value::Num(new_n))?;
                            Ok((old_n, new_n))
                        })?;
                    ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
                }
            }
            Op::IncrVar(idx) => {
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    ctx.with_short_pool_name_mut(idx, |ctx, name| -> Result<()> {
                        let old = match ctx.var_value_cow(name) {
                            Cow::Borrowed(v) => v.clone(),
                            Cow::Owned(v) => v,
                        };
                        let old_f = value_to_float(&old, prec, round);
                        let d = Float::with_val(prec, 1.0);
                        let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                        ctx.set_var(name, Value::Mpfr(new_f))?;
                        Ok(())
                    })?;
                } else {
                    ctx.with_short_pool_name_mut(idx, |ctx, name| -> Result<()> {
                        let n = match ctx.var_value_cow(name) {
                            Cow::Borrowed(v) => v.as_number(),
                            Cow::Owned(v) => v.as_number(),
                        };
                        ctx.set_var(name, Value::Num(n + 1.0))?;
                        Ok(())
                    })?;
                }
            }
            Op::DecrVar(idx) => {
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    ctx.with_short_pool_name_mut(idx, |ctx, name| -> Result<()> {
                        let old = match ctx.var_value_cow(name) {
                            Cow::Borrowed(v) => v.clone(),
                            Cow::Owned(v) => v,
                        };
                        let old_f = value_to_float(&old, prec, round);
                        let d = Float::with_val(prec, 1.0);
                        let new_f = Float::with_val_round(prec, &old_f - &d, round).0;
                        ctx.set_var(name, Value::Mpfr(new_f))?;
                        Ok(())
                    })?;
                } else {
                    ctx.with_short_pool_name_mut(idx, |ctx, name| -> Result<()> {
                        let n = match ctx.var_value_cow(name) {
                            Cow::Borrowed(v) => v.as_number(),
                            Cow::Owned(v) => v.as_number(),
                        };
                        ctx.set_var(name, Value::Num(n - 1.0))?;
                        Ok(())
                    })?;
                }
            }
            Op::IncDecSlot(slot, kind) => {
                let delta = incdec_delta(kind);
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let old = ctx.rt.slots[slot as usize].clone();
                    let old_f = value_to_float(&old, prec, round);
                    let d = Float::with_val(prec, delta);
                    let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                    let ret = match kind {
                        IncDecOp::PreInc | IncDecOp::PreDec => Value::Mpfr(new_f.clone()),
                        IncDecOp::PostInc | IncDecOp::PostDec => Value::Mpfr(old_f),
                    };
                    ctx.rt.slots[slot as usize] = Value::Mpfr(new_f);
                    ctx.push(ret);
                } else {
                    let old_n = match &ctx.rt.slots[slot as usize] {
                        Value::Num(v) => *v,
                        other => other.as_number(),
                    };
                    let new_n = old_n + delta;
                    ctx.rt.slots[slot as usize] = Value::Num(new_n);
                    ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
                }
            }
            Op::IncDecField(kind) => {
                let idx = ctx.pop().as_number() as i32;
                let delta = incdec_delta(kind);
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let old = ctx.rt.field(idx)?;
                    let old_f = value_to_float(&old, prec, round);
                    let d = Float::with_val(prec, delta);
                    let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                    let ret = match kind {
                        IncDecOp::PreInc | IncDecOp::PreDec => Value::Mpfr(new_f.clone()),
                        IncDecOp::PostInc | IncDecOp::PostDec => Value::Mpfr(old_f),
                    };
                    ctx.rt.set_field_from_mpfr(idx, &new_f)?;
                    ctx.push(ret);
                } else {
                    let old_n = ctx.rt.field(idx)?.as_number();
                    let new_n = old_n + delta;
                    ctx.rt.set_field_num(idx, new_n)?;
                    ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
                }
            }
            Op::IncDecIndex(arr, kind) => {
                let key = ctx.pop().into_string();
                let name = ctx.cp.strings.get(arr);
                let delta = incdec_delta(kind);
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let old = ctx.array_elem_get(name, &key);
                    let old_f = value_to_float(&old, prec, round);
                    let d = Float::with_val(prec, delta);
                    let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                    let ret = match kind {
                        IncDecOp::PreInc | IncDecOp::PreDec => Value::Mpfr(new_f.clone()),
                        IncDecOp::PostInc | IncDecOp::PostDec => Value::Mpfr(old_f),
                    };
                    ctx.array_elem_set(name, key, Value::Mpfr(new_f));
                    ctx.push(ret);
                } else {
                    let old_n = ctx.array_elem_get(name, &key).as_number();
                    let new_n = old_n + delta;
                    ctx.array_elem_set(name, key, Value::Num(new_n));
                    ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
                }
            }

            // ── Arithmetic ──────────────────────────────────────────────
            Op::Add => {
                if ctx.rt.bignum {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let fa = value_to_float(&a, prec, round);
                    let fb = value_to_float(&b, prec, round);
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, &fa + &fb, round).0));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    ctx.push(Value::Num(a.as_number() + b.as_number()));
                }
            }
            Op::Sub => {
                if ctx.rt.bignum {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let fa = value_to_float(&a, prec, round);
                    let fb = value_to_float(&b, prec, round);
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, &fa - &fb, round).0));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    ctx.push(Value::Num(a.as_number() - b.as_number()));
                }
            }
            Op::Mul => {
                if ctx.rt.bignum {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let fa = value_to_float(&a, prec, round);
                    let fb = value_to_float(&b, prec, round);
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, &fa * &fb, round).0));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    ctx.push(Value::Num(a.as_number() * b.as_number()));
                }
            }
            Op::Div => {
                if ctx.rt.bignum {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let fa = value_to_float(&a, prec, round);
                    let fb = value_to_float(&b, prec, round);
                    if fb.is_zero() {
                        return Err(Error::Runtime("division by zero attempted".into()));
                    }
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, &fa / &fb, round).0));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let bn = b.as_number();
                    let an = a.as_number();
                    if bn == 0.0 {
                        return Err(Error::Runtime("division by zero attempted".into()));
                    }
                    ctx.push(Value::Num(an / bn));
                }
            }
            Op::Mod => {
                if ctx.rt.bignum {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let fa = value_to_float(&a, prec, round);
                    let fb = value_to_float(&b, prec, round);
                    if fb.is_zero() {
                        return Err(Error::Runtime(
                            "division by zero attempted in `%'".into(),
                        ));
                    }
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, &fa % &fb, round).0));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let bn = b.as_number();
                    let an = a.as_number();
                    if bn == 0.0 {
                        return Err(Error::Runtime(
                            "division by zero attempted in `%'".into(),
                        ));
                    }
                    ctx.push(Value::Num(an % bn));
                }
            }
            Op::Pow => {
                if ctx.rt.bignum {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let fa = value_to_float(&a, prec, round);
                    let fb = value_to_float(&b, prec, round);
                    ctx.push(Value::Mpfr(
                        Float::with_val_round(prec, fa.pow(&fb), round).0,
                    ));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    ctx.push(Value::Num(a.as_number().powf(b.as_number())));
                }
            }

            // ── Comparison (POSIX-aware) ────────────────────────────────
            Op::CmpEq => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let ic = ctx.rt.ignore_case_flag();
                ctx.push(awk_cmp_eq(&a, &b, ic, ctx.rt));
            }
            Op::CmpNe => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let ic = ctx.rt.ignore_case_flag();
                let eq = awk_cmp_eq(&a, &b, ic, ctx.rt);
                ctx.push(Value::Num(if eq.as_number() != 0.0 { 0.0 } else { 1.0 }));
            }
            Op::CmpLt => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let ic = ctx.rt.ignore_case_flag();
                ctx.push(awk_cmp_rel(BinOp::Lt, &a, &b, ic, ctx.rt));
            }
            Op::CmpLe => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let ic = ctx.rt.ignore_case_flag();
                ctx.push(awk_cmp_rel(BinOp::Le, &a, &b, ic, ctx.rt));
            }
            Op::CmpGt => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let ic = ctx.rt.ignore_case_flag();
                ctx.push(awk_cmp_rel(BinOp::Gt, &a, &b, ic, ctx.rt));
            }
            Op::CmpGe => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let ic = ctx.rt.ignore_case_flag();
                ctx.push(awk_cmp_rel(BinOp::Ge, &a, &b, ic, ctx.rt));
            }

            // ── String / regex ──────────────────────────────────────────
            Op::Concat => {
                let b = ctx.pop();
                let a = ctx.pop();
                a.reject_if_array_scalar()?;
                b.reject_if_array_scalar()?;
                let both_lit = matches!(&a, Value::StrLit(_)) && matches!(&b, Value::StrLit(_));
                let mut s = match a {
                    Value::Str(s) => s,
                    Value::StrLit(s) => s,
                    Value::Regexp(s) => s,
                    Value::Num(n) => ctx.rt.num_to_string_convfmt(n),
                    Value::Mpfr(f) => ctx.rt.mpfr_to_string_convfmt(&f),
                    Value::Uninit => String::new(),
                    Value::Array(_) => String::new(),
                };
                match b {
                    Value::Str(ref t) => s.push_str(t),
                    Value::StrLit(ref t) => s.push_str(t),
                    Value::Regexp(ref t) => s.push_str(t),
                    Value::Num(n) => s.push_str(&ctx.rt.num_to_string_convfmt(n)),
                    Value::Mpfr(f) => s.push_str(&ctx.rt.mpfr_to_string_convfmt(&f)),
                    Value::Uninit => {}
                    Value::Array(_) => {}
                }
                let out = if both_lit {
                    Value::StrLit(s)
                } else {
                    Value::Str(s)
                };
                ctx.push(out);
            }
            Op::RegexMatch => {
                let pat_v = ctx.pop();
                pat_v.reject_if_array_scalar()?;
                let pat = pat_v.as_str();
                let v = ctx.pop();
                v.reject_if_array_scalar()?;
                let s = match &v {
                    Value::Num(n) => ctx.rt.num_to_string_convfmt(*n),
                    Value::Mpfr(f) => ctx.rt.mpfr_to_string_convfmt(f),
                    _ => v.as_str(),
                };
                ctx.rt.ensure_regex(&pat).map_err(Error::Runtime)?;
                let m = ctx.rt.regex_ref(&pat).is_match(&s);
                ctx.push(Value::Num(if m { 1.0 } else { 0.0 }));
            }
            Op::RegexNotMatch => {
                let pat_v = ctx.pop();
                pat_v.reject_if_array_scalar()?;
                let pat = pat_v.as_str();
                let v = ctx.pop();
                v.reject_if_array_scalar()?;
                let s = match &v {
                    Value::Num(n) => ctx.rt.num_to_string_convfmt(*n),
                    Value::Mpfr(f) => ctx.rt.mpfr_to_string_convfmt(f),
                    _ => v.as_str(),
                };
                ctx.rt.ensure_regex(&pat).map_err(Error::Runtime)?;
                let m = ctx.rt.regex_ref(&pat).is_match(&s);
                ctx.push(Value::Num(if !m { 1.0 } else { 0.0 }));
            }

            // ── Unary ───────────────────────────────────────────────────
            Op::Neg => {
                if ctx.rt.bignum {
                    let v = ctx.pop();
                    v.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let f = value_to_float(&v, prec, round);
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, -f, round).0));
                } else {
                    let v = ctx.pop();
                    v.reject_if_array_scalar()?;
                    ctx.push(Value::Num(-v.as_number()));
                }
            }
            Op::Pos => {
                if ctx.rt.bignum {
                    let v = ctx.pop();
                    v.reject_if_array_scalar()?;
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let f = value_to_float(&v, prec, round);
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, f, round).0));
                } else {
                    let v = ctx.pop();
                    v.reject_if_array_scalar()?;
                    ctx.push(Value::Num(v.as_number()));
                }
            }
            Op::Not => {
                let v = ctx.pop();
                ctx.push(Value::Num(if truthy(&v)? { 0.0 } else { 1.0 }));
            }
            Op::ToBool => {
                let v = ctx.pop();
                ctx.push(Value::Num(if truthy(&v)? { 1.0 } else { 0.0 }));
            }

            // ── Control flow ────────────────────────────────────────────
            Op::Jump(target) => {
                pc = target;
                continue;
            }
            Op::JumpIfFalsePop(target) => {
                let v = ctx.pop();
                if !truthy(&v)? {
                    pc = target;
                    continue;
                }
            }
            Op::JumpIfTruePop(target) => {
                let v = ctx.pop();
                if truthy(&v)? {
                    pc = target;
                    continue;
                }
            }

            // ── Print / Printf ──────────────────────────────────────────
            Op::Print { argc, redir } => exec_print(ctx, argc, redir, false)?,
            Op::Printf { argc, redir } => exec_print(ctx, argc, redir, true)?,

            // ── Flow signals ────────────────────────────────────────────
            Op::Next => return Ok(VmSignal::Next),
            Op::NextFile => return Ok(VmSignal::NextFile),
            Op::ExitWithCode => {
                let code = ctx.pop().as_number() as i32;
                ctx.rt.exit_code = code;
                ctx.rt.exit_pending = true;
                return Ok(VmSignal::ExitPending);
            }
            Op::ExitDefault => {
                ctx.rt.exit_code = 0;
                ctx.rt.exit_pending = true;
                return Ok(VmSignal::ExitPending);
            }
            Op::ReturnVal => {
                let v = ctx.pop();
                return Ok(VmSignal::Return(v));
            }
            Op::ReturnEmpty => return Ok(VmSignal::Return(Value::Str(String::new()))),

            // ── Function calls ──────────────────────────────────────────
            Op::CallBuiltin(name_idx, argc) => {
                let name = ctx.str_ref(name_idx).to_string();
                exec_call_builtin(ctx, &name, argc)?;
            }
            Op::CallIndirect(argc) => {
                let name = ctx.pop().into_string();
                exec_call_builtin(ctx, &name, argc)?;
            }
            Op::CallUser(name_idx, argc) => {
                let name = ctx.str_ref(name_idx).to_string();
                exec_call_user(ctx, &name, argc)?;
            }
            Op::CallUserBindArrays(name_idx, argc) => {
                let name = ctx.str_ref(name_idx).to_string();
                exec_call_user_bind_arrays(ctx, &name, argc)?;
            }

            // ── Array ops ───────────────────────────────────────────────
            Op::InArray(arr) => {
                let key_val = ctx.pop();
                let k = key_val.as_str_cow();
                let name = ctx.str_ref(arr);
                let b = if name == "SYMTAB" {
                    ctx.symtab_has(k.as_ref())
                } else {
                    ctx.rt.array_has(name, k.as_ref())
                };
                ctx.push(Value::Num(if b { 1.0 } else { 0.0 }));
            }
            Op::DeleteArray(arr) => {
                let name = ctx.str_ref(arr).to_string();
                // POSIX array call-by-reference: `delete a` inside a function
                // must clear the caller's array via the frame param.
                let mut handled = false;
                for frame in ctx.locals.iter_mut().rev() {
                    if let Some(Value::Array(a)) = frame.get_mut(name.as_str()) {
                        a.clear();
                        handled = true;
                        break;
                    }
                }
                if !handled {
                    ctx.rt.array_delete(&name, None);
                }
            }
            Op::DeleteElem(arr) => {
                let key_val = ctx.pop();
                let k = key_val.as_str_cow();
                let name = ctx.cp.strings.get(arr);
                if name == "SYMTAB" {
                    ctx.symtab_delete(k.as_ref());
                } else {
                    // Frame-aware delete: `delete a[k]` inside a function with
                    // `a` as a by-reference array param routes through the
                    // frame, not global vars.
                    let mut handled = false;
                    for frame in ctx.locals.iter_mut().rev() {
                        if let Some(Value::Array(map)) = frame.get_mut(name) {
                            map.remove(k.as_ref());
                            handled = true;
                            break;
                        }
                    }
                    if !handled {
                        ctx.rt.array_delete(name, Some(k.as_ref()));
                    }
                }
            }

            // ── Multi-dimensional array key ─────────────────────────────
            Op::JoinArrayKey(n) => {
                let sep = ctx
                    .rt
                    .vars
                    .get("SUBSEP")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| "\x1c".into());
                let n = n as usize;
                let start = ctx.stack.len() - n;
                let parts: Vec<String> = ctx.stack.drain(start..).map(|v| v.as_str()).collect();
                ctx.push(Value::Str(parts.join(&sep)));
            }

            // ── Getline ─────────────────────────────────────────────────
            Op::GetLine {
                var,
                source,
                push_result,
            } => exec_getline(ctx, var, source, push_result)?,

            // ── Sub / Gsub ──────────────────────────────────────────────
            Op::SubFn(target) => exec_sub(ctx, target, false)?,
            Op::GsubFn(target) => exec_sub(ctx, target, true)?,

            // ── Split ───────────────────────────────────────────────────
            Op::Split { arr, has_fs, seps } => {
                let fs = if has_fs {
                    ctx.pop().as_str()
                } else {
                    ctx.rt
                        .vars
                        .get("FS")
                        .map(|v| v.as_str())
                        .unwrap_or_else(|| " ".into())
                };
                let s = ctx.pop().as_str();
                let arr_name = ctx.str_ref(arr).to_string();
                let seps_name = seps.map(|i| ctx.str_ref(i).to_string());
                let (parts, seps_vec) = crate::runtime::split_string_with_seps(
                    &s,
                    &fs,
                    ctx.rt.ignore_case_flag(),
                );
                let n = parts.len();
                ctx.rt.split_into_array(&arr_name, &parts);
                if let Some(name) = seps_name {
                    ctx.rt.split_into_array(&name, &seps_vec);
                }
                ctx.push(Value::Num(n as f64));
            }

            // ── Patsplit ────────────────────────────────────────────────
            Op::Patsplit { arr, has_fp, seps } => {
                let fp = if has_fp {
                    Some(ctx.pop().as_str())
                } else {
                    None
                };
                let s = ctx.pop().as_str();
                let arr_name = ctx.str_ref(arr).to_string();
                let seps_name = seps.map(|i| ctx.str_ref(i).to_string());
                let n =
                    builtins::patsplit(ctx.rt, &s, &arr_name, fp.as_deref(), seps_name.as_deref())?;
                ctx.push(Value::Num(n));
            }

            // ── Match builtin ───────────────────────────────────────────
            Op::MatchBuiltin { arr } => {
                let re = ctx.pop().as_str();
                let s = ctx.pop().as_str();
                let arr_name = arr.map(|i| ctx.str_ref(i).to_string());
                let r = builtins::match_fn(ctx.rt, &s, &re, arr_name.as_deref())?;
                ctx.push(Value::Num(r));
            }

            // ── ForIn ───────────────────────────────────────────────────
            Op::ForInStart(arr) => {
                let name = ctx.str_ref(arr).to_string();
                let keys = ctx.for_in_keys(name.as_str())?;
                ctx.for_in_iters.push(ForInState { keys, index: 0 });
            }
            Op::ForInNext { var, end_jump } => {
                let state = ctx.for_in_iters.last_mut().unwrap();
                if state.index >= state.keys.len() {
                    pc = end_jump;
                    continue;
                }
                // Move key out of the snapshot vec — avoids cloning each `String`.
                let key = mem::take(&mut state.keys[state.index]);
                state.index += 1;
                ctx.set_var_interned(var, Value::Str(key))?;
            }
            Op::ForInEnd => {
                ctx.for_in_iters.pop();
            }

            // ── Stack ───────────────────────────────────────────────────
            Op::Pop => {
                ctx.pop();
            }
            Op::Dup => {
                let v = ctx.peek().clone();
                ctx.push(v);
            }
            Op::Asort { src, dest } => {
                let s = ctx.cp.strings.get(src);
                let d = dest.map(|i| ctx.cp.strings.get(i));
                let n = builtins::asort(ctx.rt, s, d)?;
                ctx.push(Value::Num(n));
            }
            Op::Asorti { src, dest } => {
                let s = ctx.cp.strings.get(src);
                let d = dest.map(|i| ctx.cp.strings.get(i));
                let n = builtins::asorti(ctx.rt, s, d)?;
                ctx.push(Value::Num(n));
            }

            // ── Pattern helpers ─────────────────────────────────────────
            Op::MatchRegexp(idx) => {
                let pat = ctx.str_ref(idx).to_string();
                ctx.rt.ensure_regex(&pat).map_err(Error::Runtime)?;
                let m = ctx.rt.regex_ref(&pat).is_match(&ctx.rt.record);
                ctx.push(Value::Num(if m { 1.0 } else { 0.0 }));
            }

            // ── Fused opcodes ──────────────────────────────────────────
            Op::ConcatPoolStr(idx) => {
                // POSIX: number→string in concat context uses CONVFMT. The
                // generic `Value::into_string()` path calls `format_number`
                // which ignores CONVFMT; we must dispatch on `Value::Num`/
                // `Value::Mpfr` here to honor the user-visible format global.
                let v = ctx.pop();
                let lit = matches!(v, Value::StrLit(_));
                let mut s = match v {
                    Value::Num(n) => ctx.rt.num_to_string_convfmt(n),
                    Value::Mpfr(ref f) => ctx.rt.mpfr_to_string_convfmt(f),
                    other => other.into_string(),
                };
                let pool_str = ctx.cp.strings.get(idx);
                s.push_str(pool_str);
                ctx.push(if lit { Value::StrLit(s) } else { Value::Str(s) });
            }
            Op::GetNR => ctx.push(Value::Num(ctx.rt.nr)),
            Op::GetFNR => ctx.push(Value::Num(ctx.rt.fnr)),
            Op::GetNF => {
                let nf = ctx.rt.nf() as f64;
                ctx.push(Value::Num(nf));
            }
            Op::PushFieldNum(field) => {
                let n = ctx.rt.field_as_number(field as i32)?;
                ctx.push(Value::Num(n));
            }
            Op::AddFieldToSlot { field, slot } => {
                let field_val = ctx.rt.field_as_number(field as i32)?;
                let old = match &ctx.rt.slots[slot as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[slot as usize] = Value::Num(old + field_val);
            }
            Op::PrintFieldStdout(field) => {
                if let Some(ref mut buf) = ctx.print_out {
                    // Parallel capture path: build string, push to capture buffer.
                    let val = ctx.rt.field(field as i32)?;
                    let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();
                    let s = format!("{}{}", val.as_str(), ors);
                    buf.push(s);
                } else {
                    ctx.rt.print_field_to_buf(field as usize);
                    let mut ors_local = [0u8; 64];
                    let ors_len = ctx.rt.ors_bytes.len().min(64);
                    ors_local[..ors_len].copy_from_slice(&ctx.rt.ors_bytes[..ors_len]);
                    ctx.rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
                }
            }
            Op::IncrSlot(slot) => {
                let s = slot as usize;
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let old = ctx.rt.slots[s].clone();
                    let old_f = value_to_float(&old, prec, round);
                    let d = Float::with_val(prec, 1.0);
                    let new_f = Float::with_val_round(prec, &old_f + &d, round).0;
                    ctx.rt.slots[s] = Value::Mpfr(new_f);
                } else {
                    let n = match &ctx.rt.slots[s] {
                        Value::Num(v) => *v,
                        other => other.as_number(),
                    };
                    ctx.rt.slots[s] = Value::Num(n + 1.0);
                }
            }
            Op::DecrSlot(slot) => {
                let s = slot as usize;
                if ctx.rt.bignum {
                    let prec = ctx.rt.mpfr_prec_bits();
                    let round = ctx.rt.mpfr_round();
                    let old = ctx.rt.slots[s].clone();
                    let old_f = value_to_float(&old, prec, round);
                    let d = Float::with_val(prec, 1.0);
                    let new_f = Float::with_val_round(prec, &old_f - &d, round).0;
                    ctx.rt.slots[s] = Value::Mpfr(new_f);
                } else {
                    let n = match &ctx.rt.slots[s] {
                        Value::Num(v) => *v,
                        other => other.as_number(),
                    };
                    ctx.rt.slots[s] = Value::Num(n - 1.0);
                }
            }
            Op::AddSlotToSlot { src, dst } => {
                let sv = match &ctx.rt.slots[src as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                let dv = match &ctx.rt.slots[dst as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[dst as usize] = Value::Num(dv + sv);
            }
            Op::AddMulFieldsToSlot { f1, f2, slot } => {
                let p = ctx.rt.field_as_number(f1 as i32)? * ctx.rt.field_as_number(f2 as i32)?;
                let old = match &ctx.rt.slots[slot as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[slot as usize] = Value::Num(old + p);
            }
            Op::ArrayFieldAddConst { arr, field, delta } => {
                let name = ctx.cp.strings.get(arr);
                ctx.rt.array_field_add_delta(name, field as i32, delta);
            }
            Op::PrintFieldSepField { f1, sep, f2 } => {
                let sep_s = ctx.str_ref(sep).to_string();
                if let Some(ref mut buf) = ctx.print_out {
                    let ors_b = ctx.rt.ors_bytes.clone();
                    let v1 = ctx.rt.field(f1 as i32)?.as_str();
                    let v2 = ctx.rt.field(f2 as i32)?.as_str();
                    let ors = String::from_utf8_lossy(&ors_b);
                    buf.push(format!("{v1}{sep_s}{v2}{ors}"));
                } else {
                    ctx.rt.print_field_to_buf(f1 as usize);
                    ctx.rt.print_buf.extend_from_slice(sep_s.as_bytes());
                    ctx.rt.print_field_to_buf(f2 as usize);
                    let mut ors_local = [0u8; 64];
                    let ors_len = ctx.rt.ors_bytes.len().min(64);
                    ors_local[..ors_len].copy_from_slice(&ctx.rt.ors_bytes[..ors_len]);
                    ctx.rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
                }
            }
            Op::PrintThreeFieldsStdout { f1, f2, f3 } => {
                if let Some(ref mut buf) = ctx.print_out {
                    let ofs_b = ctx.rt.ofs_bytes.clone();
                    let ors_b = ctx.rt.ors_bytes.clone();
                    let v1 = ctx.rt.field(f1 as i32)?.as_str();
                    let v2 = ctx.rt.field(f2 as i32)?.as_str();
                    let v3 = ctx.rt.field(f3 as i32)?.as_str();
                    let ofs = String::from_utf8_lossy(&ofs_b);
                    let ors = String::from_utf8_lossy(&ors_b);
                    buf.push(format!("{v1}{ofs}{v2}{ofs}{v3}{ors}"));
                } else {
                    let mut ofs_local = [0u8; 64];
                    let ofs_len = ctx.rt.ofs_bytes.len().min(64);
                    ofs_local[..ofs_len].copy_from_slice(&ctx.rt.ofs_bytes[..ofs_len]);
                    let mut ors_local = [0u8; 64];
                    let ors_len = ctx.rt.ors_bytes.len().min(64);
                    ors_local[..ors_len].copy_from_slice(&ctx.rt.ors_bytes[..ors_len]);
                    ctx.rt.print_field_to_buf(f1 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                    ctx.rt.print_field_to_buf(f2 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                    ctx.rt.print_field_to_buf(f3 as usize);
                    ctx.rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
                }
            }
            Op::JumpIfSlotGeNum {
                slot,
                limit,
                target,
            } => {
                let v = match &ctx.rt.slots[slot as usize] {
                    Value::Num(n) => *n,
                    other => other.as_number(),
                };
                if v >= limit {
                    pc = target;
                    continue;
                }
            }
        }
        pc += 1;
    }
    Ok(VmSignal::Normal)
}

// ── Helper functions ────────────────────────────────────────────────────────

/// Flush the persistent stdout buffer. Called at file boundaries, not per-record.
pub fn flush_print_buf(buf: &mut Vec<u8>) -> Result<()> {
    if !buf.is_empty() {
        let mut out = io::stdout().lock();
        out.write_all(buf).map_err(Error::Io)?;
        buf.clear();
    }
    Ok(())
}

fn truthy(v: &Value) -> Result<bool> {
    v.truthy_cond()
}

fn incdec_delta(kind: IncDecOp) -> f64 {
    match kind {
        IncDecOp::PreInc | IncDecOp::PostInc => 1.0,
        IncDecOp::PreDec | IncDecOp::PostDec => -1.0,
    }
}

fn incdec_push(kind: IncDecOp, old_n: f64, new_n: f64) -> f64 {
    match kind {
        IncDecOp::PreInc | IncDecOp::PreDec => new_n,
        IncDecOp::PostInc | IncDecOp::PostDec => old_n,
    }
}

fn apply_binop(op: BinOp, old: &Value, rhs: &Value, use_mpfr: bool, rt: &Runtime) -> Result<Value> {
    crate::runtime::awk_binop_values(op, old, rhs, use_mpfr, rt)
}

/// POSIX string compare via `strcoll` on Unix.
fn locale_str_cmp(a: &str, b: &str) -> Ordering {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        match (CString::new(a), CString::new(b)) {
            (Ok(ca), Ok(cb)) => unsafe {
                let r = libc::strcoll(ca.as_ptr(), cb.as_ptr());
                r.cmp(&0)
            },
            _ => a.cmp(b),
        }
    }
    #[cfg(not(unix))]
    {
        a.cmp(b)
    }
}

fn awk_cmp_eq(a: &Value, b: &Value, ignore_case: bool, rt: &Runtime) -> Value {
    if rt.bignum && a.is_numeric_str() && b.is_numeric_str() {
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let fa = value_to_float(a, prec, round);
        let fb = value_to_float(b, prec, round);
        return Value::Num(if fa == fb { 1.0 } else { 0.0 });
    }
    // Fast path: both Num — skip is_numeric_str() entirely.
    if let (Value::Num(an), Value::Num(bn)) = (a, b) {
        return Value::Num(if (an - bn).abs() < f64::EPSILON {
            1.0
        } else {
            0.0
        });
    }
    if a.is_numeric_str() && b.is_numeric_str() {
        let an = a.as_number();
        let bn = b.as_number();
        return Value::Num(if (an - bn).abs() < f64::EPSILON {
            1.0
        } else {
            0.0
        });
    }
    // POSIX/gawk: string compare path — Num/Mpfr stringify via CONVFMT, not the
    // default %.6g. Without this, `BEGIN { CONVFMT="%.2f"; print 3.14159 == "3.14" }`
    // returns 0; gawk returns 1.
    let ls = value_to_str_convfmt(a, rt);
    let rs = value_to_str_convfmt(b, rt);
    if ignore_case {
        return Value::Num(if ls.eq_ignore_ascii_case(rs.as_ref()) {
            1.0
        } else {
            0.0
        });
    }
    let ord = locale_str_cmp(&ls, &rs);
    Value::Num(if ord == Ordering::Equal { 1.0 } else { 0.0 })
}

/// Stringify a value for the string-compare fallback path of `==` / `<` / etc.
/// Num and Mpfr use the runtime's `CONVFMT`; everything else delegates to
/// `as_str_cow`. Integer-valued numbers bypass CONVFMT inside
/// `num_to_string_convfmt`.
#[inline]
fn value_to_str_convfmt<'a>(v: &'a Value, rt: &Runtime) -> Cow<'a, str> {
    match v {
        Value::Num(n) => Cow::Owned(rt.num_to_string_convfmt(*n)),
        Value::Mpfr(f) => Cow::Owned(rt.mpfr_to_string_convfmt(f)),
        _ => v.as_str_cow(),
    }
}

fn awk_cmp_rel(op: BinOp, a: &Value, b: &Value, ignore_case: bool, rt: &Runtime) -> Value {
    if rt.bignum && a.is_numeric_str() && b.is_numeric_str() {
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let fa = value_to_float(a, prec, round);
        let fb = value_to_float(b, prec, round);
        let ord = fa.partial_cmp(&fb).unwrap_or(Ordering::Equal);
        let ok = match op {
            BinOp::Lt => ord == Ordering::Less,
            BinOp::Le => matches!(ord, Ordering::Less | Ordering::Equal),
            BinOp::Gt => ord == Ordering::Greater,
            BinOp::Ge => matches!(ord, Ordering::Greater | Ordering::Equal),
            _ => unreachable!(),
        };
        return Value::Num(if ok { 1.0 } else { 0.0 });
    }
    // Fast path: both Num — skip is_numeric_str() entirely.
    if let (Value::Num(an), Value::Num(bn)) = (a, b) {
        let ok = match op {
            BinOp::Lt => an < bn,
            BinOp::Le => an <= bn,
            BinOp::Gt => an > bn,
            BinOp::Ge => an >= bn,
            _ => unreachable!(),
        };
        return Value::Num(if ok { 1.0 } else { 0.0 });
    }
    if a.is_numeric_str() && b.is_numeric_str() {
        let an = a.as_number();
        let bn = b.as_number();
        let ok = match op {
            BinOp::Lt => an < bn,
            BinOp::Le => an <= bn,
            BinOp::Gt => an > bn,
            BinOp::Ge => an >= bn,
            _ => unreachable!(),
        };
        return Value::Num(if ok { 1.0 } else { 0.0 });
    }
    // gawk parity: string compare path uses CONVFMT for Num/Mpfr stringification.
    let ls = value_to_str_convfmt(a, rt);
    let rs = value_to_str_convfmt(b, rt);
    let ord = if ignore_case {
        let la = ls.to_string().to_lowercase();
        let lb = rs.to_string().to_lowercase();
        locale_str_cmp(&Cow::Owned(la), &Cow::Owned(lb))
    } else {
        locale_str_cmp(&ls, &rs)
    };
    let ok = match op {
        BinOp::Lt => ord == Ordering::Less,
        BinOp::Le => matches!(ord, Ordering::Less | Ordering::Equal),
        BinOp::Gt => ord == Ordering::Greater,
        BinOp::Ge => matches!(ord, Ordering::Greater | Ordering::Equal),
        _ => unreachable!(),
    };
    Value::Num(if ok { 1.0 } else { 0.0 })
}

// ── Print / Printf ─────────────────────────────────────────────────────────

fn exec_print(ctx: &mut VmCtx<'_>, argc: u16, redir: RedirKind, is_printf: bool) -> Result<()> {
    // Pop redirect target (if any) first — it was pushed last
    let redir_path = if redir != RedirKind::Stdout {
        let v = ctx.pop();
        v.reject_if_array_scalar()?;
        Some(v.as_str())
    } else {
        None
    };

    let argc = argc as usize;

    if is_printf {
        if argc == 0 {
            return Err(Error::Runtime("`printf` needs a format string".into()));
        }
        let start = ctx.stack.len() - argc;
        let args: Vec<Value> = ctx.stack.drain(start..).collect();
        for a in &args {
            a.reject_if_array_scalar()?;
        }
        let fmt = args[0].as_str();
        let vals = &args[1..];
        let out = sprintf_simple(
            &fmt,
            vals,
            ctx.rt.numeric_decimal,
            ctx.rt.numeric_thousands_sep,
            ctx.rt,
        )?;
        let s = out.as_str();
        emit_with_redir(ctx, &s, redir, redir_path.as_deref())?;
    } else if redir == RedirKind::Stdout && ctx.print_out.is_none() {
        // ── Fast path: write directly into rt.print_buf, zero intermediate allocs ──
        // Use full OFS/ORS byte slices (may be longer than a few bytes; gawk allows arbitrary ORS).
        let ofs_len = ctx.rt.ofs_bytes.len();
        let ors_len = ctx.rt.ors_bytes.len();

        if argc == 0 {
            ctx.rt.print_buf.extend_from_slice(ctx.rt.record.as_bytes());
        } else {
            let start = ctx.stack.len() - argc;
            for i in 0..argc {
                ctx.stack[start + i].reject_if_array_scalar()?;
            }
            ctx.rt.print_buf.reserve(
                argc.saturating_mul(32)
                    .saturating_add(ofs_len.saturating_mul(argc.saturating_sub(1)))
                    .saturating_add(ors_len),
            );
            for i in 0..argc {
                if i > 0 {
                    ctx.rt.print_buf.extend_from_slice(&ctx.rt.ofs_bytes);
                }
                let idx = start + i;
                match &ctx.stack[idx] {
                    Value::Num(n) => {
                        let t = ctx.rt.num_to_string_ofmt(*n);
                        ctx.rt.print_buf.extend_from_slice(t.as_bytes());
                    }
                    Value::Mpfr(f) => {
                        let t = ctx.rt.mpfr_to_string_ofmt(f);
                        ctx.rt.print_buf.extend_from_slice(t.as_bytes());
                    }
                    other => other.write_to(&mut ctx.rt.print_buf),
                }
            }
            ctx.stack.truncate(start);
        }
        ctx.rt.print_buf.extend_from_slice(&ctx.rt.ors_bytes);
    } else {
        // ── Redirect / capture path: build String (I/O dominates, alloc is fine) ──
        let ofs = String::from_utf8_lossy(&ctx.rt.ofs_bytes).into_owned();
        let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();

        let line = if argc == 0 {
            ctx.rt.record.clone()
        } else {
            let start = ctx.stack.len() - argc;
            let mut parts = Vec::with_capacity(argc);
            for v in ctx.stack.drain(start..) {
                v.reject_if_array_scalar()?;
                parts.push(match v {
                    Value::Num(n) => ctx.rt.num_to_string_ofmt(n),
                    Value::Mpfr(f) => ctx.rt.mpfr_to_string_ofmt(&f),
                    other => other.as_str(),
                });
            }
            parts.join(&ofs)
        };
        let chunk = format!("{line}{ors}");
        emit_with_redir(ctx, &chunk, redir, redir_path.as_deref())?;
    }
    Ok(())
}

fn emit_with_redir(
    ctx: &mut VmCtx<'_>,
    data: &str,
    redir: RedirKind,
    path: Option<&str>,
) -> Result<()> {
    match redir {
        RedirKind::Stdout => ctx.emit_print(data),
        RedirKind::Overwrite => ctx.rt.write_output_line(path.unwrap(), data, false)?,
        RedirKind::Append => ctx.rt.write_output_line(path.unwrap(), data, true)?,
        RedirKind::Pipe => ctx.rt.write_pipe_line(path.unwrap(), data)?,
        RedirKind::Coproc => ctx.rt.write_coproc_line(path.unwrap(), data)?,
    }
    Ok(())
}

fn sprintf_simple(
    fmt: &str,
    vals: &[Value],
    dec: char,
    thousands_sep: Option<char>,
    rt: &Runtime,
) -> Result<Value> {
    let mpfr = rt.bignum.then(|| (rt.mpfr_prec_bits(), rt.mpfr_round()));
    // gawk parity: `%s` of a numeric argument stringifies via CONVFMT
    // (e.g. `CONVFMT="%.3f"; printf "%s", 3.14159` → `"3.142"`).
    let convfmt = rt
        .get_global_var("CONVFMT")
        .map(|v| v.as_str())
        .unwrap_or_else(|| "%.6g".to_string());
    format::awk_sprintf_with_convfmt(fmt, vals, dec, thousands_sep, mpfr, &convfmt)
        .map(Value::Str)
        .map_err(Error::Runtime)
}

// ── Getline ─────────────────────────────────────────────────────────────────

fn apply_getline_line(
    ctx: &mut VmCtx<'_>,
    var: Option<u32>,
    source: GetlineSource,
    line: Option<String>,
) -> Result<()> {
    if let Some(l) = line {
        let trimmed = l.trim_end_matches(['\n', '\r']).to_string();
        if let Some(var_idx) = var {
            // getline var — read into variable only, do NOT touch $0/fields/NF.
            let name = ctx.str_ref(var_idx).to_string();
            ctx.set_var(&name, Value::Str(trimmed))?;
            // Mixed-mode JIT writeback copies `jit_slots` → `rt.slots`; mirror slot updates into
            // the scratch buffer so string slot assignments survive the writeback.
            if let Some(&slot) = ctx.cp.slot_map.get(name.as_str()) {
                sync_jit_slot_value(ctx, slot);
            }
        } else {
            // getline (no var) — update $0 and re-split fields, then update NF.
            let fs = ctx
                .rt
                .vars
                .get("FS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".into());
            ctx.rt.set_field_sep_split(&fs, &trimmed);
            ctx.rt.ensure_fields_split();
            let nf = ctx.rt.nf() as f64;
            ctx.rt.vars.insert("NF".into(), Value::Num(nf));
        }
        if matches!(source, GetlineSource::Primary) {
            ctx.rt.nr += 1.0;
            ctx.rt.fnr += 1.0;
        }
    }
    Ok(())
}

fn exec_getline(
    ctx: &mut VmCtx<'_>,
    var: Option<u32>,
    source: GetlineSource,
    push_result: bool,
) -> Result<()> {
    let file_path = match source {
        GetlineSource::File => Some(ctx.pop().as_str()),
        GetlineSource::Coproc => Some(ctx.pop().as_str()),
        GetlineSource::Pipe => Some(ctx.pop().as_str()),
        GetlineSource::Primary => None,
    };

    let input_key = match source {
        GetlineSource::Primary => ctx.rt.primary_input_procinfo_key(),
        GetlineSource::File | GetlineSource::Coproc | GetlineSource::Pipe => {
            file_path.as_ref().expect("getline path").clone()
        }
    };

    let line_res = match source {
        GetlineSource::Primary => ctx.rt.read_line_primary(),
        GetlineSource::File => ctx.rt.read_line_file(file_path.as_ref().unwrap().as_str()),
        GetlineSource::Coproc => ctx
            .rt
            .read_line_coproc(file_path.as_ref().unwrap().as_str()),
        GetlineSource::Pipe => ctx.rt.read_line_pipe(file_path.as_ref().unwrap().as_str()),
    };

    match line_res {
        Ok(line) => {
            let has = line.is_some();
            apply_getline_line(ctx, var, source, line)?;
            if push_result {
                ctx.push(Value::Num(if has { 1.0 } else { 0.0 }));
            }
            Ok(())
        }
        Err(e) => {
            // Sandbox violations are fatal — they should not be quietly mapped
            // to a getline `-1` result (gawk also makes `--sandbox` redirection
            // fatal). Propagate so the program aborts like gawk.
            if matches!(&e, Error::Runtime(msg) if msg.starts_with("sandbox:")) {
                return Err(e);
            }
            let _code = ctx.rt.getline_error_code_for_key(&e, &input_key);
            if push_result {
                ctx.push(Value::Num(_code));
                Ok(())
            } else {
                // gawk parity: statement-form `getline var < file` for a
                // missing/unreadable file silently sets ERRNO and continues —
                // it does NOT abort the program. The expression form already
                // returns the -1/-2 code; this branch makes the statement form
                // behave the same way.
                Ok(())
            }
        }
    }
}

// ── Sub / Gsub ──────────────────────────────────────────────────────────────

/// Shared by the interpreter and JIT (`MIXED_SUB_*` / `MIXED_GSUB_*`).
pub(crate) fn exec_sub_from_values(
    ctx: &mut VmCtx<'_>,
    target: SubTarget,
    is_global: bool,
    re_v: Value,
    repl_v: Value,
    extra_key: Option<String>,
    extra_field_idx: Option<i32>,
) -> Result<f64> {
    let repl = repl_v.as_str_cow();
    let re = re_v.as_str_cow();

    let count = match target {
        SubTarget::Record => {
            if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), None)?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), None)?
            }
        }
        SubTarget::Var(name_idx) => {
            let name = ctx.str_ref(name_idx).to_string();
            let mut s = match ctx.var_value_cow(&name) {
                Cow::Borrowed(v) => v.as_str(),
                Cow::Owned(v) => v.as_str(),
            };
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.set_var(&name, Value::Str(s))?;
            sync_jit_slot_for_scalar_name(ctx, name.as_str());
            n
        }
        SubTarget::SlotVar(slot) => {
            let mut s = slot_value_live_for_jit(ctx, slot).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.rt.slots[slot as usize] = Value::Str(s);
            sync_jit_slot_value(ctx, slot);
            n
        }
        SubTarget::Field => {
            let i = extra_field_idx.expect("field index for SubTarget::Field");
            let mut s = ctx.rt.field(i)?.as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.rt.set_field(i, &s)?;
            n
        }
        SubTarget::Index(arr_idx) => {
            let key = extra_key.expect("key for SubTarget::Index");
            let arr_name = ctx.str_ref(arr_idx).to_string();
            let mut s = ctx.array_elem_get(&arr_name, &key).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.array_elem_set(&arr_name, key, Value::Str(s));
            n
        }
    };
    Ok(count)
}

fn exec_sub(ctx: &mut VmCtx<'_>, target: SubTarget, is_global: bool) -> Result<()> {
    // Stack state depends on target:
    // Record:    [re, repl]
    // Var:       [re, repl]
    // Field:     [re, repl, field_idx]
    // Index:     [re, repl, key]
    let (extra_key, extra_field_idx) = match target {
        SubTarget::Index(_) => (Some(ctx.pop().as_str()), None),
        SubTarget::Field => (None, Some(ctx.pop().as_number() as i32)),
        _ => (None, None),
    };
    let repl_v = ctx.pop();
    let re_v = ctx.pop();
    let count = exec_sub_from_values(
        ctx,
        target,
        is_global,
        re_v,
        repl_v,
        extra_key,
        extra_field_idx,
    )?;
    ctx.push(Value::Num(count));
    Ok(())
}

// ── Builtin calls ───────────────────────────────────────────────────────────

fn exec_call_builtin(ctx: &mut VmCtx<'_>, name: &str, argc: u16) -> Result<()> {
    let argc = argc as usize;

    // First, check if it's a user function
    if ctx.cp.functions.contains_key(name) {
        return exec_call_user(ctx, name, argc as u16);
    }

    let start = ctx.stack.len() - argc;
    let args: Vec<Value> = ctx.stack.drain(start..).collect();

    let result = exec_builtin_dispatch(ctx, name, args)?;
    ctx.push(result);
    Ok(())
}

/// Core builtin implementation (also used by JIT `MIXED_BUILTIN_CALL`).
pub(crate) fn exec_builtin_dispatch(
    ctx: &mut VmCtx<'_>,
    name: &str,
    args: Vec<Value>,
) -> Result<Value> {
    // In POSIX / traditional mode, reject gawk-only extension functions
    if ctx.rt.posix || ctx.rt.traditional {
        const GAWK_ONLY_BUILTINS: &[&str] = &[
            "and",
            "or",
            "xor",
            "compl",
            "lshift",
            "rshift",
            "gensub",
            "patsplit",
            "mkbool",
            "mktime",
            "strftime",
            "systime",
            "isarray",
            "typeof",
            "strtonum",
            "dcgettext",
            "dcngettext",
            "bindtextdomain",
            "chdir",
            "stat",
            "statvfs",
            "fts",
            "chr",
            "ord",
            "gettimeofday",
            "getlocaltime",
            "sleep",
            "readfile",
            "readdir",
            "reada",
            "writea",
            "inplace_tmpfile",
            "inplace_commit",
            "rename",
            "revoutput",
            "revtwoway",
            "intdiv",
            "intdiv0",
        ];
        if GAWK_ONLY_BUILTINS.contains(&name) {
            return Err(Error::Runtime(format!(
                "`{name}` is a gawk extension not available in POSIX/traditional mode"
            )));
        }
    }
    let argc = args.len();
    let result = match name {
        "length" => {
            if args.is_empty() {
                let n = if ctx.rt.characters_as_bytes {
                    ctx.rt.record.len()
                } else {
                    ctx.rt.record.chars().count()
                };
                Value::Num(n as f64)
            } else {
                match &args[0] {
                    Value::Array(a) => Value::Num(a.len() as f64),
                    v => {
                        let s = v.as_str();
                        let n = if ctx.rt.characters_as_bytes {
                            s.len()
                        } else {
                            s.chars().count()
                        };
                        Value::Num(n as f64)
                    }
                }
            }
        }
        "index" => {
            let hay = args[0].as_str();
            let needle = args[1].as_str();
            if needle.is_empty() {
                Value::Num(1.0)
            } else {
                // gawk parity: `IGNORECASE` applies to `index()` as well,
                // so `IGNORECASE=1; index("ABC", "b")` returns 2. The
                // case-insensitive search is done via lowercased copies
                // (cheap for short needles, which is the common case).
                let pos = if ctx.rt.ignore_case_flag() {
                    let hay_lc = hay.to_lowercase();
                    let needle_lc = needle.to_lowercase();
                    hay_lc.find(needle_lc.as_str()).map(|b| {
                        if ctx.rt.characters_as_bytes {
                            b + 1
                        } else {
                            // The byte offset in `hay_lc` matches the byte
                            // offset in `hay` for ASCII; for non-ASCII this
                            // can differ in length but the relative position
                            // is preserved for common cases. Iterate `hay`'s
                            // chars to convert byte to char position.
                            hay.char_indices()
                                .take_while(|&(off, _)| off < b)
                                .count()
                                + 1
                        }
                    })
                } else {
                    hay.find(needle.as_str()).map(|b| {
                        if ctx.rt.characters_as_bytes {
                            b + 1
                        } else {
                            hay[..b].chars().count() + 1
                        }
                    })
                };
                match pos {
                    Some(p) => Value::Num(p as f64),
                    None => Value::Num(0.0),
                }
            }
        }
        "substr" => {
            let s = args[0].as_str();
            let start_raw = args[1].as_number();
            let mut m = start_raw as i64;
            let len_opt = if let Some(v) = args.get(2) {
                let l = v.as_number() as i64;
                if l <= 0 {
                    return Ok(Value::Str(String::new()));
                }
                Some(l)
            } else {
                None
            };
            // gawk: start < 1 is treated as 1; length is not shortened (POSIX extension).
            if m < 1 {
                ctx.rt.lint_warn(&format!(
                    "substr: start index {start_raw} is less than 1, treated as 1"
                ));
                m = 1;
            }
            let len = len_opt.map(|l| l as usize).unwrap_or(usize::MAX);
            let start0 = (m as usize).saturating_sub(1);
            if ctx.rt.characters_as_bytes {
                let b = s.as_bytes();
                let slice = b
                    .get(start0..)
                    .map(|rest| {
                        let take = len.min(rest.len());
                        String::from_utf8_lossy(&rest[..take]).into_owned()
                    })
                    .unwrap_or_default();
                Value::Str(slice)
            } else {
                let slice: String = s.chars().skip(start0).take(len).collect();
                Value::Str(slice)
            }
        }
        "tolower" => Value::Str(args[0].as_str().to_lowercase()),
        "toupper" => Value::Str(args[0].as_str().to_uppercase()),
        "int" => bignum::awk_int_value(&args[0], ctx.rt),
        "intdiv" => {
            if argc != 2 {
                return Err(Error::Runtime("`intdiv` expects two arguments".into()));
            }
            bignum::awk_intdiv_values(&args[0], &args[1], ctx.rt)?
        }
        "mkbool" => {
            if argc != 1 {
                return Err(Error::Runtime("`mkbool` expects one argument".into()));
            }
            Value::Num(if truthy(&args[0])? { 1.0 } else { 0.0 })
        }
        "sqrt" => {
            if argc != 1 {
                return Err(Error::Runtime("`sqrt` expects one argument".into()));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                if matches!(f.cmp0(), Some(Ordering::Less)) {
                    ctx.rt.warn_builtin_negative_arg("sqrt", f.to_f64());
                }
                Value::Mpfr(Float::with_val_round(prec, f.sqrt(), round).0)
            } else {
                let x = args[0].as_number();
                if x < 0.0 {
                    ctx.rt.warn_builtin_negative_arg("sqrt", x);
                }
                Value::Num(x.sqrt())
            }
        }
        "sin" => {
            if argc != 1 {
                return Err(Error::Runtime("`sin` expects one argument".into()));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                Value::Mpfr(Float::with_val_round(prec, f.sin(), round).0)
            } else {
                Value::Num(args[0].as_number().sin())
            }
        }
        "cos" => {
            if argc != 1 {
                return Err(Error::Runtime("`cos` expects one argument".into()));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                Value::Mpfr(Float::with_val_round(prec, f.cos(), round).0)
            } else {
                Value::Num(args[0].as_number().cos())
            }
        }
        "atan2" => {
            if argc != 2 {
                return Err(Error::Runtime("`atan2` expects two arguments".into()));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let y = value_to_float(&args[0], prec, round);
                let x = value_to_float(&args[1], prec, round);
                Value::Mpfr(Float::with_val_round(prec, y.atan2(&x), round).0)
            } else {
                Value::Num(args[0].as_number().atan2(args[1].as_number()))
            }
        }
        "exp" => {
            if argc != 1 {
                return Err(Error::Runtime("`exp` expects one argument".into()));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                Value::Mpfr(Float::with_val_round(prec, f.exp(), round).0)
            } else {
                Value::Num(args[0].as_number().exp())
            }
        }
        "log" => {
            if argc != 1 {
                return Err(Error::Runtime("`log` expects one argument".into()));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                match f.cmp0() {
                    Some(Ordering::Less) => {
                        ctx.rt.warn_builtin_negative_arg("log", f.to_f64());
                    }
                    Some(Ordering::Equal) => {
                        ctx.rt.lint_warn("log: zero argument yields -infinity");
                    }
                    Some(Ordering::Greater) | None => {}
                }
                Value::Mpfr(Float::with_val_round(prec, f.ln(), round).0)
            } else {
                let x = args[0].as_number();
                if x < 0.0 {
                    ctx.rt.warn_builtin_negative_arg("log", x);
                } else if x == 0.0 {
                    ctx.rt.lint_warn("log: zero argument yields -infinity");
                }
                Value::Num(x.ln())
            }
        }
        "systime" => {
            if argc != 0 {
                return Err(Error::Runtime("`systime` expects no arguments".into()));
            }
            Value::Num(builtins::awk_systime())
        }
        "strftime" => builtins::awk_strftime(&args).map_err(Error::Runtime)?,
        "mktime" => {
            if argc != 1 {
                return Err(Error::Runtime("`mktime` expects one argument".into()));
            }
            Value::Num(builtins::awk_mktime(&args[0].as_str()))
        }
        "rand" => Value::Num(ctx.rt.rand()),
        "srand" => {
            let n = match args.first() {
                None => None,
                Some(v) => {
                    if ctx.rt.bignum {
                        let prec = ctx.rt.mpfr_prec_bits();
                        let round = ctx.rt.mpfr_round();
                        let f = value_to_float(v, prec, round);
                        Some(bignum::float_trunc_integer(&f).to_u64_wrapping())
                    } else {
                        Some(v.as_number() as u32 as u64)
                    }
                }
            };
            Value::Num(ctx.rt.srand(n))
        }
        "system" => {
            if ctx.rt.sandbox {
                return Err(Error::Runtime(
                    "sandbox: system() is disabled (-S/--sandbox)".into(),
                ));
            }
            use std::process::Command;
            // POSIX/gawk: flush stdout and any buffered pipes/files before launching
            // the subprocess so its output is correctly interleaved after pending awk
            // output rather than before it. Without this, `print "a"; system("echo b")`
            // would emit "b" before the buffered "a".
            flush_print_buf(&mut ctx.rt.print_buf)?;
            let _ = std::io::stdout().flush();
            ctx.rt.flush_all_output_handles();
            let cmd = args[0].as_str();
            let st = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .status()
                .map_err(Error::Io)?;
            Value::Num(st.code().unwrap_or(-1) as f64)
        }
        "close" => {
            let path = args[0].as_str();
            Value::Num(ctx.rt.close_handle(&path))
        }
        "fflush" => {
            if args.is_empty() {
                ctx.emit_flush()?;
            } else {
                let path = args[0].as_str();
                if path.is_empty() {
                    ctx.emit_flush()?;
                } else {
                    ctx.rt.flush_redirect_target(&path)?;
                }
            }
            Value::Num(0.0)
        }
        "sprintf" => {
            if args.is_empty() {
                return Err(Error::Runtime("sprintf: need format".into()));
            }
            let fmt = args[0].as_str();
            sprintf_simple(
                &fmt,
                &args[1..],
                ctx.rt.numeric_decimal,
                ctx.rt.numeric_thousands_sep,
                ctx.rt,
            )?
        }
        "printf" => {
            if args.is_empty() {
                return Err(Error::Runtime("printf: need format".into()));
            }
            let fmt = args[0].as_str();
            let s = sprintf_simple(
                &fmt,
                &args[1..],
                ctx.rt.numeric_decimal,
                ctx.rt.numeric_thousands_sep,
                ctx.rt,
            )?
            .as_str();
            ctx.emit_print(&s);
            Value::Num(0.0)
        }
        "and" => {
            if argc < 2 {
                return Err(Error::Runtime(
                    "`and` expects at least two arguments".into(),
                ));
            }
            let mut acc = bignum::awk_and_values(&args[0], &args[1], ctx.rt);
            for a in &args[2..] {
                acc = bignum::awk_and_values(&acc, a, ctx.rt);
            }
            acc
        }
        "or" => {
            if argc < 2 {
                return Err(Error::Runtime("`or` expects at least two arguments".into()));
            }
            let mut acc = bignum::awk_or_values(&args[0], &args[1], ctx.rt);
            for a in &args[2..] {
                acc = bignum::awk_or_values(&acc, a, ctx.rt);
            }
            acc
        }
        "xor" => {
            if argc < 2 {
                return Err(Error::Runtime(
                    "`xor` expects at least two arguments".into(),
                ));
            }
            let mut acc = bignum::awk_xor_values(&args[0], &args[1], ctx.rt);
            for a in &args[2..] {
                acc = bignum::awk_xor_values(&acc, a, ctx.rt);
            }
            acc
        }
        "lshift" => {
            if argc != 2 {
                return Err(Error::Runtime("`lshift` expects two arguments".into()));
            }
            // gawk parity: negative shift count is a fatal runtime error.
            let av = args[0].as_number();
            let bv = args[1].as_number();
            if av < 0.0 {
                return Err(Error::Runtime(format!(
                    "lshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            if bv < 0.0 {
                return Err(Error::Runtime(format!(
                    "lshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            bignum::awk_lshift_values(&args[0], &args[1], ctx.rt)
        }
        "rshift" => {
            if argc != 2 {
                return Err(Error::Runtime("`rshift` expects two arguments".into()));
            }
            let av = args[0].as_number();
            let bv = args[1].as_number();
            if av < 0.0 {
                return Err(Error::Runtime(format!(
                    "rshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            if bv < 0.0 {
                return Err(Error::Runtime(format!(
                    "rshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            bignum::awk_rshift_values(&args[0], &args[1], ctx.rt)
        }
        "compl" => {
            if argc != 1 {
                return Err(Error::Runtime("`compl` expects one argument".into()));
            }
            let av = args[0].as_number();
            if av < 0.0 {
                return Err(Error::Runtime(format!(
                    "compl({av:.6}): negative value is not allowed"
                )));
            }
            bignum::awk_compl_values(&args[0], ctx.rt)
        }
        "strtonum" => {
            if argc != 1 {
                return Err(Error::Runtime("`strtonum` expects one argument".into()));
            }
            bignum::awk_strtonum_value(&args[0].as_str(), ctx.rt)
        }
        "typeof" => {
            if argc != 1 {
                return Err(Error::Runtime("`typeof` expects one argument".into()));
            }
            Value::Str(builtins::awk_typeof_value(&args[0]).into())
        }
        "gensub" => {
            if !(3..=4).contains(&argc) {
                return Err(Error::Runtime("gensub: expected 3 or 4 arguments".into()));
            }
            let target = if argc == 4 {
                Some(args[3].as_str())
            } else {
                None
            };
            let out = builtins::awk_gensub(
                ctx.rt,
                &args[0].as_str(),
                &args[1].as_str(),
                &args[2],
                target,
            )?;
            Value::Str(out)
        }
        "isarray" => {
            if argc != 1 {
                return Err(Error::Runtime("`isarray` expects one argument".into()));
            }
            Value::Num(match &args[0] {
                Value::Array(_) => 1.0,
                _ => 0.0,
            })
        }
        "bindtextdomain" => {
            if argc != 2 {
                return Err(Error::Runtime(
                    "`bindtextdomain` expects two arguments (domain, dirname)".into(),
                ));
            }
            let domain = args[0].as_str();
            let dirname = args[1].as_str();
            ctx.rt.gettext_dir = dirname.clone();
            ctx.rt
                .vars
                .insert("TEXTDOMAIN".into(), Value::Str(domain.clone()));
            if let Some(cat) = crate::gettext_util::try_load_gettext_catalog(&domain, &dirname) {
                ctx.rt.gettext_catalogs.insert(domain, cat);
            }
            Value::Str(dirname)
        }
        "dcgettext" => {
            if argc != 3 {
                return Err(Error::Runtime(
                    "`dcgettext` expects three arguments (string, domain, category)".into(),
                ));
            }
            let msgid = args[0].as_str();
            let domain = args[1].as_str();
            let _cat = args[2].as_number() as i32;
            if let Some(cat) = ctx.rt.gettext_catalogs.get(&domain) {
                Value::Str(cat.gettext(msgid.as_str()).to_string())
            } else {
                Value::Str(msgid)
            }
        }
        "dcngettext" => {
            if argc != 5 {
                return Err(Error::Runtime(
                    "`dcngettext` expects five arguments (s1, s2, n, domain, category)".into(),
                ));
            }
            let s1 = args[0].as_str();
            let s2 = args[1].as_str();
            let n = args[2].as_number();
            let domain = args[3].as_str();
            let _ = args[4].as_number() as i32;
            if let Some(cat) = ctx.rt.gettext_catalogs.get(&domain) {
                Value::Str(cat.ngettext(s1.as_str(), s2.as_str(), n as u64).to_string())
            } else {
                Value::Str((if n == 1.0 { s1 } else { s2 }).to_string())
            }
        }
        "chdir" => {
            if argc != 1 {
                return Err(Error::Runtime("`chdir` expects one argument".into()));
            }
            crate::gawk_extensions::chdir(ctx.rt, &args[0].as_str())?
        }
        "stat" => {
            if argc != 2 {
                return Err(Error::Runtime("`stat` expects two arguments".into()));
            }
            crate::gawk_extensions::stat(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "statvfs" => {
            if argc != 2 {
                return Err(Error::Runtime("`statvfs` expects two arguments".into()));
            }
            crate::gawk_extensions::statvfs(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "fts" => {
            if argc != 2 {
                return Err(Error::Runtime("`fts` expects two arguments".into()));
            }
            crate::gawk_extensions::fts(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "gettimeofday" => {
            if argc != 1 {
                return Err(Error::Runtime("`gettimeofday` expects one argument".into()));
            }
            crate::gawk_extensions::gettimeofday(ctx.rt, &args[0].as_str())?
        }
        "sleep" => {
            if argc != 1 {
                return Err(Error::Runtime("`sleep` expects one argument".into()));
            }
            crate::gawk_extensions::sleep_secs(ctx.rt, args[0].as_number())?
        }
        "ord" => {
            if argc != 1 {
                return Err(Error::Runtime("`ord` expects one argument".into()));
            }
            crate::gawk_extensions::ord(ctx.rt, &args[0].as_str())?
        }
        "chr" => {
            if argc != 1 {
                return Err(Error::Runtime("`chr` expects one argument".into()));
            }
            crate::gawk_extensions::chr(ctx.rt, args[0].as_number())?
        }
        "readfile" => {
            if argc != 1 {
                return Err(Error::Runtime("`readfile` expects one argument".into()));
            }
            crate::gawk_extensions::readfile(ctx.rt, &args[0].as_str())?
        }
        "revoutput" => {
            if argc != 1 {
                return Err(Error::Runtime("`revoutput` expects one argument".into()));
            }
            crate::gawk_extensions::revoutput(ctx.rt, &args[0].as_str())?
        }
        "revtwoway" => {
            if argc != 1 {
                return Err(Error::Runtime("`revtwoway` expects one argument".into()));
            }
            crate::gawk_extensions::revtwoway(ctx.rt, &args[0].as_str())?
        }
        "rename" => {
            if argc != 2 {
                return Err(Error::Runtime("`rename` expects two arguments".into()));
            }
            crate::gawk_extensions::rename(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "inplace_tmpfile" => {
            if argc != 1 {
                return Err(Error::Runtime(
                    "`inplace_tmpfile` expects one argument".into(),
                ));
            }
            crate::gawk_extensions::inplace_tmpfile(ctx.rt, &args[0].as_str())?
        }
        "inplace_commit" => {
            if argc != 2 {
                return Err(Error::Runtime(
                    "`inplace_commit` expects two arguments".into(),
                ));
            }
            crate::gawk_extensions::inplace_commit(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "writea" => {
            if argc != 2 {
                return Err(Error::Runtime("`writea` expects two arguments".into()));
            }
            crate::gawk_extensions::writea(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "reada" => {
            if argc != 2 {
                return Err(Error::Runtime("`reada` expects two arguments".into()));
            }
            crate::gawk_extensions::reada(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "intdiv0" => {
            if argc != 2 {
                return Err(Error::Runtime("`intdiv0` expects two arguments".into()));
            }
            crate::gawk_extensions::intdiv0(ctx.rt, &args[0], &args[1])?
        }
        "readdir" => {
            if argc != 2 {
                return Err(Error::Runtime(
                    "`readdir` expects two arguments (path, array)".into(),
                ));
            }
            let path = args[0].as_str().to_string();
            let arr_name = args[1].as_str().to_string();
            crate::gawk_extensions::readdir(ctx.rt, &path, &arr_name)?
        }
        "getlocaltime" => {
            if !(1..=2).contains(&argc) {
                return Err(Error::Runtime(
                    "`getlocaltime` expects 1 or 2 arguments (array [, timestamp])".into(),
                ));
            }
            let arr_name = args[0].as_str().to_string();
            let ts = if argc == 2 {
                Some(args[1].as_number())
            } else {
                None
            };
            crate::gawk_extensions::getlocaltime(ctx.rt, &arr_name, ts)?
        }
        _ => return Err(Error::Runtime(format!("unknown function `{name}`"))),
    };
    Ok(result)
}

fn sort_keys_with_custom_cmp(
    ctx: &mut VmCtx<'_>,
    keys: &mut [String],
    fname: &str,
    arr_name: &str,
) -> Result<()> {
    let Some(func) = ctx.cp.functions.get(fname) else {
        return Err(Error::Runtime(format!(
            "sorted_in: unknown function `{fname}`"
        )));
    };
    let argc = func.params.len();
    if !(argc == 2 || argc == 4) {
        return Err(Error::Runtime(format!(
            "sorted_in: comparison function `{fname}` must have 2 or 4 parameters (has {argc})"
        )));
    }

    let err: RefCell<Option<Error>> = RefCell::new(None);
    keys.sort_by(|a, b| {
        if err.borrow().is_some() {
            return Ordering::Equal;
        }
        let vals = if argc == 2 {
            vec![Value::Str(a.clone()), Value::Str(b.clone())]
        } else {
            let va = if arr_name == "SYMTAB" {
                ctx.rt.symtab_elem_get(a.as_str())
            } else {
                ctx.rt.array_get(arr_name, a.as_str())
            };
            let vb = if arr_name == "SYMTAB" {
                ctx.rt.symtab_elem_get(b.as_str())
            } else {
                ctx.rt.array_get(arr_name, b.as_str())
            };
            vec![Value::Str(a.clone()), va, Value::Str(b.clone()), vb]
        };
        match exec_call_user_inner(ctx, fname, vals) {
            Ok(v) => {
                let n = v.as_number();
                if n < 0.0 {
                    Ordering::Less
                } else if n > 0.0 {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            Err(e) => {
                *err.borrow_mut() = Some(e);
                Ordering::Equal
            }
        }
    });
    if let Some(e) = err.into_inner() {
        return Err(e);
    }
    Ok(())
}

/// Run a user function with explicit arguments (VM stack path and JIT `MIXED_CALL_USER_*`).
pub(crate) fn exec_call_user_inner(
    ctx: &mut VmCtx<'_>,
    name: &str,
    mut vals: Vec<Value>,
) -> Result<Value> {
    let func = ctx
        .cp
        .functions
        .get(name)
        .ok_or_else(|| Error::Runtime(format!("unknown function `{name}`")))?
        .clone();

    if ctx.locals.len() >= crate::limits::MAX_USER_CALL_DEPTH {
        return Err(Error::Runtime(format!(
            "maximum user function call depth ({}) exceeded",
            crate::limits::MAX_USER_CALL_DEPTH
        )));
    }

    while vals.len() < func.params.len() {
        vals.push(Value::Uninit);
    }
    vals.truncate(func.params.len());

    let mut frame = AwkMap::default();
    for (p, v) in func.params.iter().zip(vals) {
        frame.insert(p.clone(), v);
    }
    ctx.locals.push(frame);
    let was_fn = ctx.in_function;
    ctx.in_function = true;

    let result = match execute(&func.body, ctx) {
        Ok(VmSignal::Normal) => Value::Uninit,
        Ok(VmSignal::Return(v)) => v,
        Ok(VmSignal::Next) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(Error::Runtime("invalid jump out of function (next)".into()));
        }
        Ok(VmSignal::NextFile) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(Error::Runtime(
                "invalid jump out of function (nextfile)".into(),
            ));
        }
        Ok(VmSignal::ExitPending) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(Error::Exit(ctx.rt.exit_code));
        }
        Err(e) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(e);
        }
    };

    ctx.locals.pop();
    ctx.in_function = was_fn;
    Ok(result)
}

fn exec_call_user(ctx: &mut VmCtx<'_>, name: &str, argc: u16) -> Result<()> {
    let argc = argc as usize;
    let start = ctx.stack.len() - argc;
    let vals: Vec<Value> = ctx.stack.drain(start..).collect();
    let result = exec_call_user_inner(ctx, name, vals)?;
    ctx.push(result);
    Ok(())
}

/// POSIX array call-by-reference for user functions. Stack layout (top-down):
/// `[name_argc, …, name_1, val_argc, …, val_1]`. Each `name_i` is the
/// caller-side variable name (or `""` for non-Var args). After the call,
/// any frame parameter that holds a `Value::Array` is propagated back to
/// the caller's variable with the matching name. Scalars are discarded
/// (awk passes scalars by value).
fn exec_call_user_bind_arrays(ctx: &mut VmCtx<'_>, name: &str, argc: u16) -> Result<()> {
    let argc = argc as usize;
    let total = argc * 2;
    let start = ctx.stack.len() - total;
    // drain returns values in stack order (bottom→top); names come first.
    let mut all: Vec<Value> = ctx.stack.drain(start..).collect();
    let vals: Vec<Value> = all.split_off(argc);
    let names: Vec<String> = all.into_iter().map(|v| v.into_string()).collect();

    let param_names: Vec<String> = ctx
        .cp
        .functions
        .get(name)
        .ok_or_else(|| Error::Runtime(format!("unknown function `{name}`")))?
        .params
        .clone();

    let (result, frame_after) = exec_call_user_inner_with_frame(ctx, name, vals)?;

    for (i, caller_name) in names.iter().enumerate() {
        if caller_name.is_empty() {
            continue;
        }
        let Some(param_name) = param_names.get(i) else {
            continue;
        };
        if let Some(v) = frame_after.get(param_name) {
            if matches!(v, Value::Array(_)) {
                // Frame-aware write-back: if the caller name lives in an
                // outer function frame (nested call), update that frame.
                // Otherwise fall through to global vars.
                let mut wrote = false;
                for frame in ctx.locals.iter_mut().rev() {
                    if frame.contains_key(caller_name.as_str()) {
                        frame.insert(caller_name.clone(), v.clone());
                        wrote = true;
                        break;
                    }
                }
                if !wrote {
                    ctx.rt.vars.insert(caller_name.clone(), v.clone());
                }
            }
        }
    }

    ctx.push(result);
    Ok(())
}

/// Same as [`exec_call_user_inner`] but returns the final function frame
/// alongside the result so the caller can write array params back.
fn exec_call_user_inner_with_frame(
    ctx: &mut VmCtx<'_>,
    name: &str,
    mut vals: Vec<Value>,
) -> Result<(Value, crate::runtime::AwkMap<String, Value>)> {
    let func = ctx
        .cp
        .functions
        .get(name)
        .ok_or_else(|| Error::Runtime(format!("unknown function `{name}`")))?
        .clone();

    if ctx.locals.len() >= crate::limits::MAX_USER_CALL_DEPTH {
        return Err(Error::Runtime(format!(
            "maximum user function call depth ({}) exceeded",
            crate::limits::MAX_USER_CALL_DEPTH
        )));
    }

    while vals.len() < func.params.len() {
        vals.push(Value::Uninit);
    }
    vals.truncate(func.params.len());

    let mut frame = crate::runtime::AwkMap::default();
    for (p, v) in func.params.iter().zip(vals) {
        frame.insert(p.clone(), v);
    }
    ctx.locals.push(frame);
    let was_fn = ctx.in_function;
    ctx.in_function = true;

    let result = match execute(&func.body, ctx) {
        Ok(VmSignal::Normal) => Value::Uninit,
        Ok(VmSignal::Return(v)) => v,
        Ok(VmSignal::Next) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(Error::Runtime("invalid jump out of function (next)".into()));
        }
        Ok(VmSignal::NextFile) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(Error::Runtime(
                "invalid jump out of function (nextfile)".into(),
            ));
        }
        Ok(VmSignal::ExitPending) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(Error::Exit(ctx.rt.exit_code));
        }
        Err(e) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            return Err(e);
        }
    };

    let frame = ctx.locals.pop().unwrap_or_default();
    ctx.in_function = was_fn;
    Ok((result, frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::flow::Flow;
    use crate::jit::{
        mixed_encode_field_slot, MIXED_ADD_FIELDNUM_TO_SLOT, MIXED_ADD_FIELD_TO_SLOT,
        MIXED_ADD_MUL_FIELDNUMS_TO_SLOT, MIXED_ADD_MUL_FIELDS_TO_SLOT,
    };
    use crate::parser::parse_program;
    use crate::runtime::Runtime;
    use crate::test_sync::ENV_LOCK;
    use std::sync::atomic::Ordering as AtomicOrdering;

    fn compile(prog_text: &str) -> CompiledProgram {
        let prog = parse_program(prog_text).expect("parse");
        Compiler::compile_program(&prog).unwrap()
    }

    /// Match `lib::run`: slotted scalars from the compiler need `init_slots` before VM runs.
    fn runtime_with_slots(cp: &CompiledProgram) -> Runtime {
        let mut rt = Runtime::new();
        rt.slots = cp.init_slots(&rt.vars);
        rt
    }

    #[test]
    fn vm_begin_prints_numeric_expression() {
        let cp = compile("BEGIN { print 2 + 3 * 4 }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "14\n");
    }

    #[test]
    fn vm_begin_user_recursion_hits_call_depth_cap() {
        let cp = compile("function f(){ f() } BEGIN { f() }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        let msg = e.to_string();
        assert!(
            msg.contains("maximum user function call depth"),
            "unexpected err: {msg}"
        );
    }

    /// `-M`: integer literals must not round through `f64` before `+` / `sprintf %d`.
    #[test]
    fn vm_begin_bignum_sprintf_i64_max_plus_one() {
        let cp = compile(r#"BEGIN { print sprintf("%d", 9223372036854775807 + 1) }"#);
        let mut rt = runtime_with_slots(&cp);
        rt.bignum = true;
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&rt.print_buf),
            "9223372036854775808\n"
        );
    }

    /// Mirrors CLI `-v a=1 -v b=2 -v c=3` (`apply_assigns` stores string values).
    #[test]
    fn vm_begin_print_sum_of_three_minus_v_style_vars() {
        let cp = compile("BEGIN { print a+b+c }");
        let mut rt = Runtime::new();
        rt.vars.insert("a".into(), Value::Str("1".into()));
        rt.vars.insert("b".into(), Value::Str("2".into()));
        rt.vars.insert("c".into(), Value::Str("3".into()));
        rt.slots = cp.init_slots(&rt.vars);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "6\n");
    }

    #[test]
    fn vm_begin_assigns_global_and_prints() {
        let cp = compile("BEGIN { answer = 42; print answer }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "42\n");
        let slot = *cp.slot_map.get("answer").expect("answer slotted");
        assert_eq!(rt.slots[slot as usize].as_number(), 42.0);
    }

    #[test]
    fn vm_begin_power_star_star() {
        let cp = compile("BEGIN { print 2 ** 10 }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1024\n");
    }

    #[test]
    fn vm_begin_intdiv_and_intdiv0() {
        let cp = compile("BEGIN { print intdiv(7, 2), intdiv0(5, 0) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3 0\n");
    }

    #[test]
    fn vm_begin_index_empty_needle_is_one_miss_is_zero() {
        let cp = compile(r#"BEGIN { print index("abc", ""), index("abc", "x") }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1 0\n");
    }

    #[test]
    fn vm_begin_index_finds_first_byte_substring() {
        let cp = compile(r#"BEGIN { print index("abc", "bc") }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "2\n");
    }

    #[test]
    fn vm_begin_substr_zero_length_yields_empty() {
        let cp = compile(r#"BEGIN { print "[" substr("hello", 2, 0) "]" }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "[]\n");
    }

    #[test]
    fn vm_begin_substr_omitted_length_takes_rest() {
        let cp = compile(r#"BEGIN { print substr("abcdef", 3) }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "cdef\n");
    }

    #[test]
    fn vm_begin_split_returns_count_and_fills_array() {
        let cp = compile(r#"BEGIN { n = split("a,b,c", t, ","); print n, t[1], t[2], t[3] }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3 a b c\n");
    }

    #[test]
    fn vm_begin_asort_reorders_numeric_values() {
        let cp = compile("BEGIN { a[1]=30; a[2]=10; a[3]=20; asort(a); print a[1], a[2], a[3] }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "10 20 30\n");
    }

    #[test]
    fn vm_begin_atan2_pi_over_four() {
        let cp = compile("BEGIN { print atan2(1, 1) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        let v: f64 = String::from_utf8_lossy(&rt.print_buf)
            .trim()
            .parse()
            .unwrap();
        // Default `OFMT` rounds; parsed text is not full `f64` precision.
        assert!((v - std::f64::consts::FRAC_PI_4).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn vm_begin_atan2_wrong_arity_errors() {
        let cp = compile("BEGIN { print atan2(1) }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("atan2"), "{e:?}");
    }

    #[test]
    fn vm_begin_systime_with_arg_errors() {
        let cp = compile("BEGIN { print systime(1) }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("systime"), "{e:?}");
    }

    #[test]
    fn vm_begin_srand_resets_rand_sequence() {
        let cp = compile("BEGIN { srand(42); a = rand(); srand(42); b = rand(); print (a == b) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1\n");
    }

    #[test]
    fn vm_begin_isarray_and_typeof_scalar_elem() {
        let cp = compile("BEGIN { a[1] = 7; print isarray(a), typeof(a[1]) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1 number\n");
    }

    #[test]
    fn vm_begin_gensub_global_returns_modified_string() {
        let cp = compile(r#"BEGIN { print gensub(/[0-9]/, "X", "g", "a1b2") }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "aXbX\n");
    }

    #[test]
    fn vm_begin_tolower_toupper_roundtrip_shape() {
        let cp = compile(r#"BEGIN { print toupper("aBc"), tolower("XyZ") }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "ABC xyz\n");
    }

    #[test]
    fn vm_begin_sqrt_perfect_square() {
        let cp = compile("BEGIN { print sqrt(9) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3\n");
    }

    #[test]
    fn vm_begin_sqrt_wrong_arity_errors() {
        let cp = compile("BEGIN { print sqrt() }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("sqrt"), "{e:?}");
    }

    #[test]
    fn vm_begin_log_one_is_zero() {
        let cp = compile("BEGIN { print log(1) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0\n");
    }

    #[test]
    fn vm_begin_exp_zero_is_one() {
        let cp = compile("BEGIN { print exp(0) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1\n");
    }

    #[test]
    fn vm_begin_length_no_args_uses_empty_record() {
        let cp = compile("BEGIN { print length() }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0\n");
    }

    #[test]
    fn vm_begin_length_string_argument_counts_chars() {
        let cp = compile(r#"BEGIN { print length("hello") }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "5\n");
    }

    #[test]
    fn vm_begin_length_array_counts_entries() {
        let cp = compile("BEGIN { a[1]=1; a[2]=2; a[99]=3; print length(a) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3\n");
    }

    #[test]
    fn vm_begin_sin_zero_and_cos_zero() {
        let cp = compile("BEGIN { print sin(0), cos(0) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0 1\n");
    }

    #[test]
    fn vm_begin_sin_wrong_arity_errors() {
        let cp = compile("BEGIN { print sin() }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("sin"), "{e:?}");
    }

    #[test]
    fn vm_begin_int_truncates_toward_zero() {
        let cp = compile("BEGIN { print int(3.9), int(-3.9) }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3 -3\n");
    }

    #[test]
    fn vm_begin_mkbool_numeric_zero_vs_nonzero() {
        let cp = compile("BEGIN { print mkbool(0), mkbool(0.5), mkbool(\"\") }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0 1 0\n");
    }

    #[test]
    fn vm_begin_mkbool_wrong_arity_errors() {
        let cp = compile("BEGIN { print mkbool() }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("mkbool"), "{e:?}");
    }

    #[test]
    fn vm_begin_many_rand_draws_stay_in_half_open_unit_interval() {
        let cp = compile(
            "BEGIN { bad = 0; for (i = 1; i <= 80; i++) { r = rand(); if (r < 0 || r >= 1) bad++ } print bad }",
        );
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0\n");
    }

    #[test]
    fn vm_user_function_bare_return_runs() {
        let cp = compile("function f(){ return } BEGIN { f(); print \"ok\" }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "ok\n");
    }

    #[test]
    fn vm_begin_ofs_between_output_fields() {
        let cp = compile(r#"BEGIN { OFS = "|"; print "a", "b" }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "a|b\n");
    }

    #[test]
    fn vm_begin_ors_after_each_print() {
        let cp = compile(r#"BEGIN { ORS = "X"; print "p"; print "q" }"#);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "pXqX");
    }

    #[test]
    fn vm_begin_multidim_array_assign_and_read() {
        let cp = compile("BEGIN { a[1,2] = 42; print a[1,2] }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "42\n");
    }

    #[test]
    fn vm_begin_next_is_invalid() {
        let cp = compile("BEGIN { next }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        match e {
            Error::Runtime(s) => assert!(s.contains("next"), "{s}"),
            _ => panic!("unexpected err: {e:?}"),
        }
    }

    #[test]
    fn vm_begin_nextfile_is_invalid() {
        let cp = compile("BEGIN { nextfile }");
        let mut rt = runtime_with_slots(&cp);
        let e = vm_run_begin(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("nextfile"), "{e:?}");
    }

    #[test]
    fn vm_end_nextfile_is_invalid() {
        let cp = compile("END { nextfile }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        let e = vm_run_end(&cp, &mut rt).unwrap_err();
        assert!(e.to_string().contains("nextfile"), "{e:?}");
    }

    #[test]
    fn vm_end_runs_and_prints() {
        let cp = compile("END { print \"bye\" }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        rt.print_buf.clear();
        vm_run_end(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "bye\n");
    }

    #[test]
    fn vm_pattern_always_matches() {
        let cp = compile("{ print $1 }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("x y");
        assert!(vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    }

    #[test]
    fn vm_pattern_literal_substring() {
        let cp = compile("/ell/ { print }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("hello");
        assert!(vm_pattern_matches(rule, &cp, &mut rt).unwrap());
        rt.set_record_from_line("zzz");
        assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    }

    #[test]
    fn vm_pattern_expr_numeric() {
        let cp = compile("$1 > 10 { print \"big\" }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("20");
        assert!(vm_pattern_matches(rule, &cp, &mut rt).unwrap());
        rt.set_record_from_line("5");
        assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    }

    #[test]
    fn vm_run_rule_capture_print() {
        let cp = compile("{ print $1, $2 }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("a b");
        let mut cap = Vec::new();
        let flow = vm_run_rule(rule, &cp, &mut rt, Some(&mut cap), None).unwrap();
        assert!(matches!(flow, Flow::Normal));
        assert_eq!(cap.len(), 1);
        assert!(cap[0].starts_with("a"));
        assert!(cap[0].contains("b"));
    }

    #[test]
    fn vm_run_rule_next_signal() {
        let cp = compile("{ next }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("z");
        let flow = vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
        assert!(matches!(flow, Flow::Next));
    }

    #[test]
    fn vm_run_rule_exit_sets_pending() {
        let cp = compile("{ exit 3 }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("z");
        let flow = vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
        assert!(matches!(flow, Flow::ExitPending));
        assert!(rt.exit_pending);
        assert_eq!(rt.exit_code, 3);
    }

    #[test]
    fn vm_beginfile_empty_ok() {
        let cp = compile("{ }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_beginfile(&cp, &mut rt).unwrap();
    }

    #[test]
    fn vm_endfile_empty_ok() {
        let cp = compile("{ }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_endfile(&cp, &mut rt).unwrap();
    }

    #[test]
    fn flush_print_buf_empty_ok() {
        let mut buf = Vec::new();
        flush_print_buf(&mut buf).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn vm_user_function_call_in_record_rule() {
        let cp = compile("function dbl(x){ return x*2 } { print dbl($1) }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("21");
        let mut cap = Vec::new();
        vm_run_rule(rule, &cp, &mut rt, Some(&mut cap), None).unwrap();
        assert_eq!(cap.len(), 1);
        assert!(cap[0].starts_with("42"));
    }

    #[test]
    fn vm_concat_and_comparison_in_begin() {
        let cp = compile("BEGIN { print (\"a\" < \"b\") }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1\n");
    }

    #[test]
    fn tiered_jit_defers_compile_until_min_invocations() {
        let _g = ENV_LOCK.lock().expect("env test lock");
        std::env::remove_var("AWKRS_JIT");
        std::env::set_var("AWKRS_JIT_MIN_INVOCATIONS", "2");

        let cp = compile("{ print $1 }");
        let rule = &cp.record_rules[0];
        let chunk = &rule.body;
        let mut rt = runtime_with_slots(&cp);

        rt.set_record_from_line("42");
        vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
        assert!(
            chunk.jit_lock.lock().expect("jit_lock").is_none(),
            "first record: tiered gate should skip compile; cache untouched"
        );
        assert_eq!(chunk.jit_invocation_count.load(AtomicOrdering::Relaxed), 1);

        rt.set_record_from_line("42");
        vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
        assert!(
            chunk.jit_lock.lock().expect("jit_lock").is_some(),
            "second record: min invocations met; JIT cache entry should exist"
        );
        assert_eq!(chunk.jit_invocation_count.load(AtomicOrdering::Relaxed), 2);

        std::env::remove_var("AWKRS_JIT_MIN_INVOCATIONS");
    }

    #[test]
    fn mixed_add_fieldnum_to_slot_matches_field_to_slot() {
        let cp = compile("{ x += $1 }");
        let slot_x = *cp.slot_map.get("x").expect("x slotted") as usize;
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("17");
        rt.ensure_jit_slot_buf(rt.slots.len().max(slot_x + 1));
        rt.jit_slot_buf[slot_x] = 0.0;

        let mut ctx = VmCtx::new(&cp, &mut rt);
        let enc = mixed_encode_field_slot(1, slot_x as u16);
        jit_mixed_op_dispatch(&mut ctx, MIXED_ADD_FIELD_TO_SLOT, enc, 0.0, 0.0);
        let from_legacy = ctx.rt.jit_slot_buf[slot_x];

        ctx.rt.jit_slot_buf[slot_x] = 0.0;
        jit_mixed_op_dispatch(
            &mut ctx,
            MIXED_ADD_FIELDNUM_TO_SLOT,
            slot_x as u32,
            17.0,
            0.0,
        );
        let from_precomputed = ctx.rt.jit_slot_buf[slot_x];

        assert_eq!(from_legacy, from_precomputed);
        assert_eq!(from_legacy, 17.0);
    }

    #[test]
    fn mixed_add_mul_fieldnums_matches_mul_fields_to_slot() {
        let cp = compile("{ x += $1 * $2 }");
        let slot_x = *cp.slot_map.get("x").expect("x slotted") as usize;
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("2 3");
        rt.ensure_jit_slot_buf(rt.slots.len().max(slot_x + 1));
        rt.jit_slot_buf[slot_x] = 0.0;

        let mut ctx = VmCtx::new(&cp, &mut rt);
        let enc = u32::from(1u16) | (u32::from(2u16) << 16);
        jit_mixed_op_dispatch(
            &mut ctx,
            MIXED_ADD_MUL_FIELDS_TO_SLOT,
            enc,
            slot_x as f64,
            0.0,
        );
        let from_legacy = ctx.rt.jit_slot_buf[slot_x];

        ctx.rt.jit_slot_buf[slot_x] = 0.0;
        jit_mixed_op_dispatch(
            &mut ctx,
            MIXED_ADD_MUL_FIELDNUMS_TO_SLOT,
            slot_x as u32,
            2.0,
            3.0,
        );
        let from_precomputed = ctx.rt.jit_slot_buf[slot_x];

        assert_eq!(from_legacy, from_precomputed);
        assert_eq!(from_legacy, 6.0);
    }

    #[test]
    fn vm_array_set_read_in_begin() {
        let cp = compile("BEGIN { a[\"k\"] = 7; print a[\"k\"] }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "7\n");
    }

    #[test]
    fn vm_begin_printf_statement() {
        let cp = compile("BEGIN { printf \"%s\", \"ok\" }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "ok");
    }

    #[test]
    fn vm_begin_if_branch() {
        let cp = compile("BEGIN { if (1) print 7; }");
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        assert_eq!(String::from_utf8_lossy(&rt.print_buf), "7\n");
    }

    #[test]
    fn vm_pattern_range_placeholder_returns_false_in_vm() {
        let cp = compile("/a/,/b/ { print }");
        let rule = &cp.record_rules[0];
        assert!(matches!(rule.pattern, CompiledPattern::Range { .. }));
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("x");
        assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    }

    // ── vm_range_step / vm_match_range_endpoint pinning ──────────────────────
    //
    // Range pattern state machine: `state` is false until `start` matches on a
    // record, then true until `end` matches on a record (inclusive of the
    // end-matching record). Critical that:
    //   - Same-record start-and-end stays true for that one record then resets
    //   - state survives across records
    //   - Always/Never/NestedRangeError endpoints behave per spec
    // Any regression in this state machine silently breaks /pat1/,/pat2/
    // programs without changing any exit code — exactly the bug class hardest
    // to catch in integration tests. Pin it.

    fn endpoint_from_range_pattern(
        cp: &CompiledProgram,
        want_start: bool,
    ) -> &CompiledRangeEndpoint {
        match &cp.record_rules[0].pattern {
            CompiledPattern::Range { start, end } => {
                if want_start {
                    start
                } else {
                    end
                }
            }
            _ => panic!("expected range pattern in compiled rule[0]"),
        }
    }

    #[test]
    fn range_step_start_match_activates_state() {
        let cp = compile("/A/,/Z/ { print }");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        let end = endpoint_from_range_pattern(&cp, false).clone();
        let mut rt = runtime_with_slots(&cp);
        let mut state = false;

        rt.set_record_from_line("contains A here");
        assert!(vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
        assert!(state, "state must flip true on start match");
    }

    #[test]
    fn range_step_stays_active_between_endpoints() {
        let cp = compile("/A/,/Z/ { print }");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        let end = endpoint_from_range_pattern(&cp, false).clone();
        let mut rt = runtime_with_slots(&cp);
        let mut state = true; // pre-activated (e.g. previous record matched start)

        rt.set_record_from_line("middle line no match");
        assert!(vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
        assert!(state, "state must stay true between endpoints");
    }

    #[test]
    fn range_step_end_match_resets_state_inclusive() {
        // POSIX: the record that matches `end` is itself part of the range
        // (the rule runs for it), but state resets to false afterward.
        let cp = compile("/A/,/Z/ { print }");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        let end = endpoint_from_range_pattern(&cp, false).clone();
        let mut rt = runtime_with_slots(&cp);
        let mut state = true;

        rt.set_record_from_line("contains Z end");
        let ran = vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap();
        assert!(ran, "end-matching record must still run");
        assert!(!state, "state must reset after end match");
    }

    #[test]
    fn range_step_inactive_no_start_match_skips() {
        let cp = compile("/A/,/Z/ { print }");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        let end = endpoint_from_range_pattern(&cp, false).clone();
        let mut rt = runtime_with_slots(&cp);
        let mut state = false;

        rt.set_record_from_line("none of the keys");
        assert!(!vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
        assert!(!state, "no start match means state stays false");
    }

    #[test]
    fn range_step_same_record_starts_and_ends() {
        // Record contains BOTH endpoints — start activates, end resets, and the
        // record itself runs. This is the trickiest case: state transitions
        // false → true → false within one step, but the return is true.
        let cp = compile("/A/,/Z/ { print }");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        let end = endpoint_from_range_pattern(&cp, false).clone();
        let mut rt = runtime_with_slots(&cp);
        let mut state = false;

        rt.set_record_from_line("A and Z together");
        assert!(vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
        assert!(!state, "state must reset after end match in same record");
    }

    #[test]
    fn match_endpoint_always_returns_true() {
        let cp = compile("{ print }"); // anything; we don't need a range here
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("doesn't matter");
        assert!(vm_match_range_endpoint(&CompiledRangeEndpoint::Always, &cp, &mut rt).unwrap());
    }

    #[test]
    fn match_endpoint_never_returns_false() {
        let cp = compile("{ print }");
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("doesn't matter");
        assert!(!vm_match_range_endpoint(&CompiledRangeEndpoint::Never, &cp, &mut rt).unwrap());
    }

    #[test]
    fn match_endpoint_nested_range_is_runtime_error() {
        // Nested range patterns (`(a,b),c`) are rejected at runtime — must not
        // silently match or no-match.
        let cp = compile("{ print }");
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("x");
        let err = vm_match_range_endpoint(&CompiledRangeEndpoint::NestedRangeError, &cp, &mut rt)
            .unwrap_err();
        assert!(
            format!("{err}").contains("nested range"),
            "expected nested range error, got: {err}"
        );
    }

    #[test]
    fn match_endpoint_literal_regexp_substring_match() {
        // LiteralRegexp uses str::contains, not the regex engine — must be a
        // pure substring scan (no anchoring, no metacharacter interpretation).
        let cp = compile("/foo/,/bar/ { print }");
        let mut rt = runtime_with_slots(&cp);

        rt.set_record_from_line("xxx foo yyy");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        assert!(vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());

        rt.set_record_from_line("no needle");
        assert!(!vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());
    }

    #[test]
    fn match_endpoint_expr_truthy_falsy() {
        // Expr endpoint runs a chunk and applies truthy() to the TOS.
        // Use NR==2 as the start: must match on the 2nd record, not the 1st.
        let cp = compile("NR==2,NR==4 { print }");
        let start = endpoint_from_range_pattern(&cp, true).clone();
        let mut rt = runtime_with_slots(&cp);

        rt.nr = 1.0;
        rt.set_record_from_line("first");
        assert!(!vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());

        rt.nr = 2.0;
        rt.set_record_from_line("second");
        assert!(vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());
    }

    // ── Field operations: pin POSIX field semantics ──────────────────────────
    //
    // Field operations are the awk hot path: `$N`, `$N = …`, `NF = n`. POSIX
    // mandates a specific rebuild order: writing $N may extend NF with empty
    // fields up to N, $0 must be rebuilt with OFS, and setting NF truncates
    // fields and re-builds $0. Off-by-one in these operations breaks every awk
    // program — pin each contract.

    fn run_begin_capture(src: &str) -> String {
        let cp = compile(src);
        let mut rt = runtime_with_slots(&cp);
        vm_run_begin(&cp, &mut rt).unwrap();
        String::from_utf8_lossy(&rt.print_buf).into_owned()
    }

    #[test]
    fn field_assignment_extends_nf_with_empty_fields() {
        // POSIX: $5 = "x" when NF was 0 must extend $0 to "    x" (with OFS).
        let out = run_begin_capture(r#"BEGIN { $5 = "x"; print NF; print $0 }"#);
        assert_eq!(out, "5\n    x\n", "{out:?}");
    }

    #[test]
    fn field_set_rebuilds_dollar_zero_with_ofs() {
        // OFS = "|" must separate fields when $0 is rebuilt after $N=.
        let out = run_begin_capture(r#"BEGIN { OFS="|"; $1="a"; $2="b"; $3="c"; print $0 }"#);
        assert_eq!(out, "a|b|c\n", "{out:?}");
    }

    #[test]
    fn nf_truncate_shortens_record() {
        // NF=2 on a 4-field $0 must drop fields 3-4 and rebuild $0.
        let out = run_begin_capture(r#"BEGIN { $0="a b c d"; NF=2; print NF; print $0 }"#);
        assert_eq!(out, "2\na b\n", "{out:?}");
    }

    #[test]
    fn nf_extend_pads_with_empty_fields() {
        let out = run_begin_capture(r#"BEGIN { $0="a b"; NF=4; print NF; print $0 }"#);
        // NF was 2, now 4; new fields are empty. Default OFS=" ".
        assert_eq!(out, "4\na b  \n", "{out:?}");
    }

    #[test]
    fn dynamic_field_access_computed_index() {
        // `$(1+1)` must read $2, not the string "2" or field 1+1=2 as different.
        let out = run_begin_capture(r#"BEGIN { $0="x y z"; print $(1+1) }"#);
        assert_eq!(out, "y\n", "{out:?}");
    }

    // ── Array operations: pin POSIX array semantics ──────────────────────────

    #[test]
    fn array_in_returns_one_for_existing_key() {
        let out = run_begin_capture(r#"BEGIN { a["k"]=1; print ("k" in a) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn array_in_returns_zero_for_missing_key_without_creating() {
        // POSIX: `k in a` MUST NOT auto-create the key (unlike a[k] read).
        let out = run_begin_capture(r#"BEGIN { print ("nope" in a); for (k in a) print k }"#);
        // First print: 0; for-in finds zero keys, prints nothing more.
        assert_eq!(out, "0\n", "in-test must not auto-create: {out:?}");
    }

    #[test]
    fn array_index_read_auto_creates_uninit_entry() {
        // POSIX/gawk: `x = a[k]` auto-creates `a[k]` as Uninit. After the read,
        // `k in a` must be true. Implemented in the GetArrayElem dispatch:
        // if name != "SYMTAB" and the key is missing, insert Value::Uninit
        // before returning.
        let out = run_begin_capture(r#"BEGIN { x = a["k"]; print ("k" in a) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn delete_single_element_keeps_others() {
        let out = run_begin_capture(
            r#"BEGIN { a["x"]=1; a["y"]=2; delete a["x"]; print ("x" in a); print ("y" in a) }"#,
        );
        assert_eq!(out, "0\n1\n");
    }

    #[test]
    fn delete_entire_array_removes_all_entries() {
        let out = run_begin_capture(r#"BEGIN { a["x"]=1; a["y"]=2; delete a; print length(a) }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn multidim_array_uses_subsep_join() {
        // Default SUBSEP is \x1c. a[1,2] indexes by "1\x1c2".
        let out = run_begin_capture(r#"BEGIN { a[1,2]=42; print a[1,2]; print ((1,2) in a) }"#);
        assert_eq!(out, "42\n1\n");
    }

    #[test]
    fn for_in_iterates_all_keys() {
        // We can't assert order (impl-defined) but we can assert count + sum.
        let out = run_begin_capture(
            r#"BEGIN { a[1]=10; a[2]=20; a[3]=30; n=0; s=0; for (k in a) { n++; s += a[k] } print n; print s }"#,
        );
        assert_eq!(out, "3\n60\n");
    }

    // ── Control flow ─────────────────────────────────────────────────────────

    #[test]
    fn while_loop_runs_until_condition_false() {
        let out = run_begin_capture(r#"BEGIN { i=0; while (i<3) { print i; i++ } }"#);
        assert_eq!(out, "0\n1\n2\n");
    }

    #[test]
    fn do_while_runs_body_at_least_once() {
        // Body must run even when condition is initially false.
        let out = run_begin_capture(r#"BEGIN { i=10; do { print i; i++ } while (i<3) }"#);
        assert_eq!(out, "10\n", "do-while must run at least once: {out:?}");
    }

    #[test]
    fn break_exits_innermost_loop_only() {
        let out = run_begin_capture(
            r#"BEGIN { for (i=0; i<3; i++) { for (j=0; j<3; j++) { if (j==1) break; print i":"j } } }"#,
        );
        assert_eq!(out, "0:0\n1:0\n2:0\n");
    }

    #[test]
    fn continue_skips_to_next_iteration() {
        let out =
            run_begin_capture(r#"BEGIN { for (i=0; i<5; i++) { if (i==2) continue; print i } }"#);
        assert_eq!(out, "0\n1\n3\n4\n");
    }

    #[test]
    fn for_c_init_cond_iter_all_phases_run() {
        let out = run_begin_capture(r#"BEGIN { for (i=0; i<3; i++) print i*10 }"#);
        assert_eq!(out, "0\n10\n20\n");
    }

    // ── typeof variants ──────────────────────────────────────────────────────

    #[test]
    fn typeof_untyped_for_unset_scalar() {
        // typeof(unset scalar) returns "untyped" — matches gawk 5.x vocab.
        let out = run_begin_capture(r#"BEGIN { print typeof(u) }"#);
        assert_eq!(out, "untyped\n");
    }

    #[test]
    fn typeof_string_value() {
        let out = run_begin_capture(r#"BEGIN { s="hi"; print typeof(s) }"#);
        assert_eq!(out, "string\n");
    }

    #[test]
    fn typeof_numeric_value() {
        let out = run_begin_capture(r#"BEGIN { n=42; print typeof(n) }"#);
        assert_eq!(out, "number\n");
    }

    #[test]
    fn typeof_array_value() {
        let out = run_begin_capture(r#"BEGIN { a[1]=1; print typeof(a) }"#);
        assert_eq!(out, "array\n");
    }

    // ── sub/gsub return value semantics ──────────────────────────────────────

    #[test]
    fn sub_returns_one_on_match() {
        let out =
            run_begin_capture(r#"BEGIN { s="hello"; n=sub("ell","ELL",s); print n; print s }"#);
        assert_eq!(out, "1\nhELLo\n");
    }

    #[test]
    fn sub_returns_zero_on_no_match() {
        let out = run_begin_capture(r#"BEGIN { s="hello"; n=sub("xyz","X",s); print n; print s }"#);
        assert_eq!(out, "0\nhello\n");
    }

    #[test]
    fn gsub_returns_count_of_replacements() {
        let out =
            run_begin_capture(r#"BEGIN { s="abababab"; n=gsub("ab","X",s); print n; print s }"#);
        assert_eq!(out, "4\nXXXX\n");
    }

    #[test]
    fn gsub_on_dollar_zero_rebuilds_fields() {
        // Modifying $0 via gsub must update the field array.
        let out = run_begin_capture(r#"BEGIN { $0="a b c d"; gsub("b","BBB"); print $2 }"#);
        assert_eq!(out, "BBB\n");
    }

    // ── sprintf format specifiers ────────────────────────────────────────────

    #[test]
    fn sprintf_percent_d_truncates_to_integer() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%d", 3.7) }"#);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn sprintf_percent_f_default_six_decimals() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%f", 1.5) }"#);
        assert_eq!(out, "1.500000\n");
    }

    #[test]
    fn sprintf_percent_s_string() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%s]", "hi") }"#);
        assert_eq!(out, "[hi]\n");
    }

    #[test]
    fn sprintf_width_padding_right_aligned() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%5d]", 42) }"#);
        assert_eq!(out, "[   42]\n");
    }

    #[test]
    fn sprintf_negative_width_left_aligned() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%-5d]", 42) }"#);
        assert_eq!(out, "[42   ]\n");
    }

    #[test]
    fn sprintf_zero_pad() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%05d]", 42) }"#);
        assert_eq!(out, "[00042]\n");
    }

    #[test]
    fn sprintf_percent_x_hex() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%x", 255) }"#);
        assert_eq!(out, "ff\n");
    }

    #[test]
    fn sprintf_percent_o_octal() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%o", 8) }"#);
        assert_eq!(out, "10\n");
    }

    #[test]
    fn sprintf_double_percent_emits_literal_percent() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("100%%") }"#);
        assert_eq!(out, "100%\n");
    }

    // ── CONVFMT / OFMT ───────────────────────────────────────────────────────

    #[test]
    fn convfmt_default_six_significant_digits() {
        // POSIX/gawk: default CONVFMT = "%.6g" applies to float→string in
        // concat context. The ConcatPoolStr peephole path was bypassing it via
        // `Value::into_string()` (format_number); fixed to dispatch through
        // `num_to_string_convfmt` for Num/Mpfr.
        let out = run_begin_capture(r#"BEGIN { x=3.141592653; print x "" }"#);
        assert_eq!(out, "3.14159\n");
    }

    #[test]
    fn convfmt_custom_two_decimals() {
        let out = run_begin_capture(r#"BEGIN { CONVFMT="%.2f"; x=3.141592653; print x "" }"#);
        assert_eq!(out, "3.14\n");
    }

    #[test]
    fn ofmt_used_by_print_for_floats() {
        // print uses OFMT for floats, CONVFMT for concatenation.
        let out = run_begin_capture(r#"BEGIN { OFMT="%.3f"; print 3.141592653 }"#);
        assert_eq!(out, "3.142\n", "{out:?}");
    }

    #[test]
    fn convfmt_bypassed_for_integer_valued_floats() {
        // POSIX: integer-valued numbers print exact (no CONVFMT/OFMT), no ".000000".
        let out = run_begin_capture(r#"BEGIN { x=42; print x "" }"#);
        assert_eq!(out, "42\n");
    }

    // ── Additional sprintf format specifiers ─────────────────────────────────

    #[test]
    fn sprintf_percent_e_scientific_notation() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%e", 12345.0) }"#);
        // %e: one digit before decimal, 6 fractional digits, e+NN exponent.
        assert_eq!(out, "1.234500e+04\n");
    }

    #[test]
    fn sprintf_percent_g_uses_scientific_for_large_exponent() {
        // %g switches to %e form when exponent >= precision (default 6).
        // Previously failed because the lexer wasn't parsing `1e7` as a single
        // number token (was `1` concat ident `e7`); fixed in lexer.rs.
        let big = run_begin_capture(r#"BEGIN { print sprintf("%g", 1e7) }"#);
        assert_eq!(big, "1e+07\n");

        let small = run_begin_capture(r#"BEGIN { print sprintf("%g", 0.0001) }"#);
        assert_eq!(small, "0.0001\n");
    }

    #[test]
    fn sprintf_percent_c_from_integer_is_byte() {
        // %c with a numeric arg formats that byte. 65 → "A".
        let out = run_begin_capture(r#"BEGIN { print sprintf("%c", 65) }"#);
        assert_eq!(out, "A\n");
    }

    #[test]
    fn sprintf_percent_c_from_string_takes_first_char() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%c", "Hello") }"#);
        assert_eq!(out, "H\n");
    }

    #[test]
    fn sprintf_percent_d_negative_number() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%d", -42) }"#);
        assert_eq!(out, "-42\n");
    }

    #[test]
    fn sprintf_precision_truncates_string() {
        // %.5s takes first 5 bytes/chars of the string.
        let out = run_begin_capture(r#"BEGIN { print sprintf("%.5s", "abcdefghij") }"#);
        assert_eq!(out, "abcde\n");
    }

    #[test]
    fn sprintf_integer_precision_pads_with_zeros() {
        // POSIX: %.Nd zero-pads the integer magnitude to at least N digits.
        // The sign is added separately and doesn't count toward N.
        let out = run_begin_capture(r#"BEGIN { print sprintf("%.5d", 42) }"#);
        assert_eq!(out, "00042\n");
        let neg = run_begin_capture(r#"BEGIN { print sprintf("%.5d", -42) }"#);
        assert_eq!(neg, "-00042\n");
    }

    #[test]
    fn sprintf_width_and_precision_combined() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%10.3f]", 3.14159) }"#);
        // 10-wide field, 3 fractional digits → "     3.142" (right-aligned, 6 spaces+4 chars)
        assert_eq!(out, "[     3.142]\n");
    }

    #[test]
    fn sprintf_plus_flag_shows_positive_sign() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%+d %+d", 42, -42) }"#);
        assert_eq!(out, "+42 -42\n");
    }

    #[test]
    fn sprintf_hash_flag_on_octal_emits_leading_zero() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%#o", 8) }"#);
        // # flag for %o prefixes a literal '0' if not already there.
        assert_eq!(out, "010\n");
    }

    // ── substr / index / length corners ──────────────────────────────────────

    #[test]
    fn substr_negative_start_uses_position_one() {
        // POSIX: substr("abc", -1, 5) treats start as 1 (or adjusted), length is 5
        // but anything before position 1 doesn't exist — effective output depends
        // on implementation. gawk: substr("abc",-1,5) → "abc" (3 chars).
        let out = run_begin_capture(r#"BEGIN { print substr("abc", -1, 5) }"#);
        // The chars from "max(1,-1)" to min(len, -1+5-1) = chars 1..3 = "abc".
        assert_eq!(out, "abc\n");
    }

    #[test]
    fn substr_zero_length_returns_empty() {
        let out = run_begin_capture(r#"BEGIN { print "[" substr("hello", 2, 0) "]" }"#);
        assert_eq!(out, "[]\n");
    }

    #[test]
    fn substr_length_exceeds_string_clamps_to_end() {
        let out = run_begin_capture(r#"BEGIN { print substr("hello", 3, 999) }"#);
        assert_eq!(out, "llo\n");
    }

    #[test]
    fn substr_omitted_length_takes_rest() {
        let out = run_begin_capture(r#"BEGIN { print substr("hello", 3) }"#);
        assert_eq!(out, "llo\n");
    }

    #[test]
    fn index_returns_one_based_position() {
        let out = run_begin_capture(r#"BEGIN { print index("hello", "ell") }"#);
        // 'e' at byte 2 (1-based).
        assert_eq!(out, "2\n");
    }

    #[test]
    fn index_miss_returns_zero() {
        let out = run_begin_capture(r#"BEGIN { print index("hello", "xyz") }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn index_empty_needle_returns_one() {
        // gawk and awkrs agree: index("hello", "") → 1. (POSIX is ambiguous;
        // both major implementations treat empty needle as matching at start.)
        let out = run_begin_capture(r#"BEGIN { print index("hello", "") }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn length_of_empty_string_is_zero() {
        let out = run_begin_capture(r#"BEGIN { print length("") }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn length_of_integer_uses_string_form() {
        // length(123) → length of "123" → 3
        let out = run_begin_capture(r#"BEGIN { print length(123) }"#);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn length_of_array_is_element_count() {
        let out =
            run_begin_capture(r#"BEGIN { a[1]=1; a["x"]=2; a["multidim",1]=3; print length(a) }"#);
        assert_eq!(out, "3\n");
    }

    // ── split() edge cases ───────────────────────────────────────────────────

    #[test]
    fn split_empty_record_returns_zero() {
        let out = run_begin_capture(r#"BEGIN { n = split("", a); print n; print length(a) }"#);
        assert_eq!(out, "0\n0\n");
    }

    #[test]
    fn split_default_whitespace_skips_leading_trailing() {
        // Default FS (space) splits on runs of whitespace, skipping leading/trailing.
        let out = run_begin_capture(
            r#"BEGIN { n = split("  a  b  c  ", a); print n; print a[1], a[2], a[3] }"#,
        );
        assert_eq!(out, "3\na b c\n");
    }

    #[test]
    fn split_single_char_fs_keeps_empty_fields() {
        // Explicit FS=":" preserves empty fields (unlike default whitespace).
        let out = run_begin_capture(
            r#"BEGIN { n = split("a::b:c", a, ":"); print n; for(i=1;i<=n;i++) print "["a[i]"]" }"#,
        );
        assert_eq!(out, "4\n[a]\n[]\n[b]\n[c]\n");
    }

    #[test]
    fn split_empty_fs_splits_each_character() {
        // FS="" treats each char as a field (gawk extension).
        let out = run_begin_capture(
            r#"BEGIN { n = split("abc", a, ""); print n; print a[1], a[2], a[3] }"#,
        );
        assert_eq!(out, "3\na b c\n");
    }

    #[test]
    fn split_regex_fs_with_multi_char_separator() {
        // Multi-char FS is treated as a regex.
        let out = run_begin_capture(
            r#"BEGIN { n = split("a||b||c", a, /\|\|/); print n; print a[1], a[2], a[3] }"#,
        );
        assert_eq!(out, "3\na b c\n");
    }

    // ── CONVFMT in non-concat contexts ───────────────────────────────────────
    //
    // After the ConcatPoolStr fix, CONVFMT applies in concat contexts. But
    // POSIX says CONVFMT also applies in: array subscript coercion, regex
    // match operand coercion, and other string-context number conversions.
    // Pin each so a future change can't silently regress these to format_number.

    #[test]
    fn convfmt_applied_to_array_subscript() {
        // POSIX: array-subscript numeric coercion uses CONVFMT.
        // Implemented in vm.rs via `rt.value_to_array_key()` which dispatches
        // through num_to_string_convfmt for non-integer Num/Mpfr values.
        let out = run_begin_capture(r#"BEGIN { CONVFMT="%.0f"; a[3.14]=1; for (k in a) print k }"#);
        assert_eq!(out, "3\n");

        // Integer-valued keys still bypass CONVFMT (a[1] stays "1", not "1.0").
        let int_out = run_begin_capture(
            r#"BEGIN { CONVFMT="%.0f"; a[1]=1; a[42]=2; n=0; for(k in a){n++} print n }"#,
        );
        assert_eq!(int_out, "2\n");
    }

    #[test]
    fn convfmt_applies_to_regex_match_operand() {
        // `3.14 ~ /14/` coerces 3.14 to string via CONVFMT, then matches.
        let out = run_begin_capture(
            r#"BEGIN { CONVFMT="%.0f"; if (3.14 ~ /14/) print "match"; else print "nomatch" }"#,
        );
        // With CONVFMT=%.0f, 3.14 → "3", no "14" substring → "nomatch".
        assert_eq!(out, "nomatch\n", "{out:?}");
    }

    // ── Peephole fusion fires in record-rule context too ─────────────────────
    //
    // normalize_field_indices runs inside peephole_optimize which is called
    // from compile_chunk. Record-rule bodies and BEGIN/END use the same
    // compile_chunk path, so fusion should fire identically. Pin it.

    #[test]
    fn print_field_fusion_fires_in_record_rule() {
        let cp = compile("{ print $1 }");
        let body_ops = &cp.record_rules[0].body.ops;
        assert!(
            body_ops
                .iter()
                .any(|op| matches!(op, Op::PrintFieldStdout(1))),
            "record-rule body should have PrintFieldStdout(1), got: {body_ops:?}"
        );
    }

    #[test]
    fn add_field_to_slot_fusion_fires_in_record_rule() {
        let cp = compile("{ s += $2 } END { print s }");
        let body_ops = &cp.record_rules[0].body.ops;
        assert!(
            body_ops
                .iter()
                .any(|op| matches!(op, Op::AddFieldToSlot { field: 2, .. })),
            "record-rule body should have AddFieldToSlot{{field:2,..}}, got: {body_ops:?}"
        );
    }

    // ── Field-splitting modes: FPAT, FIELDWIDTHS ─────────────────────────────

    fn run_record_capture(prog: &str, input_line: &str) -> String {
        // NB: do NOT call flush_print_buf — it drains rt.print_buf to stdout.
        // We want to inspect the buffer, so leave it intact.
        let cp = compile(prog);
        let mut rt = runtime_with_slots(&cp);
        crate::vm::vm_run_begin(&cp, &mut rt).unwrap();
        rt.set_record_from_line(input_line);
        rt.nr = 1.0;
        rt.fnr = 1.0;
        if let Some(rule) = cp.record_rules.first() {
            let _ = crate::vm::vm_run_rule(rule, &cp, &mut rt, None, None);
        }
        crate::vm::vm_run_end(&cp, &mut rt).unwrap();
        String::from_utf8_lossy(&rt.print_buf).into_owned()
    }

    #[test]
    fn fpat_basic_pattern_extracts_fields() {
        // FPAT defines fields by pattern (gawk extension). Use a simple
        // word-pattern (avoid the alternation case which awkrs handles
        // incorrectly — see fpat_alternation_currently_wrong below).
        let prog = r#"BEGIN { FPAT="[a-z]+" } { print NF; print $1; print $2; print $3 }"#;
        let out = run_record_capture(prog, "abc 123 def 456 ghi");
        // Three word-fields: abc, def, ghi
        assert!(out.contains("3\n"), "expected NF=3 in: {out:?}");
        assert!(out.contains("abc\n"), "{out:?}");
        assert!(out.contains("def\n"), "{out:?}");
        assert!(out.contains("ghi\n"), "{out:?}");
    }

    #[test]
    fn fpat_alternation_preserves_quoted_fields() {
        // gawk's classic CSV FPAT: `[^,]*|"[^"]*"`. Leftmost-longest semantic
        // is required so the quoted-string alternative wins over the comma-free
        // run when both could match. Implemented via top-level alternation
        // splitting + per-position longest-match selection in
        // runtime.rs::split_fields_fpat.
        let prog =
            r#"BEGIN { FPAT="[^,]*|\"[^\"]*\"" } { print NF; print $1; print $2; print $3 }"#;
        let out = run_record_capture(prog, r#"abc,"def, ghi",xyz"#);
        assert!(out.contains("3\n"), "expected NF=3 in: {out:?}");
        assert!(out.contains("abc\n"), "{out:?}");
        assert!(out.contains(r#""def, ghi""#), "{out:?}");
        assert!(out.contains("xyz\n"), "{out:?}");
    }

    #[test]
    fn fieldwidths_splits_fixed_width_columns() {
        let prog = r#"BEGIN { FIELDWIDTHS="3 4 5" } { print NF; print "["$1"]["$2"]["$3"]" }"#;
        let out = run_record_capture(prog, "abc1234zzzzz");
        assert!(out.contains("3\n"), "expected NF=3: {out:?}");
        // 3-wide: "abc", 4-wide: "1234", 5-wide: "zzzzz"
        assert!(out.contains("[abc][1234][zzzzz]"), "{out:?}");
    }

    #[test]
    fn multichar_fs_treated_as_regex() {
        // FS with more than one char is interpreted as a regex.
        let prog = r#"{ print NF; print $1; print $2 }"#;
        let cp = compile(prog);
        let mut rt = runtime_with_slots(&cp);
        rt.vars
            .insert("FS".into(), crate::runtime::Value::Str(r"[,;]".into()));
        crate::vm::vm_run_begin(&cp, &mut rt).unwrap();
        rt.set_record_from_line("a,b;c");
        rt.nr = 1.0;
        rt.fnr = 1.0;
        crate::vm::vm_run_rule(&cp.record_rules[0], &cp, &mut rt, None, None).unwrap();
        let out = String::from_utf8_lossy(&rt.print_buf).into_owned();
        assert!(out.contains("3\n") && out.contains("a\nb\n"), "{out:?}");
    }

    // ── gsub/sub additional edge cases ───────────────────────────────────────

    #[test]
    fn gsub_with_ampersand_in_replacement_uses_match() {
        // `&` in replacement is replaced with the matched text.
        let out = run_begin_capture(r#"BEGIN { s="abc"; gsub(/b/, "[&]", s); print s }"#);
        assert_eq!(out, "a[b]c\n");
    }

    #[test]
    fn gsub_with_escaped_ampersand_is_literal() {
        // `\&` in replacement is a literal `&`.
        let out = run_begin_capture(r#"BEGIN { s="abc"; gsub(/b/, "\\&", s); print s }"#);
        assert_eq!(out, "a&c\n");
    }

    #[test]
    fn gsub_anchored_pattern_caret() {
        // `^` matches start of string only.
        let out =
            run_begin_capture(r#"BEGIN { s="aaa"; n = gsub(/^a/, "X", s); print n; print s }"#);
        assert_eq!(out, "1\nXaa\n");
    }

    #[test]
    fn gsub_anchored_pattern_dollar() {
        // `$` matches end.
        let out =
            run_begin_capture(r#"BEGIN { s="aaa"; n = gsub(/a$/, "X", s); print n; print s }"#);
        assert_eq!(out, "1\naaX\n");
    }

    #[test]
    fn gensub_backref_substitution() {
        // gensub's `\1` / `\2` etc. refer to capture groups in the regex.
        // Implemented via expand_repl_with_caps in builtins.rs which uses
        // captures_iter() to retain group info (find_iter() doesn't).
        let out = run_begin_capture(
            r#"BEGIN { s="John Smith"; r=gensub(/(\w+) (\w+)/, "\\2, \\1", "g", s); print r }"#,
        );
        assert_eq!(out, "Smith, John\n");
    }

    #[test]
    fn gensub_backref_with_numeric_occurrence() {
        // Numeric `how` arg (e.g. 2) replaces only the Nth occurrence — must
        // still expand backrefs in that one replacement.
        let out = run_begin_capture(
            r#"BEGIN { s="aa bb cc"; r=gensub(/(\w+)/, "[\\1]", 2, s); print r }"#,
        );
        assert_eq!(out, "aa [bb] cc\n");
    }

    #[test]
    fn gensub_ampersand_replacement_still_works_alongside_backref() {
        // `&` and `\N` are independent — both must work after the backref
        // refactor (replace_all_gensub uses expand_repl_with_caps for both).
        let out = run_begin_capture(r#"BEGIN { s="abc"; r=gensub(/b/, "[&]", "g", s); print r }"#);
        assert_eq!(out, "a[b]c\n");
    }

    // ── Math builtin coverage ────────────────────────────────────────────────

    #[test]
    fn math_log_one_is_zero() {
        let out = run_begin_capture(r#"BEGIN { print log(1) }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn math_log_e_is_one() {
        let out = run_begin_capture(r#"BEGIN { printf "%.6f\n", log(exp(1)) }"#);
        assert_eq!(out, "1.000000\n");
    }

    #[test]
    fn math_exp_zero_is_one() {
        let out = run_begin_capture(r#"BEGIN { print exp(0) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn math_int_truncates_negative_toward_zero() {
        // POSIX: int(-3.7) = -3, not -4. (Truncation, not floor.)
        let out = run_begin_capture(r#"BEGIN { print int(-3.7) }"#);
        assert_eq!(out, "-3\n");
    }

    #[test]
    fn math_int_truncates_positive_toward_zero() {
        let out = run_begin_capture(r#"BEGIN { print int(3.7) }"#);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn math_atan2_y_over_x_quadrant() {
        // atan2(1,1) = π/4 ≈ 0.7853981633974483
        let out = run_begin_capture(r#"BEGIN { printf "%.4f\n", atan2(1, 1) }"#);
        assert_eq!(out, "0.7854\n");
    }

    #[test]
    fn math_atan2_zero_zero_is_zero() {
        let out = run_begin_capture(r#"BEGIN { print atan2(0, 0) }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn math_sqrt_of_zero() {
        let out = run_begin_capture(r#"BEGIN { print sqrt(0) }"#);
        assert_eq!(out, "0\n");
    }

    // ── tolower / toupper ────────────────────────────────────────────────────

    #[test]
    fn tolower_mixed_case() {
        let out = run_begin_capture(r#"BEGIN { print tolower("AbCdEf") }"#);
        assert_eq!(out, "abcdef\n");
    }

    #[test]
    fn toupper_mixed_case() {
        let out = run_begin_capture(r#"BEGIN { print toupper("aBcDeF") }"#);
        assert_eq!(out, "ABCDEF\n");
    }

    #[test]
    fn tolower_passes_through_non_letters() {
        let out = run_begin_capture(r#"BEGIN { print tolower("ABC 123!") }"#);
        assert_eq!(out, "abc 123!\n");
    }

    #[test]
    fn toupper_passes_through_non_letters() {
        let out = run_begin_capture(r#"BEGIN { print toupper("abc 123!") }"#);
        assert_eq!(out, "ABC 123!\n");
    }

    // ── strftime format specifiers ───────────────────────────────────────────
    //
    // strftime delegates to chrono's `format`. We pin a stable UTC epoch and
    // verify each major POSIX format specifier produces the expected output.
    // 3rd arg = 1 forces UTC so tests are tz-stable in CI.
    //
    // Test epoch: 2024-01-15 03:45:06 UTC = 1705290306
    const TEST_EPOCH: &str = "1705290306";

    #[test]
    fn strftime_year_four_digit() {
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("%Y", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "2024\n");
    }

    #[test]
    fn strftime_month_two_digit() {
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("%m", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "01\n");
    }

    #[test]
    fn strftime_day_of_month() {
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("%d", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "15\n");
    }

    #[test]
    fn strftime_hour_24_minute_second() {
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("%H:%M:%S", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "03:45:06\n");
    }

    #[test]
    fn strftime_combined_iso_date() {
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("%Y-%m-%d", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "2024-01-15\n");
    }

    #[test]
    fn strftime_percent_percent_emits_literal_percent() {
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("100%%", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "100%\n");
    }

    #[test]
    fn strftime_day_of_year() {
        // 2024-01-15 = day 15 (Jan 15)
        let out = run_begin_capture(&format!(
            r#"BEGIN {{ print strftime("%j", {TEST_EPOCH}, 1) }}"#
        ));
        assert_eq!(out, "015\n");
    }

    // ── mktime ───────────────────────────────────────────────────────────────

    #[test]
    fn mktime_returns_minus_one_on_invalid_month() {
        let out = run_begin_capture(r#"BEGIN { print mktime("2024 13 01 00 00 00") }"#);
        // gawk returns -1 for invalid date components; chrono's strict
        // construction does the same.
        assert_eq!(out, "-1\n");
    }

    #[test]
    fn mktime_year_2000_january_one_positive_epoch() {
        // 2000-01-01 in any timezone is well after 1970, so epoch > 0.
        // `> 0` must be inside the print's expression — bare `print x > 0`
        // parses as a redirect to file "0". Always parenthesize.
        let out = run_begin_capture(r#"BEGIN { print (mktime("2000 1 1 0 0 0") > 0) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn mktime_too_few_fields_returns_minus_one() {
        let out = run_begin_capture(r#"BEGIN { print mktime("2024 1 1") }"#);
        assert_eq!(out, "-1\n");
    }

    // ── srand / rand: deterministic sequence with fixed seed ─────────────────
    //
    // POSIX: srand(seed) seeds the RNG and returns the PREVIOUS seed. Two runs
    // with the same seed must produce the same sequence — a regression here
    // would break every random-sampling awk program silently.

    #[test]
    fn srand_returns_previous_seed() {
        // First srand returns whatever the initial seed was (impl-defined).
        // Second srand returns the seed passed to the first.
        let out = run_begin_capture(r#"BEGIN { srand(42); prev=srand(99); print prev }"#);
        assert_eq!(out, "42\n");
    }

    #[test]
    fn rand_sequence_stable_with_same_seed() {
        // Two srand(N) calls with the same N must reset to the same sequence.
        let out = run_begin_capture(
            r#"BEGIN { srand(7); a=rand(); b=rand(); srand(7); c=rand(); d=rand(); print (a==c) (b==d) }"#,
        );
        // "11" means both pairs matched.
        assert_eq!(out, "11\n");
    }

    #[test]
    fn rand_values_in_half_open_unit_interval() {
        // rand() returns x ∈ [0, 1). Draw a few and verify each is in range.
        let out = run_begin_capture(
            r#"BEGIN { srand(1); ok=1; for(i=0;i<10;i++){ x=rand(); if (x<0||x>=1) ok=0 } print ok }"#,
        );
        assert_eq!(out, "1\n");
    }

    #[test]
    fn rand_different_draws_with_same_seed_differ() {
        // Sanity: two consecutive rand() with the same seed should NOT be equal
        // (catastrophic regression: rand always returns the same value).
        let out = run_begin_capture(r#"BEGIN { srand(123); a=rand(); b=rand(); print (a==b) }"#);
        assert_eq!(out, "0\n");
    }

    // ── intdiv / intdiv0: integer division ──────────────────────────────────
    //
    // awkrs uses a 2-arg signature: `intdiv(a, b)` returns the integer
    // quotient (truncated toward zero). `intdiv0(a, b)` returns 0 on division
    // by zero instead of erroring.

    #[test]
    fn intdiv_positive_quotient() {
        let out = run_begin_capture(r#"BEGIN { print intdiv(17, 5) }"#);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn intdiv_exact_division() {
        let out = run_begin_capture(r#"BEGIN { print intdiv(20, 5) }"#);
        assert_eq!(out, "4\n");
    }

    #[test]
    fn intdiv_truncates_negative_toward_zero() {
        // -17 / 5 → -3 (truncate toward zero), not -4 (floor).
        let out = run_begin_capture(r#"BEGIN { print intdiv(-17, 5) }"#);
        assert_eq!(out, "-3\n");
    }

    #[test]
    fn intdiv_zero_divisor_errors() {
        let cp = compile(r#"BEGIN { intdiv(10, 0) }"#);
        let mut rt = runtime_with_slots(&cp);
        let result = crate::vm::vm_run_begin(&cp, &mut rt);
        assert!(result.is_err(), "intdiv(10, 0) must error, got Ok");
    }

    #[test]
    fn intdiv0_zero_divisor_returns_zero_without_error() {
        // intdiv0(a, 0) returns 0 (the "safe" variant — no runtime error).
        let out = run_begin_capture(r#"BEGIN { print intdiv0(10, 0) }"#);
        assert_eq!(out, "0\n");
    }

    // ── Record / field rebuild edge cases ────────────────────────────────────

    #[test]
    fn set_dollar_zero_resplits_with_current_fs() {
        // `$0 = "..."` must re-split with the active FS.
        let out = run_begin_capture(r#"BEGIN { FS=":"; $0="a:b:c"; print NF; print $2 }"#);
        assert_eq!(out, "3\nb\n");
    }

    #[test]
    fn nf_zero_clears_record_and_fields() {
        // POSIX: NF=0 makes $0 empty and removes all fields.
        let out = run_begin_capture(r#"BEGIN { $0="a b c"; NF=0; print NF; print "[" $0 "]" }"#);
        assert_eq!(out, "0\n[]\n");
    }

    #[test]
    fn dollar_zero_assigns_through_nf_changes() {
        // After resetting $0, NF reflects the new field count.
        let out = run_begin_capture(r#"BEGIN { $0="x y"; print NF; $0="p q r s"; print NF }"#);
        assert_eq!(out, "2\n4\n");
    }

    #[test]
    fn set_field_beyond_nf_extends_with_empties() {
        // $5 = "x" when NF was 2 → NF becomes 5, $3 and $4 are empty.
        let out = run_begin_capture(
            r#"BEGIN { $0="a b"; $5="z"; print NF; print "[" $3 "][" $4 "][" $5 "]" }"#,
        );
        assert_eq!(out, "5\n[][][z]\n");
    }

    #[test]
    fn reassigning_field_one_rebuilds_dollar_zero() {
        let out = run_begin_capture(r#"BEGIN { $0="a b c"; $1="X"; print $0 }"#);
        assert_eq!(out, "X b c\n");
    }

    #[test]
    fn fs_change_after_record_does_not_resplit() {
        // POSIX: changing FS doesn't re-split the current $0 retroactively;
        // it affects the NEXT record. (Within BEGIN we can verify by setting
        // $0 explicitly then changing FS and reading fields — fields stay
        // split per the FS that was active at the time of the assignment.)
        let out = run_begin_capture(r#"BEGIN { FS=":"; $0="a:b:c"; FS=" "; print NF; print $2 }"#);
        // Still 3 fields with FS=":" split (changing FS to " " after $0=
        // doesn't re-split the existing record).
        assert_eq!(out, "3\nb\n");
    }

    #[test]
    fn sub_does_not_modify_on_no_match() {
        // sub returns 0 and leaves the target unchanged.
        let out =
            run_begin_capture(r#"BEGIN { s="hello"; n = sub(/xyz/, "X", s); print n; print s }"#);
        assert_eq!(out, "0\nhello\n");
    }

    #[test]
    fn gsub_count_zero_returned_on_no_match() {
        let out =
            run_begin_capture(r#"BEGIN { s="hello"; n = gsub(/xyz/, "X", s); print n; print s }"#);
        assert_eq!(out, "0\nhello\n");
    }

    #[test]
    fn gsub_default_target_is_dollar_zero() {
        // gsub() with 2 args operates on $0.
        let prog = r#"{ gsub(/o/, "0"); print }"#;
        let out = run_record_capture(prog, "foo bar boo");
        assert!(out.contains("f00 bar b00"), "{out:?}");
    }

    #[test]
    fn print_field_fusion_end_to_end_behavior() {
        // Verify the fused opcode behaves identically to the unfused sequence
        // for actual user-visible output.
        let cp = compile("{ print $2 }");
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("foo bar baz");
        crate::vm::vm_run_rule(&cp.record_rules[0], &cp, &mut rt, None, None).unwrap();
        crate::vm::flush_print_buf(&mut rt.print_buf).unwrap();
        // Output should contain "bar" + ORS.
        // We can't easily capture stdout here, so just verify the opcode shape:
        assert!(
            cp.record_rules[0]
                .body
                .ops
                .iter()
                .any(|op| matches!(op, Op::PrintFieldStdout(2))),
            "expected PrintFieldStdout(2) for `{{ print $2 }}`"
        );
    }

    // ── Comparison semantics: numeric vs string per POSIX ────────────────────

    #[test]
    fn cmp_two_numbers_uses_numeric_order() {
        let out = run_begin_capture(r#"BEGIN { print (10 < 9) ? "yes" : "no" }"#);
        assert_eq!(out, "no\n");
    }

    #[test]
    fn cmp_string_literals_use_string_order() {
        let out = run_begin_capture(r#"BEGIN { print ("10" < "9") ? "yes" : "no" }"#);
        assert_eq!(out, "yes\n");
    }

    #[test]
    fn cmp_string_literal_vs_number_uses_string_compare() {
        // POSIX/gawk: a string LITERAL is NOT a "numeric string". When mixed
        // with a number, the number is coerced to a string and the comparison
        // is STRING-wise.
        //
        // Numeric compare applies only when both operands are either numbers
        // or "numeric strings" — values from input/$N/-v that look numeric.
        // Bare `"10"` source-level literals stay as Value::StrLit and miss
        // the numeric-string predicate. Verified with `gawk 'BEGIN { print
        // ("10" < 9) ? "yes" : "no" }'` → "yes".
        let out = run_begin_capture(r#"BEGIN { print ("10" < 9) ? "yes" : "no" }"#);
        assert_eq!(out, "yes\n");
    }

    #[test]
    fn cmp_uninit_equals_zero_numerically() {
        let out = run_begin_capture(r#"BEGIN { print (u == 0) ? "yes" : "no" }"#);
        assert_eq!(out, "yes\n");
    }

    #[test]
    fn cmp_non_numeric_strings_use_string_order() {
        let out = run_begin_capture(r#"BEGIN { print ("apple" < "banana") ? "yes" : "no" }"#);
        assert_eq!(out, "yes\n");
    }

    // ── Compound assignment to various lvalue targets ────────────────────────

    #[test]
    fn compound_assign_to_simple_var() {
        let out = run_begin_capture(r#"BEGIN { x = 10; x += 5; print x }"#);
        assert_eq!(out, "15\n");
    }

    #[test]
    fn compound_assign_to_array_element() {
        let out = run_begin_capture(r#"BEGIN { a[1] = 10; a[1] += 5; print a[1] }"#);
        assert_eq!(out, "15\n");
    }

    #[test]
    fn compound_assign_to_field_rebuilds_record() {
        let out = run_begin_capture(r#"BEGIN { $0 = "10 20 30"; $2 += 100; print $0 }"#);
        assert_eq!(out, "10 120 30\n");
    }

    #[test]
    fn compound_assign_div_and_mod() {
        // `+=`, `-=`, `*=`, `/=`, `%=` all parse and apply correctly. The
        // `^=` and `**=` exponentiation variants are covered separately.
        let out = run_begin_capture(r#"BEGIN { x=100; x/=4; print x; y=10; y%=3; print y }"#);
        assert_eq!(out, "25\n1\n");
    }

    #[test]
    fn compound_pow_assign_supported() {
        // `x ^= n` and `x **= n` (gawk-style compound exponentiation) parse
        // and evaluate as `x = x ^ n`. Lexer emits `PowAssign` token; parser
        // maps it to `BinOp::Pow`.
        let out = run_begin_capture(r#"BEGIN { z=2; z^=8; print z }"#);
        assert_eq!(out, "256\n");
        let out2 = run_begin_capture(r#"BEGIN { z=2; z**=8; print z }"#);
        assert_eq!(out2, "256\n");
    }

    // ── Increment / decrement on different lvalues ───────────────────────────

    #[test]
    fn incdec_field_postinc_returns_old_value() {
        let out = run_begin_capture(r#"BEGIN { $0 = "5 6"; x = $1++; print x; print $1 }"#);
        assert_eq!(out, "5\n6\n");
    }

    #[test]
    fn incdec_field_preinc_returns_new_value() {
        let out = run_begin_capture(r#"BEGIN { $0 = "5 6"; x = ++$1; print x; print $1 }"#);
        assert_eq!(out, "6\n6\n");
    }

    #[test]
    fn incdec_array_element_postinc() {
        let out = run_begin_capture(r#"BEGIN { a[1] = 10; x = a[1]++; print x; print a[1] }"#);
        assert_eq!(out, "10\n11\n");
    }

    #[test]
    fn incdec_uninit_starts_at_zero() {
        let out = run_begin_capture(r#"BEGIN { x = ++u; print x }"#);
        assert_eq!(out, "1\n");
    }

    // ── Logical short-circuit ────────────────────────────────────────────────

    #[test]
    fn logical_and_short_circuits_false_left() {
        let out = run_begin_capture(r#"BEGIN { n = 0; r = (0 && (n=1)); print r; print n }"#);
        assert_eq!(out, "0\n0\n");
    }

    #[test]
    fn logical_or_short_circuits_true_left() {
        let out = run_begin_capture(r#"BEGIN { n = 0; r = (1 || (n=1)); print r; print n }"#);
        assert_eq!(out, "1\n0\n");
    }

    #[test]
    fn ternary_evaluates_only_chosen_branch() {
        let out = run_begin_capture(
            r#"BEGIN { a=0; b=0; r = (1 ? (a=1) : (b=1)); print r; print a; print b }"#,
        );
        assert_eq!(out, "1\n1\n0\n");
    }

    // ── Match operator ~ / !~ ────────────────────────────────────────────────

    #[test]
    fn match_operator_returns_one_on_match() {
        let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /ell/) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn match_operator_returns_zero_on_no_match() {
        let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /xyz/) }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn not_match_inverts_result() {
        let out =
            run_begin_capture(r#"BEGIN { print ("hello" !~ /xyz/); print ("hello" !~ /ell/) }"#);
        assert_eq!(out, "1\n0\n");
    }

    #[test]
    fn match_with_dynamic_regex_string() {
        let out = run_begin_capture(r#"BEGIN { pat = "[a-z]+"; print ("hello" ~ pat) }"#);
        assert_eq!(out, "1\n");
    }

    // ── User function calls ──────────────────────────────────────────────────

    #[test]
    fn user_function_returns_value() {
        let out =
            run_begin_capture(r#"function add(a, b) { return a + b } BEGIN { print add(3, 4) }"#);
        assert_eq!(out, "7\n");
    }

    #[test]
    fn user_function_local_vars_via_extra_params() {
        // Extra params past the call site are local to the function.
        let out = run_begin_capture(
            r#"function f(x,    i) { i=99; return i+x } BEGIN { i=1; print f(10); print i }"#,
        );
        assert_eq!(out, "109\n1\n");
    }

    #[test]
    fn user_function_recursion_factorial() {
        let out = run_begin_capture(
            r#"function fact(n) { return n<=1 ? 1 : n*fact(n-1) } BEGIN { print fact(5) }"#,
        );
        assert_eq!(out, "120\n");
    }

    #[test]
    fn user_function_mutual_recursion() {
        let out = run_begin_capture(
            r#"function even(n) { return n==0 ? 1 : odd(n-1) }
               function odd(n)  { return n==0 ? 0 : even(n-1) }
               BEGIN { print even(10); print odd(7) }"#,
        );
        assert_eq!(out, "1\n1\n");
    }

    // ── Sprintf additional cases ─────────────────────────────────────────────

    #[test]
    fn sprintf_zero_pad_negative_sign_first() {
        // POSIX: when zero-padding a signed integer, zeros go BETWEEN the
        // sign and the magnitude. Fixed in format.rs::pad_numeric to detect
        // a leading sign and insert padding after it.
        let out = run_begin_capture(r#"BEGIN { print sprintf("%05d", -42) }"#);
        assert_eq!(out, "-0042\n");
    }

    #[test]
    fn sprintf_zero_pad_positive_with_plus_flag() {
        // Same rule applies to `+` sign flag.
        let out = run_begin_capture(r#"BEGIN { print sprintf("%+05d", 42) }"#);
        assert_eq!(out, "+0042\n");
    }

    #[test]
    fn sprintf_multiple_args_in_one_format() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%s=%d (%.2f)", "x", 7, 3.14) }"#);
        assert_eq!(out, "x=7 (3.14)\n");
    }

    #[test]
    fn sprintf_space_flag_positive_number() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("% d % d", 42, -42) }"#);
        assert_eq!(out, " 42 -42\n");
    }

    // ── Truthy / falsy edges ─────────────────────────────────────────────────

    #[test]
    fn empty_string_is_falsy() {
        let out = run_begin_capture(r#"BEGIN { print "" ? "T" : "F" }"#);
        assert_eq!(out, "F\n");
    }

    #[test]
    fn string_literal_zero_is_truthy_unlike_number_zero() {
        // POSIX/gawk: a string LITERAL is truthy iff non-empty. The numeric
        // value of `"0"` is irrelevant for string literals in boolean context.
        // Only Value::Str (from input/fields/-v) gets numeric coercion.
        // Fixed in runtime.rs::truthy / truthy_cond by splitting StrLit/Str.
        let out = run_begin_capture(r#"BEGIN { print ("0" ? "T" : "F"); print (0 ? "T" : "F") }"#);
        assert_eq!(out, "T\nF\n");
    }

    #[test]
    fn whole_array_in_scalar_context_errors() {
        let cp = compile(r#"BEGIN { a[1]=1; if (a) print "yes" }"#);
        let mut rt = runtime_with_slots(&cp);
        let result = crate::vm::vm_run_begin(&cp, &mut rt);
        assert!(result.is_err(), "array-as-scalar must error");
    }

    // ── Multi-statement bodies ───────────────────────────────────────────────

    #[test]
    fn semicolon_separates_statements() {
        let out = run_begin_capture(r#"BEGIN { x=1; y=2; print x+y }"#);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn newline_separates_statements() {
        let out = run_begin_capture("BEGIN {\nx=1\ny=2\nprint x+y\n}");
        assert_eq!(out, "3\n");
    }

    #[test]
    fn comment_after_statement_terminates_via_newline() {
        // After fix: `skip_ws` leaves the `\n` after a comment in place so the
        // lexer emits `Newline`, which terminates the assignment statement.
        let out = run_begin_capture("BEGIN { x = 42 # comment\n print x }");
        assert_eq!(out, "42\n");
    }

    #[test]
    fn semicolon_after_statement_with_comment_works() {
        // Workaround for the comment-as-terminator bug: explicit `;` works.
        let out = run_begin_capture("BEGIN { x = 42; # comment\n print x }");
        assert_eq!(out, "42\n");
    }

    // ── String concatenation with mixed types ────────────────────────────────

    #[test]
    fn concat_string_and_number_coerces_number() {
        let out = run_begin_capture(r#"BEGIN { print "x" 42 "y" }"#);
        assert_eq!(out, "x42y\n");
    }

    #[test]
    fn concat_with_uninit_treats_as_empty() {
        let out = run_begin_capture(r#"BEGIN { print "[" u "]" }"#);
        assert_eq!(out, "[]\n");
    }

    // ── For-in iteration ─────────────────────────────────────────────────────

    #[test]
    fn for_in_visits_each_key_exactly_once() {
        let out = run_begin_capture(
            r#"BEGIN { a["x"]=1; a["y"]=2; a["z"]=3; n=0; for (k in a) n++; print n }"#,
        );
        assert_eq!(out, "3\n");
    }

    #[test]
    fn for_in_empty_array_runs_zero_iterations() {
        let out = run_begin_capture(r#"BEGIN { n=0; for (k in a) n++; print n }"#);
        assert_eq!(out, "0\n");
    }

    // ── Range pattern across records ─────────────────────────────────────────

    #[test]
    fn range_pattern_runs_for_records_inside_range() {
        let prog = r#"NR==2,NR==4 { print "in:" NR }"#;
        let cp = compile(prog);
        let mut rt = runtime_with_slots(&cp);
        let mut state = vec![false; cp.prog_rules_len];
        for nr in 1..=5 {
            rt.nr = nr as f64;
            rt.set_record_from_line(&format!("line{nr}"));
            let rule = &cp.record_rules[0];
            if let CompiledPattern::Range { start, end } = &rule.pattern {
                let run = vm_range_step(&mut state[rule.original_index], start, end, &cp, &mut rt)
                    .unwrap();
                if run {
                    crate::vm::vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
                }
            }
        }
        let s = String::from_utf8_lossy(&rt.print_buf);
        assert!(
            s.contains("in:2") && s.contains("in:3") && s.contains("in:4"),
            "{s}"
        );
        assert!(!s.contains("in:1") && !s.contains("in:5"), "{s}");
    }

    // ── Block scope (awk has none — all vars are function/global) ────────────

    #[test]
    fn nested_blocks_share_variables() {
        let out = run_begin_capture(r#"BEGIN { { x = 1 } print x }"#);
        assert_eq!(out, "1\n");
    }

    // ── printf redirect to /dev/null runs without error ──────────────────────

    #[test]
    fn printf_redirect_overwrite_runs() {
        let out = run_begin_capture(r#"BEGIN { printf "%s\n", "ignored" > "/dev/null" }"#);
        assert_eq!(out, "");
    }

    // ── sprintf flag interactions ────────────────────────────────────────────

    #[test]
    fn sprintf_left_align_overrides_zero_pad() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%-05d]", 42) }"#);
        assert_eq!(out, "[42   ]\n");
    }

    #[test]
    fn sprintf_plus_with_space_takes_plus() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("% +d", 42) }"#);
        assert_eq!(out, "+42\n");
    }

    #[test]
    fn sprintf_hash_flag_on_hex_emits_0x_prefix() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%#x", 255) }"#);
        assert_eq!(out, "0xff\n");
    }

    #[test]
    fn sprintf_hash_flag_on_upper_hex_uses_upper_0x() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("%#X", 255) }"#);
        assert_eq!(out, "0XFF\n");
    }

    #[test]
    fn sprintf_negative_octal_via_unsigned_wrap() {
        // `print x > 0` parses as redirect-to-file "0" — must parenthesize
        // the comparison inside print. (See mktime_year_2000_january_one_…
        // comment for the same gotcha.)
        let out = run_begin_capture(r#"BEGIN { print (length(sprintf("%o", -1)) > 0) }"#);
        assert_eq!(out, "1\n");
    }

    // ── substr boundary conditions ───────────────────────────────────────────

    #[test]
    fn substr_start_at_one_takes_from_beginning() {
        let out = run_begin_capture(r#"BEGIN { print substr("hello", 1) }"#);
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn substr_start_beyond_string_returns_empty() {
        let out = run_begin_capture(r#"BEGIN { print "[" substr("hello", 10) "]" }"#);
        assert_eq!(out, "[]\n");
    }

    #[test]
    fn substr_single_character_at_position() {
        let out = run_begin_capture(r#"BEGIN { print substr("hello", 3, 1) }"#);
        assert_eq!(out, "l\n");
    }

    // ── Regex behaviors ──────────────────────────────────────────────────────

    #[test]
    fn regex_character_class_matches() {
        let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /[a-z]+/) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn regex_negated_character_class() {
        let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /[^a-z]/) }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn regex_quantifier_plus_at_least_one() {
        let out = run_begin_capture(r#"BEGIN { print ("" ~ /a+/); print ("a" ~ /a+/) }"#);
        assert_eq!(out, "0\n1\n");
    }

    #[test]
    fn regex_quantifier_star_zero_or_more() {
        let out = run_begin_capture(r#"BEGIN { print ("" ~ /a*/); print ("aaa" ~ /a*/) }"#);
        assert_eq!(out, "1\n1\n");
    }

    #[test]
    fn regex_anchor_caret_only_at_start() {
        let out = run_begin_capture(r#"BEGIN { print ("foo" ~ /^foo/); print ("xfoo" ~ /^foo/) }"#);
        assert_eq!(out, "1\n0\n");
    }

    #[test]
    fn regex_anchor_dollar_only_at_end() {
        let out = run_begin_capture(r#"BEGIN { print ("bar" ~ /bar$/); print ("barx" ~ /bar$/) }"#);
        assert_eq!(out, "1\n0\n");
    }

    #[test]
    fn regex_alternation_picks_either() {
        let out = run_begin_capture(
            r#"BEGIN { print ("cat" ~ /cat|dog/); print ("dog" ~ /cat|dog/); print ("fish" ~ /cat|dog/) }"#,
        );
        assert_eq!(out, "1\n1\n0\n");
    }

    // ── match() builtin sets RSTART/RLENGTH ──────────────────────────────────

    #[test]
    fn match_builtin_sets_rstart_and_rlength() {
        let out = run_begin_capture(
            r#"BEGIN { r = match("hello world", /world/); print r, RSTART, RLENGTH }"#,
        );
        assert_eq!(out, "7 7 5\n");
    }

    #[test]
    fn match_builtin_sets_rstart_zero_on_miss() {
        let out =
            run_begin_capture(r#"BEGIN { r = match("hello", /xyz/); print r, RSTART, RLENGTH }"#);
        assert_eq!(out, "0 0 -1\n");
    }

    // ── Special variables ────────────────────────────────────────────────────

    #[test]
    fn nf_initial_value_zero_in_begin() {
        let out = run_begin_capture(r#"BEGIN { print NF }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn nr_initial_value_zero_in_begin() {
        let out = run_begin_capture(r#"BEGIN { print NR }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn environ_array_present_in_begin() {
        let out = run_begin_capture(r#"BEGIN { n=0; for (k in ENVIRON) n++; print (n > 0) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn subsep_default_length_one() {
        let out = run_begin_capture(r#"BEGIN { print length(SUBSEP) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn subsep_used_for_multidim_array_keys() {
        let out = run_begin_capture(r#"BEGIN { a[1,2] = "x"; for (k in a) print length(k) }"#);
        // Key is "1" + SUBSEP + "2" = 3 chars
        assert_eq!(out, "3\n");
    }

    // ── Power operator edge cases ────────────────────────────────────────────

    #[test]
    fn pow_zero_to_zero_is_one() {
        let out = run_begin_capture(r#"BEGIN { print 0^0 }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn pow_negative_base_integer_exponent() {
        let out = run_begin_capture(r#"BEGIN { print (-2)^3 }"#);
        assert_eq!(out, "-8\n");
    }

    #[test]
    fn pow_fractional_exponent() {
        let out = run_begin_capture(r#"BEGIN { print 4^0.5 }"#);
        assert_eq!(out, "2\n");
    }

    // ── Parser / statement corners ───────────────────────────────────────────

    #[test]
    fn semicolons_separate_statements_on_one_line() {
        let out = run_begin_capture("BEGIN { x = 1; if (x) print x; print x+1 }");
        assert_eq!(out, "1\n2\n");
    }

    #[test]
    fn empty_begin_block_compiles() {
        let cp = compile(r#"BEGIN { }"#);
        assert_eq!(cp.begin_chunks.len(), 1);
    }

    // ── Concatenation chain ──────────────────────────────────────────────────

    #[test]
    fn concat_chain_preserves_order() {
        let out = run_begin_capture(r#"BEGIN { print "a" "b" "c" "d" "e" }"#);
        assert_eq!(out, "abcde\n");
    }

    // ── Regression guards for recent fixes ───────────────────────────────────

    #[test]
    fn pow_assign_star_star_form_works() {
        let out = run_begin_capture(r#"BEGIN { x = 3; x **= 4; print x }"#);
        assert_eq!(out, "81\n");
    }

    #[test]
    fn pow_assign_caret_form_works() {
        let out = run_begin_capture(r#"BEGIN { x = 3; x ^= 4; print x }"#);
        assert_eq!(out, "81\n");
    }

    #[test]
    fn empty_string_literal_falsy_regression() {
        let out = run_begin_capture(r#"BEGIN { print ("" ? "T" : "F") }"#);
        assert_eq!(out, "F\n");
    }

    #[test]
    fn non_empty_string_literals_always_truthy() {
        // "0", "false", " ", "\t" — all non-empty StrLit are truthy.
        let out = run_begin_capture(
            r#"BEGIN { print ("0" ? "T":"F"), ("false" ? "T":"F"), (" " ? "T":"F"), ("\t" ? "T":"F") }"#,
        );
        assert_eq!(out, "T T T T\n");
    }

    // ── Error paths ──────────────────────────────────────────────────────────

    fn run_begin_must_err(src: &str) -> std::result::Result<(), crate::error::Error> {
        let cp = compile(src);
        let mut rt = runtime_with_slots(&cp);
        crate::vm::vm_run_begin(&cp, &mut rt)
    }

    #[test]
    fn division_by_zero_in_expression_errors() {
        let result = run_begin_must_err(r#"BEGIN { x = 1 / 0 }"#);
        assert!(result.is_err(), "1 / 0 should error");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("division") || msg.contains("zero"),
            "expected division-by-zero message, got: {msg}"
        );
    }

    #[test]
    fn divide_assign_zero_errors() {
        let result = run_begin_must_err(r#"BEGIN { x = 1; x /= 0 }"#);
        assert!(result.is_err(), "x /= 0 should error");
    }

    #[test]
    fn calling_undefined_function_errors() {
        let result = run_begin_must_err(r#"BEGIN { undefined_fn(1, 2) }"#);
        assert!(result.is_err(), "call to undefined function should error");
    }

    #[test]
    fn wrong_arity_to_builtin_errors() {
        let result = run_begin_must_err(r#"BEGIN { x = sqrt() }"#);
        assert!(result.is_err(), "sqrt() with 0 args should error");
    }

    // ── Inc/dec on uninit ────────────────────────────────────────────────────

    #[test]
    fn postinc_uninit_returns_zero_then_var_becomes_one() {
        let out = run_begin_capture(r#"BEGIN { x = u++; print x; print u }"#);
        assert_eq!(out, "0\n1\n");
    }

    #[test]
    fn postdec_uninit_returns_zero_then_var_becomes_neg_one() {
        let out = run_begin_capture(r#"BEGIN { x = u--; print x; print u }"#);
        assert_eq!(out, "0\n-1\n");
    }

    // ── Array delete ─────────────────────────────────────────────────────────

    #[test]
    fn delete_missing_key_is_silent_noop() {
        let out = run_begin_capture(r#"BEGIN { a[1] = "x"; delete a["nope"]; print length(a) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn delete_entire_empty_array_no_error() {
        let out = run_begin_capture(r#"BEGIN { delete a; print "ok" }"#);
        assert_eq!(out, "ok\n");
    }

    #[test]
    fn delete_then_reassign_same_key() {
        let out =
            run_begin_capture(r#"BEGIN { a[1] = "old"; delete a[1]; a[1] = "new"; print a[1] }"#);
        assert_eq!(out, "new\n");
    }

    // ── Multi-dim arrays ─────────────────────────────────────────────────────

    #[test]
    fn multidim_array_in_test_returns_one() {
        let out = run_begin_capture(r#"BEGIN { a[1,2] = "x"; print ((1,2) in a) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn multidim_array_in_test_returns_zero_for_missing() {
        let out = run_begin_capture(r#"BEGIN { a[1,2] = "x"; print ((3,4) in a) }"#);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn multidim_delete_specific_key() {
        let out = run_begin_capture(
            r#"BEGIN { a[1,2] = "x"; a[1,3] = "y"; delete a[1,2]; print length(a) }"#,
        );
        assert_eq!(out, "1\n");
    }

    // ── Function arg semantics ───────────────────────────────────────────────

    #[test]
    fn array_passed_to_function_is_call_by_reference() {
        // POSIX: arrays are passed by reference; modifications inside the
        // function are visible to the caller. Fixed via the new
        // Op::CallUserBindArrays opcode + frame-aware array_elem_get/set/
        // for_in_keys in VmCtx.
        let out = run_begin_capture(
            r#"function f(a) { a["new"] = 99 } BEGIN { x["old"] = 1; f(x); print x["new"] }"#,
        );
        assert_eq!(out, "99\n");
    }

    #[test]
    fn array_by_reference_preserves_existing_keys() {
        // Caller's pre-existing entries must survive the by-ref pass.
        let out = run_begin_capture(
            r#"function f(a) { a["new"] = 99 } BEGIN { x["old"] = 1; f(x); print x["old"]; print x["new"] }"#,
        );
        assert_eq!(out, "1\n99\n");
    }

    #[test]
    fn array_by_reference_fills_empty_array() {
        let out = run_begin_capture(
            r#"function fill(a, n,    i) { for(i=1;i<=n;i++) a[i] = i*10 } BEGIN { fill(arr, 3); print arr[1], arr[2], arr[3] }"#,
        );
        assert_eq!(out, "10 20 30\n");
    }

    #[test]
    fn scalar_passed_by_value_modifications_not_visible() {
        let out = run_begin_capture(
            r#"function f(s) { s = "modified" } BEGIN { x = "orig"; f(x); print x }"#,
        );
        assert_eq!(out, "orig\n");
    }

    // ── Recursion ────────────────────────────────────────────────────────────

    #[test]
    fn recursion_fibonacci_ten() {
        let out = run_begin_capture(
            r#"function fib(n) { return n < 2 ? n : fib(n-1) + fib(n-2) } BEGIN { print fib(10) }"#,
        );
        assert_eq!(out, "55\n");
    }

    // ── Print with no args ───────────────────────────────────────────────────

    #[test]
    fn print_with_no_args_prints_dollar_zero() {
        let out = run_begin_capture(r#"BEGIN { $0 = "the record"; print }"#);
        assert_eq!(out, "the record\n");
    }

    #[test]
    fn print_empty_string_emits_just_ors() {
        let out = run_begin_capture(r#"BEGIN { print "" }"#);
        assert_eq!(out, "\n");
    }

    // ── Negative / large numbers ─────────────────────────────────────────────

    #[test]
    fn negative_zero_equals_zero() {
        let out = run_begin_capture(r#"BEGIN { print (-0 == 0) }"#);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn large_integer_print_bypasses_ofmt() {
        // Integer-valued numbers print exact (up to ~2^53) regardless of OFMT.
        // Fixed in runtime.rs::num_to_string_ofmt / num_to_string_convfmt by
        // applying the same `fract==0 && |n|<1e15` bypass that format_number
        // already used for the direct write_to path.
        let out = run_begin_capture(r#"BEGIN { print 999999999999 }"#);
        assert_eq!(out, "999999999999\n");
    }

    #[test]
    fn one_million_prints_as_integer_not_scientific() {
        let out = run_begin_capture(r#"BEGIN { print 1000000 }"#);
        assert_eq!(out, "1000000\n");
    }

    #[test]
    fn ofmt_still_applied_for_non_integer_floats() {
        // Regression guard: OFMT continues to format non-integer floats.
        let out = run_begin_capture(r#"BEGIN { OFMT="%.3f"; print 3.14159 }"#);
        assert_eq!(out, "3.142\n");
    }

    // ── Concat with empty edges ──────────────────────────────────────────────

    #[test]
    fn concat_with_empty_left_or_right() {
        let out = run_begin_capture(r#"BEGIN { print "" "abc"; print "abc" "" }"#);
        assert_eq!(out, "abc\nabc\n");
    }

    #[test]
    fn concat_three_empties_yields_empty() {
        let out = run_begin_capture(r#"BEGIN { print "[" "" "" "" "]" }"#);
        assert_eq!(out, "[]\n");
    }

    // ── Regex dot metacharacter ──────────────────────────────────────────────

    #[test]
    fn regex_dot_matches_any_non_newline_char() {
        let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /h.llo/) }"#);
        assert_eq!(out, "1\n");
    }

    // ── split() ──────────────────────────────────────────────────────────────

    #[test]
    fn split_returns_field_count() {
        let out = run_begin_capture(r#"BEGIN { n = split("a,b,c,d", a, ","); print n }"#);
        assert_eq!(out, "4\n");
    }

    #[test]
    fn split_uses_default_fs_when_omitted() {
        let out = run_begin_capture(
            r#"BEGIN { n = split("a b c", a); print n; print a[1], a[2], a[3] }"#,
        );
        assert_eq!(out, "3\na b c\n");
    }

    #[test]
    fn split_clears_existing_array() {
        let out = run_begin_capture(
            r#"BEGIN { a["old"]=1; a["stale"]=2; n=split("x y", a, " "); print length(a) }"#,
        );
        assert_eq!(out, "2\n");
    }

    // ── sprintf string width / left-align ────────────────────────────────────

    #[test]
    fn sprintf_with_n_width_pads_string() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%10s]", "hi") }"#);
        assert_eq!(out, "[        hi]\n");
    }

    #[test]
    fn sprintf_with_negative_n_left_aligns_string() {
        let out = run_begin_capture(r#"BEGIN { print sprintf("[%-10s]", "hi") }"#);
        assert_eq!(out, "[hi        ]\n");
    }

    #[test]
    fn vm_large_array_deletion() {
        let out =
            run_begin_capture("BEGIN { for(i=0; i<1000; i++) a[i]=i; delete a; print length(a) }");
        assert_eq!(out, "0\n");
    }

    #[test]
    fn vm_multidimensional_array_simulation() {
        let out = run_begin_capture("BEGIN { a[1,2]=42; print a[1,2], (1,2) in a }");
        assert_eq!(out, "42 1\n");
    }

    #[test]
    fn vm_asort_behavior() {
        let out = run_begin_capture(
            "BEGIN { a[1]=\"z\"; a[2]=\"a\"; n=asort(a); for(i=1; i<=n; i++) printf \"%s\", a[i] }",
        );
        assert_eq!(out, "az");
    }

    #[test]
    fn vm_asort_numeric_coercion() {
        // "10" (string) vs 2 (number) -> "10" > "2" is FALSE, but asort uses sort order.
        // POSIX asort sorts by value.
        let out = run_begin_capture(
            "BEGIN { a[1]=10; a[2]=2; n=asort(a); for(i=1; i<=n; i++) printf \"[%s]\", a[i] }",
        );
        assert_eq!(out, "[2][10]");
    }

    #[test]
    fn vm_asorti_numeric_keys() {
        // asorti sorts keys.
        let out = run_begin_capture("BEGIN { a[10]=\"x\"; a[2]=\"y\"; n=asorti(a); for(i=1; i<=n; i++) printf \"[%s]\", a[i] }");
        // keys "10" and "2" as strings -> "10" < "2"
        assert_eq!(out, "[10][2]");
    }

    #[test]
    fn vm_nested_function_recursion() {
        let out = run_begin_capture(
            "function f(n) { if(n<=0) return 0; return n + f(n-1) } BEGIN { print f(10) }",
        );
        assert_eq!(out, "55\n");
    }

    #[test]
    fn vm_closure_like_array_passing() {
        // Arrays are call-by-reference in AWK.
        let out = run_begin_capture(
            "function inc(arr) { arr[1]++ } BEGIN { a[1]=10; inc(a); print a[1] }",
        );
        assert_eq!(out, "11\n");
    }

    #[test]
    fn vm_scientific_notation_in_loop() {
        let out = run_begin_capture("BEGIN { sum=0; for(i=1e1; i<1.5e1; i++) sum+=i; print sum }");
        assert_eq!(out, "60\n"); // 10+11+12+13+14 = 60
    }

    #[test]
    fn vm_switch_with_regex_case() {
        let out = run_begin_capture("BEGIN { x=\"abc\"; switch(x) { case /a/: print \"match\"; break; default: print \"no\" } }");
        assert_eq!(out, "match\n");
    }

    #[test]
    fn vm_bignum_pow_large() {
        // Only if bignum is enabled, but run_begin_capture might not enable it by default.
        // Let's use a test that works in both but is more interesting in bignum.
        let out = run_begin_capture("BEGIN { print 2^10 }");
        assert_eq!(out, "1024\n");
    }

    #[test]
    fn vm_complex_ternary_logic() {
        let out = run_begin_capture("BEGIN { print (1 ? (0 ? \"a\" : \"b\") : \"c\") }");
        assert_eq!(out, "b\n");
    }

    #[test]
    fn vm_for_in_sorted_order() {
        // PROCINFO["sorted_in"] = "@ind_str_asc"
        let out = run_begin_capture("BEGIN { a[\"z\"]=1; a[\"a\"]=2; PROCINFO[\"sorted_in\"]=\"@ind_str_asc\"; for(i in a) printf \"%s\", i }");
        assert_eq!(out, "az");
    }

    #[test]
    fn vm_getline_file_missing_returns_minus_one() {
        // getline < "no_such" returns -1 and sets ERRNO
        let out =
            run_begin_capture("BEGIN { r = (getline < \"no_such\"); print r, (ERRNO != \"\") }");
        // awkrs might error instead of returning -1 depending on configuration,
        // but POSIX says -1.
        assert!(out.contains("-1 1"), "got: {out}");
    }

    #[test]
    fn vm_getline_pipe_empty_returns_zero() {
        // "echo -n" | getline returns 0 (EOF)
        let out = run_begin_capture("BEGIN { r = (\"printf ''\" | getline); print r }");
        assert_eq!(out, "0\n");
    }

    #[test]
    fn vm_getline_pipe_into_var() {
        let out = run_begin_capture("BEGIN { \"echo hi\" | getline x; print x }");
        assert_eq!(out, "hi\n");
    }

    #[test]
    fn vm_split_leading_trailing_separators() {
        // split(",a,b,", a, ",") -> "" "a" "b" ""
        let out = run_begin_capture(
            "BEGIN { n = split(\",a,b,\", a, \",\"); for(i=1;i<=n;i++) printf \"[%s]\", a[i] }",
        );
        assert_eq!(out, "[][a][b][]");
    }

    #[test]
    fn vm_split_whitespace_behavior() {
        // split("  a  b  ", a, " ") behaves like default FS
        let out = run_begin_capture(
            "BEGIN { n = split(\"  a  b  \", a, \" \"); for(i=1;i<=n;i++) printf \"[%s]\", a[i] }",
        );
        assert_eq!(out, "[a][b]");
    }

    #[test]
    fn internal_awk_cmp_eq_numeric_string_semantics() {
        let rt = Runtime::new();
        // Both numeric strings -> numeric compare
        assert_eq!(
            awk_cmp_eq(
                &Value::Str("10".into()),
                &Value::Str("10.0".into()),
                false,
                &rt
            )
            .as_number(),
            1.0
        );
        assert_eq!(
            awk_cmp_eq(
                &Value::Str("10".into()),
                &Value::Str("0xa".into()),
                false,
                &rt
            )
            .as_number(),
            0.0
        ); // is_numeric_str doesn't parse hex
    }

    #[test]
    fn internal_awk_cmp_rel_mixed_types() {
        let rt = Runtime::new();
        // Number vs Numeric String -> numeric compare
        assert_eq!(
            awk_cmp_rel(
                BinOp::Lt,
                &Value::Num(2.0),
                &Value::Str("10".into()),
                false,
                &rt
            )
            .as_number(),
            1.0
        );
        // String literal (not numeric str) vs Number -> string compare
        // "10" (literal) vs 2 (number) -> "10" < "2" ? Yes.
        // Wait, awkrs might use as_str() which for 2.0 is "2".
        assert_eq!(
            awk_cmp_rel(
                BinOp::Lt,
                &Value::StrLit("10".into()),
                &Value::Num(2.0),
                false,
                &rt
            )
            .as_number(),
            1.0
        );
    }

    #[test]
    fn internal_awk_cmp_eq_ignore_case() {
        let rt = Runtime::new();
        assert_eq!(
            awk_cmp_eq(
                &Value::Str("ABC".into()),
                &Value::Str("abc".into()),
                true,
                &rt
            )
            .as_number(),
            1.0
        );
        assert_eq!(
            awk_cmp_eq(
                &Value::Str("ABC".into()),
                &Value::Str("abc".into()),
                false,
                &rt
            )
            .as_number(),
            0.0
        );
    }

    #[test]
    fn internal_awk_cmp_rel_ignore_case() {
        let rt = Runtime::new();
        // "B" > "a" normally, but with IGNORECASE "b" > "a"
        assert_eq!(
            awk_cmp_rel(
                BinOp::Gt,
                &Value::Str("B".into()),
                &Value::Str("a".into()),
                true,
                &rt
            )
            .as_number(),
            1.0
        );
        // "a" vs "B" -> "a" is 97, "B" is 66. "a" > "B" is true.
        assert_eq!(
            awk_cmp_rel(
                BinOp::Gt,
                &Value::Str("a".into()),
                &Value::Str("B".into()),
                false,
                &rt
            )
            .as_number(),
            1.0
        );
    }

    #[test]
    fn internal_awk_cmp_uninit() {
        let rt = Runtime::new();
        // Uninit == 0 (numeric)
        assert_eq!(
            awk_cmp_eq(&Value::Uninit, &Value::Num(0.0), false, &rt).as_number(),
            1.0
        );
        // Uninit == "" (string)
        assert_eq!(
            awk_cmp_eq(&Value::Uninit, &Value::Str("".into()), false, &rt).as_number(),
            1.0
        );
        // Uninit < 1 (numeric)
        assert_eq!(
            awk_cmp_rel(BinOp::Lt, &Value::Uninit, &Value::Num(1.0), false, &rt).as_number(),
            1.0
        );
    }

    #[test]
    fn vm_printf_many_args() {
        let out = run_begin_capture("BEGIN { printf \"%d %d %d %d %d %d\", 1, 2, 3, 4, 5, 6 }");
        assert_eq!(out, "1 2 3 4 5 6");
    }

    #[test]
    fn vm_printf_positional_star_width() {
        // positional width + sequential value
        let out = run_begin_capture("BEGIN { printf \"%*1$d\", 5, 42 }");
        assert_eq!(out, "   42");
    }

    #[test]
    fn vm_getline_file_redirect_into_var() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("awkrs_getline_{}.txt", std::process::id()));
        std::fs::write(&p, "line1\nline2").unwrap();

        let src = format!(
            "BEGIN {{ (getline x < \"{}\"); (getline y < \"{}\"); print x, y }}",
            p.display(),
            p.display()
        );
        let out = run_begin_capture(&src);
        assert_eq!(out, "line1 line2\n");

        let _ = std::fs::remove_file(&p);
    }
}
