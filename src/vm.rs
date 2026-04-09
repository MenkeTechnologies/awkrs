//! Stack-based virtual machine that executes compiled bytecode.

use crate::ast::{BinOp, IncDecOp};
use crate::builtins;
use crate::bytecode::*;
use crate::error::{Error, Result};
use crate::format;
use crate::interp::Flow;
use crate::runtime::AwkMap;
use crate::runtime::{Runtime, Value};
use std::cmp::Ordering;
use std::io::{self, Write};

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

    fn get_var(&self, name: &str) -> Value {
        for frame in self.locals.iter().rev() {
            if let Some(v) = frame.get(name) {
                return v.clone();
            }
        }
        // Check slots (for builtins / cold path that access vars by name)
        if let Some(&slot) = self.cp.slot_map.get(name) {
            return self.rt.slots[slot as usize].clone();
        }
        self.rt
            .get_global_var(name)
            .cloned()
            .unwrap_or_else(|| match name {
                "NR" => Value::Num(self.rt.nr),
                "FNR" => Value::Num(self.rt.fnr),
                "NF" => Value::Num(if self.rt.fields_dirty {
                    self.rt.fields.len()
                } else {
                    self.rt.field_ranges.len()
                } as f64),
                "FILENAME" => Value::Str(self.rt.filename.clone()),
                _ => Value::Uninit,
            })
    }

    fn set_var(&mut self, name: &str, val: Value) {
        for frame in self.locals.iter_mut().rev() {
            if frame.contains_key(name) {
                frame.insert(name.to_string(), val);
                return;
            }
        }
        // Check slots
        if let Some(&slot) = self.cp.slot_map.get(name) {
            self.rt.slots[slot as usize] = val;
            return;
        }
        // Update cached OFS/ORS bytes when those vars change.
        match name {
            "OFS" => self.rt.ofs_bytes = val.as_str().into_bytes(),
            "ORS" => self.rt.ors_bytes = val.as_str().into_bytes(),
            _ => {}
        }
        self.rt.vars.insert(name.to_string(), val);
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
            let val = truthy(&ctx.pop());
            // Drop VmSignal — Expr patterns can't produce Next/Exit
            let _ = r;
            ctx.recycle();
            Ok(val)
        }
        CompiledPattern::Range => Ok(false), // handled externally via range_step
    }
}

/// Execute a rule body. Returns the same `Flow` enum used by the AST interpreter
/// so the caller doesn't need to change.
pub fn vm_run_rule(
    rule: &CompiledRule,
    cp: &CompiledProgram,
    rt: &mut Runtime,
    print_out: Option<&mut Vec<String>>,
) -> Result<Flow> {
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
    /// Raw pointer to the current Runtime, set before JIT execution so the
    /// field callback can access `field_as_number`, `nr`, `fnr`, `nf`.
    static JIT_RT_PTR: Cell<*mut Runtime> = const { Cell::new(std::ptr::null_mut()) };
}

/// Field callback passed to JIT-compiled code.
/// Positive i → field $i as f64.
/// Negative i → special: -1=NR, -2=FNR, -3=NF.
extern "C" fn jit_field_callback(i: i32) -> f64 {
    JIT_RT_PTR.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            return 0.0;
        }
        let rt = unsafe { &mut *ptr };
        match i {
            -1 => rt.nr,
            -2 => rt.fnr,
            -3 => rt.nf() as f64,
            _ => rt.field_as_number(i),
        }
    })
}

/// Try JIT dispatch for the full instruction set. Converts slots to/from f64[],
/// sets up the field callback via thread-local, and executes.
fn try_jit_dispatch(ops: &[Op], ctx: &mut VmCtx<'_>) -> Option<f64> {
    if !crate::jit::jit_enabled() || !crate::jit::is_jit_eligible(ops) {
        return None;
    }

    // Build f64 slot array from runtime slots
    let slot_count = ctx.rt.slots.len();
    let mut jit_slots: Vec<f64> = ctx.rt.slots.iter().map(|v| v.as_number()).collect();

    // Set thread-local Runtime pointer for the field callback
    let rt_ptr: *mut Runtime = ctx.rt;
    JIT_RT_PTR.with(|cell| cell.set(rt_ptr));

    let mut jit_state = crate::jit::JitRuntimeState::new(&mut jit_slots, jit_field_callback);
    let result = crate::jit::try_jit_execute(ops, &mut jit_state);

    // Clear the pointer
    JIT_RT_PTR.with(|cell| cell.set(std::ptr::null_mut()));

    // Write back modified slots
    if let Some(_) = result {
        for i in 0..slot_count {
            let old = ctx.rt.slots[i].as_number();
            if (jit_slots[i] - old).abs() > f64::EPSILON || (old == 0.0 && jit_slots[i] != 0.0) {
                ctx.rt.slots[i] = Value::Num(jit_slots[i]);
            }
        }
    }

    result
}

