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
use std::io::{self, Write};
use std::mem;

/// Max interned identifier length resolved via stack buffer (`with_short_pool_name_mut`).
const POOL_NAME_STACK_MAX: usize = 128;

// ── VM context ──────────────────────────────────────────────────────────────

struct ForInState {
    keys: Vec<String>,
    index: usize,
}
/// `VmCtx` — see fields for the structure layout.
pub struct VmCtx<'a> {
    /// `cp` field.
    pub cp: &'a CompiledProgram,
    /// `rt` field.
    pub rt: &'a mut Runtime,
    locals: Vec<AwkMap<String, Value>>,
    in_function: bool,
    print_out: Option<&'a mut Vec<String>>,
    for_in_iters: Vec<ForInState>,
    stack: Vec<Value>,
}

impl<'a> VmCtx<'a> {
    /// `new` — see implementation for the contract.
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
    /// `with_print_capture` — see implementation for the contract.
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

    /// Frame-aware snapshot of an array's `(key, value)` pairs for the
    /// sort builtins (`asort` / `asorti`). Walks the locals stack innermost-out
    /// so an array passed by reference into a user function is found in the
    /// frame rather than falling through to the (empty) global by that name.
    ///
    /// Returns `Ok(Vec::new())` for unassigned names (gawk parity); a scalar
    /// at the same name is a fatal "`{name}` is not an array".
    fn array_pairs_for_sort(&self, name: &str, fn_name: &str) -> Result<Vec<(String, Value)>> {
        for frame in self.locals.iter().rev() {
            match frame.get(name) {
                Some(Value::Array(a)) => {
                    return Ok(a.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                }
                Some(Value::Uninit) | None => {}
                Some(_) => {
                    return Err(Error::Runtime(format!(
                        "{fn_name}: `{name}` is not an array"
                    )));
                }
            }
        }
        match self.rt.get_global_var(name) {
            Some(Value::Array(a)) => Ok(a.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
            None => Ok(Vec::new()),
            _ => Err(Error::Runtime(format!(
                "{fn_name}: `{name}` is not an array"
            ))),
        }
    }

    /// Frame-aware array replace. The new array contents replace whatever
    /// lives at `name` — if `name` is bound in a local frame (function array
    /// param) the frame's slot is overwritten; otherwise the global var map
    /// receives the array. Mirrors the lookup order used by
    /// [`Self::array_pairs_for_sort`] so `asort` / `asorti` writeback hits the
    /// caller's storage.
    fn array_replace(&mut self, name: &str, pairs: Vec<(String, Value)>) {
        let mut new_map = AwkMap::default();
        for (k, v) in pairs {
            new_map.insert(k, v);
        }
        for frame in self.locals.iter_mut().rev() {
            if let Some(slot) = frame.get_mut(name) {
                *slot = Value::Array(new_map);
                return;
            }
        }
        self.rt.vars.insert(name.to_string(), Value::Array(new_map));
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
    /// `Normal` variant.
    Normal,
    /// `Next` variant.
    Next,
    /// `NextFile` variant.
    NextFile,
    /// `Return` variant.
    Return(Value),
    /// `ExitPending` variant.
    ExitPending,
}

// ── Public API ──────────────────────────────────────────────────────────────
/// `vm_run_begin` — see implementation for the contract.
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
/// `vm_run_end` — see implementation for the contract.
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
/// `vm_run_beginfile` — see implementation for the contract.
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
/// `vm_run_endfile` — see implementation for the contract.
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

// ── fusevm dispatch (shared VM + JIT) ──────────────────────────────────────

/// Seed a freshly-constructed fusevm VM's base-frame slots from awkrs runtime
/// slot values (coerced to `f64`). Slots flow in as *data* rather than baked
/// `LoadFloat` constants so the translated chunk's `op_hash` is stable across
/// records — a prerequisite for fusevm's op_hash-keyed block-JIT warmup and
/// persistent on-disk native cache to engage.
/// Walk a built fusevm chunk and collect the sorted-unique set of slot
/// indices it WRITES to via `fusevm::Op::SetSlot`. Computed once per chunk
/// at cache time so the per-record writeback only iterates the modified
/// slots instead of walking 0..N runtime slots. Awkrs's bridge translates
/// every slot-write (assign, compound-assign, incr/decr) into a sequence
/// ending in `SetSlot`, so this scan captures the full write set.
fn compute_written_slots(chunk: &fusevm::Chunk) -> Vec<u16> {
    let mut v: Vec<u16> = chunk
        .ops
        .iter()
        .filter_map(|op| match op {
            fusevm::Op::SetSlot(idx) => Some(*idx),
            _ => None,
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Direct-iter seed from awkrs runtime slots — no intermediate `Vec<f64>`
/// allocation. Per-record hot path: `try_fusevm_dispatch` /
/// `run_fusevm_region` call this every record. (A narrow-read-slots version
/// was tried but regressed: for typical awkrs slot counts of 3-5, the
/// per-iteration bounds check on `slots.get(idx)` costs more than the
/// saved `set_slot` calls. The flat-iter version has no bounds check and
/// wins for small N.)
fn seed_fusevm_slots_from_runtime(vm: &mut fusevm::VM, slots: &[Value]) {
    for (i, slot) in slots.iter().enumerate() {
        vm.set_slot(i as u16, fusevm::Value::Float(slot.as_number()));
    }
}

thread_local! {
    /// Pointer to the awkrs [`Runtime`] currently executing a fusevm chunk on
    /// this thread, used by [`awkrs_fusevm_field_num_hook`] to satisfy
    /// `$N` numeric reads inside JIT-compiled chunks. Set by
    /// [`FieldHookGuard::new`] before `vm.run()` and cleared on drop, so the
    /// hook only sees a live runtime during a real dispatch.
    static AWKRS_RT_PTR: std::cell::Cell<*mut Runtime> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Hook installed in fusevm (via [`fusevm::set_awk_field_num_hook`]) so the
/// JIT-compiled `fusevm::Op::AwkGetFieldNum` can read the active awk record's
/// `$idx` field as a number. Reads `AWKRS_RT_PTR` for the active runtime;
/// returns `0.0` when no dispatch is in flight (matches awk's missing-field
/// coercion) or when the underlying `field_as_number` call errors (only
/// possible on `idx < 0`, which the bytecode emitter never produces).
extern "C" fn awkrs_fusevm_field_num_hook(idx: i64) -> f64 {
    let ptr = AWKRS_RT_PTR.with(|c| c.get());
    if ptr.is_null() {
        return 0.0;
    }
    // SAFETY: AWKRS_RT_PTR is non-null only between FieldHookGuard::new and its
    // Drop. The guard is constructed from `&mut Runtime` and outlives every
    // hook call that fires inside `vm.run()`. fusevm calls the hook only on
    // the same thread that ran `set_awk_field_num_hook`, so there's no
    // cross-thread aliasing.
    let rt = unsafe { &mut *ptr };
    rt.field_as_number(idx as i32).unwrap_or(0.0)
}

/// RAII guard: install the fusevm field-num hook and stash the active Runtime
/// pointer for the duration of a chunk dispatch. Restoration happens on drop,
/// so panics during `vm.run()` still tear the TLS down cleanly. Reentrant
/// (nested dispatches save and restore the previous pointer).
struct FieldHookGuard {
    prev_ptr: *mut Runtime,
}

impl FieldHookGuard {
    fn new(rt: &mut Runtime) -> Self {
        let new_ptr: *mut Runtime = rt as *mut _;
        let prev_ptr = AWKRS_RT_PTR.with(|c| c.replace(new_ptr));
        // Re-install on every call (cheap) so a host that toggles the hook
        // between dispatches doesn't observe a stale binding.
        fusevm::set_awk_field_num_hook(Some(awkrs_fusevm_field_num_hook));
        Self { prev_ptr }
    }
}

impl Drop for FieldHookGuard {
    fn drop(&mut self) {
        AWKRS_RT_PTR.with(|c| c.set(self.prev_ptr));
        if self.prev_ptr.is_null() {
            fusevm::set_awk_field_num_hook(None);
        }
    }
}

/// Translate an awkrs chunk to a fusevm chunk and execute via fusevm's VM.
/// Eligibility + the op→fusevm translation live in
/// `fusevm_bridge::build_numeric_chunk` (the single source of truth); this
/// wrapper marshals awkrs slot values in, runs, and writes modified slots back.
fn try_fusevm_dispatch(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<Option<VmSignal>> {
    let ops = &chunk.ops;
    let cp = ctx.cp;
    let slot_count = ctx.rt.slots.len();

    // Per-(chunk pointer, bignum) cache for the built fusevm::Chunk. The
    // translation is identical across every dispatch on the same awkrs Chunk
    // (chunks are immutable for the program's lifetime); caching skips the
    // eligibility check + 2-pass op→fusevm translation that
    // build_numeric_chunk would otherwise repeat per record. `Some(None)` =
    // checked-and-not-eligible (short-circuit). bignum is in the key because
    // it affects eligibility (numeric ops on Mpfr aren't lowered).
    let cache_key = (chunk as *const Chunk as usize, ctx.rt.bignum);
    // Side-table fast path: 99%+ of dispatches in a tight record loop hit
    // the last-seen chunk. One tuple compare + Arc::clone, no HashMap.
    let cached_arc: Option<std::sync::Arc<(fusevm::Chunk, Vec<u16>)>> =
        if cache_key == ctx.rt.fuse_last_chunk_key {
            ctx.rt.fuse_last_chunk_value.clone()
        } else {
            // HashMap fallback. Also populate / update the side cache so
            // subsequent dispatches of THIS chunk skip the HashMap.
            let entry = match ctx.rt.fuse_chunk_cache.get(&cache_key) {
                Some(v) => v.clone(),
                None => {
                    let built = crate::fusevm_bridge::build_numeric_chunk(
                        ops,
                        ctx.rt.bignum,
                        |idx| cp.strings.get(idx).parse::<f64>().unwrap_or(0.0),
                        |idx| cp.strings.get(idx),
                    );
                    let cache_entry = built.map(|c| {
                        let w = compute_written_slots(&c);
                        std::sync::Arc::new((c, w))
                    });
                    ctx.rt.fuse_chunk_cache.insert(cache_key, cache_entry.clone());
                    cache_entry
                }
            };
            ctx.rt.fuse_last_chunk_key = cache_key;
            ctx.rt.fuse_last_chunk_value = entry.clone();
            entry
        };
    let (fuse_chunk, written_slots) = match cached_arc {
        Some(arc) => (arc.0.clone(), arc.1.clone()),
        None => return Ok(None),
    };

    // Execute via fusevm VM. Enable the tracing JIT so fusevm's Cranelift
    // tiers engage: block JIT for fully-eligible chunks (whole chunk → native,
    // warmed across records via fusevm's op_hash-keyed TLS cache) and the
    // tracing JIT for hot in-chunk loops (e.g. BEGIN/END counted loops). Both
    // tiers round-trip awk's f64 slots via fusevm's SlotKind bit-pattern model;
    // cold/one-shot chunks fall through to the interpreter at zero extra cost.
    // Acquire a recycled VM from the pool (or allocate a fresh one if the
    // pool is empty). `VM::reset(chunk)` preserves Vec capacities (stack,
    // frames, slot_buf, …) so subsequent records reuse the underlying
    // allocations. For an awk one-liner over millions of records this skips
    // per-record VM allocation of the VM's stack/frame/global storage.
    let mut vm = ctx.rt.fuse_vm_pool.acquire(fuse_chunk);
    // Seed only the slots the chunk READS (precomputed once at cache time).
    // For `{ sum += $1 }` that's 1 slot (sum) instead of all N runtime slots
    // (NR/NF/FS/sum/…). Cuts per-record seeding work proportionally.
    seed_fusevm_slots_from_runtime(&mut vm, &ctx.rt.slots);
    vm.enable_tracing_jit();
    // Install the field-num hook for the duration of vm.run(). Chunks that
    // never emit AwkGetFieldNum pay only the (cheap) TLS write.
    let _hook_guard = FieldHookGuard::new(ctx.rt);
    let result = vm.run();
    drop(_hook_guard);

    // Write back only the slots the chunk actually MODIFIES (precomputed
    // once at cache time from the chunk's `Op::SetSlot` instances — see
    // `compute_written_slots`). Old loop walked 0..slot_count on every
    // record; for an awk program like `{ sum += $1 }` only one slot is
    // written, so this drops O(N) writeback work to O(K) where K is
    // typically 1-3 even for moderately complex programs.
    if let Some(frame) = vm.frames.last() {
        let max_idx = frame.slots.len().min(slot_count);
        for &slot_idx in &written_slots {
            let idx = slot_idx as usize;
            if idx >= max_idx {
                continue;
            }
            match &frame.slots[idx] {
                fusevm::Value::Float(f) => ctx.rt.slots[idx] = Value::Num(*f),
                fusevm::Value::Int(n) => ctx.rt.slots[idx] = Value::Num(*n as f64),
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
    let signal = match result {
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
    };
    // Return the VM to the pool — allocations preserved for the next record.
    ctx.rt.fuse_vm_pool.release(vm);
    signal
}

/// Run an eligible numeric *prefix region* (`ops`, identified by
/// [`fusevm_bridge::eligible_loop_prefix`]) on fusevm with the tracing JIT
/// enabled, then write the resulting slot values back into the awkrs runtime.
/// Unlike [`try_fusevm_dispatch`] this leaves nothing on the awkrs stack — the
/// region is stack-neutral by construction — so the awkrs interpreter resumes
/// cleanly at the op following the region.
fn run_fusevm_region(ops: &[Op], ctx: &mut VmCtx<'_>) -> Result<bool> {
    let cp = ctx.cp;
    let slot_count = ctx.rt.slots.len();

    // Per-(slice base, slice len, bignum) cache. The slice is &chunk.ops[..k]
    // where k is determined by `eligible_loop_prefix` on the same chunk —
    // same chunk → same k → same (ptr, len). Caching skips repeated
    // build_numeric_chunk work per record. See `Runtime::fuse_prefix_chunk_cache`.
    let cache_key = (ops.as_ptr() as usize, ops.len(), ctx.rt.bignum);
    let cached_arc: Option<std::sync::Arc<(fusevm::Chunk, Vec<u16>)>> =
        match ctx.rt.fuse_prefix_chunk_cache.get(&cache_key) {
            Some(v) => v.clone(),
            None => {
                let built = crate::fusevm_bridge::build_numeric_chunk(
                    ops,
                    ctx.rt.bignum,
                    |idx| cp.strings.get(idx).parse::<f64>().unwrap_or(0.0),
                    |idx| cp.strings.get(idx),
                );
                let cache_entry = built.map(|c| {
                    let w = compute_written_slots(&c);
                    std::sync::Arc::new((c, w))
                });
                ctx.rt.fuse_prefix_chunk_cache.insert(cache_key, cache_entry.clone());
                cache_entry
            }
        };
    let (fuse_chunk, written_slots) = match cached_arc {
        Some(arc) => (arc.0.clone(), arc.1.clone()),
        // The region couldn't be lowered. Report "didn't run" so the
        // caller falls back to the interpreter. (Should be
        // unreachable while `eligible_loop_prefix`'s `stack_delta`
        // stays a subset of `is_fusevm_eligible`.)
        None => return Ok(false),
    };

    // Pool-acquired VM (see `try_fusevm_dispatch` for the rationale).
    let mut vm = ctx.rt.fuse_vm_pool.acquire(fuse_chunk);
    seed_fusevm_slots_from_runtime(&mut vm, &ctx.rt.slots);
    vm.enable_tracing_jit();
    // This region is always a hot loop (`eligible_loop_prefix` requires a
    // backward jump) but the offloaded chunk runs `vm.run()` exactly once, so
    // the block JIT's default warmup threshold (compile on the 2nd invocation)
    // would never trip — the loop would run on fusevm's *interpreter*, which is
    // slower than awkrs's own. Force eager block compilation (threshold 0) for
    // the duration of this run so the loop JIT-compiles on its single
    // invocation, then restore the prior thread config. Measured: a 30M-iter
    // numeric loop drops from ~21s (interpreter offload) / ~11s (awkrs
    // interpreter) to ~0.1s (eager native).
    let jit = fusevm::JitCompiler::new();
    let saved_cfg = jit.get_config();
    let mut eager_cfg = saved_cfg;
    eager_cfg.block_threshold = 0;
    jit.set_config(eager_cfg);
    // Install the field-num hook for the duration of vm.run() (same as
    // `try_fusevm_dispatch`).
    let _hook_guard = FieldHookGuard::new(ctx.rt);
    let result = vm.run();
    drop(_hook_guard);
    jit.set_config(saved_cfg);
    if let fusevm::VMResult::Error(msg) = result {
        ctx.rt.fuse_vm_pool.release(vm);
        return Err(Error::Runtime(msg));
    }

    if let Some(frame) = vm.frames.last() {
        let max_idx = frame.slots.len().min(slot_count);
        for &slot_idx in &written_slots {
            let idx = slot_idx as usize;
            if idx >= max_idx {
                continue;
            }
            match &frame.slots[idx] {
                fusevm::Value::Float(f) => ctx.rt.slots[idx] = Value::Num(*f),
                fusevm::Value::Int(n) => ctx.rt.slots[idx] = Value::Num(*n as f64),
                _ => {}
            }
        }
    }
    ctx.rt.fuse_vm_pool.release(vm);
    Ok(true)
}

// ── Core VM loop ────────────────────────────────────────────────────────────

/// Whether fusevm dispatch is enabled. **On by default**; set `AWKRS_FUSEVM=0`
/// to force the awkrs interpreter.
///
/// Eligible chunks (`is_fusevm_eligible`) are pure-numeric and semantically
/// identical on fusevm's shared VM, so routing them through fusevm's executor +
/// persistent JIT cache is safe by default. Division/modulo stay eligible: the
/// bridge lowers them to the awk-specific `fusevm::Op::AwkDiv`/`AwkMod`, which
/// raise awkrs's fatal "division by zero attempted" error on a zero divisor
/// (POSIX awk) — distinct from fusevm's shared `Op::Div`/`Op::Mod` (used by
/// zshrs/stryke shell arithmetic), which yield `Undef`/`0`. So a divide-by-zero
/// surfaces faithfully on the fusevm path. `AWKRS_JIT=0` / `--no-optimize` still
/// force the interpreter regardless. Read once and cached to keep `execute()`
/// (per-record hot path) allocation- and syscall-free.
fn fusevm_dispatch_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("AWKRS_FUSEVM")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

fn execute(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<VmSignal> {
    let ops = &chunk.ops;
    let mut pc: usize = 0;
    // Tier 1: optionally offload eligible numeric chunks to fusevm's shared VM.
    // Gated by `jit_enabled` (`--no-optimize` / `AWKRS_JIT=0` force the awkrs
    // interpreter) AND `fusevm_dispatch_enabled()` (on by default; `AWKRS_FUSEVM=0`
    // forces the interpreter).
    if ctx.rt.jit_enabled && fusevm_dispatch_enabled() {
        match try_fusevm_dispatch(chunk, ctx) {
            Ok(Some(signal)) => return Ok(signal),
            Ok(None) => {
                // Whole chunk wasn't eligible (e.g. a trailing `print`); offload
                // just the eligible numeric loop prefix to fusevm's JIT, then
                // resume the interpreter at the first non-numeric op.
                if let Some(k) =
                    crate::fusevm_bridge::eligible_loop_prefix(ops, ctx.rt.bignum, |idx| {
                        ctx.cp.strings.get(idx)
                    })
                {
                    if run_fusevm_region(&ops[..k], ctx)? {
                        pc = k;
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
    let len = ops.len();

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
                // gawk parity: reading `x[k]` where `x` is a scalar is a fatal
                // "attempt to use scalar `x' as an array". POSIX auto-creates
                // arrays from missing names; we only error on existing non-array,
                // non-uninit values.
                check_array_target(ctx, &name)?;
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
                // gawk parity: `x[k] = …` on a scalar `x` is a fatal "attempt
                // to use scalar `x' as an array". Check both the local frames
                // and globals before delegating to `array_elem_set` (which
                // would silently overwrite a scalar with an array).
                check_array_target(ctx, name)?;
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
                        return Err(Error::Runtime("division by zero attempted in `%'".into()));
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
                        return Err(Error::Runtime("division by zero attempted in `%'".into()));
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
                let name = ctx.str_ref(arr).to_string();
                // gawk parity: `key in x` on a scalar `x` raises "attempt to
                // use scalar `x' as an array". Earlier awkrs returned 0.
                check_array_target(ctx, &name)?;
                let b = if name == "SYMTAB" {
                    ctx.symtab_has(k.as_ref())
                } else {
                    // Frame-aware: a function array parameter lives in
                    // `ctx.locals`, not the global var map. Walk frames first
                    // so `(key in arr)` inside a user function sees the
                    // caller's data; fall through to the global lookup
                    // otherwise.
                    let mut found = None;
                    for frame in ctx.locals.iter().rev() {
                        match frame.get(name.as_str()) {
                            Some(Value::Array(a)) => {
                                found = Some(a.contains_key(k.as_ref()));
                                break;
                            }
                            Some(Value::Uninit) | None => {}
                            Some(_) => break,
                        }
                    }
                    found.unwrap_or_else(|| ctx.rt.array_has(&name, k.as_ref()))
                };
                ctx.push(Value::Num(if b { 1.0 } else { 0.0 }));
            }
            Op::DeleteArray(arr) => {
                let name = ctx.str_ref(arr).to_string();
                // gawk parity: `delete x` on a scalar variable is a fatal
                // "attempt to use scalar `x' as an array". Unassigned names
                // silently no-op (POSIX). Check frames and globals for a
                // non-array, non-uninit value before falling through.
                let mut handled = false;
                let mut scalar_err = false;
                for frame in ctx.locals.iter_mut().rev() {
                    match frame.get_mut(name.as_str()) {
                        Some(Value::Array(a)) => {
                            a.clear();
                            handled = true;
                            break;
                        }
                        Some(Value::Uninit) | None => {}
                        Some(_) => {
                            scalar_err = true;
                            break;
                        }
                    }
                }
                if scalar_err {
                    return Err(Error::Runtime(format!(
                        "attempt to use scalar `{name}' as an array"
                    )));
                }
                if !handled {
                    match ctx.rt.get_global_var(&name) {
                        Some(Value::Array(_)) | None | Some(Value::Uninit) => {
                            ctx.rt.array_delete(&name, None);
                        }
                        Some(_) => {
                            return Err(Error::Runtime(format!(
                                "attempt to use scalar `{name}' as an array"
                            )));
                        }
                    }
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
                    // frame, not global vars. Like DeleteArray above, a scalar
                    // value at that name is a fatal "attempt to use scalar as
                    // an array" (gawk parity).
                    let mut handled = false;
                    let mut scalar_err = false;
                    for frame in ctx.locals.iter_mut().rev() {
                        match frame.get_mut(name) {
                            Some(Value::Array(map)) => {
                                map.remove(k.as_ref());
                                handled = true;
                                break;
                            }
                            Some(Value::Uninit) | None => {}
                            Some(_) => {
                                scalar_err = true;
                                break;
                            }
                        }
                    }
                    if scalar_err {
                        return Err(Error::Runtime(format!(
                            "attempt to use scalar `{name}' as an array"
                        )));
                    }
                    if !handled {
                        match ctx.rt.get_global_var(name) {
                            Some(Value::Array(_)) | None | Some(Value::Uninit) => {
                                ctx.rt.array_delete(name, Some(k.as_ref()));
                            }
                            Some(_) => {
                                return Err(Error::Runtime(format!(
                                    "attempt to use scalar `{name}' as an array"
                                )));
                            }
                        }
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
                let (parts, seps_vec) =
                    crate::runtime::split_string_with_seps(&s, &fs, ctx.rt.ignore_case_flag());
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
                // gawk parity: `for (k in x)` where `x` is a scalar raises
                // "attempt to use scalar `x' as an array". Earlier awkrs
                // ran zero iterations and continued silently.
                check_array_target(ctx, &name)?;
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
                let src_name = ctx.cp.strings.get(src).to_string();
                let dest_name = dest.map(|i| ctx.cp.strings.get(i).to_string());
                if src_name.is_empty() {
                    return Err(Error::Runtime(
                        "0 is invalid as number of arguments for asort".into(),
                    ));
                }
                let mut pairs = ctx.array_pairs_for_sort(&src_name, "asort")?;
                let ic = ctx.rt.ignore_case_flag();
                pairs
                    .sort_by(|(_, va), (_, vb)| builtins::awk_value_sort_cmp_with_case(va, vb, ic));
                let n = pairs.len() as f64;
                let reindexed: Vec<(String, Value)> = pairs
                    .into_iter()
                    .enumerate()
                    .map(|(i, (_, v))| (format!("{}", i + 1), v))
                    .collect();
                let target = dest_name.as_deref().unwrap_or(&src_name);
                ctx.array_replace(target, reindexed);
                ctx.push(Value::Num(n));
            }
            Op::Asorti { src, dest } => {
                let src_name = ctx.cp.strings.get(src).to_string();
                let dest_name = dest.map(|i| ctx.cp.strings.get(i).to_string());
                if src_name.is_empty() {
                    return Err(Error::Runtime(
                        "0 is invalid as number of arguments for asorti".into(),
                    ));
                }
                let mut pairs = ctx.array_pairs_for_sort(&src_name, "asorti")?;
                let ic = ctx.rt.ignore_case_flag();
                pairs.sort_by(|(ka, _), (kb, _)| {
                    if ic {
                        builtins::locale_str_cmp_sort(&ka.to_lowercase(), &kb.to_lowercase())
                    } else {
                        builtins::locale_str_cmp_sort(ka, kb)
                    }
                });
                let n = pairs.len() as f64;
                let reindexed: Vec<(String, Value)> = pairs
                    .into_iter()
                    .enumerate()
                    .map(|(i, (k, _))| (format!("{}", i + 1), Value::Str(k)))
                    .collect();
                let target = dest_name.as_deref().unwrap_or(&src_name);
                ctx.array_replace(target, reindexed);
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

/// gawk parity: subscript assignment (`x[k] = …`) requires `x` to be an array
/// (or unassigned). A pre-existing scalar value raises "attempt to use scalar
/// `x' as an array". Checks local frames first (function array-by-reference),
/// then globals.
fn check_array_target(ctx: &VmCtx<'_>, name: &str) -> Result<()> {
    for frame in ctx.locals.iter().rev() {
        match frame.get(name) {
            Some(Value::Array(_)) => return Ok(()),
            Some(Value::Uninit) | None => continue,
            Some(_) => {
                return Err(Error::Runtime(format!(
                    "attempt to use scalar `{name}' as an array"
                )));
            }
        }
    }
    match ctx.rt.get_global_var(name) {
        Some(Value::Array(_)) | None | Some(Value::Uninit) => Ok(()),
        Some(_) => Err(Error::Runtime(format!(
            "attempt to use scalar `{name}' as an array"
        ))),
    }
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
    // POSIX / gawk parity: numeric `==` is BIT-EXACT, not a fuzzy tolerance.
    // Previously awkrs used `(an - bn).abs() < f64::EPSILON` (~2.22e-16), which
    // wrongly reported `0.1 + 0.2 == 0.3` as true (the difference is 5.55e-17,
    // below EPSILON). gawk and POSIX awk return 0 for that comparison.
    if let (Value::Num(an), Value::Num(bn)) = (a, b) {
        return Value::Num(if an == bn { 1.0 } else { 0.0 });
    }
    if a.is_numeric_str() && b.is_numeric_str() {
        let an = a.as_number();
        let bn = b.as_number();
        return Value::Num(if an == bn { 1.0 } else { 0.0 });
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
            n
        }
        SubTarget::SlotVar(slot) => {
            // Read from rt.slots (the authoritative bytecode store) rather than
            // jit_slot_buf. After a JIT chunk runs and a subsequent bytecode
            // chunk does `raw = "abc"` via Op::SetSlot, rt.slots is updated but
            // jit_slot_buf still holds the stale (often Uninit) value from
            // chunk-entry prep. Reading via slot_value_live_for_jit would then
            // decode Uninit and run sub on an empty string, silently zeroing
            // out the variable.
            let mut s = ctx.rt.slots[slot as usize].as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.rt.slots[slot as usize] = Value::Str(s);
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

// ── Builtin calls (implemented in vm_builtins.rs) ────────────────────────────
#[path = "vm_builtins.rs"]
mod vm_builtins;
use vm_builtins::{exec_call_builtin, exec_call_user_inner, sort_keys_with_custom_cmp};

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
#[path = "vm_tests.rs"]
mod tests;
