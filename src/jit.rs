//! Cranelift JIT compiler for AWK bytecode chunks.
//!
//! Compiles eligible bytecode `Op` sequences into native machine code.
//! The JIT handles numeric expressions, slot variables, control flow (loops,
//! conditionals, `for`-`in` iteration), field access, fused peephole opcodes,
//! print side-effects, `MatchRegexp` pattern tests, flow signals
//! (`Next`/`NextFile`/`Exit`/`Return`), fused `ArrayFieldAddConst`, and
//! `asort`/`asorti`.
//!
//! General array ops (`GetArrayElem`, `SetArrayElem`, etc.) are intentionally
//! excluded: f64 keys lose string identity (field `"x"` → 0.0 → key `"0"`).
//! The fused `ArrayFieldAddConst` is safe because its callback reads the
//! original field string from the runtime.
//!
//! Execution takes a [`JitRuntimeState`]: mutable `f64` slot storage and seven
//! `extern "C"` callbacks — `field_fn`, `array_field_add`, `var_dispatch`,
//! `field_dispatch`, `io_dispatch`, and `val_dispatch`.
//!
//! Enable with `AWKRS_JIT=1`. The VM tries [`try_jit_execute`] before falling
//! back to the interpreter for eligible chunks. Chunks with `printf`, string
//! concatenation, general array subscripts, user/builtin calls, getline, or
//! other unsupported opcodes still use the bytecode loop.

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

// ── `jit_var_dispatch` opcodes (must match `jit_var_dispatch` in vm.rs) ─────

/// `jit_var_dispatch(op, name_idx, arg)` — read global/local/slot variable as `f64`.
pub const JIT_VAR_OP_GET: u32 = 0;
/// Peek TOS as `arg` — store named variable (numeric).
pub const JIT_VAR_OP_SET: u32 = 1;
/// Statement `name++` fused (`IncrVar`).
pub const JIT_VAR_OP_INCR: u32 = 2;
pub const JIT_VAR_OP_DECR: u32 = 3;
pub const JIT_VAR_OP_COMPOUND_ADD: u32 = 4;
pub const JIT_VAR_OP_COMPOUND_SUB: u32 = 5;
pub const JIT_VAR_OP_COMPOUND_MUL: u32 = 6;
pub const JIT_VAR_OP_COMPOUND_DIV: u32 = 7;
pub const JIT_VAR_OP_COMPOUND_MOD: u32 = 8;
/// Expression `++name` / `--name` (prefix) on HashMap-path names — `arg` ignored.
///
/// **`field_dispatch`** uses the same opcode values for `$idx` **compound** and
/// **inc/dec** (`CompoundAssignField`, `IncDecField`); plain assignment uses
/// [`JIT_FIELD_OP_SET_NUM`].
pub const JIT_VAR_OP_INCDEC_PRE_INC: u32 = 9;
pub const JIT_VAR_OP_INCDEC_POST_INC: u32 = 10;
pub const JIT_VAR_OP_INCDEC_PRE_DEC: u32 = 11;
pub const JIT_VAR_OP_INCDEC_POST_DEC: u32 = 12;
/// `$idx = val` (numeric) — **`field_dispatch` only** (not used by `var_dispatch`).
pub const JIT_FIELD_OP_SET_NUM: u32 = 13;

// ── `jit_io_dispatch` opcodes (print side-effects) ───────────────────────

/// `print $field` to stdout.
pub const JIT_IO_PRINT_FIELD: u32 = 0;
/// `print $f1 sep $f2` to stdout (sep by pool index).
pub const JIT_IO_PRINT_FIELD_SEP_FIELD: u32 = 1;
/// `print $f1, $f2, $f3` to stdout (OFS between, ORS after).
pub const JIT_IO_PRINT_THREE_FIELDS: u32 = 2;
/// Bare `print` (argc=0) — print `$0 ORS` to stdout.
pub const JIT_IO_PRINT_RECORD: u32 = 3;

// ── `jit_val_dispatch` opcodes (array, match, signals) ───────────────────

pub const JIT_VAL_MATCH_REGEXP: u32 = 0;
pub const JIT_VAL_SIGNAL_NEXT: u32 = 1;
pub const JIT_VAL_SIGNAL_NEXT_FILE: u32 = 2;
pub const JIT_VAL_SIGNAL_EXIT_DEFAULT: u32 = 3;
pub const JIT_VAL_SIGNAL_EXIT_CODE: u32 = 4;
pub const JIT_VAL_ARRAY_GET: u32 = 5;
pub const JIT_VAL_ARRAY_SET: u32 = 6;
pub const JIT_VAL_ARRAY_IN: u32 = 7;
pub const JIT_VAL_ARRAY_DELETE_ELEM: u32 = 8;
pub const JIT_VAL_ARRAY_DELETE_ALL: u32 = 9;
pub const JIT_VAL_ARRAY_COMPOUND_ADD: u32 = 10;
pub const JIT_VAL_ARRAY_COMPOUND_SUB: u32 = 11;
pub const JIT_VAL_ARRAY_COMPOUND_MUL: u32 = 12;
pub const JIT_VAL_ARRAY_COMPOUND_DIV: u32 = 13;
pub const JIT_VAL_ARRAY_COMPOUND_MOD: u32 = 14;
pub const JIT_VAL_ARRAY_INCDEC_PRE_INC: u32 = 15;
pub const JIT_VAL_ARRAY_INCDEC_POST_INC: u32 = 16;
pub const JIT_VAL_ARRAY_INCDEC_PRE_DEC: u32 = 17;
pub const JIT_VAL_ARRAY_INCDEC_POST_DEC: u32 = 18;

// ── Return signals ───────────────────────────────────────────────────────
pub const JIT_VAL_SIGNAL_RETURN_VAL: u32 = 19;
pub const JIT_VAL_SIGNAL_RETURN_EMPTY: u32 = 20;

// ── ForIn iteration ──────────────────────────────────────────────────────
pub const JIT_VAL_FORIN_START: u32 = 21;
/// Returns 1.0 (has next key, stored in var) or 0.0 (exhausted).
pub const JIT_VAL_FORIN_NEXT: u32 = 22;
pub const JIT_VAL_FORIN_END: u32 = 23;

// ── Array sorting ────────────────────────────────────────────────────────
pub const JIT_VAL_ASORT: u32 = 24;
pub const JIT_VAL_ASORTI: u32 = 25;

#[inline]
fn jit_val_op_for_array_compound(bop: BinOp) -> u32 {
    match bop {
        BinOp::Add => JIT_VAL_ARRAY_COMPOUND_ADD,
        BinOp::Sub => JIT_VAL_ARRAY_COMPOUND_SUB,
        BinOp::Mul => JIT_VAL_ARRAY_COMPOUND_MUL,
        BinOp::Div => JIT_VAL_ARRAY_COMPOUND_DIV,
        BinOp::Mod => JIT_VAL_ARRAY_COMPOUND_MOD,
        _ => unreachable!("filtered by is_jit_eligible"),
    }
}