// ── Core VM loop ────────────────────────────────────────────────────────────

fn execute(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<VmSignal> {
    let ops = &chunk.ops;
    if let Some(v) = try_jit_dispatch(ops, ctx) {
        ctx.push(Value::Num(v));
        return Ok(VmSignal::Normal);
    }
    let len = ops.len();
    let mut pc: usize = 0;

    while pc < len {
        match ops[pc] {
            // ── Constants ───────────────────────────────────────────────
            Op::PushNum(n) => ctx.push(Value::Num(n)),
            Op::PushStr(idx) => ctx.push(Value::Str(ctx.str_ref(idx).to_string())),

            // ── Variable access ─────────────────────────────────────────
            Op::GetVar(idx) => {
                let name = ctx.str_ref(idx);
                let v = ctx.get_var(name);
                ctx.push(v);
            }
            Op::SetVar(idx) => {
                let val = ctx.peek().clone();
                let name = ctx.str_ref(idx).to_string();
                ctx.set_var(&name, val);
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
                let v = ctx.rt.field(i);
                ctx.push(v);
            }
            Op::SetField => {
                let val = ctx.pop();
                let idx = ctx.pop().as_number() as i32;
                let s = val.as_str();
                ctx.rt.set_field(idx, &s);
                ctx.push(val);
            }
            Op::GetArrayElem(arr) => {
                let key = ctx.pop().into_string();
                let name = ctx.str_ref(arr);
                let v = ctx.rt.array_get(name, &key);
                ctx.push(v);
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
                let key = ctx.pop().into_string();
                let name = ctx.str_ref(arr);
                let t = builtins::awk_typeof_array_elem(ctx.rt, name, &key);
                ctx.push(Value::Str(t.into()));
            }
            Op::TypeofField => {
                let i = ctx.pop().as_number() as i32;
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
                ctx.rt.array_set(name, key, val.clone());
                ctx.push(val);
            }

            // ── Compound assignment ─────────────────────────────────────
            Op::CompoundAssignVar(idx, bop) => {
                let rhs = ctx.pop();
                let name = ctx.str_ref(idx).to_string();
                let old = ctx.get_var(&name);
                let new_val = apply_binop(bop, &old, &rhs)?;
                ctx.set_var(&name, new_val.clone());
                ctx.push(new_val);
            }
            Op::CompoundAssignSlot(slot, bop) => {
                let rhs = ctx.pop();
                let new_val = apply_binop(bop, &ctx.rt.slots[slot as usize], &rhs)?;
                ctx.rt.slots[slot as usize] = new_val.clone();
                ctx.push(new_val);
            }
            Op::CompoundAssignField(bop) => {
                let rhs = ctx.pop();
                let idx = ctx.pop().as_number() as i32;
                let old = ctx.rt.field(idx);
                let new_val = apply_binop(bop, &old, &rhs)?;
                let s = new_val.as_str();
                ctx.rt.set_field(idx, &s);
                ctx.push(new_val);
            }
            Op::CompoundAssignIndex(arr, bop) => {
                let rhs = ctx.pop();
                let key = ctx.pop().into_string();
                let name = ctx.cp.strings.get(arr);
                let old = ctx.rt.array_get(name, &key);
                let new_val = apply_binop(bop, &old, &rhs)?;
                ctx.rt.array_set(name, key, new_val.clone());
                ctx.push(new_val);
            }

            Op::IncDecVar(idx, kind) => {
                let name = ctx.str_ref(idx).to_string();
                let old = ctx.get_var(&name);
                let old_n = old.as_number();
                let delta = incdec_delta(kind);
                let new_n = old_n + delta;
                ctx.set_var(&name, Value::Num(new_n));
                ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
            }
            Op::IncrVar(idx) => {
                let name = ctx.str_ref(idx).to_string();
                let n = ctx.get_var(&name).as_number();
                ctx.set_var(&name, Value::Num(n + 1.0));
            }
            Op::DecrVar(idx) => {
                let name = ctx.str_ref(idx).to_string();
                let n = ctx.get_var(&name).as_number();
                ctx.set_var(&name, Value::Num(n - 1.0));
            }
            Op::IncDecSlot(slot, kind) => {
                let old_n = match &ctx.rt.slots[slot as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                let delta = incdec_delta(kind);
                let new_n = old_n + delta;
                ctx.rt.slots[slot as usize] = Value::Num(new_n);
                ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
            }
            Op::IncDecField(kind) => {
                let idx = ctx.pop().as_number() as i32;
                let old = ctx.rt.field(idx);
                let old_n = old.as_number();
                let delta = incdec_delta(kind);
                let new_n = old_n + delta;
                ctx.rt.set_field_num(idx, new_n);
                ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
            }
            Op::IncDecIndex(arr, kind) => {
                let key = ctx.pop().into_string();
                let name = ctx.cp.strings.get(arr);
                let old_n = ctx.rt.array_get(name, &key).as_number();
                let delta = incdec_delta(kind);
                let new_n = old_n + delta;
                ctx.rt.array_set(name, key, Value::Num(new_n));
                ctx.push(Value::Num(incdec_push(kind, old_n, new_n)));
            }

            // ── Arithmetic ──────────────────────────────────────────────
            Op::Add => {
                let b = ctx.pop().as_number();
                let a = ctx.pop().as_number();
                ctx.push(Value::Num(a + b));
            }
            Op::Sub => {
                let b = ctx.pop().as_number();
                let a = ctx.pop().as_number();
                ctx.push(Value::Num(a - b));
            }
            Op::Mul => {
                let b = ctx.pop().as_number();
                let a = ctx.pop().as_number();
                ctx.push(Value::Num(a * b));
            }
            Op::Div => {
                let b = ctx.pop().as_number();
                let a = ctx.pop().as_number();
                ctx.push(Value::Num(a / b));
            }
            Op::Mod => {
                let b = ctx.pop().as_number();
                let a = ctx.pop().as_number();
                ctx.push(Value::Num(a % b));
            }

            // ── Comparison (POSIX-aware) ────────────────────────────────
            Op::CmpEq => {
                let b = ctx.pop();
                let a = ctx.pop();
                ctx.push(awk_cmp_eq(&a, &b));
            }
            Op::CmpNe => {
                let b = ctx.pop();
                let a = ctx.pop();
                let eq = awk_cmp_eq(&a, &b);
                ctx.push(Value::Num(if eq.as_number() != 0.0 { 0.0 } else { 1.0 }));
            }
            Op::CmpLt => {
                let b = ctx.pop();
                let a = ctx.pop();
                ctx.push(awk_cmp_rel(BinOp::Lt, &a, &b));
            }
            Op::CmpLe => {
                let b = ctx.pop();
                let a = ctx.pop();
                ctx.push(awk_cmp_rel(BinOp::Le, &a, &b));
            }
            Op::CmpGt => {
                let b = ctx.pop();
                let a = ctx.pop();
                ctx.push(awk_cmp_rel(BinOp::Gt, &a, &b));
            }
            Op::CmpGe => {
                let b = ctx.pop();
                let a = ctx.pop();
                ctx.push(awk_cmp_rel(BinOp::Ge, &a, &b));
            }

            // ── String / regex ──────────────────────────────────────────
            Op::Concat => {
                let b = ctx.pop();
                let a = ctx.pop();
                // Reuse a's String allocation when possible.
                let mut s = a.into_string();
                b.append_to_string(&mut s);
                ctx.push(Value::Str(s));
            }
            Op::RegexMatch => {
                let pat = ctx.pop().as_str();
                let s = ctx.pop().as_str();
                ctx.rt.ensure_regex(&pat).map_err(Error::Runtime)?;
                let m = ctx.rt.regex_ref(&pat).is_match(&s);
                ctx.push(Value::Num(if m { 1.0 } else { 0.0 }));
            }
            Op::RegexNotMatch => {
                let pat = ctx.pop().as_str();
                let s = ctx.pop().as_str();
                ctx.rt.ensure_regex(&pat).map_err(Error::Runtime)?;
                let m = ctx.rt.regex_ref(&pat).is_match(&s);
                ctx.push(Value::Num(if !m { 1.0 } else { 0.0 }));
            }

            // ── Unary ───────────────────────────────────────────────────
            Op::Neg => {
                let v = ctx.pop().as_number();
                ctx.push(Value::Num(-v));
            }
            Op::Pos => {
                let v = ctx.pop().as_number();
                ctx.push(Value::Num(v));
            }
            Op::Not => {
                let v = ctx.pop();
                ctx.push(Value::Num(if truthy(&v) { 0.0 } else { 1.0 }));
            }
            Op::ToBool => {
                let v = ctx.pop();
                ctx.push(Value::Num(if truthy(&v) { 1.0 } else { 0.0 }));
            }

            // ── Control flow ────────────────────────────────────────────
            Op::Jump(target) => {
                pc = target;
                continue;
            }
            Op::JumpIfFalsePop(target) => {
                let v = ctx.pop();
                if !truthy(&v) {
                    pc = target;
                    continue;
                }
            }
            Op::JumpIfTruePop(target) => {
                let v = ctx.pop();
                if truthy(&v) {
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
            Op::CallUser(name_idx, argc) => {
                let name = ctx.str_ref(name_idx).to_string();
                exec_call_user(ctx, &name, argc)?;
            }

            // ── Array ops ───────────────────────────────────────────────
            Op::InArray(arr) => {
                let key = ctx.pop().into_string();
                let name = ctx.str_ref(arr);
                let b = ctx.rt.array_has(name, &key);
                ctx.push(Value::Num(if b { 1.0 } else { 0.0 }));
            }
            Op::DeleteArray(arr) => {
                let name = ctx.str_ref(arr).to_string();
                ctx.rt.array_delete(&name, None);
            }
            Op::DeleteElem(arr) => {
                let key = ctx.pop().into_string();
                let name = ctx.str_ref(arr).to_string();
                ctx.rt.array_delete(&name, Some(&key));
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
            Op::GetLine { var, source } => exec_getline(ctx, var, source)?,

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
                let parts: Vec<String> = if fs.is_empty() {
                    s.chars().map(|c| c.to_string()).collect()
                } else if fs == " " {
                    s.split_whitespace().map(String::from).collect()
                } else if fs.len() == 1 {
                    s.split(&*fs).map(String::from).collect()
                } else {
                    // POSIX: multi-char FS is a regex.
                    match regex::Regex::new(&fs) {
                        Ok(re) => re.split(&s).map(String::from).collect(),
                        Err(_) => s.split(&*fs).map(String::from).collect(),
                    }
                };
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
                let name = ctx.str_ref(arr);
                let keys = ctx.rt.array_keys(name);
                ctx.for_in_iters.push(ForInState { keys, index: 0 });
            }
            Op::ForInNext { var, end_jump } => {
                let state = ctx.for_in_iters.last_mut().unwrap();
                if state.index >= state.keys.len() {
                    pc = end_jump;
                    continue;
                }
                let key = state.keys[state.index].clone();
                state.index += 1;
                let name = ctx.str_ref(var).to_string();
                ctx.set_var(&name, Value::Str(key));
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
                let mut s = ctx.pop().into_string();
                s.push_str(pool_str);
                ctx.push(Value::Str(s));
            }
            Op::GetNR => ctx.push(Value::Num(ctx.rt.nr)),
            Op::GetFNR => ctx.push(Value::Num(ctx.rt.fnr)),
            Op::GetNF => {
                let nf = ctx.rt.nf() as f64;
                ctx.push(Value::Num(nf));
            }
            Op::PushFieldNum(field) => {
                let n = ctx.rt.field_as_number(field as i32);
                ctx.push(Value::Num(n));
            }
            Op::AddFieldToSlot { field, slot } => {
                let field_val = ctx.rt.field_as_number(field as i32);
                let old = match &ctx.rt.slots[slot as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[slot as usize] = Value::Num(old + field_val);
            }
            Op::PrintFieldStdout(field) => {
                if let Some(ref mut buf) = ctx.print_out {
                    // Parallel capture path: build string, push to capture buffer.
                    let val = ctx.rt.field(field as i32);
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
                let n = match &ctx.rt.slots[s] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[s] = Value::Num(n + 1.0);
            }
            Op::DecrSlot(slot) => {
                let s = slot as usize;
                let n = match &ctx.rt.slots[s] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[s] = Value::Num(n - 1.0);
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
                let p = ctx.rt.field_as_number(f1 as i32) * ctx.rt.field_as_number(f2 as i32);
                let old = match &ctx.rt.slots[slot as usize] {
                    Value::Num(v) => *v,
                    other => other.as_number(),
                };
                ctx.rt.slots[slot as usize] = Value::Num(old + p);
            }
            Op::ArrayFieldAddConst { arr, field, delta } => {
                let name = ctx.cp.strings.get(arr);
                let key = ctx.rt.field(field as i32).as_str();
                let old = ctx.rt.array_get(name, &key).as_number();
                ctx.rt.array_set(name, key, Value::Num(old + delta));
            }
            Op::PrintFieldSepField { f1, sep, f2 } => {
                let sep_s = ctx.str_ref(sep).to_string();
                if let Some(ref mut buf) = ctx.print_out {
                    let ors_b = ctx.rt.ors_bytes.clone();
                    let v1 = ctx.rt.field(f1 as i32).as_str();
                    let v2 = ctx.rt.field(f2 as i32).as_str();
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
                    let v1 = ctx.rt.field(f1 as i32).as_str();
                    let v2 = ctx.rt.field(f2 as i32).as_str();
                    let v3 = ctx.rt.field(f3 as i32).as_str();
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

fn truthy(v: &Value) -> bool {
    v.truthy()
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

fn apply_binop(op: BinOp, old: &Value, rhs: &Value) -> Result<Value> {
    let a = old.as_number();
    let b = rhs.as_number();
    let n = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Mod => a % b,
        _ => return Err(Error::Runtime("invalid compound assignment op".into())),
    };
    Ok(Value::Num(n))
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

fn awk_cmp_eq(a: &Value, b: &Value) -> Value {
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
    let ord = locale_str_cmp(&ls, &rs);
    Value::Num(if ord == Ordering::Equal { 1.0 } else { 0.0 })
}

fn awk_cmp_rel(op: BinOp, a: &Value, b: &Value) -> Value {
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
    let ord = locale_str_cmp(&ls, &rs);
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
        Some(ctx.pop().as_str())
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
        let fmt = args[0].as_str();
        let vals = &args[1..];
        let out = sprintf_simple(&fmt, vals, ctx.rt.numeric_decimal)?;
        let s = out.as_str();
        emit_with_redir(ctx, &s, redir, redir_path.as_deref())?;
    } else if redir == RedirKind::Stdout && ctx.print_out.is_none() {
        // ── Fast path: write directly into rt.print_buf, zero intermediate allocs ──
        // Copy separators to stack (typically 1-2 bytes) to avoid borrow conflict with print_buf.
        let mut ofs_local = [0u8; 64];
        let ofs_len = ctx.rt.ofs_bytes.len().min(64);
        ofs_local[..ofs_len].copy_from_slice(&ctx.rt.ofs_bytes[..ofs_len]);
        let mut ors_local = [0u8; 64];
        let ors_len = ctx.rt.ors_bytes.len().min(64);
        ors_local[..ors_len].copy_from_slice(&ctx.rt.ors_bytes[..ors_len]);

        if argc == 0 {
            ctx.rt.print_buf.extend_from_slice(ctx.rt.record.as_bytes());
        } else {
            let start = ctx.stack.len() - argc;
            for i in 0..argc {
                if i > 0 {
                    ctx.rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                }
                ctx.stack[start + i].write_to(&mut ctx.rt.print_buf);
            }
            ctx.stack.truncate(start);
        }
        ctx.rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
    } else {
        // ── Redirect / capture path: build String (I/O dominates, alloc is fine) ──
        let ofs = String::from_utf8_lossy(&ctx.rt.ofs_bytes).into_owned();
        let ors = String::from_utf8_lossy(&ctx.rt.ors_bytes).into_owned();

        let line = if argc == 0 {
            ctx.rt.record.clone()
        } else {
            let start = ctx.stack.len() - argc;
            let parts: Vec<String> = ctx.stack.drain(start..).map(|v| v.as_str()).collect();
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

fn sprintf_simple(fmt: &str, vals: &[Value], dec: char) -> Result<Value> {
    let s = if dec == '.' {
        format::awk_sprintf(fmt, vals)
    } else {
        format::awk_sprintf_with_decimal(fmt, vals, dec)
    };
    s.map(Value::Str).map_err(Error::Runtime)
}

// ── Getline ─────────────────────────────────────────────────────────────────

fn exec_getline(ctx: &mut VmCtx<'_>, var: Option<u32>, source: GetlineSource) -> Result<()> {
    let file_path = match source {
        GetlineSource::File => Some(ctx.pop().as_str()),
        GetlineSource::Coproc => Some(ctx.pop().as_str()),
        GetlineSource::Primary => None,
    };

    let line = match source {
        GetlineSource::Primary => ctx.rt.read_line_primary()?,
        GetlineSource::File => ctx.rt.read_line_file(file_path.as_ref().unwrap())?,
        GetlineSource::Coproc => ctx.rt.read_line_coproc(file_path.as_ref().unwrap())?,
    };

    if let Some(l) = line {
        let trimmed = l.trim_end_matches(['\n', '\r']).to_string();
        if let Some(var_idx) = var {
            // getline var — read into variable only, do NOT touch $0/fields/NF.
            let name = ctx.str_ref(var_idx).to_string();
            ctx.set_var(&name, Value::Str(trimmed));
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

// ── Sub / Gsub ──────────────────────────────────────────────────────────────

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
            let mut s = ctx.get_var(&name).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.set_var(&name, Value::Str(s));
            n
        }
        SubTarget::SlotVar(slot) => {
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
            let i = extra_field_idx.unwrap();
            let mut s = ctx.rt.field(i).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.rt.set_field(i, &s);
            n
        }
        SubTarget::Index(arr_idx) => {
            let key = extra_key.unwrap();
            let arr_name = ctx.str_ref(arr_idx).to_string();
            let mut s = ctx.rt.array_get(&arr_name, &key).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, re.as_ref(), repl.as_ref(), Some(&mut s))?
            };
            ctx.rt.array_set(&arr_name, key, Value::Str(s));
            n
        }
    };
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

    let result = match name {
        "length" => {
            let s = if args.is_empty() {
                ctx.rt.record.clone()
            } else {
                args[0].as_str()
            };
            Value::Num(s.chars().count() as f64)
        }
        "index" => {
            let hay = args[0].as_str();
            let needle = args[1].as_str();
            if needle.is_empty() {
                Value::Num(0.0)
            } else {
                let pos = hay.find(&needle).map(|i| i + 1).unwrap_or(0);
                Value::Num(pos as f64)
            }
        }
        "substr" => {
            let s = args[0].as_str();
            let start = args[1].as_number() as usize;
            let len = args
                .get(2)
                .map(|v| v.as_number() as usize)
                .unwrap_or(usize::MAX);
            if start < 1 {
                Value::Str(String::new())
            } else {
                let s0 = start - 1;
                let slice: String = s.chars().skip(s0).take(len).collect();
                Value::Str(slice)
            }
        }
        "tolower" => Value::Str(args[0].as_str().to_lowercase()),
        "toupper" => Value::Str(args[0].as_str().to_uppercase()),
        "int" => Value::Num(args[0].as_number().trunc()),
        "sqrt" => Value::Num(args[0].as_number().sqrt()),
        "sin" => {
            if argc != 1 {
                return Err(Error::Runtime("`sin` expects one argument".into()));
            }
            Value::Num(args[0].as_number().sin())
        }
        "cos" => {
            if argc != 1 {
                return Err(Error::Runtime("`cos` expects one argument".into()));
            }
            Value::Num(args[0].as_number().cos())
        }
        "atan2" => {
            if argc != 2 {
                return Err(Error::Runtime("`atan2` expects two arguments".into()));
            }
            Value::Num(args[0].as_number().atan2(args[1].as_number()))
        }
        "exp" => {
            if argc != 1 {
                return Err(Error::Runtime("`exp` expects one argument".into()));
            }
            Value::Num(args[0].as_number().exp())
        }
        "log" => {
            if argc != 1 {
                return Err(Error::Runtime("`log` expects one argument".into()));
            }
            Value::Num(args[0].as_number().ln())
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
            let n = args.first().map(|v| v.as_number() as u32);
            Value::Num(ctx.rt.srand(n))
        }
        "system" => {
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
            sprintf_simple(&fmt, &args[1..], ctx.rt.numeric_decimal)?
        }
        "printf" => {
            if args.is_empty() {
                return Err(Error::Runtime("printf: need format".into()));
            }
            let fmt = args[0].as_str();
            let s = sprintf_simple(&fmt, &args[1..], ctx.rt.numeric_decimal)?.as_str();
            ctx.emit_print(&s);
            Value::Num(0.0)
        }
        "and" => {
            if argc != 2 {
                return Err(Error::Runtime("`and` expects two arguments".into()));
            }
            Value::Num(builtins::awk_and(args[0].as_number(), args[1].as_number()))
        }
        "or" => {
            if argc != 2 {
                return Err(Error::Runtime("`or` expects two arguments".into()));
            }
            Value::Num(builtins::awk_or(args[0].as_number(), args[1].as_number()))
        }
        "xor" => {
            if argc != 2 {
                return Err(Error::Runtime("`xor` expects two arguments".into()));
            }
            Value::Num(builtins::awk_xor(args[0].as_number(), args[1].as_number()))
        }
        "lshift" => {
            if argc != 2 {
                return Err(Error::Runtime("`lshift` expects two arguments".into()));
            }
            Value::Num(builtins::awk_lshift(
                args[0].as_number(),
                args[1].as_number(),
            ))
        }
        "rshift" => {
            if argc != 2 {
                return Err(Error::Runtime("`rshift` expects two arguments".into()));
            }
            Value::Num(builtins::awk_rshift(
                args[0].as_number(),
                args[1].as_number(),
            ))
        }
        "compl" => {
            if argc != 1 {
                return Err(Error::Runtime("`compl` expects one argument".into()));
            }
            Value::Num(builtins::awk_compl(args[0].as_number()))
        }
        "strtonum" => {
            if argc != 1 {
                return Err(Error::Runtime("`strtonum` expects one argument".into()));
            }
            Value::Num(builtins::awk_strtonum(&args[0].as_str()))
        }
        "typeof" => {
            if argc != 1 {
                return Err(Error::Runtime("`typeof` expects one argument".into()));
            }
            Value::Str(builtins::awk_typeof_value(&args[0]).into())
        }
        _ => return Err(Error::Runtime(format!("unknown function `{name}`"))),
    };
    ctx.push(result);
    Ok(())
}

fn exec_call_user(ctx: &mut VmCtx<'_>, name: &str, argc: u16) -> Result<()> {
    let argc = argc as usize;
    let func = ctx
        .cp
        .functions
        .get(name)
        .ok_or_else(|| Error::Runtime(format!("unknown function `{name}`")))?
        .clone();

    let start = ctx.stack.len() - argc;
    let mut vals: Vec<Value> = ctx.stack.drain(start..).collect();

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
    ctx.push(result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::interp::Flow;
    use crate::parser::parse_program;
    use crate::runtime::Runtime;

    fn compile(prog_text: &str) -> CompiledProgram {
        let prog = parse_program(prog_text).expect("parse");
        Compiler::compile_program(&prog)
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
        let flow = vm_run_rule(rule, &cp, &mut rt, Some(&mut cap)).unwrap();
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
        let flow = vm_run_rule(rule, &cp, &mut rt, None).unwrap();
        assert!(matches!(flow, Flow::Next));
    }

    #[test]
    fn vm_run_rule_exit_sets_pending() {
        let cp = compile("{ exit 3 }");
        let rule = &cp.record_rules[0];
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("z");
        let flow = vm_run_rule(rule, &cp, &mut rt, None).unwrap();
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
        vm_run_rule(rule, &cp, &mut rt, Some(&mut cap)).unwrap();
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
        assert!(matches!(rule.pattern, CompiledPattern::Range));
        let mut rt = runtime_with_slots(&cp);
        rt.set_record_from_line("x");
        assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    }
}
