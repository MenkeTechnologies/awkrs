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

    /// `arr[key]` — `SYMTAB` uses live global/slot resolution (gawk lvalue semantics).
    fn array_elem_get(&self, name: &str, key: &str) -> Value {
        if name == "SYMTAB" {
            return self.rt.symtab_elem_get(key);
        }
        self.rt.array_get(name, key)
    }

    fn array_elem_set(&mut self, name: &str, key: String, val: Value) {
        if name == "SYMTAB" {
            self.rt.symtab_elem_set(&key, val);
            return;
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
        if let SortedInMode::CustomFn(fname) = sorted_in_mode(self.rt) {
            if name == "SYMTAB" {
                let mut keys = self.rt.symtab_keys_reflect();
                if self.rt.posix {
                    return Ok(keys);
                }
                sort_keys_with_custom_cmp(self, &mut keys, fname.as_str(), name)?;
                return Ok(keys);
            }
            let Some(Value::Array(a)) = self.rt.get_global_var(name) else {
                return Ok(Vec::new());
            };
            let mut keys: Vec<String> = a.keys().cloned().collect();
            if self.rt.posix {
                return Ok(keys);
            }
            sort_keys_with_custom_cmp(self, &mut keys, fname.as_str(), name)?;
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
        Value::Str("uninitialized".into())
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
            Ok(rt.record.contains(pat))
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
            Ok(rt.record.contains(pat))
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
    Value::Str("uninitialized".into())
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
                "uninitialized"
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

// ── Core VM loop ────────────────────────────────────────────────────────────

fn execute(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<VmSignal> {
    let ops = &chunk.ops;
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
                let s = val.as_str();
                ctx.rt.set_field(idx, &s)?;
                ctx.push(val);
            }
            Op::GetArrayElem(arr) => {
                let key_val = ctx.pop();
                let k = key_val.as_str_cow();
                let name = ctx.str_ref(arr);
                let v = ctx.array_elem_get(name, k.as_ref());
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
                let t = builtins::awk_typeof_value(&ctx.rt.slots[slot as usize]);
                ctx.push(Value::Str(t.into()));
            }
            Op::TypeofArrayElem(arr) => {
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
                let i = ctx.pop().as_number() as i32;
                if i < 0 {
                    return Err(Error::Runtime("attempt to access field number -1".into()));
                }
                let t = if ctx.rt.field_is_unassigned(i) {
                    "uninitialized"
                } else {
                    "string"
                };
                ctx.push(Value::Str(t.into()));
            }
            Op::TypeofValue => {
                let v = ctx.pop();
                let t = builtins::awk_typeof_value(&v);
                ctx.push(Value::Str(t.into()));
            }
            Op::SetArrayElem(arr) => {
                let val = ctx.pop();
                let key = ctx.pop().into_string();
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
                    ctx.push(Value::Mpfr(Float::with_val_round(prec, &fa % &fb, round).0));
                } else {
                    let b = ctx.pop();
                    let a = ctx.pop();
                    a.reject_if_array_scalar()?;
                    b.reject_if_array_scalar()?;
                    ctx.push(Value::Num(a.as_number() % b.as_number()));
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
                ctx.rt.array_delete(&name, None);
            }
            Op::DeleteElem(arr) => {
                let key_val = ctx.pop();
                let k = key_val.as_str_cow();
                let name = ctx.cp.strings.get(arr);
                if name == "SYMTAB" {
                    ctx.symtab_delete(k.as_ref());
                } else {
                    ctx.rt.array_delete(name, Some(k.as_ref()));
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
            Op::Split { arr, has_fs } => {
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
                let parts = crate::runtime::split_string_by_field_separator(
                    &s,
                    &fs,
                    ctx.rt.ignore_case_flag(),
                );
                let n = parts.len();
                ctx.rt.split_into_array(&arr_name, &parts);
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
                let pool_str = ctx.cp.strings.get(idx);
                // Reuse the TOS String allocation: pop, append, push back.
                let v = ctx.pop();
                let lit = matches!(v, Value::StrLit(_));
                let mut s = v.into_string();
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
    let ls = a.as_str_cow();
    let rs = b.as_str_cow();
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
    let ls = a.as_str_cow();
    let rs = b.as_str_cow();
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
    format::awk_sprintf_with_decimal(fmt, vals, dec, thousands_sep, mpfr)
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
            let code = ctx.rt.getline_error_code_for_key(&e, &input_key);
            if push_result {
                ctx.push(Value::Num(code));
                Ok(())
            } else {
                Err(e)
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
            } else if let Some(b) = hay.find(needle.as_str()) {
                let pos = if ctx.rt.characters_as_bytes {
                    b + 1
                } else {
                    hay[..b].chars().count() + 1
                };
                Value::Num(pos as f64)
            } else {
                Value::Num(0.0)
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
            if argc != 2 {
                return Err(Error::Runtime("`and` expects two arguments".into()));
            }
            bignum::awk_and_values(&args[0], &args[1], ctx.rt)
        }
        "or" => {
            if argc != 2 {
                return Err(Error::Runtime("`or` expects two arguments".into()));
            }
            bignum::awk_or_values(&args[0], &args[1], ctx.rt)
        }
        "xor" => {
            if argc != 2 {
                return Err(Error::Runtime("`xor` expects two arguments".into()));
            }
            bignum::awk_xor_values(&args[0], &args[1], ctx.rt)
        }
        "lshift" => {
            if argc != 2 {
                return Err(Error::Runtime("`lshift` expects two arguments".into()));
            }
            bignum::awk_lshift_values(&args[0], &args[1], ctx.rt)
        }
        "rshift" => {
            if argc != 2 {
                return Err(Error::Runtime("`rshift` expects two arguments".into()));
            }
            bignum::awk_rshift_values(&args[0], &args[1], ctx.rt)
        }
        "compl" => {
            if argc != 1 {
                return Err(Error::Runtime("`compl` expects one argument".into()));
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
        Ok(VmSignal::Normal) => Value::Str(String::new()),
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
}