#[inline]
fn jit_val_op_for_array_incdec(kind: IncDecOp) -> u32 {
    match kind {
        IncDecOp::PreInc => JIT_VAL_ARRAY_INCDEC_PRE_INC,
        IncDecOp::PostInc => JIT_VAL_ARRAY_INCDEC_POST_INC,
        IncDecOp::PreDec => JIT_VAL_ARRAY_INCDEC_PRE_DEC,
        IncDecOp::PostDec => JIT_VAL_ARRAY_INCDEC_POST_DEC,
    }
}

#[inline]
fn jit_var_op_for_compound(bop: BinOp) -> u32 {
    match bop {
        BinOp::Add => JIT_VAR_OP_COMPOUND_ADD,
        BinOp::Sub => JIT_VAR_OP_COMPOUND_SUB,
        BinOp::Mul => JIT_VAR_OP_COMPOUND_MUL,
        BinOp::Div => JIT_VAR_OP_COMPOUND_DIV,
        BinOp::Mod => JIT_VAR_OP_COMPOUND_MOD,
        _ => unreachable!("filtered by is_jit_eligible"),
    }
}

#[inline]
fn jit_var_op_for_incdec(kind: IncDecOp) -> u32 {
    match kind {
        IncDecOp::PreInc => JIT_VAR_OP_INCDEC_PRE_INC,
        IncDecOp::PostInc => JIT_VAR_OP_INCDEC_POST_INC,
        IncDecOp::PreDec => JIT_VAR_OP_INCDEC_PRE_DEC,
        IncDecOp::PostDec => JIT_VAR_OP_INCDEC_POST_DEC,
    }
}

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
/// slots, the field resolver, and the fused array update callback.
///
/// The VM fills `slots` from the interpreter runtime and supplies callbacks
/// that match the bytecode interpreter (`field_fn`, `array_field_add`,
/// `var_dispatch`, and `field_dispatch` for `SetField` / `CompoundAssignField` / `IncDecField`).
pub struct JitRuntimeState<'a> {
    pub slots: &'a mut [f64],
    pub field_fn: extern "C" fn(i32) -> f64,
    /// Fused `a[$field] += delta` — interned array name index, constant field
    /// number, delta (see VM).
    pub array_field_add: extern "C" fn(u32, i32, f64),
    /// Multiplexed HashMap-path variable ops — see `JIT_VAR_OP_GET` and friends.
    pub var_dispatch: extern "C" fn(u32, u32, f64) -> f64,
    /// `$idx = val`, compound assign, `++$idx` — [`JIT_FIELD_OP_SET_NUM`] for assignment;
    /// same `JIT_VAR_OP_*` as [`JitRuntimeState::var_dispatch`] for compound and inc/dec.
    pub field_dispatch: extern "C" fn(u32, i32, f64) -> f64,
    /// Print side-effects: `PrintFieldStdout`, `PrintFieldSepField`, `PrintThreeFieldsStdout`,
    /// bare `print` (argc=0).  `(op, a1, a2, a3)` — void.
    pub io_dispatch: extern "C" fn(u32, i32, i32, i32),
    /// Array ops, `MatchRegexp`, flow signals (Next/Exit).
    /// `(op, a1, a2, a3) -> f64`.
    pub val_dispatch: extern "C" fn(u32, u32, f64, f64) -> f64,
}

impl<'a> JitRuntimeState<'a> {
    #[inline]
    pub fn new(
        slots: &'a mut [f64],
        field_fn: extern "C" fn(i32) -> f64,
        array_field_add: extern "C" fn(u32, i32, f64),
        var_dispatch: extern "C" fn(u32, u32, f64) -> f64,
        field_dispatch: extern "C" fn(u32, i32, f64) -> f64,
        io_dispatch: extern "C" fn(u32, i32, i32, i32),
        val_dispatch: extern "C" fn(u32, u32, f64, f64) -> f64,
    ) -> Self {
        Self {
            slots,
            field_fn,
            array_field_add,
            var_dispatch,
            field_dispatch,
            io_dispatch,
            val_dispatch,
        }
    }
}

// ── Compiled chunk ─────────────────────────────────────────────────────────

/// Holds generated machine code. Keep alive while calling [`JitChunk::execute`].
pub struct JitChunk {
    _module: JITModule,
    /// `extern "C" fn(slots, field_fn, array_field_add, var_dispatch, field_dispatch) -> f64`
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

/// Machine ABI: `array_field_add` is for fused `a[$field] += delta`; `var_dispatch`
/// multiplexes `GetVar` / `SetVar` / `IncrVar` / `DecrVar` / `CompoundAssignVar` /
/// `IncDecVar`; `field_dispatch` multiplexes `SetField` ([`JIT_FIELD_OP_SET_NUM`]),
/// `CompoundAssignField` / `IncDecField` (reuses `JIT_VAR_OP_*` for compound and inc/dec);
/// `io_dispatch` handles print side-effects; `val_dispatch` handles array ops,
/// `MatchRegexp`, and flow signals.
type JitFn = extern "C" fn(
    *mut f64,
    extern "C" fn(i32) -> f64,
    extern "C" fn(u32, i32, f64),
    extern "C" fn(u32, u32, f64) -> f64,
    extern "C" fn(u32, i32, f64) -> f64,
    extern "C" fn(u32, i32, i32, i32),
    extern "C" fn(u32, u32, f64, f64) -> f64,
) -> f64;

impl JitChunk {
    /// Run the compiled chunk using the given [`JitRuntimeState`].
    pub fn execute(&self, state: &mut JitRuntimeState<'_>) -> f64 {
        let f: JitFn = unsafe { mem::transmute(self.fn_ptr) };
        f(
            state.slots.as_mut_ptr(),
            state.field_fn,
            state.array_field_add,
            state.var_dispatch,
            state.field_dispatch,
            state.io_dispatch,
            state.val_dispatch,
        )
    }
}

// ── Eligibility check ──────────────────────────────────────────────────────

/// Check if a chunk can be JIT-compiled.
///
/// Eligible ops: numeric constants, slot access, arithmetic, comparisons,
/// control flow, field access (`PushFieldNum`, `GetField`, NR/FNR/NF, fused
/// field+slot ops), fused `a[$n] += delta`, and
/// other fused peephole opcodes.
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

            // HashMap-path variable (same numeric subset as compound slot)
            Op::CompoundAssignVar(_, bop) => {
                if depth < 1 {
                    return false;
                }
                match bop {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {}
                    _ => return false,
                }
            }
            Op::GetVar(_) => depth += 1,
            Op::SetVar(_) => {}
            Op::IncrVar(_) | Op::DecrVar(_) => {}

            // HashMap-path `++x` / `x++` when not fused to IncrVar/DecrVar (push result)
            Op::IncDecVar(_, _) => depth += 1,

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

            // Fused `a[$field] += delta` — mutates runtime array, no stack effect.
            Op::ArrayFieldAddConst { .. } => {}

