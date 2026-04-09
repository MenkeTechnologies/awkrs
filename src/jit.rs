//! Cranelift JIT compiler for numeric bytecode chunks.
//!
//! Compiles eligible bytecode `Op` sequences into native machine code.
//! The JIT handles numeric expressions, slot variables, control flow (loops and
//! conditionals), field access via callback (constant `PushFieldNum`, dynamic
//! `GetField`, NR/FNR/NF, and fused field+slot ops), and fused peephole opcodes.
//!
//! Execution takes a [`JitRuntimeState`]: mutable `f64` slot storage plus an
//! `extern "C"` field callback (`i32` field index → `f64`; negative indices for
//! NR/FNR/NF per VM convention).
//!
//! Enable with `AWKRS_JIT=1`. The VM tries [`try_jit_execute`] before falling
//! back to the interpreter for eligible chunks.

use crate::ast::{BinOp, IncDecOp};
use crate::bytecode::Op;
use cranelift_codegen::ir::{types, AbiParam, Block, InstBuilder, MemFlags, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::Mutex;

// ── JIT cache ──────────────────────────────────────────────────────────────

/// Global cache of compiled JIT chunks keyed by ops hash.
static JIT_CACHE: Mutex<Option<HashMap<u64, Option<JitChunk>>>> = Mutex::new(None);

fn ops_hash(ops: &[Op]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Hash the raw bytes of the ops slice — Op is Copy so this is safe.
    let bytes = unsafe {
        std::slice::from_raw_parts(ops.as_ptr() as *const u8, std::mem::size_of_val(ops))
    };
    bytes.hash(&mut h);
    h.finish()
}

// ── Runtime state passed into JIT execution ───────────────────────────────

/// Per-invocation inputs for running a [`JitChunk`]: backing storage for numeric
/// slots and the field resolver used by `PushFieldNum`, fused field+slot ops, etc.
///
/// The VM fills `slots` from the interpreter runtime and supplies a callback
/// that reads `$N` as `f64` (and NR/FNR/NF for negative indices).
pub struct JitRuntimeState<'a> {
    pub slots: &'a mut [f64],
    pub field_fn: extern "C" fn(i32) -> f64,
}

impl<'a> JitRuntimeState<'a> {
    #[inline]
    pub fn new(slots: &'a mut [f64], field_fn: extern "C" fn(i32) -> f64) -> Self {
        Self { slots, field_fn }
    }
}

// ── Compiled chunk ─────────────────────────────────────────────────────────

/// Holds generated machine code. Keep alive while calling [`JitChunk::execute`].
pub struct JitChunk {
    _module: JITModule,
    /// `extern "C" fn(slots: *mut f64, field_fn: extern "C" fn(i32) -> f64) -> f64`
    fn_ptr: *const u8,
    /// Number of f64 values the function expects in the slots pointer (diagnostic).
    #[allow(dead_code)]
    slot_count: u16,
    /// Whether this chunk needs the field callback (diagnostic).
    #[allow(dead_code)]
    needs_fields: bool,
}

// JitChunk is Send+Sync because the function pointer is a finalized code pointer
// from Cranelift — it doesn't reference thread-local state.
unsafe impl Send for JitChunk {}
unsafe impl Sync for JitChunk {}

/// Machine ABI: `(slots: *mut f64, field_fn: extern "C" fn(i32) -> f64) -> f64`
type JitFn = extern "C" fn(*mut f64, extern "C" fn(i32) -> f64) -> f64;

impl JitChunk {
    /// Run the compiled chunk using the given [`JitRuntimeState`] (slots + field callback).
    pub fn execute(&self, state: &mut JitRuntimeState<'_>) -> f64 {
        let f: JitFn = unsafe { mem::transmute(self.fn_ptr) };
        f(state.slots.as_mut_ptr(), state.field_fn)
    }
}

// ── Eligibility check ──────────────────────────────────────────────────────

/// Check if a chunk can be JIT-compiled.
///
/// Eligible ops: numeric constants, slot access, arithmetic, comparisons,
/// control flow, field access (`PushFieldNum`, `GetField`, NR/FNR/NF, fused
/// field+slot ops), and fused peephole opcodes.
pub fn is_jit_eligible(ops: &[Op]) -> bool {
    if ops.is_empty() {
        return false;
    }
    let mut depth: i32 = 0;
    for op in ops {
        match op {
            // Constants
            Op::PushNum(_) => depth += 1,

            // Slot access
            Op::GetSlot(_) => depth += 1,
            Op::SetSlot(_) => { /* peek TOS, no depth change */ }

            // Arithmetic (pop 2, push 1)
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }

            // Comparison (pop 2, push 1)
            Op::CmpEq | Op::CmpNe | Op::CmpLt | Op::CmpLe | Op::CmpGt | Op::CmpGe => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }

            // Unary (pop 1, push 1)
            Op::Neg | Op::Pos | Op::Not | Op::ToBool => {
                if depth < 1 {
                    return false;
                }
            }

            // Control flow
            Op::Jump(_) | Op::JumpIfFalsePop(_) | Op::JumpIfTruePop(_) => {
                // JumpIf* pops consume 1 value from stack; we can't statically verify
                // depth across all paths in a single pass, so accept and trust the compiler.
            }

            // Stack
            Op::Pop => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            Op::Dup => {
                if depth < 1 {
                    return false;
                }
                depth += 1;
            }

            // Compound assign slot (pop rhs, push result)
            Op::CompoundAssignSlot(_, bop) => {
                if depth < 1 {
                    return false;
                }
                match bop {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {}
                    _ => return false,
                }
            }

            // Inc/dec slot (push result)
            Op::IncDecSlot(_, _) => depth += 1,

            // Fused slot ops (no stack effect — statement context)
            Op::IncrSlot(_) | Op::DecrSlot(_) => {}
            Op::AddSlotToSlot { .. } => {}

            // Field access (push 1)
            Op::PushFieldNum(_) => depth += 1,
            // Dynamic `$idx`: pop index, push field as number.
            Op::GetField => {
                if depth < 1 {
                    return false;
                }
            }
            Op::GetNR | Op::GetFNR | Op::GetNF => depth += 1,

            // Fused field+slot ops (no stack effect)
            Op::AddFieldToSlot { .. } => {}
            Op::AddMulFieldsToSlot { .. } => {}

            // Fused loop condition
            Op::JumpIfSlotGeNum { .. } => {}

            _ => return false,
        }
    }
    // For chunks with control flow we can't easily verify final depth statically.
    // Trust the compiler produced valid bytecode.
    true
}

