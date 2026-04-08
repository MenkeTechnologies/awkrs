//! Stack-based virtual machine that executes compiled bytecode.

use crate::ast::BinOp;
use crate::builtins;
use crate::bytecode::*;
use crate::error::{Error, Result};
use crate::format;
use crate::interp::Flow;
use crate::runtime::{Runtime, Value};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{self, Write};

// ── VM context ──────────────────────────────────────────────────────────────

struct ForInState {
    keys: Vec<String>,
    index: usize,
}

pub struct VmCtx<'a> {
    pub cp: &'a CompiledProgram,
    pub rt: &'a mut Runtime,
    locals: Vec<HashMap<String, Value>>,
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
                _ => Value::Str(String::new()),
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
}

static EMPTY_STR: Value = Value::Str(String::new());

// ── Signal from VM execution ────────────────────────────────────────────────

enum VmSignal {
    Normal,
    Next,
    Return(Value),
    ExitPending,
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn vm_run_begin(cp: &CompiledProgram, rt: &mut Runtime) -> Result<()> {
    let mut ctx = VmCtx::new(cp, rt);
    for chunk in &cp.begin_chunks {
        match execute(chunk, &mut ctx)? {
            VmSignal::Next => return Err(Error::Runtime("`next` is invalid in BEGIN".into())),
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
        VmSignal::Return(_) => Err(Error::Runtime(
            "`return` used outside function in rule action".into(),
        )),
        VmSignal::ExitPending => Ok(Flow::ExitPending),
    };
    ctx.recycle();
    result
}

// ── Core VM loop ────────────────────────────────────────────────────────────

fn execute(chunk: &Chunk, ctx: &mut VmCtx<'_>) -> Result<VmSignal> {
    let ops = &chunk.ops;
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
                ctx.push(ctx.rt.slots[slot as usize].clone());
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
                let key = ctx.pop().as_str();
                let name = ctx.str_ref(arr);
                let v = ctx.rt.array_get(name, &key);
                ctx.push(v);
            }
            Op::SetArrayElem(arr) => {
                let val = ctx.pop();
                let key = ctx.pop().as_str();
                let name = ctx.str_ref(arr).to_string();
                ctx.rt.array_set(&name, key, val.clone());
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
                let old = &ctx.rt.slots[slot as usize];
                let new_val = apply_binop(bop, old, &rhs)?;
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
                let key = ctx.pop().as_str();
                let name = ctx.str_ref(arr);
                let old = ctx.rt.array_get(name, &key);
                let new_val = apply_binop(bop, &old, &rhs)?;
                let name = name.to_string();
                ctx.rt.array_set(&name, key, new_val.clone());
                ctx.push(new_val);
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
                let key = ctx.pop().as_str();
                let name = ctx.str_ref(arr);
                let b = ctx.rt.array_has(name, &key);
                ctx.push(Value::Num(if b { 1.0 } else { 0.0 }));
            }
            Op::DeleteArray(arr) => {
                let name = ctx.str_ref(arr).to_string();
                ctx.rt.array_delete(&name, None);
            }
            Op::DeleteElem(arr) => {
                let key = ctx.pop().as_str();
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
                } else {
                    s.split(&fs).map(String::from).collect()
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

            // ── Pattern helpers ─────────────────────────────────────────
            Op::MatchRegexp(idx) => {
                let pat = ctx.str_ref(idx).to_string();
                ctx.rt.ensure_regex(&pat).map_err(Error::Runtime)?;
                let m = ctx.rt.regex_ref(&pat).is_match(&ctx.rt.record);
                ctx.push(Value::Num(if m { 1.0 } else { 0.0 }));
            }

            // ── Fused opcodes ──────────────────────────────────────────
            Op::AddFieldToSlot { field, slot } => {
                let field_val = ctx.rt.field_as_number(field as i32);
                let old = ctx.rt.slots[slot as usize].as_number();
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
                let n = ctx.rt.slots[s].as_number();
                ctx.rt.slots[s] = Value::Num(n + 1.0);
            }
            Op::AddSlotToSlot { src, dst } => {
                let sv = ctx.rt.slots[src as usize].as_number();
                let dv = ctx.rt.slots[dst as usize].as_number();
                ctx.rt.slots[dst as usize] = Value::Num(dv + sv);
            }
            Op::JumpIfSlotGeNum {
                slot,
                limit,
                target,
            } => {
                let v = ctx.rt.slots[slot as usize].as_number();
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
    if a.is_numeric_str() && b.is_numeric_str() {
        let an = a.as_number();
        let bn = b.as_number();
        return Value::Num(if (an - bn).abs() < f64::EPSILON {
            1.0
        } else {
            0.0
        });
    }
    let ord = locale_str_cmp(&a.as_str(), &b.as_str());
    Value::Num(if ord == Ordering::Equal { 1.0 } else { 0.0 })
}

fn awk_cmp_rel(op: BinOp, a: &Value, b: &Value) -> Value {
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
    let ls = a.as_str();
    let rs = b.as_str();
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
            let name = ctx.str_ref(var_idx).to_string();
            ctx.set_var(&name, Value::Str(trimmed));
        } else {
            let fs = ctx
                .rt
                .vars
                .get("FS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".into());
            ctx.rt.set_field_sep_split(&fs, &trimmed);
        }
        if matches!(source, GetlineSource::Primary) {
            ctx.rt.nr += 1.0;
            ctx.rt.fnr += 1.0;
        }
    }
    ctx.rt
        .vars
        .insert("NF".into(), Value::Num(ctx.rt.fields.len() as f64));
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
    let repl = ctx.pop().as_str();
    let re = ctx.pop().as_str();

    let count = match target {
        SubTarget::Record => {
            if is_global {
                builtins::gsub(ctx.rt, &re, &repl, None)?
            } else {
                builtins::sub_fn(ctx.rt, &re, &repl, None)?
            }
        }
        SubTarget::Var(name_idx) => {
            let name = ctx.str_ref(name_idx).to_string();
            let mut s = ctx.get_var(&name).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, &re, &repl, Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, &re, &repl, Some(&mut s))?
            };
            ctx.set_var(&name, Value::Str(s));
            n
        }
        SubTarget::SlotVar(slot) => {
            let mut s = ctx.rt.slots[slot as usize].as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, &re, &repl, Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, &re, &repl, Some(&mut s))?
            };
            ctx.rt.slots[slot as usize] = Value::Str(s);
            n
        }
        SubTarget::Field => {
            let i = extra_field_idx.unwrap();
            let mut s = ctx.rt.field(i).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, &re, &repl, Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, &re, &repl, Some(&mut s))?
            };
            ctx.rt.set_field(i, &s);
            n
        }
        SubTarget::Index(arr_idx) => {
            let key = extra_key.unwrap();
            let arr_name = ctx.str_ref(arr_idx).to_string();
            let mut s = ctx.rt.array_get(&arr_name, &key).as_str();
            let n = if is_global {
                builtins::gsub(ctx.rt, &re, &repl, Some(&mut s))?
            } else {
                builtins::sub_fn(ctx.rt, &re, &repl, Some(&mut s))?
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
        vals.push(Value::Str(String::new()));
    }
    vals.truncate(func.params.len());

    let mut frame = HashMap::new();
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