            // `$n` compound / inc-dec (pop field index as number; push result)
            Op::CompoundAssignField(bop) => {
                if depth < 2 {
                    return false;
                }
                match bop {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {}
                    _ => return false,
                }
                depth -= 1;
            }
            Op::IncDecField(_) => {
                if depth < 1 {
                    return false;
                }
            }
            // `$idx = val` — pop value, pop index, push value (numeric JIT path).
            Op::SetField => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }

            // ── Fused print ops (no stack effect) ──────────────────────
            Op::PrintFieldStdout(_) => {}
            Op::PrintFieldSepField { .. } => {}
            Op::PrintThreeFieldsStdout { .. } => {}
            // Bare `print` (argc=0, stdout only)
            Op::Print {
                argc: 0,
                redir: crate::bytecode::RedirKind::Stdout,
            } => {}

            // ── MatchRegexp (push 0/1) ─────────────────────────────────
            Op::MatchRegexp(_) => depth += 1,

            // ── Flow signals (terminate chunk) ─────────────────────────
            Op::Next | Op::NextFile | Op::ExitDefault => {}
            Op::ExitWithCode => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }

            // NOTE: General array ops (GetArrayElem, SetArrayElem, InArray,
            // DeleteElem, DeleteArray, CompoundAssignIndex, IncDecIndex) are
            // intentionally NOT eligible.  Array keys on the f64 JIT stack lose
            // string identity (field "x" → 0.0 → key "0"), producing wrong
            // results for non-numeric keys.  The fused `ArrayFieldAddConst`
            // remains eligible because its callback reads the original field
            // string directly from the runtime.

            // ── Return signals ─────────────────────────────────────────
            Op::ReturnVal => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            Op::ReturnEmpty => {}

            // ── ForIn iteration ────────────────────────────────────────
            Op::ForInStart(_) => {}
            Op::ForInNext { .. } => {}
            Op::ForInEnd => {}

            // ── Array sorting (push count) ─────────────────────────────
            Op::Asort { .. } | Op::Asorti { .. } => depth += 1,

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
            Op::ForInNext { end_jump, .. } => {
                targets.insert(*end_jump);
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
                | Op::ArrayFieldAddConst { .. }
                | Op::SetField
                | Op::CompoundAssignField(_)
                | Op::IncDecField(_)
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

    // Function signature: (slots, field_fn, array_field_add, var_dispatch, field_dispatch,
    //                      io_dispatch, val_dispatch) -> f64
    let ptr_type = module.target_config().pointer_type();
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr_type)); // slots pointer
    sig.params.push(AbiParam::new(ptr_type)); // field callback fn pointer
    sig.params.push(AbiParam::new(ptr_type)); // array_field_add fn pointer
    sig.params.push(AbiParam::new(ptr_type)); // var_dispatch fn pointer
    sig.params.push(AbiParam::new(ptr_type)); // field_dispatch ($n compound / incdec)
    sig.params.push(AbiParam::new(ptr_type)); // io_dispatch (print)
    sig.params.push(AbiParam::new(ptr_type)); // val_dispatch (array, match, signal)
    sig.returns.push(AbiParam::new(types::F64));

    // Declare the field callback signature for indirect calls
    let mut field_sig = module.make_signature();
    field_sig.params.push(AbiParam::new(types::I32));
    field_sig.returns.push(AbiParam::new(types::F64));
    let _field_sig_ref = module.declare_anonymous_function(&field_sig).ok()?;

    // `array_field_add(arr_pool_idx, field, delta)` — void
    let mut array_sig = module.make_signature();
    array_sig.params.push(AbiParam::new(types::I32));
    array_sig.params.push(AbiParam::new(types::I32));
    array_sig.params.push(AbiParam::new(types::F64));
    let _array_sig_ref = module.declare_anonymous_function(&array_sig).ok()?;

    let mut var_sig = module.make_signature();
    var_sig.params.push(AbiParam::new(types::I32));
    var_sig.params.push(AbiParam::new(types::I32));
    var_sig.params.push(AbiParam::new(types::F64));
    var_sig.returns.push(AbiParam::new(types::F64));
    let _var_sig_ref = module.declare_anonymous_function(&var_sig).ok()?;

    // `field_dispatch(op, field_idx, arg)` — `$n` compound assign / inc-dec (reuses `JIT_VAR_OP_*`)
    let mut field_mut_sig = module.make_signature();
    field_mut_sig.params.push(AbiParam::new(types::I32));
    field_mut_sig.params.push(AbiParam::new(types::I32));
    field_mut_sig.params.push(AbiParam::new(types::F64));
    field_mut_sig.returns.push(AbiParam::new(types::F64));
    let _field_mut_sig_ref = module.declare_anonymous_function(&field_mut_sig).ok()?;

    // `io_dispatch(op, a1, a2, a3)` — void (print side-effects)
    let mut io_sig = module.make_signature();
    io_sig.params.push(AbiParam::new(types::I32));
    io_sig.params.push(AbiParam::new(types::I32));
    io_sig.params.push(AbiParam::new(types::I32));
    io_sig.params.push(AbiParam::new(types::I32));
    let _io_sig_ref = module.declare_anonymous_function(&io_sig).ok()?;

    // `val_dispatch(op, a1, a2, a3) -> f64` (array, match, signals)
    let mut val_sig = module.make_signature();
    val_sig.params.push(AbiParam::new(types::I32));
    val_sig.params.push(AbiParam::new(types::I32));
    val_sig.params.push(AbiParam::new(types::F64));
    val_sig.params.push(AbiParam::new(types::F64));
    val_sig.returns.push(AbiParam::new(types::F64));
    let _val_sig_ref = module.declare_anonymous_function(&val_sig).ok()?;

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

        // Import callback signatures
        let field_sig_ir = builder.import_signature(field_sig);
        let array_sig_ir = builder.import_signature(array_sig);
        let var_sig_ir = builder.import_signature(var_sig);
        let field_mut_sig_ir = builder.import_signature(field_mut_sig);
        let io_sig_ir = builder.import_signature(io_sig);
        let val_sig_ir = builder.import_signature(val_sig);

        // ── Cranelift Variables for function params (survive across blocks) ──
        let var_slots_ptr = builder.declare_var(ptr_type);
        let var_field_fn = builder.declare_var(ptr_type);
        let var_array_fn = builder.declare_var(ptr_type);
        let var_var_fn = builder.declare_var(ptr_type);
        let var_field_mut_fn = builder.declare_var(ptr_type);
        let var_io_fn = builder.declare_var(ptr_type);
        let var_val_fn = builder.declare_var(ptr_type);

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
        let array_fn_val = builder.block_params(entry_block)[2];
        let var_dispatch_val = builder.block_params(entry_block)[3];
        let field_mut_dispatch_val = builder.block_params(entry_block)[4];
        let io_dispatch_val = builder.block_params(entry_block)[5];
        let val_dispatch_val = builder.block_params(entry_block)[6];
        builder.def_var(var_slots_ptr, slots_ptr_val);
        builder.def_var(var_field_fn, field_fn_val);
        builder.def_var(var_array_fn, array_fn_val);
        builder.def_var(var_var_fn, var_dispatch_val);
        builder.def_var(var_field_mut_fn, field_mut_dispatch_val);
        builder.def_var(var_io_fn, io_dispatch_val);
        builder.def_var(var_val_fn, val_dispatch_val);

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
            let array_fn_ptr = builder.use_var(var_array_fn);
            let var_fn_ptr = builder.use_var(var_var_fn);
            let field_mut_fn_ptr = builder.use_var(var_field_mut_fn);
            let io_fn_ptr = builder.use_var(var_io_fn);
            let val_fn_ptr = builder.use_var(var_val_fn);

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

                Op::GetVar(idx) => {
                    let opv = builder.ins().iconst(types::I32, i64::from(JIT_VAR_OP_GET));
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder
                        .ins()
                        .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::SetVar(idx) => {
                    let v = *stack.last().expect("SetVar: empty stack");
                    let opv = builder.ins().iconst(types::I32, i64::from(JIT_VAR_OP_SET));
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    builder
                        .ins()
                        .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, v]);
                }
                Op::IncrVar(idx) => {
                    let opv = builder.ins().iconst(types::I32, i64::from(JIT_VAR_OP_INCR));
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, z]);
                }
                Op::DecrVar(idx) => {
                    let opv = builder.ins().iconst(types::I32, i64::from(JIT_VAR_OP_DECR));
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, z]);
                }
                Op::CompoundAssignVar(idx, bop) => {
                    let rhs = stack.pop().expect("CompoundAssignVar");
                    let cop = jit_var_op_for_compound(bop);
                    let opv = builder.ins().iconst(types::I32, i64::from(cop));
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    let call = builder
                        .ins()
                        .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, rhs]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::IncDecVar(idx, kind) => {
                    let cop = jit_var_op_for_incdec(kind);
                    let opv = builder.ins().iconst(types::I32, i64::from(cop));
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder
                        .ins()
                        .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::CompoundAssignField(bop) => {
                    let rhs = stack.pop().expect("CompoundAssignField");
                    let idx_f = stack.pop().expect("CompoundAssignField idx");
                    let idx_i32 = builder.ins().fcvt_to_sint_sat(types::I32, idx_f);
                    let cop = jit_var_op_for_compound(bop);
                    let opv = builder.ins().iconst(types::I32, i64::from(cop));
                    let call = builder.ins().call_indirect(
                        field_mut_sig_ir,
                        field_mut_fn_ptr,
                        &[opv, idx_i32, rhs],
                    );
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::IncDecField(kind) => {
                    let idx_f = stack.pop().expect("IncDecField");
                    let idx_i32 = builder.ins().fcvt_to_sint_sat(types::I32, idx_f);
                    let cop = jit_var_op_for_incdec(kind);
                    let opv = builder.ins().iconst(types::I32, i64::from(cop));
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(
                        field_mut_sig_ir,
                        field_mut_fn_ptr,
                        &[opv, idx_i32, z],
                    );
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::SetField => {
                    let val = stack.pop().expect("SetField val");
                    let idx_f = stack.pop().expect("SetField idx");
                    let idx_i32 = builder.ins().fcvt_to_sint_sat(types::I32, idx_f);
                    let opv = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_FIELD_OP_SET_NUM));
                    let call = builder.ins().call_indirect(
                        field_mut_sig_ir,
                        field_mut_fn_ptr,
                        &[opv, idx_i32, val],
                    );
                    stack.push(builder.inst_results(call)[0]);
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

                Op::ArrayFieldAddConst { arr, field, delta } => {
                    let arr_idx = builder.ins().iconst(types::I32, i64::from(arr));
                    let field_idx = builder.ins().iconst(types::I32, i64::from(field));
                    let d = builder.ins().f64const(delta);
                    builder.ins().call_indirect(
                        array_sig_ir,
                        array_fn_ptr,
                        &[arr_idx, field_idx, d],
                    );
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

                // ── Fused print ops (side-effect only) ─────────────────
                Op::PrintFieldStdout(field) => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_IO_PRINT_FIELD));
                    let a1 = builder.ins().iconst(types::I32, field as i64);
                    let z = builder.ins().iconst(types::I32, 0);
                    builder.ins().call_indirect(io_sig_ir, io_fn_ptr, &[op_c, a1, z, z]);
                }
                Op::PrintFieldSepField { f1, sep, f2 } => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_IO_PRINT_FIELD_SEP_FIELD));
                    let a1 = builder.ins().iconst(types::I32, f1 as i64);
                    let a2 = builder.ins().iconst(types::I32, sep as i64);
                    let a3 = builder.ins().iconst(types::I32, f2 as i64);
                    builder.ins().call_indirect(io_sig_ir, io_fn_ptr, &[op_c, a1, a2, a3]);
                }
                Op::PrintThreeFieldsStdout { f1, f2, f3 } => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_IO_PRINT_THREE_FIELDS));
                    let a1 = builder.ins().iconst(types::I32, f1 as i64);
                    let a2 = builder.ins().iconst(types::I32, f2 as i64);
                    let a3 = builder.ins().iconst(types::I32, f3 as i64);
                    builder.ins().call_indirect(io_sig_ir, io_fn_ptr, &[op_c, a1, a2, a3]);
                }
                Op::Print { argc: 0, redir: crate::bytecode::RedirKind::Stdout } => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_IO_PRINT_RECORD));
                    let z = builder.ins().iconst(types::I32, 0);
                    builder.ins().call_indirect(io_sig_ir, io_fn_ptr, &[op_c, z, z, z]);
                }

                // ── MatchRegexp (push 0/1) ────────────────────────────────
                Op::MatchRegexp(idx) => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_MATCH_REGEXP));
                    let a1 = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    stack.push(builder.inst_results(call)[0]);
                }

                // ── Flow signals ──────────────────────────────────────────
                Op::Next => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_SIGNAL_NEXT));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::NextFile => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_SIGNAL_NEXT_FILE));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::ExitDefault => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_SIGNAL_EXIT_DEFAULT));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::ExitWithCode => {
                    let code = stack.pop().expect("ExitWithCode");
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_SIGNAL_EXIT_CODE));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, code, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }

                // ── Array ops ─────────────────────────────────────────────
                Op::GetArrayElem(arr) => {
                    let key = stack.pop().expect("GetArrayElem key");
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ARRAY_GET));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::SetArrayElem(arr) => {
                    let val = stack.pop().expect("SetArrayElem val");
                    let key = stack.pop().expect("SetArrayElem key");
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ARRAY_SET));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, val]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::InArray(arr) => {
                    let key = stack.pop().expect("InArray key");
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ARRAY_IN));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::DeleteElem(arr) => {
                    let key = stack.pop().expect("DeleteElem key");
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ARRAY_DELETE_ELEM));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                }
                Op::DeleteArray(arr) => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ARRAY_DELETE_ALL));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                }
                Op::CompoundAssignIndex(arr, bop) => {
                    let rhs = stack.pop().expect("CompoundAssignIndex rhs");
                    let key = stack.pop().expect("CompoundAssignIndex key");
                    let cop = jit_val_op_for_array_compound(bop);
                    let op_c = builder.ins().iconst(types::I32, i64::from(cop));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, rhs]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::IncDecIndex(arr, kind) => {
                    let key = stack.pop().expect("IncDecIndex key");
                    let cop = jit_val_op_for_array_incdec(kind);
                    let op_c = builder.ins().iconst(types::I32, i64::from(cop));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                    stack.push(builder.inst_results(call)[0]);
                }

                // ── Return signals ────────────────────────────────────────
                Op::ReturnVal => {
                    let val = stack.pop().expect("ReturnVal");
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_SIGNAL_RETURN_VAL));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, val, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::ReturnEmpty => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_SIGNAL_RETURN_EMPTY));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }

                // ── ForIn iteration ───────────────────────────────────────
                Op::ForInStart(arr) => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_FORIN_START));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                }
                Op::ForInNext { var, end_jump } => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_FORIN_NEXT));
                    let a1 = builder.ins().iconst(types::I32, var as i64);
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    let result = builder.inst_results(call)[0];
                    // 0.0 = exhausted → jump to end_jump; 1.0 = has next → continue
                    let zero = builder.ins().f64const(0.0);
                    let exhausted = builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::Equal,
                        result,
                        zero,
                    );
                    let end_block = block_map[&end_jump];
                    let fall_through = builder.create_block();
                    builder.ins().brif(exhausted, end_block, &[], fall_through, &[]);
                    builder.switch_to_block(fall_through);
                    stack.clear();
                }
                Op::ForInEnd => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_FORIN_END));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                }

                // ── Array sorting ─────────────────────────────────────────
                Op::Asort { src, dest } => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ASORT));
                    let a1 = builder.ins().iconst(types::I32, src as i64);
                    // Encode dest: -1.0 = None, otherwise pool index as f64.
                    let d = match dest {
                        Some(d) => builder.ins().f64const(d as f64),
                        None => builder.ins().f64const(-1.0),
                    };
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, d, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::Asorti { src, dest } => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(JIT_VAL_ASORTI));
                    let a1 = builder.ins().iconst(types::I32, src as i64);
                    let d = match dest {
                        Some(d) => builder.ins().f64const(d as f64),
                        None => builder.ins().f64const(-1.0),
                    };
                    let z = builder.ins().f64const(0.0);
                    let call = builder.ins().call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, d, z]);
                    stack.push(builder.inst_results(call)[0]);
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
/// The caller supplies [`JitRuntimeState`] (slots and the four `extern "C"` callbacks).
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
        extern "C" fn dummy_array(_: u32, _: i32, _: f64) {}
        extern "C" fn dummy_var(_: u32, _: u32, _: f64) -> f64 {
            0.0
        }
        extern "C" fn dummy_field_dispatch(_: u32, _: i32, _: f64) -> f64 {
            0.0
        }
        extern "C" fn dummy_io_dispatch(_: u32, _: i32, _: i32, _: i32) {}
        extern "C" fn dummy_val_dispatch(_: u32, _: u32, _: f64, _: f64) -> f64 {
            0.0
        }
        let mut empty_slots: [f64; 0] = [];
        let mut state = JitRuntimeState::new(
            &mut empty_slots,
            dummy_field,
            dummy_array,
            dummy_var,
            dummy_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
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
    use crate::runtime::Value;
    use std::cell::RefCell;

    thread_local! {
        static TEST_JIT_VARS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }

    thread_local! {
        static TEST_JIT_FIELDS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }

    /// Minimal in-process `var_dispatch` for unit tests (mirrors `jit_var_dispatch` numerics).
    extern "C" fn test_var_dispatch(op: u32, name_idx: u32, arg: f64) -> f64 {
        use crate::ast::BinOp;
        use crate::jit::{
            JIT_VAR_OP_COMPOUND_ADD, JIT_VAR_OP_COMPOUND_DIV, JIT_VAR_OP_COMPOUND_MOD,
            JIT_VAR_OP_COMPOUND_MUL, JIT_VAR_OP_COMPOUND_SUB, JIT_VAR_OP_DECR, JIT_VAR_OP_GET,
            JIT_VAR_OP_INCDEC_POST_DEC, JIT_VAR_OP_INCDEC_POST_INC, JIT_VAR_OP_INCDEC_PRE_DEC,
            JIT_VAR_OP_INCDEC_PRE_INC, JIT_VAR_OP_INCR, JIT_VAR_OP_SET,
        };
        TEST_JIT_VARS.with(|cell| {
            let mut v = cell.borrow_mut();
            let i = name_idx as usize;
            if v.len() <= i {
                v.resize(i + 1, 0.0);
            }
            match op {
                JIT_VAR_OP_GET => v[i],
                JIT_VAR_OP_SET => {
                    v[i] = arg;
                    arg
                }
                JIT_VAR_OP_INCR => {
                    v[i] += 1.0;
                    0.0
                }
                JIT_VAR_OP_DECR => {
                    v[i] -= 1.0;
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
                    let a = v[i];
                    let b = arg;
                    let n = match bop {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => a / b,
                        BinOp::Mod => a % b,
                        _ => 0.0,
                    };
                    v[i] = n;
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
                    let old_n = v[i];
                    let delta = match kind {
                        IncDecOp::PreInc | IncDecOp::PostInc => 1.0,
                        IncDecOp::PreDec | IncDecOp::PostDec => -1.0,
                    };
                    let new_n = old_n + delta;
                    v[i] = new_n;
                    match kind {
                        IncDecOp::PreInc | IncDecOp::PreDec => new_n,
                        IncDecOp::PostInc | IncDecOp::PostDec => old_n,
                    }
                }
                _ => 0.0,
            }
        })
    }

    /// Test double for `field_dispatch` — field index `i` stored at `Vec` index `max(0,i)`.
    extern "C" fn test_field_dispatch(op: u32, field_idx: i32, arg: f64) -> f64 {
        use crate::ast::BinOp;
        use crate::jit::{
            JIT_FIELD_OP_SET_NUM, JIT_VAR_OP_COMPOUND_ADD, JIT_VAR_OP_COMPOUND_DIV,
            JIT_VAR_OP_COMPOUND_MOD, JIT_VAR_OP_COMPOUND_MUL, JIT_VAR_OP_COMPOUND_SUB,
            JIT_VAR_OP_INCDEC_POST_DEC, JIT_VAR_OP_INCDEC_POST_INC, JIT_VAR_OP_INCDEC_PRE_DEC,
            JIT_VAR_OP_INCDEC_PRE_INC,
        };
        let i = field_idx.max(0) as usize;
        TEST_JIT_FIELDS.with(|cell| {
            let mut v = cell.borrow_mut();
            if v.len() <= i {
                v.resize(i + 1, 0.0);
            }
            match op {
                JIT_FIELD_OP_SET_NUM => {
                    v[i] = arg;
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
                    let a = v[i];
                    let b = arg;
                    let n = match bop {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => a / b,
                        BinOp::Mod => a % b,
                        _ => 0.0,
                    };
                    v[i] = n;
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
                    let old_n = v[i];
                    let delta = match kind {
                        IncDecOp::PreInc | IncDecOp::PostInc => 1.0,
                        IncDecOp::PreDec | IncDecOp::PostDec => -1.0,
                    };
                    let new_n = old_n + delta;
                    v[i] = new_n;
                    match kind {
                        IncDecOp::PreInc | IncDecOp::PreDec => new_n,
                        IncDecOp::PostInc | IncDecOp::PostDec => old_n,
                    }
                }
                _ => 0.0,
            }
        })
    }

    fn setup_test_vars(initial: &[f64]) {
        TEST_JIT_VARS.with(|c| {
            *c.borrow_mut() = initial.to_vec();
        });
    }

    fn snapshot_test_vars() -> Vec<f64> {
        TEST_JIT_VARS.with(|c| c.borrow().clone())
    }

    fn setup_test_fields(initial: &[(i32, f64)]) {
        TEST_JIT_FIELDS.with(|c| {
            let mut v = c.borrow_mut();
            v.clear();
            for &(idx, val) in initial {
                let i = idx.max(0) as usize;
                if v.len() <= i {
                    v.resize(i + 1, 0.0);
                }
                v[i] = val;
            }
        });
    }

    fn field_at(idx: i32) -> f64 {
        let i = idx.max(0) as usize;
        TEST_JIT_FIELDS.with(|c| {
            let v = c.borrow();
            v.get(i).copied().unwrap_or(0.0)
        })
    }

    fn exec_with_test_var(ops: &[Op]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut slots = [0.0f64; 0];
        let mut state = JitRuntimeState::new(
            &mut slots,
            dummy_field,
            dummy_array,
            test_var_dispatch,
            dummy_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
        chunk.execute(&mut state)
    }

    fn exec_with_test_field(ops: &[Op]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut slots = [0.0f64; 0];
        let mut state = JitRuntimeState::new(
            &mut slots,
            dummy_field,
            dummy_array,
            dummy_var,
            test_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
        chunk.execute(&mut state)
    }

    extern "C" fn dummy_field(_: i32) -> f64 {
        0.0
    }

    extern "C" fn dummy_array(_: u32, _: i32, _: f64) {}

    extern "C" fn dummy_var(_: u32, _: u32, _: f64) -> f64 {
        0.0
    }

    extern "C" fn dummy_field_dispatch(_: u32, _: i32, _: f64) -> f64 {
        0.0
    }

    extern "C" fn dummy_io_dispatch(_: u32, _: i32, _: i32, _: i32) {}

    extern "C" fn dummy_val_dispatch(_: u32, _: u32, _: f64, _: f64) -> f64 {
        0.0
    }

    fn exec(ops: &[Op]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut slots = [0.0f64; 0];
        let mut state = JitRuntimeState::new(
            &mut slots,
            dummy_field,
            dummy_array,
            dummy_var,
            dummy_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
        chunk.execute(&mut state)
    }

    fn exec_with_slots(ops: &[Op], slots: &mut [f64]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut state = JitRuntimeState::new(
            slots,
            dummy_field,
            dummy_array,
            dummy_var,
            dummy_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
        chunk.execute(&mut state)
    }

    fn exec_with_fields(ops: &[Op], slots: &mut [f64], field_fn: extern "C" fn(i32) -> f64) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut state = JitRuntimeState::new(
            slots,
            field_fn,
            dummy_array,
            dummy_var,
            dummy_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
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

    #[test]
    fn jit_eligible_array_field_add_const() {
        assert!(is_jit_eligible(&[
            Op::ArrayFieldAddConst {
                arr: 0,
                field: 1,
                delta: 1.0,
            },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_hashmap_var_ops() {
        assert!(is_jit_eligible(&[
            Op::GetVar(0),
            Op::PushNum(1.0),
            Op::Add,
            Op::IncrVar(1),
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_incdec_var() {
        assert!(is_jit_eligible(&[
            Op::IncDecVar(0, IncDecOp::PostInc),
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_incdec_var_post_inc() {
        setup_test_vars(&[10.0]);
        let r = exec_with_test_var(&[Op::IncDecVar(0, IncDecOp::PostInc)]);
        assert!((r - 10.0).abs() < 1e-15);
        assert!((snapshot_test_vars()[0] - 11.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incdec_var_pre_inc() {
        setup_test_vars(&[10.0]);
        let r = exec_with_test_var(&[Op::IncDecVar(0, IncDecOp::PreInc)]);
        assert!((r - 11.0).abs() < 1e-15);
        assert!((snapshot_test_vars()[0] - 11.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incdec_var_post_dec() {
        setup_test_vars(&[10.0]);
        let r = exec_with_test_var(&[Op::IncDecVar(0, IncDecOp::PostDec)]);
        assert!((r - 10.0).abs() < 1e-15);
        assert!((snapshot_test_vars()[0] - 9.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incdec_var_pre_dec() {
        setup_test_vars(&[10.0]);
        let r = exec_with_test_var(&[Op::IncDecVar(0, IncDecOp::PreDec)]);
        assert!((r - 9.0).abs() < 1e-15);
        assert!((snapshot_test_vars()[0] - 9.0).abs() < 1e-15);
    }

    #[test]
    fn jit_eligible_compound_assign_field() {
        assert!(is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::PushNum(5.0),
            Op::CompoundAssignField(BinOp::Add),
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_compound_assign_field_add() {
        setup_test_fields(&[(1, 100.0)]);
        let r = exec_with_test_field(&[
            Op::PushNum(1.0),
            Op::PushNum(5.0),
            Op::CompoundAssignField(BinOp::Add),
        ]);
        assert!((r - 105.0).abs() < 1e-15);
        assert!((field_at(1) - 105.0).abs() < 1e-15);
    }

    #[test]
    fn jit_incdec_field_post_inc() {
        setup_test_fields(&[(1, 10.0)]);
        let r = exec_with_test_field(&[Op::PushNum(1.0), Op::IncDecField(IncDecOp::PostInc)]);
        assert!((r - 10.0).abs() < 1e-15);
        assert!((field_at(1) - 11.0).abs() < 1e-15);
    }

    #[test]
    fn jit_eligible_set_field() {
        assert!(is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::PushNum(42.0),
            Op::SetField,
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_set_field_numeric() {
        setup_test_fields(&[(1, 0.0)]);
        let r = exec_with_test_field(&[
            Op::PushNum(1.0),
            Op::PushNum(42.0),
            Op::SetField,
        ]);
        assert!((r - 42.0).abs() < 1e-15);
        assert!((field_at(1) - 42.0).abs() < 1e-15);
    }

    #[test]
    fn jit_array_field_add_const_straight_line() {
        let ops = [
            Op::ArrayFieldAddConst {
                arr: 0,
                field: 1,
                delta: 2.0,
            },
            Op::PushNum(0.0),
        ];
        assert!((exec(&ops)).abs() < 1e-15);
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

    // ── Print opcodes (eligibility only — side effects tested via integration) ──

    #[test]
    fn jit_eligible_print_field_stdout() {
        assert!(is_jit_eligible(&[
            Op::PrintFieldStdout(1),
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_print_field_sep_field() {
        assert!(is_jit_eligible(&[
            Op::PrintFieldSepField { f1: 1, sep: 0, f2: 2 },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_print_three_fields() {
        assert!(is_jit_eligible(&[
            Op::PrintThreeFieldsStdout { f1: 1, f2: 2, f3: 3 },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_print_record() {
        assert!(is_jit_eligible(&[
            Op::Print { argc: 0, redir: crate::bytecode::RedirKind::Stdout },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_rejects_print_with_args() {
        // Print with argc > 0 is NOT eligible (for now).
        assert!(!is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::Print { argc: 1, redir: crate::bytecode::RedirKind::Stdout },
        ]));
    }

    // ── Print codegen (compiles without crash) ────────────────────────────

    #[test]
    fn jit_print_field_compiles_and_runs() {
        // Verify the codegen doesn't crash — side-effect goes to dummy.
        let ops = [Op::PrintFieldStdout(1), Op::PushNum(0.0)];
        let r = exec(&ops);
        assert!(r.abs() < 1e-15);
    }

    #[test]
    fn jit_print_three_fields_compiles() {
        let ops = [Op::PrintThreeFieldsStdout { f1: 1, f2: 2, f3: 3 }, Op::PushNum(0.0)];
        let r = exec(&ops);
        assert!(r.abs() < 1e-15);
    }

    // ── MatchRegexp ───────────────────────────────────────────────────────

    #[test]
    fn jit_eligible_match_regexp() {
        assert!(is_jit_eligible(&[Op::MatchRegexp(0)]));
    }

    #[test]
    fn jit_match_regexp_compiles() {
        // With dummy val_dispatch, match always returns 0.0.
        let r = exec(&[Op::MatchRegexp(0)]);
        assert!(r.abs() < 1e-15);
    }

    // ── Flow signals ──────────────────────────────────────────────────────

    #[test]
    fn jit_eligible_next() {
        assert!(is_jit_eligible(&[Op::Next]));
    }

    #[test]
    fn jit_eligible_exit_default() {
        assert!(is_jit_eligible(&[Op::ExitDefault]));
    }

    #[test]
    fn jit_eligible_exit_with_code() {
        assert!(is_jit_eligible(&[Op::PushNum(1.0), Op::ExitWithCode]));
    }

    #[test]
    fn jit_next_compiles() {
        let r = exec(&[Op::Next]);
        assert!(r.abs() < 1e-15);
    }

    #[test]
    fn jit_exit_default_compiles() {
        let r = exec(&[Op::ExitDefault]);
        assert!(r.abs() < 1e-15);
    }

    #[test]
    fn jit_exit_with_code_compiles() {
        let r = exec(&[Op::PushNum(42.0), Op::ExitWithCode]);
        assert!(r.abs() < 1e-15);
    }

    // ── Array opcodes ─────────────────────────────────────────────────────

    // ── Array ops rejected (f64 keys lose string identity) ──────────────

    #[test]
    fn jit_rejects_array_get() {
        assert!(!is_jit_eligible(&[Op::PushNum(1.0), Op::GetArrayElem(0)]));
    }

    #[test]
    fn jit_rejects_array_set() {
        assert!(!is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::PushNum(42.0),
            Op::SetArrayElem(0),
        ]));
    }

    #[test]
    fn jit_rejects_in_array() {
        assert!(!is_jit_eligible(&[Op::PushNum(1.0), Op::InArray(0)]));
    }

    #[test]
    fn jit_rejects_compound_assign_index() {
        assert!(!is_jit_eligible(&[
            Op::PushNum(1.0),
            Op::PushNum(5.0),
            Op::CompoundAssignIndex(0, BinOp::Add),
        ]));
    }

    // ── Array via test val_dispatch (functional) ──────────────────────────

    thread_local! {
        static TEST_ARRAY: RefCell<Vec<(String, f64)>> = const { RefCell::new(Vec::new()) };
    }

    fn setup_test_array(data: &[(&str, f64)]) {
        TEST_ARRAY.with(|c| {
            *c.borrow_mut() = data.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        });
    }

    fn array_val(key: &str) -> Option<f64> {
        TEST_ARRAY.with(|c| {
            c.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| *v)
        })
    }

    /// Test val_dispatch that uses TEST_ARRAY.
    extern "C" fn test_val_dispatch(op: u32, a1: u32, a2: f64, a3: f64) -> f64 {
        use crate::jit::{
            JIT_VAL_ARRAY_COMPOUND_ADD, JIT_VAL_ARRAY_DELETE_ALL, JIT_VAL_ARRAY_DELETE_ELEM,
            JIT_VAL_ARRAY_GET, JIT_VAL_ARRAY_IN, JIT_VAL_ARRAY_INCDEC_POST_DEC,
            JIT_VAL_ARRAY_INCDEC_POST_INC, JIT_VAL_ARRAY_INCDEC_PRE_DEC,
            JIT_VAL_ARRAY_INCDEC_PRE_INC, JIT_VAL_ARRAY_SET, JIT_VAL_MATCH_REGEXP,
            JIT_VAL_SIGNAL_NEXT,
        };
        let _ = a1; // array index (unused — single test array)
        let key = Value::Num(a2).as_str();
        match op {
            JIT_VAL_MATCH_REGEXP => 0.0,
            JIT_VAL_SIGNAL_NEXT => 0.0,
            JIT_VAL_ARRAY_GET => TEST_ARRAY.with(|c| {
                c.borrow()
                    .iter()
                    .find(|(k, _)| k == &key)
                    .map_or(0.0, |(_, v)| *v)
            }),
            JIT_VAL_ARRAY_SET => {
                TEST_ARRAY.with(|c| {
                    let mut arr = c.borrow_mut();
                    if let Some(entry) = arr.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = a3;
                    } else {
                        arr.push((key.to_string(), a3));
                    }
                });
                a3
            }
            JIT_VAL_ARRAY_IN => TEST_ARRAY.with(|c| {
                if c.borrow().iter().any(|(k, _)| k == &key) {
                    1.0
                } else {
                    0.0
                }
            }),
            JIT_VAL_ARRAY_DELETE_ELEM => {
                TEST_ARRAY.with(|c| {
                    c.borrow_mut().retain(|(k, _)| k != &key);
                });
                0.0
            }
            JIT_VAL_ARRAY_DELETE_ALL => {
                TEST_ARRAY.with(|c| c.borrow_mut().clear());
                0.0
            }
            JIT_VAL_ARRAY_COMPOUND_ADD => {
                TEST_ARRAY.with(|c| {
                    let mut arr = c.borrow_mut();
                    let old = arr.iter().find(|(k, _)| k == &key).map_or(0.0, |(_, v)| *v);
                    let n = old + a3;
                    if let Some(entry) = arr.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = n;
                    } else {
                        arr.push((key.to_string(), n));
                    }
                    n
                })
            }
            JIT_VAL_ARRAY_INCDEC_PRE_INC => {
                TEST_ARRAY.with(|c| {
                    let mut arr = c.borrow_mut();
                    let old = arr.iter().find(|(k, _)| k == &key).map_or(0.0, |(_, v)| *v);
                    let n = old + 1.0;
                    if let Some(entry) = arr.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = n;
                    } else {
                        arr.push((key.to_string(), n));
                    }
                    n
                })
            }
            JIT_VAL_ARRAY_INCDEC_POST_INC => {
                TEST_ARRAY.with(|c| {
                    let mut arr = c.borrow_mut();
                    let old = arr.iter().find(|(k, _)| k == &key).map_or(0.0, |(_, v)| *v);
                    let n = old + 1.0;
                    if let Some(entry) = arr.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = n;
                    } else {
                        arr.push((key.to_string(), n));
                    }
                    old
                })
            }
            JIT_VAL_ARRAY_INCDEC_PRE_DEC => {
                TEST_ARRAY.with(|c| {
                    let mut arr = c.borrow_mut();
                    let old = arr.iter().find(|(k, _)| k == &key).map_or(0.0, |(_, v)| *v);
                    let n = old - 1.0;
                    if let Some(entry) = arr.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = n;
                    } else {
                        arr.push((key.to_string(), n));
                    }
                    n
                })
            }
            JIT_VAL_ARRAY_INCDEC_POST_DEC => {
                TEST_ARRAY.with(|c| {
                    let mut arr = c.borrow_mut();
                    let old = arr.iter().find(|(k, _)| k == &key).map_or(0.0, |(_, v)| *v);
                    let n = old - 1.0;
                    if let Some(entry) = arr.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = n;
                    } else {
                        arr.push((key.to_string(), n));
                    }
                    old
                })
            }
            _ => 0.0,
        }
    }

    fn exec_with_test_val(ops: &[Op]) -> f64 {
        let chunk = try_compile(ops).expect("compile failed");
        let mut slots = [0.0f64; 0];
        let mut state = JitRuntimeState::new(
            &mut slots,
            dummy_field,
            dummy_array,
            dummy_var,
            dummy_field_dispatch,
            dummy_io_dispatch,
            test_val_dispatch,
        );
        chunk.execute(&mut state)
    }

    // NOTE: Functional array tests removed — general array ops are not
    // JIT-eligible due to f64 key identity loss. The fused ArrayFieldAddConst
    // (tested via integration tests) remains the correct JIT array path.

    // ── Conditional Next ──────────────────────────────────────────────────

    #[test]
    fn jit_conditional_next() {
        // if (1) next; else fall through
        let ops = [
            Op::PushNum(1.0),
            Op::JumpIfFalsePop(3),
            Op::Next,           // signal raised — JIT returns immediately
            Op::PushNum(99.0),  // not reached
        ];
        // Compiles and runs without crash.
        let r = exec(&ops);
        assert!(r.abs() < 1e-15);
    }

    // ── Mixed: print + arithmetic in same chunk ───────────────────────────

    #[test]
    fn jit_print_then_arithmetic() {
        // `{ print $1; sum += $2 }` — fused print followed by slot math.
        extern "C" fn fields(i: i32) -> f64 {
            match i { 1 => 10.0, 2 => 20.0, _ => 0.0 }
        }
        let ops = [
            Op::PrintFieldStdout(1),              // side-effect (dummy)
            Op::AddFieldToSlot { field: 2, slot: 0 }, // sum += $2
            Op::GetSlot(0),                        // return sum
        ];
        let mut slots = [5.0];
        let chunk = try_compile(&ops).expect("compile");
        let mut state = JitRuntimeState::new(
            &mut slots,
            fields,
            dummy_array,
            dummy_var,
            dummy_field_dispatch,
            dummy_io_dispatch,
            dummy_val_dispatch,
        );
        let r = chunk.execute(&mut state);
        assert!((r - 25.0).abs() < 1e-15); // 5 + 20
    }

    // ── Return signals ────────────────────────────────────────────────────

    #[test]
    fn jit_eligible_return_val() {
        assert!(is_jit_eligible(&[Op::PushNum(42.0), Op::ReturnVal]));
    }

    #[test]
    fn jit_eligible_return_empty() {
        assert!(is_jit_eligible(&[Op::ReturnEmpty]));
    }

    #[test]
    fn jit_return_val_compiles() {
        let r = exec(&[Op::PushNum(42.0), Op::ReturnVal]);
        assert!(r.abs() < 1e-15); // returns 0.0 (signal, not the value)
    }

    #[test]
    fn jit_return_empty_compiles() {
        let r = exec(&[Op::ReturnEmpty]);
        assert!(r.abs() < 1e-15);
    }

    // ── ForIn ─────────────────────────────────────────────────────────────

    #[test]
    fn jit_eligible_forin() {
        assert!(is_jit_eligible(&[
            Op::ForInStart(0),
            Op::ForInNext { var: 1, end_jump: 4 },
            Op::IncrSlot(0),
            Op::Jump(1),
            Op::ForInEnd,
            Op::GetSlot(0),
        ]));
    }

    #[test]
    fn jit_forin_compiles() {
        // ForIn with empty array (dummy val_dispatch returns 0 for FORIN_NEXT)
        let ops = [
            Op::ForInStart(0),
            Op::ForInNext { var: 1, end_jump: 4 },
            Op::IncrSlot(0),
            Op::Jump(1),
            Op::ForInEnd,
            Op::GetSlot(0),
        ];
        let mut slots = [0.0];
        let r = exec_with_slots(&ops, &mut slots);
        // With dummy val_dispatch, FORIN_NEXT returns 0 immediately → loop never executes
        assert!(r.abs() < 1e-15);
    }

    // ── Asort / Asorti ────────────────────────────────────────────────────

    #[test]
    fn jit_eligible_asort() {
        assert!(is_jit_eligible(&[Op::Asort { src: 0, dest: None }]));
    }

    #[test]
    fn jit_eligible_asorti() {
        assert!(is_jit_eligible(&[Op::Asorti {
            src: 0,
            dest: Some(1),
        }]));
    }

    #[test]
    fn jit_asort_compiles() {
        let r = exec(&[Op::Asort { src: 0, dest: None }]);
        assert!(r.abs() < 1e-15); // dummy returns 0
    }

    #[test]
    fn jit_asorti_compiles() {
        let r = exec(&[Op::Asorti {
            src: 0,
            dest: Some(1),
        }]);
        assert!(r.abs() < 1e-15);
    }
}