// ── Cranelift codegen ──────────────────────────────────────────────────────

fn new_jit_module() -> Option<JITModule> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").ok()?;
    flag_builder.set("is_pic", "false").ok()?;
    flag_builder.set("opt_level", "speed").ok()?;
    let isa_builder = cranelift_native::builder().ok()?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .ok()?;
    let builder = JITBuilder::with_isa(isa, default_libcall_names());
    Some(JITModule::new(builder))
}

/// Collect all bytecode positions that are jump targets so we can create Cranelift blocks.
fn collect_jump_targets(ops: &[Op]) -> HashSet<usize> {
    let mut targets = HashSet::new();
    for op in ops {
        match op {
            Op::Jump(t) | Op::JumpIfFalsePop(t) | Op::JumpIfTruePop(t) => {
                targets.insert(*t);
            }
            Op::JumpIfSlotGeNum { target, .. } => {
                targets.insert(*target);
            }
            _ => {}
        }
    }
    targets
}

/// Check if the ops need the field callback.
fn needs_field_callback(ops: &[Op]) -> bool {
    ops.iter().any(|op| {
        matches!(
            op,
            Op::PushFieldNum(_)
                | Op::GetField
                | Op::AddFieldToSlot { .. }
                | Op::AddMulFieldsToSlot { .. }
                | Op::GetNR
                | Op::GetFNR
                | Op::GetNF
        )
    })
}

/// Find the maximum slot index referenced.
fn max_slot(ops: &[Op]) -> u16 {
    let mut m: u16 = 0;
    for op in ops {
        let s = match op {
            Op::GetSlot(s) | Op::SetSlot(s) | Op::IncrSlot(s) | Op::DecrSlot(s) => *s,
            Op::CompoundAssignSlot(s, _) | Op::IncDecSlot(s, _) => *s,
            Op::AddSlotToSlot { src, dst } => (*src).max(*dst),
            Op::AddFieldToSlot { slot, .. } => *slot,
            Op::AddMulFieldsToSlot { slot, .. } => *slot,
            Op::JumpIfSlotGeNum { slot, .. } => *slot,
            _ => continue,
        };
        m = m.max(s);
    }
    m
}

/// Compile a JIT-eligible chunk to native code.
pub fn try_compile(ops: &[Op]) -> Option<JitChunk> {
    if !is_jit_eligible(ops) {
        return None;
    }

    let mut module = new_jit_module()?;
    let mut ctx = module.make_context();
    let mut func_ctx = FunctionBuilderContext::new();

    // Function signature: (slots: *mut f64, field_fn: fn(i32) -> f64) -> f64
    let ptr_type = module.target_config().pointer_type();
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr_type)); // slots pointer
    sig.params.push(AbiParam::new(ptr_type)); // field callback fn pointer
    sig.returns.push(AbiParam::new(types::F64));

    // Declare the field callback signature for indirect calls
    let mut field_sig = module.make_signature();
    field_sig.params.push(AbiParam::new(types::I32));
    field_sig.returns.push(AbiParam::new(types::F64));
    let _field_sig_ref = module.declare_anonymous_function(&field_sig).ok()?;

    let func_id = module
        .declare_function("awkrs_jit_chunk", Linkage::Export, &sig)
        .ok()?;

    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, func_id.as_u32());

    let slot_count = if ops.iter().any(|op| {
        matches!(
            op,
            Op::GetSlot(_)
                | Op::SetSlot(_)
                | Op::IncrSlot(_)
                | Op::DecrSlot(_)
                | Op::CompoundAssignSlot(_, _)
                | Op::IncDecSlot(_, _)
                | Op::AddSlotToSlot { .. }
                | Op::AddFieldToSlot { .. }
                | Op::AddMulFieldsToSlot { .. }
                | Op::JumpIfSlotGeNum { .. }
        )
    }) {
        max_slot(ops) + 1
    } else {
        0
    };

    let has_fields = needs_field_callback(ops);

    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);

        // Import the field callback signature
        let field_sig_ir = builder.import_signature(field_sig);

        // ── Cranelift Variables for function params (survive across blocks) ──
        let var_slots_ptr = builder.declare_var(ptr_type);
        let var_field_fn = builder.declare_var(ptr_type);

        // Entry block with function params
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);

        // Collect jump targets and create blocks for each.
        // Also create blocks for fall-through after unconditional jumps.
        let jump_targets = collect_jump_targets(ops);
        let mut block_map: HashMap<usize, Block> = HashMap::new();
        for &target in &jump_targets {
            block_map.insert(target, builder.create_block());
        }
        // Also need blocks for instructions immediately after unconditional jumps
        // (these are dead code targets that may be needed for subsequent ops).
        for (i, op) in ops.iter().enumerate() {
            if matches!(op, Op::Jump(_)) && i + 1 < ops.len() && !block_map.contains_key(&(i + 1)) {
                block_map.insert(i + 1, builder.create_block());
            }
        }

        builder.switch_to_block(entry_block);

        // Store params into variables so they're accessible from any block
        let slots_ptr_val = builder.block_params(entry_block)[0];
        let field_fn_val = builder.block_params(entry_block)[1];
        builder.def_var(var_slots_ptr, slots_ptr_val);
        builder.def_var(var_field_fn, field_fn_val);

        // Seal entry block — its only predecessor is the function entry
        builder.seal_block(entry_block);

        // Cranelift value stack (mirrors the VM's operand stack).
        // At block boundaries (jump targets), the stack must be empty — all
        // JIT-eligible chunks with control flow operate on slots, not the stack.
        let mut stack: Vec<cranelift_codegen::ir::Value> = Vec::new();

        // Track whether current block is terminated (unreachable code after jump)
        let mut block_terminated = false;

        // Process each bytecode instruction
        let mut pc: usize = 0;
        while pc < ops.len() {
            // If this pc is a jump target, switch to its block
            if let Some(&target_block) = block_map.get(&pc) {
                if !block_terminated {
                    // Fall-through from previous block
                    builder.ins().jump(target_block, &[]);
                }
                builder.switch_to_block(target_block);
                block_terminated = false;
                // Stack values from a previous block don't survive across blocks.
                stack.clear();
            }

            // Skip unreachable code after an unconditional jump
            if block_terminated {
                pc += 1;
                continue;
            }

            // Read function params from variables (valid in any block)
            let slots_ptr = builder.use_var(var_slots_ptr);
            let field_fn_ptr = builder.use_var(var_field_fn);

            match ops[pc] {
                // ── Constants ───────────────────────────────────────────
                Op::PushNum(n) => {
                    stack.push(builder.ins().f64const(n));
                }

                // ── Slot access ────────────────────────────────────────
                Op::GetSlot(slot) => {
                    let offset = (slot as i32) * 8;
                    let v = builder
                        .ins()
                        .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    stack.push(v);
                }
                Op::SetSlot(slot) => {
                    let v = *stack.last().expect("SetSlot: empty stack");
                    let offset = (slot as i32) * 8;
                    builder
                        .ins()
                        .store(MemFlags::trusted(), v, slots_ptr, offset);
                }

                // ── Arithmetic ─────────────────────────────────────────
                Op::Add => {
                    let b = stack.pop().expect("Add");
                    let a = stack.pop().expect("Add");
                    stack.push(builder.ins().fadd(a, b));
                }
                Op::Sub => {
                    let b = stack.pop().expect("Sub");
                    let a = stack.pop().expect("Sub");
                    stack.push(builder.ins().fsub(a, b));
                }
                Op::Mul => {
                    let b = stack.pop().expect("Mul");
                    let a = stack.pop().expect("Mul");
                    stack.push(builder.ins().fmul(a, b));
                }
                Op::Div => {
                    let b = stack.pop().expect("Div");
                    let a = stack.pop().expect("Div");
                    stack.push(builder.ins().fdiv(a, b));
                }
                Op::Mod => {
                    let b = stack.pop().expect("Mod");
                    let a = stack.pop().expect("Mod");
                    let div = builder.ins().fdiv(a, b);
                    let trunc = builder.ins().trunc(div);
                    let prod = builder.ins().fmul(trunc, b);
                    stack.push(builder.ins().fsub(a, prod));
                }

                // ── Comparison ─────────────────────────────────────────
                Op::CmpEq => {
                    let b = stack.pop().expect("CmpEq");
                    let a = stack.pop().expect("CmpEq");
                    let cmp =
                        builder
                            .ins()
                            .fcmp(cranelift_codegen::ir::condcodes::FloatCC::Equal, a, b);
                    let i = builder.ins().uextend(types::I32, cmp);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }
                Op::CmpNe => {
                    let b = stack.pop().expect("CmpNe");
                    let a = stack.pop().expect("CmpNe");
                    let cmp = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                        a,
                        b,
                    );
                    let i = builder.ins().uextend(types::I32, cmp);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }
                Op::CmpLt => {
                    let b = stack.pop().expect("CmpLt");
                    let a = stack.pop().expect("CmpLt");
                    let cmp = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::LessThan,
                        a,
                        b,
                    );
                    let i = builder.ins().uextend(types::I32, cmp);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }
                Op::CmpLe => {
                    let b = stack.pop().expect("CmpLe");
                    let a = stack.pop().expect("CmpLe");
                    let cmp = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::LessThanOrEqual,
                        a,
                        b,
                    );
                    let i = builder.ins().uextend(types::I32, cmp);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }
                Op::CmpGt => {
                    let b = stack.pop().expect("CmpGt");
                    let a = stack.pop().expect("CmpGt");
                    let cmp = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThan,
                        a,
                        b,
                    );
                    let i = builder.ins().uextend(types::I32, cmp);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }
                Op::CmpGe => {
                    let b = stack.pop().expect("CmpGe");
                    let a = stack.pop().expect("CmpGe");
                    let cmp = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual,
                        a,
                        b,
                    );
                    let i = builder.ins().uextend(types::I32, cmp);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }

                // ── Unary ──────────────────────────────────────────────
                Op::Neg => {
                    let a = stack.pop().expect("Neg");
                    stack.push(builder.ins().fneg(a));
                }
                Op::Pos => {
                    // No-op for numeric values (identity).
                }
                Op::Not => {
                    let a = stack.pop().expect("Not");
                    let zero = builder.ins().f64const(0.0);
                    let is_zero = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::Equal,
                        a,
                        zero,
                    );
                    let i = builder.ins().uextend(types::I32, is_zero);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }
                Op::ToBool => {
                    let a = stack.pop().expect("ToBool");
                    let zero = builder.ins().f64const(0.0);
                    let ne = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                        a,
                        zero,
                    );
                    let i = builder.ins().uextend(types::I32, ne);
                    stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                }

                // ── Control flow ───────────────────────────────────────
                Op::Jump(target) => {
                    let target_block = block_map[&target];
                    builder.ins().jump(target_block, &[]);
                    block_terminated = true;
                }
                Op::JumpIfFalsePop(target) => {
                    let v = stack.pop().expect("JumpIfFalsePop");
                    let zero = builder.ins().f64const(0.0);
                    let is_false = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::Equal,
                        v,
                        zero,
                    );
                    let target_block = block_map[&target];
                    let fall_through = builder.create_block();
                    builder
                        .ins()
                        .brif(is_false, target_block, &[], fall_through, &[]);
                    builder.switch_to_block(fall_through);
                    stack.clear(); // stack doesn't survive branch
                }
                Op::JumpIfTruePop(target) => {
                    let v = stack.pop().expect("JumpIfTruePop");
                    let zero = builder.ins().f64const(0.0);
                    let is_true = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                        v,
                        zero,
                    );
                    let target_block = block_map[&target];
                    let fall_through = builder.create_block();
                    builder
                        .ins()
                        .brif(is_true, target_block, &[], fall_through, &[]);
                    builder.switch_to_block(fall_through);
                    stack.clear(); // stack doesn't survive branch
                }

                // ── Stack ops ──────────────────────────────────────────
                Op::Pop => {
                    stack.pop();
                }
                Op::Dup => {
                    let v = *stack.last().expect("Dup");
                    stack.push(v);
                }

                // ── Compound assign slot ───────────────────────────────
                Op::CompoundAssignSlot(slot, bop) => {
                    let rhs = stack.pop().expect("CompoundAssignSlot");
                    let offset = (slot as i32) * 8;
                    let old =
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let new_val = match bop {
                        BinOp::Add => builder.ins().fadd(old, rhs),
                        BinOp::Sub => builder.ins().fsub(old, rhs),
                        BinOp::Mul => builder.ins().fmul(old, rhs),
                        BinOp::Div => builder.ins().fdiv(old, rhs),
                        BinOp::Mod => {
                            let div = builder.ins().fdiv(old, rhs);
                            let trunc = builder.ins().trunc(div);
                            let prod = builder.ins().fmul(trunc, rhs);
                            builder.ins().fsub(old, prod)
                        }
                        _ => unreachable!("filtered by is_jit_eligible"),
                    };
                    builder
                        .ins()
                        .store(MemFlags::trusted(), new_val, slots_ptr, offset);
                    stack.push(new_val);
                }

                // ── Inc/dec slot (expression context — pushes result) ──
                Op::IncDecSlot(slot, kind) => {
                    let offset = (slot as i32) * 8;
                    let old =
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let delta = match kind {
                        IncDecOp::PreInc | IncDecOp::PostInc => builder.ins().f64const(1.0),
                        IncDecOp::PreDec | IncDecOp::PostDec => builder.ins().f64const(-1.0),
                    };
                    let new_val = builder.ins().fadd(old, delta);
                    builder
                        .ins()
                        .store(MemFlags::trusted(), new_val, slots_ptr, offset);
                    let push_val = match kind {
                        IncDecOp::PreInc | IncDecOp::PreDec => new_val,
                        IncDecOp::PostInc | IncDecOp::PostDec => old,
                    };
                    stack.push(push_val);
                }

                // ── Fused slot ops (statement context) ─────────────────
                Op::IncrSlot(slot) => {
                    let offset = (slot as i32) * 8;
                    let old =
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let one = builder.ins().f64const(1.0);
                    let new_val = builder.ins().fadd(old, one);
                    builder
                        .ins()
                        .store(MemFlags::trusted(), new_val, slots_ptr, offset);
                }
                Op::DecrSlot(slot) => {
                    let offset = (slot as i32) * 8;
                    let old =
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let one = builder.ins().f64const(1.0);
                    let new_val = builder.ins().fsub(old, one);
                    builder
                        .ins()
                        .store(MemFlags::trusted(), new_val, slots_ptr, offset);
                }
                Op::AddSlotToSlot { src, dst } => {
                    let sv = builder.ins().load(
                        types::F64,
                        MemFlags::trusted(),
                        slots_ptr,
                        (src as i32) * 8,
                    );
                    let dv = builder.ins().load(
                        types::F64,
                        MemFlags::trusted(),
                        slots_ptr,
                        (dst as i32) * 8,
                    );
                    let sum = builder.ins().fadd(dv, sv);
                    builder
                        .ins()
                        .store(MemFlags::trusted(), sum, slots_ptr, (dst as i32) * 8);
                }

                // ── Field access via callback ──────────────────────────
                Op::PushFieldNum(field) => {
                    let arg = builder.ins().iconst(types::I32, field as i64);
                    let call = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                    let result = builder.inst_results(call)[0];
                    stack.push(result);
                }
                Op::GetField => {
                    let fv = stack.pop().expect("GetField");
                    // Match VM: `ctx.pop().as_number() as i32` — use saturating float→int
                    // (same family of semantics as Rust’s `f64 as i32` on recent editions).
                    let idx_i32 = builder.ins().fcvt_to_sint_sat(types::I32, fv);
                    let call = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[idx_i32]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::GetNR => {
                    let arg = builder.ins().iconst(types::I32, -1i64);
                    let call = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::GetFNR => {
                    let arg = builder.ins().iconst(types::I32, -2i64);
                    let call = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::GetNF => {
                    let arg = builder.ins().iconst(types::I32, -3i64);
                    let call = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                    stack.push(builder.inst_results(call)[0]);
                }

                // ── Fused field+slot ops ───────────────────────────────
                Op::AddFieldToSlot { field, slot } => {
                    let arg = builder.ins().iconst(types::I32, field as i64);
                    let call = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                    let fv = builder.inst_results(call)[0];
                    let offset = (slot as i32) * 8;
                    let old =
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let sum = builder.ins().fadd(old, fv);
                    builder
                        .ins()
                        .store(MemFlags::trusted(), sum, slots_ptr, offset);
                }
                Op::AddMulFieldsToSlot { f1, f2, slot } => {
                    let a1 = builder.ins().iconst(types::I32, f1 as i64);
                    let c1 = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[a1]);
                    let v1 = builder.inst_results(c1)[0];
                    let a2 = builder.ins().iconst(types::I32, f2 as i64);
                    let c2 = builder
                        .ins()
                        .call_indirect(field_sig_ir, field_fn_ptr, &[a2]);
                    let v2 = builder.inst_results(c2)[0];
                    let prod = builder.ins().fmul(v1, v2);
                    let offset = (slot as i32) * 8;
                    let old =
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let sum = builder.ins().fadd(old, prod);
                    builder
                        .ins()
                        .store(MemFlags::trusted(), sum, slots_ptr, offset);
                }

                // ── Fused loop condition ───────────────────────────────
                Op::JumpIfSlotGeNum {
                    slot,
                    limit,
                    target,
                } => {
                    let offset = (slot as i32) * 8;
                    let v = builder
                        .ins()
                        .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                    let lim = builder.ins().f64const(limit);
                    let ge = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual,
                        v,
                        lim,
                    );
                    let target_block = block_map[&target];
                    let fall_through = builder.create_block();
                    builder.ins().brif(ge, target_block, &[], fall_through, &[]);
                    builder.switch_to_block(fall_through);
                    stack.clear();
                }

                _ => unreachable!("filtered by is_jit_eligible"),
            }
            pc += 1;
        }

        // Return the top of stack, or 0.0 if empty.
        if !block_terminated {
            let result = stack.pop().unwrap_or_else(|| builder.ins().f64const(0.0));
            builder.ins().return_(&[result]);
        }

        // Jump targets past the last opcode: VM exits with an empty stack → return 0.0.
        // Without this, blocks only referenced by jumps never get a terminator and codegen fails.
        for &t in &jump_targets {
            if t >= ops.len() {
                if let Some(&blk) = block_map.get(&t) {
                    builder.switch_to_block(blk);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().return_(&[z]);
                }
            }
        }

        builder.seal_all_blocks();
        builder.finalize();
    }

    module.define_function(func_id, &mut ctx).ok()?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().ok()?;

    let fn_ptr = module.get_finalized_function(func_id);

    Some(JitChunk {
        _module: module,
        fn_ptr,
        slot_count,
        needs_fields: has_fields,
    })
}

// ── Public dispatch API ────────────────────────────────────────────────────

/// Check if JIT is enabled (AWKRS_JIT=1 env var).
#[inline]
pub fn jit_enabled() -> bool {
    // Cache the env check in a static to avoid repeated lookups.
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("AWKRS_JIT").as_deref() == Some("1".as_ref()))
}

/// Try to JIT-compile and execute a chunk. Returns `Some(f64)` on success.
///
/// The caller supplies [`JitRuntimeState`] (bytecode slots as `f64` and the field callback).
pub fn try_jit_execute(ops: &[Op], state: &mut JitRuntimeState<'_>) -> Option<f64> {
    if !jit_enabled() {
        return None;
    }

    let hash = ops_hash(ops);

    // Check cache first
    {
        let cache = JIT_CACHE.lock().ok()?;
        if let Some(ref map) = *cache {
            if let Some(entry) = map.get(&hash) {
                return entry.as_ref().map(|chunk| chunk.execute(state));
            }
        }
    }

    // Compile and cache
    let chunk = try_compile(ops);
    let result = chunk.as_ref().map(|c| c.execute(state));

    {
        let mut cache = JIT_CACHE.lock().ok()?;
        let map = cache.get_or_insert_with(HashMap::new);
        map.insert(hash, chunk);
    }

    result
}

// ── Legacy API (backward compat with existing VM integration) ──────────────

/// True if `ops` is a straight-line numeric expression ending with exactly one value.
/// (Legacy check — superseded by [`is_jit_eligible`] but kept for the public API.)
pub fn is_numeric_stack_eligible(ops: &[Op]) -> bool {
    let mut depth: i32 = 0;
    for op in ops {
        match op {
            Op::PushNum(_) => depth += 1,
            Op::Add | Op::Sub | Op::Mul | Op::Div => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::Neg => {
                if depth < 1 {
                    return false;
                }
            }
            Op::Pop => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            _ => return false,
        }
    }
    depth == 1
}

/// Compile a pure-numeric expression (legacy API).
pub fn try_compile_numeric_expr(ops: &[Op]) -> Option<JitNumericChunk> {
    if !is_numeric_stack_eligible(ops) {
        return None;
    }
    // Use the new compiler but wrap in legacy struct
    let chunk = try_compile(ops)?;
    Some(JitNumericChunk { inner: chunk })
}

/// Legacy wrapper — holds a JIT chunk compiled from pure numeric ops.
pub struct JitNumericChunk {
    inner: JitChunk,
}

impl JitNumericChunk {
    /// Run the compiled expression; returns the single `f64` left on the conceptual stack.
    pub fn call_f64(&self) -> f64 {
        extern "C" fn dummy_field(_: i32) -> f64 {
            0.0
        }
        let mut empty_slots: [f64; 0] = [];
        let mut state = JitRuntimeState::new(&mut empty_slots, dummy_field);
        self.inner.execute(&mut state)
    }
}

/// Legacy dispatch — if `AWKRS_JIT=1` and the chunk is pure-numeric, run via JIT.
pub fn try_jit_dispatch_numeric_chunk(ops: &[Op]) -> Option<f64> {
    if !jit_enabled() {
        return None;
    }
    let jit = try_compile_numeric_expr(ops)?;
    Some(jit.call_f64())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn dummy_field(_: i32) -> f64 {
        0.0
    }

    fn exec(ops: &[Op]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut slots = [0.0f64; 0];
        let mut state = JitRuntimeState::new(&mut slots, dummy_field);
        chunk.execute(&mut state)
    }

    fn exec_with_slots(ops: &[Op], slots: &mut [f64]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut state = JitRuntimeState::new(slots, dummy_field);
        chunk.execute(&mut state)
    }

    fn exec_with_fields(ops: &[Op], slots: &mut [f64], field_fn: extern "C" fn(i32) -> f64) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut state = JitRuntimeState::new(slots, field_fn);
        chunk.execute(&mut state)
    }

    // ── Legacy pure-numeric tests ──────────────────────────────────────

    #[test]
    fn jit_adds_constants() {
        let ops = [Op::PushNum(3.0), Op::PushNum(4.0), Op::Add];
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 7.0).abs() < 1e-15);
    }

    #[test]
    fn jit_complex_expr() {
        // (10 - 2) * 3 / 4 = 6
        let ops = [
            Op::PushNum(10.0),
            Op::PushNum(2.0),
            Op::Sub,
            Op::PushNum(3.0),
            Op::Mul,
            Op::PushNum(4.0),
            Op::Div,
        ];
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 6.0).abs() < 1e-15);
    }

    #[test]
    fn jit_rejects_non_numeric_ops() {
        let ops = [Op::PushNum(1.0), Op::PushStr(0)];
        assert!(try_compile_numeric_expr(&ops).is_none());
    }

    // ── Arithmetic ─────────────────────────────────────────────────────

    #[test]
    fn jit_modulo() {
        let r = exec(&[Op::PushNum(10.0), Op::PushNum(3.0), Op::Mod]);
        assert!((r - 1.0).abs() < 1e-10);
    }

    #[test]
    fn jit_negation() {
        let r = exec(&[Op::PushNum(42.0), Op::Neg]);
        assert!((r - (-42.0)).abs() < 1e-15);
    }

    #[test]
    fn jit_not_zero_is_one() {
        let r = exec(&[Op::PushNum(0.0), Op::Not]);
        assert!((r - 1.0).abs() < 1e-15);
    }

    #[test]
    fn jit_not_nonzero_is_zero() {
        let r = exec(&[Op::PushNum(5.0), Op::Not]);
        assert!(r.abs() < 1e-15);
    }

    #[test]
    fn jit_to_bool() {
        assert!((exec(&[Op::PushNum(0.0), Op::ToBool])).abs() < 1e-15);
        assert!((exec(&[Op::PushNum(std::f64::consts::PI), Op::ToBool]) - 1.0).abs() < 1e-15);
    }

    // ── Comparisons ────────────────────────────────────────────────────

    #[test]
    fn jit_cmp_eq() {
        assert!((exec(&[Op::PushNum(5.0), Op::PushNum(5.0), Op::CmpEq]) - 1.0).abs() < 1e-15);
        assert!(exec(&[Op::PushNum(5.0), Op::PushNum(6.0), Op::CmpEq]).abs() < 1e-15);
    }

    #[test]
    fn jit_cmp_ne() {
        assert!((exec(&[Op::PushNum(5.0), Op::PushNum(6.0), Op::CmpNe]) - 1.0).abs() < 1e-15);
        assert!(exec(&[Op::PushNum(5.0), Op::PushNum(5.0), Op::CmpNe]).abs() < 1e-15);
    }

    #[test]
    fn jit_cmp_lt() {
        assert!((exec(&[Op::PushNum(3.0), Op::PushNum(5.0), Op::CmpLt]) - 1.0).abs() < 1e-15);
        assert!(exec(&[Op::PushNum(5.0), Op::PushNum(3.0), Op::CmpLt]).abs() < 1e-15);
    }

    #[test]
    fn jit_cmp_le() {
        assert!((exec(&[Op::PushNum(5.0), Op::PushNum(5.0), Op::CmpLe]) - 1.0).abs() < 1e-15);
        assert!(exec(&[Op::PushNum(6.0), Op::PushNum(5.0), Op::CmpLe]).abs() < 1e-15);
    }

    #[test]
    fn jit_cmp_gt() {
        assert!((exec(&[Op::PushNum(5.0), Op::PushNum(3.0), Op::CmpGt]) - 1.0).abs() < 1e-15);
        assert!(exec(&[Op::PushNum(3.0), Op::PushNum(5.0), Op::CmpGt]).abs() < 1e-15);
    }

    #[test]
    fn jit_cmp_ge() {
        assert!((exec(&[Op::PushNum(5.0), Op::PushNum(5.0), Op::CmpGe]) - 1.0).abs() < 1e-15);
        assert!(exec(&[Op::PushNum(3.0), Op::PushNum(5.0), Op::CmpGe]).abs() < 1e-15);
    }

    // ── Slot variables ─────────────────────────────────────────────────

    #[test]
    fn jit_get_set_slot() {
        let mut slots = [0.0, 42.0];
        // Push slot 1, return it
        let r = exec_with_slots(&[Op::GetSlot(1)], &mut slots);
        assert!((r - 42.0).abs() < 1e-15);
    }

    #[test]
    fn jit_set_slot_stores() {
        let mut slots = [0.0, 0.0];
        // Push 99, set slot 1, return it (SetSlot peeks, doesn't pop)
        exec_with_slots(&[Op::PushNum(99.0), Op::SetSlot(1)], &mut slots);
        assert!((slots[1] - 99.0).abs() < 1e-15);
    }

    #[test]
    fn jit_compound_assign_slot_add() {
        let mut slots = [10.0];
        let r = exec_with_slots(
            &[Op::PushNum(5.0), Op::CompoundAssignSlot(0, BinOp::Add)],
            &mut slots,
        );
        assert!((r - 15.0).abs() < 1e-15);
        assert!((slots[0] - 15.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incr_slot() {
        let mut slots = [10.0];
        exec_with_slots(&[Op::IncrSlot(0), Op::PushNum(0.0)], &mut slots);
        assert!((slots[0] - 11.0).abs() < 1e-15);
    }

    #[test]
    fn jit_decr_slot() {
        let mut slots = [10.0];
        exec_with_slots(&[Op::DecrSlot(0), Op::PushNum(0.0)], &mut slots);
        assert!((slots[0] - 9.0).abs() < 1e-15);
    }

    #[test]
    fn jit_add_slot_to_slot() {
        let mut slots = [3.0, 7.0];
        exec_with_slots(
            &[Op::AddSlotToSlot { src: 0, dst: 1 }, Op::PushNum(0.0)],
            &mut slots,
        );
        assert!((slots[1] - 10.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incdec_slot_pre_inc() {
        let mut slots = [10.0];
        let r = exec_with_slots(&[Op::IncDecSlot(0, IncDecOp::PreInc)], &mut slots);
        assert!((r - 11.0).abs() < 1e-15);
        assert!((slots[0] - 11.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incdec_slot_post_inc() {
        let mut slots = [10.0];
        let r = exec_with_slots(&[Op::IncDecSlot(0, IncDecOp::PostInc)], &mut slots);
        assert!((r - 10.0).abs() < 1e-15); // returns old value
        assert!((slots[0] - 11.0).abs() < 1e-15);
    }

    // ── Control flow ───────────────────────────────────────────────────

    #[test]
    fn jit_simple_jump() {
        // Jump over the PushNum(99) — use slots to store result since stack
        // values don't survive across block boundaries.
        let mut slots = [0.0];
        let ops = [
            Op::PushNum(1.0),
            Op::SetSlot(0),    // 1: store in slot
            Op::Pop,           // 2: clean stack
            Op::Jump(5),       // 3: jump to 5
            Op::PushNum(99.0), // 4: skipped
            Op::GetSlot(0),    // 5: target — read back from slot
        ];
        let r = exec_with_slots(&ops, &mut slots);
        assert!((r - 1.0).abs() < 1e-15);
    }

    #[test]
    fn jit_conditional_jump_false() {
        // if (0) skip to push 42; else push 99
        let ops = [
            Op::PushNum(0.0),      // 0: condition is false
            Op::JumpIfFalsePop(3), // 1: jump to 3
            Op::PushNum(99.0),     // 2: not reached via jump path
            Op::PushNum(42.0),     // 3: target
        ];
        let r = exec(&ops);
        assert!((r - 42.0).abs() < 1e-15);
    }

    #[test]
    fn jit_conditional_jump_true_no_jump() {
        // if (1) fall through to push 42
        let ops = [
            Op::PushNum(1.0),      // condition is true
            Op::JumpIfFalsePop(3), // doesn't jump (pops condition)
            Op::PushNum(42.0),     // 2: executes
                                   // 3: would be target (past end)
        ];
        let r = exec(&ops);
        assert!((r - 42.0).abs() < 1e-15);
    }

    // ── Loop: sum 1..10 using slots ────────────────────────────────────

    #[test]
    fn jit_loop_sum_1_to_10() {
        // sum = 0 (slot 0), i = 1 (slot 1)
        // loop: if i >= 11 goto end
        //   sum += i
        //   i++
        //   goto loop
        // end: return sum
        let ops = [
            Op::PushNum(0.0), // 0: push 0
            Op::SetSlot(0),   // 1: sum = 0
            Op::Pop,          // 2: pop
            Op::PushNum(1.0), // 3: push 1
            Op::SetSlot(1),   // 4: i = 1
            Op::Pop,          // 5: pop
            // loop body at pc=6:
            Op::JumpIfSlotGeNum {
                slot: 1,
                limit: 11.0,
                target: 10,
            }, // 6: if i >= 11 goto 10
            Op::AddSlotToSlot { src: 1, dst: 0 }, // 7: sum += i
            Op::IncrSlot(1),                      // 8: i++
            Op::Jump(6),                          // 9: goto loop
            // end at pc=10:
            Op::GetSlot(0), // 10: push sum
        ];
        let mut slots = [0.0, 0.0];
        let r = exec_with_slots(&ops, &mut slots);
        assert!((r - 55.0).abs() < 1e-15);
    }

    // ── Field access ───────────────────────────────────────────────────

    extern "C" fn test_field_fn(i: i32) -> f64 {
        match i {
            1 => 10.0,
            2 => 20.0,
            3 => 30.0,
            -1 => 100.0, // NR
            -2 => 50.0,  // FNR
            -3 => 3.0,   // NF
            _ => 0.0,
        }
    }

    #[test]
    fn jit_push_field_num() {
        let mut slots = [0.0; 0];
        let r = exec_with_fields(
            &[Op::PushFieldNum(1), Op::PushFieldNum(2), Op::Add],
            &mut slots,
            test_field_fn,
        );
        assert!((r - 30.0).abs() < 1e-15);
    }

    #[test]
    fn jit_get_field_dynamic() {
        let mut slots = [0.0; 0];
        let r = exec_with_fields(&[Op::PushNum(2.0), Op::GetField], &mut slots, test_field_fn);
        assert!((r - 20.0).abs() < 1e-15);
    }

    #[test]
    fn jit_loop_sum_fields_dynamic() {
        // while (i < 4) { sum += $i; i++ } with NF=3 via callback — same shape as summing `$i` for i in 1..=NF when NF=3.
        extern "C" fn fields(i: i32) -> f64 {
            match i {
                1 => 100.0,
                2 => 200.0,
                3 => 300.0,
                -3 => 3.0, // NF (unused here; limit is explicit)
                _ => 0.0,
            }
        }
        let ops = [
            Op::PushNum(0.0),
            Op::SetSlot(0),
            Op::Pop,
            Op::PushNum(1.0),
            Op::SetSlot(1),
            Op::Pop,
            Op::JumpIfSlotGeNum {
                slot: 1,
                limit: 4.0,
                target: 13,
            },
            Op::GetSlot(1),
            Op::GetField,
            Op::CompoundAssignSlot(0, BinOp::Add),
            Op::Pop,
            Op::IncrSlot(1),
            Op::Jump(6),
            Op::GetSlot(0),
        ];
        let mut slots = [0.0, 0.0];
        let r = exec_with_fields(&ops, &mut slots, fields);
        assert!((r - 600.0).abs() < 1e-15);
    }

    #[test]
    fn jit_add_field_to_slot() {
        let mut slots = [5.0];
        exec_with_fields(
            &[Op::AddFieldToSlot { field: 2, slot: 0 }, Op::PushNum(0.0)],
            &mut slots,
            test_field_fn,
        );
        assert!((slots[0] - 25.0).abs() < 1e-15);
    }

    #[test]
    fn jit_add_mul_fields_to_slot() {
        let mut slots = [0.0];
        exec_with_fields(
            &[
                Op::AddMulFieldsToSlot {
                    f1: 1,
                    f2: 2,
                    slot: 0,
                },
                Op::PushNum(0.0),
            ],
            &mut slots,
            test_field_fn,
        );
        // 10 * 20 = 200
        assert!((slots[0] - 200.0).abs() < 1e-15);
    }

    #[test]
    fn jit_get_nr_fnr_nf() {
        let mut slots = [0.0; 0];
        let nr = exec_with_fields(&[Op::GetNR], &mut slots, test_field_fn);
        assert!((nr - 100.0).abs() < 1e-15);
        let fnr = exec_with_fields(&[Op::GetFNR], &mut slots, test_field_fn);
        assert!((fnr - 50.0).abs() < 1e-15);
        let nf = exec_with_fields(&[Op::GetNF], &mut slots, test_field_fn);
        assert!((nf - 3.0).abs() < 1e-15);
    }

    // ── Fused loop condition ───────────────────────────────────────────

    #[test]
    fn jit_jump_if_slot_ge_num() {
        let mut slots = [15.0];
        // slot 0 = 15, limit = 10 → should jump
        let r = exec_with_slots(
            &[
                Op::JumpIfSlotGeNum {
                    slot: 0,
                    limit: 10.0,
                    target: 2,
                },
                Op::PushNum(99.0), // skipped
                Op::PushNum(1.0),  // target
            ],
            &mut slots,
        );
        assert!((r - 1.0).abs() < 1e-15);
    }

    // ── Eligibility ────────────────────────────────────────────────────

    #[test]
    fn jit_eligible_with_slots_and_control_flow() {
        let ops = [
            Op::PushNum(0.0),
            Op::SetSlot(0),
            Op::Pop,
            Op::GetSlot(0),
            Op::PushNum(10.0),
            Op::CmpLt,
            Op::JumpIfFalsePop(9),
            Op::IncrSlot(0),
            Op::Jump(3),
            Op::GetSlot(0),
        ];
        assert!(is_jit_eligible(&ops));
    }

    #[test]
    fn jit_rejects_string_ops() {
        assert!(!is_jit_eligible(&[Op::PushStr(0)]));
        assert!(!is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::PushStr(0),
            Op::Concat
        ]));
    }

    #[test]
    fn jit_rejects_print() {
        assert!(!is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::Print {
                argc: 1,
                redir: crate::bytecode::RedirKind::Stdout,
            },
        ]));
    }

    // ── Dup ────────────────────────────────────────────────────────────

    #[test]
    fn jit_dup() {
        let r = exec(&[Op::PushNum(7.0), Op::Dup, Op::Add]);
        assert!((r - 14.0).abs() < 1e-15);
    }

    // ── Realistic: sum fields in a loop ────────────────────────────────

    #[test]
    fn jit_sum_fields_loop() {
        // sum = 0 (slot 0), i = 1 (slot 1)
        // while i <= NF: sum += $i; i++
        // Simulates: { for (i=1; i<=NF; i++) sum += $i }
        extern "C" fn fields(i: i32) -> f64 {
            match i {
                1 => 100.0,
                2 => 200.0,
                3 => 300.0,
                -3 => 3.0, // NF = 3
                _ => 0.0,
            }
        }
        // Unrolled `sum += $1` … `$3`; dynamic `for (i=1;i<=NF;i++) sum+=$i` is covered by
        // `jit_loop_sum_fields_dynamic` (`GetField`).
        let ops = [
            Op::PushNum(0.0),                         // 0
            Op::SetSlot(0),                           // 1: sum = 0
            Op::Pop,                                  // 2
            Op::AddFieldToSlot { field: 1, slot: 0 }, // 3: sum += $1
            Op::AddFieldToSlot { field: 2, slot: 0 }, // 4: sum += $2
            Op::AddFieldToSlot { field: 3, slot: 0 }, // 5: sum += $3
            Op::GetSlot(0),                           // 6: return sum
        ];
        let mut slots = [0.0, 0.0];
        let r = exec_with_fields(&ops, &mut slots, fields);
        assert!((r - 600.0).abs() < 1e-15);
    }
}
