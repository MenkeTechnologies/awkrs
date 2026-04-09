#![allow(rustdoc::private_intra_doc_links)] // `Op` / `BinOp` live in non-`pub` modules; links are for maintainers.

//! Cranelift JIT compiler for AWK bytecode chunks.
//!
//! Compiles eligible bytecode `Op` sequences into native machine code.
//! The JIT handles numeric expressions, slot variables, control flow (loops,
//! conditionals, `for`-`in` iteration), field access, fused peephole opcodes,
//! print side-effects, `MatchRegexp` pattern tests, flow signals
//! (`Next`/`NextFile`/`Exit`/`Return`), fused `ArrayFieldAddConst`, and
//! `asort`/`asorti`. User-defined functions ([`Op::CallUser`]) and `sub`/`gsub`
//! ([`Op::SubFn`]/[`Op::GsubFn`]) lower to mixed `MIXED_CALL_USER_*` and
//! `MIXED_SUB_*` / `MIXED_GSUB_*` when [`is_jit_eligible`] accepts the stack shape;
//! [`jit_call_builtins_ok`] applies to [`Op::CallBuiltin`] and [`Op::CallUser`] names.
//!
//! General array subscripts and string-producing ops compile in **mixed mode**:
//! stack values may be NaN-boxed string handles, and `val_dispatch` opcodes ≥ 100
//! (`MIXED_*`) preserve string keys and coercion semantics. Fused slot peepholes
//! (`IncrSlot`, `IncDecSlot`, `AddFieldToSlot`, `JumpIfSlotGeNum`, …) in a mixed
//! chunk also use `MIXED_*` so slot values are read/written with `Value` coercion,
//! not raw `fadd` on NaN-boxed bits. `SetField` / `CompoundAssignField` in mixed
//! chunks use `MIXED_SET_FIELD` / `MIXED_COMPOUND_ASSIGN_FIELD` so RHS values are
//! NaN-box aware. Multidimensional keys (`JoinArrayKey`) use `MIXED_JOIN_KEY_ARG`
//! / `MIXED_JOIN_ARRAY_KEY` with `SUBSEP` like the VM. The fused `ArrayFieldAddConst`
//! remains a separate fast path (field index is numeric).
//!
//! Execution takes a [`JitRuntimeState`]: mutable `f64` slot storage and seven
//! `extern "C"` callbacks — `field_fn`, `array_field_add`, `var_dispatch`,
//! `field_dispatch`, `io_dispatch`, and `val_dispatch`.
//!
//! The VM tries [`try_jit_execute`] before falling back to the interpreter for
//! eligible chunks. Set **`AWKRS_JIT=0`** to force the bytecode interpreter
//! (for A/B benchmarks against JIT; default is to attempt JIT). Mixed-mode chunks (strings,
//! regex `~`, general array ops, `print`/`printf` with arguments, whitelisted
//! [`Op::CallBuiltin`], [`Op::CallUser`], [`Op::SubFn`]/[`Op::GsubFn`], etc.)
//! compile through `val_dispatch` (`MIXED_*`). Chunks that fail [`is_jit_eligible`]
//! or [`jit_call_builtins_ok`] (unsupported builtin, shadowed name, bad arity) use
//! the bytecode loop.
//! [`Op::Split`] compiles to [`MIXED_SPLIT`] / [`MIXED_SPLIT_WITH_FS`] (same split rules as the VM).
//! [`Op::Patsplit`] and [`Op::MatchBuiltin`] use additional `MIXED_*` opcodes; `patsplit` with both a
//! custom field pattern and a `seps` array packs `arr` and `seps` pool indices in `a1` (16-bit each)
//! when both are `< 65536`, otherwise [`MIXED_PATSPLIT_STASH_SEPS`] + [`MIXED_PATSPLIT_FP_SEP_WIDE`].
//! Non-stdout `print` / `printf` use [`MIXED_PRINT_FLUSH_REDIR`] / [`MIXED_PRINTF_FLUSH_REDIR`] with
//! [`pack_print_redir`] (same stack order as the VM: redirect path is TOS).
//! [`Op::GetLine`] uses [`MIXED_GETLINE_PRIMARY`] / [`MIXED_GETLINE_FILE`] / [`MIXED_GETLINE_COPROC`];
//! [`MIXED_GETLINE_INTO_RECORD`] in `a1` means read into `$0` / fields (no named variable).
//! Whitelisted [`Op::CallBuiltin`] (see [`jit_call_builtins_ok`]) uses `MIXED_BUILTIN_*`
//! including `sprintf`/`printf` and I/O helpers when arity and [`jit_call_builtins_ok`] allow.
//! The **`printf`** *statement* opcode ([`Op::Printf`] to stdout) uses `MIXED_PRINT_ARG` +
//! [`MIXED_PRINTF_FLUSH`] (same buffer as `print`, then `sprintf_simple` to the output buffer).
//! `typeof` (`TypeofVar` / `TypeofSlot` / `TypeofArrayElem` / `TypeofField` / `TypeofValue`)
//! compiles to `MIXED_TYPEOF_*` and returns NaN-boxed pool/dynamic strings like other mixed ops.
//!
//! ## Performance (implemented vs future)
//!
//! - **Chunk dispatch cache** — each [`crate::bytecode::Chunk`] holds a `jit_lock` with the first
//!   eligibility/compile result so the VM does not re-run [`is_jit_eligible`] / [`try_compile`] every
//!   record ([`crate::vm::try_jit_dispatch`]).
//! - **Thread-local compile cache** — [`try_jit_execute`] (legacy callers without a [`Chunk`]) uses a
//!   thread-local map keyed by [`ops_hash`] instead of a global mutex.
//! - **Slot buffer reuse** — [`crate::runtime::Runtime::jit_slot_buf`] is resized once and reused
//!   for marshaling instead of allocating a fresh `Vec<f64>` per JIT invocation. The VM clears
//!   `JIT_DYN_STRINGS` *before* filling that buffer in mixed mode so NaN-boxed string slots (e.g.
//!   `-v` values stored as strings) still match the dynamic-string pool during execution.
//! - **Single-block slot SSA** — for non-mixed chunks with no jumps and no early `return`/flow
//!   signals, scalar slots are held in Cranelift [`Variable`]s and flushed to `slots_ptr` once before
//!   the function return (phi-free; no backedges).
//! - **Future** — full Cranelift SSA across loop headers (phis for slot vars); keep interpreter
//!   [`crate::runtime::Value`] slots and the JIT `f64` buffer unified where semantics allow.
//!   TLS for `BEGIN`/chunks is already skipped when [`jit_chunk_needs_vm_tls`] is `false`.

use crate::ast::{BinOp, IncDecOp};
use crate::bytecode::{CompiledProgram, GetlineSource, Op, SubTarget};
use cranelift_codegen::ir::{types, AbiParam, Block, InstBuilder, MemFlags, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::Arc;

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

// ── NaN-boxing ───────────────────────────────────────────────────────────
//
// In "mixed mode" (chunks that contain string-producing ops), f64 stack
// values may be **NaN-boxed string handles**: quiet NaN with a 32-bit
// string index in the low bits.
//
// Encoding: bits[63:32] == 0x7FFC_0000 → string handle; bits[31:0] = index.
//   • Bit 47 = 0 → pool string (CompiledProgram.strings)
//   • Bit 47 = 1 → dynamic string (JIT_DYN_STRINGS thread-local)
//
// Regular f64 values (including canonical NaN 0x7FF8_0000_0000_0000) are
// never confused with string handles because their high 32 bits differ.

/// Upper 32 bits of a NaN-boxed string handle.
pub const NAN_STR_TAG_HI32: u32 = 0x7FFC_0000;
/// Full 64-bit tag with zero payload.
pub const NAN_STR_TAG: u64 = (NAN_STR_TAG_HI32 as u64) << 32;
/// Bit set in payload to mark a dynamic (non-pool) string.
pub const NAN_STR_DYN_BIT: u64 = 1 << 47;

/// Check if raw f64 bits represent a NaN-boxed string handle.
///
/// [`NAN_STR_DYN_BIT`] lives in the high 32 bits (bit 47 of the full f64), so the
/// upper half is `0x7ffc8000` for dynamic strings and `0x7ffc0000` for pool indices.
#[inline]
pub fn is_nan_str(bits: u64) -> bool {
    let hi = (bits >> 32) as u32;
    let dyn_in_hi = (NAN_STR_DYN_BIT >> 32) as u32;
    (hi & !dyn_in_hi) == NAN_STR_TAG_HI32
}

/// Create a NaN-boxed pool string handle.
#[inline]
pub fn nan_str_pool(pool_idx: u32) -> f64 {
    f64::from_bits(NAN_STR_TAG | pool_idx as u64)
}

/// Create a NaN-boxed dynamic string handle.
#[inline]
pub fn nan_str_dyn(dyn_idx: u32) -> f64 {
    f64::from_bits(NAN_STR_TAG | NAN_STR_DYN_BIT | dyn_idx as u64)
}

/// Decode a NaN-boxed string handle: `(is_dynamic, index)`.
#[inline]
pub fn decode_nan_str_bits(bits: u64) -> Option<(bool, u32)> {
    if !is_nan_str(bits) {
        return None;
    }
    let is_dyn = (bits & NAN_STR_DYN_BIT) != 0;
    let idx = (bits & 0xffff_ffff) as u32;
    Some((is_dyn, idx))
}

// ── Uninit in mixed-mode JIT slots ─────────────────────────────────────────
//
// `Value::Uninit` cannot use raw `0.0` in the slot buffer: that decodes as numeric
// zero and breaks `typeof` / string coercions. Use a dedicated quiet-NaN pattern
// (high 32 bits `0x7FFD_0000`, low 32 bits zero) that does not match
// [`is_nan_str`].

/// Upper 32 bits of the mixed-mode JIT encoding for [`crate::runtime::Value::Uninit`].
pub const NAN_UNINIT_HI32: u32 = 0x7FFD_0000;
/// Full bit pattern for [`nan_uninit`].
pub const NAN_UNINIT_TAG: u64 = (NAN_UNINIT_HI32 as u64) << 32;

#[inline]
pub fn nan_uninit() -> f64 {
    f64::from_bits(NAN_UNINIT_TAG)
}

#[inline]
pub fn is_nan_uninit(bits: u64) -> bool {
    bits == NAN_UNINIT_TAG
}

/// Encode `arr` pool index with [`BinOp`] for [`MIXED_ARRAY_COMPOUND`].
#[inline]
pub fn mixed_encode_array_compound(arr: u32, bop: BinOp) -> u32 {
    let b: u32 = match bop {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::Div => 3,
        BinOp::Mod => 4,
        _ => 0,
    };
    arr | (b << 16)
}

/// Encode `arr` with [`IncDecOp`] for [`MIXED_ARRAY_INCDEC`].
#[inline]
pub fn mixed_encode_array_incdec(arr: u32, kind: IncDecOp) -> u32 {
    let k: u32 = match kind {
        IncDecOp::PreInc => 0,
        IncDecOp::PostInc => 1,
        IncDecOp::PreDec => 2,
        IncDecOp::PostDec => 3,
    };
    arr | (k << 16)
}

// ── Mixed-mode val_dispatch opcodes (100+) ───────────────────────────────
//
// In mixed-mode chunks, arithmetic, comparison, and truthiness ops are
// dispatched through val_dispatch callbacks so NaN-boxed string handles
// are coerced / compared correctly.  The callbacks receive raw f64 values
// (which may be NaN-boxed) in a2/a3 and return f64 (possibly NaN-boxed).

pub const MIXED_ADD: u32 = 100;
pub const MIXED_SUB: u32 = 101;
pub const MIXED_MUL: u32 = 102;
pub const MIXED_DIV: u32 = 103;
pub const MIXED_MOD: u32 = 104;
pub const MIXED_NEG: u32 = 105;
pub const MIXED_POS: u32 = 106;
pub const MIXED_NOT: u32 = 107;
pub const MIXED_TO_BOOL: u32 = 108;
pub const MIXED_CMP_EQ: u32 = 110;
pub const MIXED_CMP_NE: u32 = 111;
pub const MIXED_CMP_LT: u32 = 112;
pub const MIXED_CMP_LE: u32 = 113;
pub const MIXED_CMP_GT: u32 = 114;
pub const MIXED_CMP_GE: u32 = 115;
/// Returns 1.0 if truthy, 0.0 if falsy (for JumpIfFalsePop/JumpIfTruePop).
pub const MIXED_TRUTHINESS: u32 = 116;
/// Push string constant: a1 = pool index → returns NaN-boxed handle.
pub const MIXED_PUSH_STR: u32 = 120;
/// Concat two values (NaN-boxed or number) → NaN-boxed string result.
pub const MIXED_CONCAT: u32 = 121;
/// Concat TOS with pool string: a1 = pool index → NaN-boxed result.
pub const MIXED_CONCAT_POOL: u32 = 122;
/// Get field as NaN-boxed handle (preserves string): a2 = field index f64.
pub const MIXED_GET_FIELD: u32 = 123;
/// Get variable as NaN-boxed handle: a1 = name index.
pub const MIXED_GET_VAR: u32 = 124;
/// Set variable from NaN-boxed value: a1 = name index, a2 = value.
pub const MIXED_SET_VAR: u32 = 125;
/// Get slot as NaN-boxed handle: a1 = slot index.
pub const MIXED_GET_SLOT: u32 = 126;
/// Two string values on stack: `a2` = haystack, `a3` = ERE pattern (both NaN-boxed or numeric) → 0/1.
pub const MIXED_REGEX_MATCH: u32 = 130;
/// Same as [`MIXED_REGEX_MATCH`] but negated.
pub const MIXED_REGEX_NOT_MATCH: u32 = 131;
/// Buffer one print arg (NaN-boxed): a1 = arg position, a2 = value.
pub const MIXED_PRINT_ARG: u32 = 140;
/// Flush buffered print args to stdout: a1 = argc.
pub const MIXED_PRINT_FLUSH: u32 = 141;
/// Array get with NaN-boxed key: a1 = arr pool index, a2 = key → NaN-boxed value.
pub const MIXED_ARRAY_GET: u32 = 150;
/// Array set: a1 = arr, a2 = key (NaN-boxed), a3 = value (NaN-boxed).
pub const MIXED_ARRAY_SET: u32 = 151;
/// `key in arr`: a1 = arr, a2 = key → 0/1.
pub const MIXED_ARRAY_IN: u32 = 152;
/// Delete element: a1 = arr, a2 = key.
pub const MIXED_ARRAY_DELETE_ELEM: u32 = 153;
/// Delete entire array: a1 = arr.
pub const MIXED_ARRAY_DELETE_ALL: u32 = 154;
/// `arr[key] op= rhs`: a1 = arr | (bop_code << 16), a2 = key, a3 = rhs → result.
pub const MIXED_ARRAY_COMPOUND: u32 = 155;
/// `++arr[key]` etc.: a1 = arr | (kind_code << 16), a2 = key → result.
pub const MIXED_ARRAY_INCDEC: u32 = 156;
/// `++slot` / `slot++` (expression): a1 = slot | (kind_code << 16) → pushed value; slot ← numeric.
pub const MIXED_INCDEC_SLOT: u32 = 157;
/// Statement `slot++` fused — a1 = slot index.
pub const MIXED_INCR_SLOT: u32 = 158;
/// Statement `slot--` fused.
pub const MIXED_DECR_SLOT: u32 = 159;
/// `dst += src` fused: a1 = src | (dst << 16).
pub const MIXED_ADD_SLOT_TO_SLOT: u32 = 160;
/// `slot += $field` fused: a1 = field | (slot << 16).
pub const MIXED_ADD_FIELD_TO_SLOT: u32 = 161;
/// `slot += $f1 * $f2`: a1 = f1 | (f2 << 16), a2 = slot as f64.
pub const MIXED_ADD_MUL_FIELDS_TO_SLOT: u32 = 162;
/// Numeric value of slot (for `JumpIfSlotGeNum` in mixed mode): a1 = slot.
pub const MIXED_SLOT_AS_NUMBER: u32 = 163;
/// `$idx = val` (mixed): a1 = field index as `i32` bit pattern, a2 = value (NaN-boxed).
pub const MIXED_SET_FIELD: u32 = 164;
/// `$idx op= rhs` (mixed): a1 = binop 0–4 (`Add`…`Mod`), a2 = index f64, a3 = rhs (NaN-boxed).
pub const MIXED_COMPOUND_ASSIGN_FIELD: u32 = 165;
/// Append one key component for [`Op::JoinArrayKey`] (buffered, then [`MIXED_JOIN_ARRAY_KEY`]).
pub const MIXED_JOIN_KEY_ARG: u32 = 167;
/// Join `a1` buffered components with `SUBSEP`, return NaN-boxed composite key string.
pub const MIXED_JOIN_ARRAY_KEY: u32 = 168;
/// `typeof(name)` — `a1` = pool index of identifier (see [`Op::TypeofVar`]).
pub const MIXED_TYPEOF_VAR: u32 = 169;
/// `typeof` for a slotted scalar — `a1` = slot index (see [`Op::TypeofSlot`]).
pub const MIXED_TYPEOF_SLOT: u32 = 170;
/// `typeof(arr[key])` — `a1` = array pool index; `a2` = NaN-boxed key (see [`Op::TypeofArrayElem`]).
pub const MIXED_TYPEOF_ARRAY_ELEM: u32 = 171;
/// `typeof($n)` — `a2` = field index as `f64` (see [`Op::TypeofField`]).
pub const MIXED_TYPEOF_FIELD: u32 = 172;
/// `typeof(expr)` — `a2` = NaN-boxed value (see [`Op::TypeofValue`]).
pub const MIXED_TYPEOF_VALUE: u32 = 173;
/// Buffered argument for [`Op::CallBuiltin`] (see [`MIXED_BUILTIN_CALL`]).
pub const MIXED_BUILTIN_ARG: u32 = 174;
/// Whitelisted `CallBuiltin` — `a1` = function name pool index, `a2` = argc as `f64`.
pub const MIXED_BUILTIN_CALL: u32 = 175;
/// After [`MIXED_PRINT_ARG`] × argc: format with `sprintf_simple` and write like `printf` (stdout).
/// `a1` = argc (format + arguments).
pub const MIXED_PRINTF_FLUSH: u32 = 176;
/// [`Op::Split`] without third argument — `a1` = array name pool index, `a2` = string, `a3` unused (uses `FS`).
pub const MIXED_SPLIT: u32 = 177;
/// [`Op::Split`] with explicit FS — `a1` = array name pool index, `a2` = string, `a3` = FS.
pub const MIXED_SPLIT_WITH_FS: u32 = 178;
/// [`Op::Patsplit`] — `FPAT` from runtime; no `seps` array.
pub const MIXED_PATSPLIT: u32 = 179;
/// [`Op::Patsplit`] without field pattern; `a3` = NaN-boxed pool string for the `seps` array name.
pub const MIXED_PATSPLIT_SEP: u32 = 180;
/// [`Op::Patsplit`] with field pattern on stack; no `seps`.
pub const MIXED_PATSPLIT_FP: u32 = 181;
/// [`Op::Patsplit`] with field pattern and `seps` — low 16 bits of `a1` = `arr`, high 16 = `seps` pool index (both &lt; 65536).
pub const MIXED_PATSPLIT_FP_SEP: u32 = 182;
/// Stash `seps` string-pool index for [`MIXED_PATSPLIT_FP_SEP_WIDE`]; `a1` = index. Emitted immediately before WIDE.
pub const MIXED_PATSPLIT_STASH_SEPS: u32 = 204;
/// Same as [`MIXED_PATSPLIT_FP_SEP`] when `arr` or `seps` index does not fit in 16 bits — uses stash + full `a1` for `arr`.
pub const MIXED_PATSPLIT_FP_SEP_WIDE: u32 = 205;
/// [`Op::MatchBuiltin`] without capture array — `a1` = 0, `a2` = s, `a3` = regex pattern.
pub const MIXED_MATCH_BUILTIN: u32 = 183;
/// [`Op::MatchBuiltin`] with capture array — `a1` = array name pool index, `a2` = s, `a3` = pattern.
pub const MIXED_MATCH_BUILTIN_ARR: u32 = 184;
/// `print` with non-stdout redirect — `a1` = [`pack_print_redir`], `a2` = NaN-boxed path string.
pub const MIXED_PRINT_FLUSH_REDIR: u32 = 185;
/// `printf` with redirect — same packing as [`MIXED_PRINT_FLUSH_REDIR`].
pub const MIXED_PRINTF_FLUSH_REDIR: u32 = 186;
/// `getline` from primary input — `a1` = var pool index or [`MIXED_GETLINE_INTO_RECORD`].
pub const MIXED_GETLINE_PRIMARY: u32 = 187;
/// `getline` from a file — `a1` as above, `a2` = path (NaN-boxed string).
pub const MIXED_GETLINE_FILE: u32 = 188;
/// `getline` from coproc — same as [`MIXED_GETLINE_FILE`].
pub const MIXED_GETLINE_COPROC: u32 = 189;
/// Sentinel for `a1`: `getline` with no variable (updates `$0` / `NF`).
pub const MIXED_GETLINE_INTO_RECORD: u32 = u32::MAX;

/// Buffered arg for [`Op::CallUser`] (then [`MIXED_CALL_USER_CALL`]).
pub const MIXED_CALL_USER_ARG: u32 = 190;
/// `a1` = function name pool index, `a2` = argc as `f64`.
pub const MIXED_CALL_USER_CALL: u32 = 191;
pub const MIXED_SUB_RECORD: u32 = 192;
pub const MIXED_GSUB_RECORD: u32 = 193;
pub const MIXED_SUB_VAR: u32 = 194;
pub const MIXED_GSUB_VAR: u32 = 195;
pub const MIXED_SUB_SLOT: u32 = 196;
pub const MIXED_GSUB_SLOT: u32 = 197;
pub const MIXED_SUB_FIELD: u32 = 198;
pub const MIXED_GSUB_FIELD: u32 = 199;
/// Stash array key (NaN-boxed) before [`MIXED_SUB_INDEX`] / [`MIXED_GSUB_INDEX`].
pub const MIXED_SUB_INDEX_STASH: u32 = 200;
pub const MIXED_SUB_INDEX: u32 = 201;
pub const MIXED_GSUB_INDEX_STASH: u32 = 202;
pub const MIXED_GSUB_INDEX: u32 = 203;

/// Pack `argc` (low 16 bits) and redirect kind (high 16 bits: 1=overwrite, 2=append, 3=pipe, 4=coproc).
#[inline]
pub fn pack_print_redir(argc: u16, redir: crate::bytecode::RedirKind) -> u32 {
    let rk: u32 = match redir {
        crate::bytecode::RedirKind::Stdout => 0,
        crate::bytecode::RedirKind::Overwrite => 1,
        crate::bytecode::RedirKind::Append => 2,
        crate::bytecode::RedirKind::Pipe => 3,
        crate::bytecode::RedirKind::Coproc => 4,
    };
    u32::from(argc) | (rk << 16)
}

#[inline]
fn mixed_encode_field_compound_binop(bop: BinOp) -> u32 {
    match bop {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::Div => 3,
        BinOp::Mod => 4,
        _ => 0,
    }
}

#[inline]
pub fn mixed_encode_slot_incdec(slot: u16, kind: IncDecOp) -> u32 {
    let k: u32 = match kind {
        IncDecOp::PreInc => 0,
        IncDecOp::PostInc => 1,
        IncDecOp::PreDec => 2,
        IncDecOp::PostDec => 3,
    };
    u32::from(slot) | (k << 16)
}

#[inline]
pub fn mixed_encode_slot_pair(src: u16, dst: u16) -> u32 {
    u32::from(src) | (u32::from(dst) << 16)
}

#[inline]
pub fn mixed_encode_field_slot(field: u16, slot: u16) -> u32 {
    u32::from(field) | (u32::from(slot) << 16)
}

#[inline]
fn mixed_op_for_binop(bop: BinOp) -> u32 {
    match bop {
        BinOp::Add => MIXED_ADD,
        BinOp::Sub => MIXED_SUB,
        BinOp::Mul => MIXED_MUL,
        BinOp::Div => MIXED_DIV,
        BinOp::Mod => MIXED_MOD,
        _ => unreachable!("filtered by is_jit_eligible"),
    }
}

#[inline]
fn mixed_op_for_cmp(op: &Op) -> u32 {
    match op {
        Op::CmpEq => MIXED_CMP_EQ,
        Op::CmpNe => MIXED_CMP_NE,
        Op::CmpLt => MIXED_CMP_LT,
        Op::CmpLe => MIXED_CMP_LE,
        Op::CmpGt => MIXED_CMP_GT,
        Op::CmpGe => MIXED_CMP_GE,
        _ => unreachable!(),
    }
}

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

// ── JIT compile cache (thread-local, keyed by ops hash) ─────────────────────

thread_local! {
    /// `None` means compile failed last time for this hash.
    static JIT_COMPILE_CACHE: RefCell<HashMap<u64, Option<Arc<JitChunk>>>> =
        RefCell::new(HashMap::new());
}

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
/// Returns `true` if any op in the chunk requires mixed-mode (NaN-boxed) codegen
/// (including [`Op::GetVar`] and [`Op::SetField`], so locals and field assignment match `Value` semantics).
pub fn needs_mixed_mode(ops: &[Op]) -> bool {
    ops.iter().any(|op| {
        matches!(
            op,
            Op::PushStr(_)
                | Op::Concat
                | Op::ConcatPoolStr(_)
                | Op::RegexMatch
                | Op::RegexNotMatch
                | Op::GetArrayElem(_)
                | Op::SetArrayElem(_)
                | Op::InArray(_)
                | Op::DeleteElem(_)
                | Op::DeleteArray(_)
                | Op::CompoundAssignIndex(_, _)
                | Op::IncDecIndex(_, _)
                | Op::JoinArrayKey(_)
                | Op::TypeofVar(_)
                | Op::TypeofSlot(_)
                | Op::TypeofArrayElem(_)
                | Op::TypeofField
                | Op::TypeofValue
                | Op::CallBuiltin(_, _)
                | Op::Split { .. }
                | Op::Patsplit { .. }
                | Op::MatchBuiltin { .. }
        ) || matches!(
            op,
            Op::Print {
                argc,
                redir: crate::bytecode::RedirKind::Stdout,
            } if *argc > 0
        ) || matches!(
            op,
            Op::Printf {
                argc,
                redir: crate::bytecode::RedirKind::Stdout,
            } if *argc > 0
        ) || matches!(
            op,
            Op::Print { redir, .. } if *redir != crate::bytecode::RedirKind::Stdout
        ) || matches!(
            op,
            Op::Printf { redir, .. } if *redir != crate::bytecode::RedirKind::Stdout
        ) || matches!(op, Op::GetLine { .. })
            || matches!(op, Op::CallUser(_, _))
            || matches!(op, Op::SubFn(_) | Op::GsubFn(_))
            // `$n = expr` must use mixed codegen so the RHS can be NaN-boxed strings.
            || matches!(op, Op::SetField)
            // Locals/globals can hold strings; non-mixed JIT would stack plain f64 and break
            // arithmetic (`x*2` when x is a numeric string) and returns (`return x`).
            || matches!(op, Op::GetVar(_))
    })
}

/// Returns `true` when every [`Op::CallBuiltin`] is JIT-supported (not shadowed by a user function)
/// and every [`Op::CallUser`] names a defined user function with a supported arity.
pub fn jit_call_builtins_ok(ops: &[Op], cp: &CompiledProgram) -> bool {
    const MAX_CALL_ARGS: u16 = 64;
    for op in ops {
        match op {
            Op::CallBuiltin(name_idx, argc) => {
                let name = cp.strings.get(*name_idx);
                if cp.functions.contains_key(name) {
                    return false;
                }
                if !builtin_supported_for_jit(name, *argc) {
                    return false;
                }
            }
            Op::CallUser(name_idx, argc) => {
                if *argc > MAX_CALL_ARGS {
                    return false;
                }
                let name = cp.strings.get(*name_idx);
                if !cp.functions.contains_key(name) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

fn builtin_supported_for_jit(name: &str, argc: u16) -> bool {
    // Cap so pathological bytecode cannot pass huge arg counts through the JIT buffer.
    const MAX_CALL_ARGS: u16 = 64;
    match name {
        // `length(expr)` must read full `Value` (arrays, strings); JIT only passes f64.
        "length" => argc == 0,
        "index" => argc == 2,
        "substr" => argc == 2 || argc == 3,
        "tolower" | "toupper" => argc == 1,
        "int" | "sqrt" | "strtonum" => argc == 1,
        "sin" | "cos" | "exp" | "log" | "compl" => argc == 1,
        "atan2" => argc == 2,
        "and" | "or" | "xor" | "lshift" | "rshift" => argc == 2,
        "systime" => argc == 0,
        "mktime" => argc == 1,
        "rand" => argc == 0,
        "srand" => argc <= 1,
        // Formatting / I/O (same paths as `exec_builtin_dispatch`)
        "sprintf" => (1..=MAX_CALL_ARGS).contains(&argc),
        "printf" => (1..=MAX_CALL_ARGS).contains(&argc),
        "strftime" => argc <= 3,
        "fflush" => argc <= 1,
        "close" => argc == 1,
        "system" => argc == 1,
        "typeof" => argc == 1,
        _ => false,
    }
}

fn empty_compiled_program() -> CompiledProgram {
    use crate::bytecode::StringPool;
    CompiledProgram {
        begin_chunks: vec![],
        end_chunks: vec![],
        beginfile_chunks: vec![],
        endfile_chunks: vec![],
        record_rules: vec![],
        functions: HashMap::new(),
        strings: StringPool::default(),
        slot_count: 0,
        slot_names: vec![],
        slot_map: HashMap::new(),
    }
}

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

            // ── String ops (mixed mode — NaN-boxed) ─────────────────
            Op::PushStr(_) => depth += 1,
            Op::Concat => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::ConcatPoolStr(_) => {
                if depth < 1 {
                    return false;
                }
            }
            Op::RegexMatch | Op::RegexNotMatch => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }

            // ── General array ops (mixed mode — NaN-boxed keys) ───────
            Op::GetArrayElem(_) => {
                if depth < 1 {
                    return false;
                }
            }
            Op::SetArrayElem(_) => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::InArray(_) => {
                if depth < 1 {
                    return false;
                }
            }
            Op::DeleteElem(_) => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            Op::DeleteArray(_) => {}
            Op::CompoundAssignIndex(_, bop) => {
                if depth < 2 {
                    return false;
                }
                match bop {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {}
                    _ => return false,
                }
                depth -= 1;
            }
            Op::IncDecIndex(_, _) => {
                if depth < 1 {
                    return false;
                }
            }

            // Multi-index key: pop `n`, push one composite string (NaN-boxed).
            Op::JoinArrayKey(n) => {
                let n = *n as i32;
                if depth < n {
                    return false;
                }
                depth -= n - 1;
            }

            // ── typeof (push string — mixed mode) ─────────────────────
            Op::TypeofVar(_) | Op::TypeofSlot(_) => {
                depth += 1;
            }
            Op::TypeofArrayElem(_) => {
                if depth < 1 {
                    return false;
                }
            }
            Op::TypeofField | Op::TypeofValue => {
                if depth < 1 {
                    return false;
                }
            }

            // ── Print with args (mixed mode — NaN-boxed values) ───────
            Op::Print {
                argc,
                redir: crate::bytecode::RedirKind::Stdout,
            } if *argc > 0 => {
                let n = *argc as i32;
                if depth < n {
                    return false;
                }
                depth -= n;
            }
            Op::Printf {
                argc,
                redir: crate::bytecode::RedirKind::Stdout,
            } if *argc > 0 => {
                let n = *argc as i32;
                if depth < n {
                    return false;
                }
                depth -= n;
            }
            Op::Print { argc, redir } if *redir != crate::bytecode::RedirKind::Stdout => {
                let n = *argc as i32;
                if *argc == 0 {
                    if depth < 1 {
                        return false;
                    }
                    depth -= 1;
                } else {
                    if depth < n + 1 {
                        return false;
                    }
                    depth -= n + 1;
                }
            }
            Op::Printf { argc, redir } if *redir != crate::bytecode::RedirKind::Stdout => {
                if *argc == 0 {
                    return false;
                }
                let n = *argc as i32;
                if depth < n + 1 {
                    return false;
                }
                depth -= n + 1;
            }

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

            // ── Whitelisted builtins (pop argc, push result) ───────────
            Op::CallBuiltin(_, argc) => {
                let n = *argc as i32;
                if depth < n {
                    return false;
                }
                depth -= n - 1;
            }

            // `split(s, a [, fs])` — pop string [, fs], push count
            Op::Split { has_fs, .. } => {
                if *has_fs {
                    if depth < 2 {
                        return false;
                    }
                    depth -= 1;
                } else if depth < 1 {
                    return false;
                }
            }

            // `patsplit(s, a [, fp [, seps]])` — pop fp then s if has_fp; push count
            Op::Patsplit { has_fp, .. } => {
                if *has_fp {
                    if depth < 2 {
                        return false;
                    }
                    depth -= 1;
                } else if depth < 1 {
                    return false;
                }
            }

            // `match(s, re [, arr])` — pop re, pop s; push RSTART
            Op::MatchBuiltin { .. } => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }

            Op::GetLine { source, .. } => match source {
                GetlineSource::Primary => {}
                GetlineSource::File | GetlineSource::Coproc => {
                    if depth < 1 {
                        return false;
                    }
                    depth -= 1;
                }
            },

            Op::CallUser(_, argc) => {
                let n = *argc as i32;
                if depth < n {
                    return false;
                }
                depth -= n - 1;
            }

            Op::SubFn(t) | Op::GsubFn(t) => {
                let need = match t {
                    SubTarget::Record | SubTarget::Var(_) | SubTarget::SlotVar(_) => 2,
                    SubTarget::Field | SubTarget::Index(_) => 3,
                };
                if depth < need {
                    return false;
                }
                depth -= need - 1;
            }

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

/// True when bytecode may emit a `return` before the implicit function return (slot SSA must flush first).
fn ops_have_early_return_or_signal(ops: &[Op]) -> bool {
    ops.iter().any(|op| {
        matches!(
            op,
            Op::ReturnVal
                | Op::ReturnEmpty
                | Op::ExitWithCode
                | Op::ExitDefault
                | Op::Next
                | Op::NextFile
        )
    })
}

/// When `false`, the JIT never calls Rust callbacks that read thread-local `VmCtx` / `Runtime`
/// pointers (`jit_io_dispatch`, `jit_var_dispatch`, mixed `val_dispatch`, etc.) — [`try_jit_dispatch`]
/// may skip installing those TLS slots.
pub(crate) fn jit_chunk_needs_vm_tls(ops: &[Op]) -> bool {
    if needs_mixed_mode(ops) {
        return true;
    }
    for op in ops {
        match op {
            Op::PushNum(_) => {}
            Op::GetSlot(_) | Op::SetSlot(_) => {}
            Op::CompoundAssignSlot(_, _) | Op::IncDecSlot(_, _) => {}
            Op::IncrSlot(_) | Op::DecrSlot(_) | Op::AddSlotToSlot { .. } => {}
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => {}
            Op::CmpEq | Op::CmpNe | Op::CmpLt | Op::CmpLe | Op::CmpGt | Op::CmpGe => {}
            Op::Neg | Op::Pos | Op::Not | Op::ToBool => {}
            Op::Dup | Op::Pop => {}
            _ => return true,
        }
    }
    false
}

fn emit_slot_ssa_flush_to_mem(
    builder: &mut FunctionBuilder,
    use_slot_ssa: bool,
    slot_vars: &[Variable],
    slots_ptr: cranelift_codegen::ir::Value,
) {
    if !use_slot_ssa {
        return;
    }
    for (i, &slot_var) in slot_vars.iter().enumerate() {
        let v = builder.use_var(slot_var);
        builder
            .ins()
            .store(MemFlags::trusted(), v, slots_ptr, (i as i32) * 8);
    }
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
pub fn try_compile(ops: &[Op], cp: &CompiledProgram) -> Option<JitChunk> {
    if !is_jit_eligible(ops) || !jit_call_builtins_ok(ops, cp) {
        return None;
    }

    let mixed = needs_mixed_mode(ops);
    use crate::jit::{
        mixed_encode_array_compound, mixed_encode_array_incdec, mixed_encode_field_slot,
        mixed_encode_slot_incdec, mixed_encode_slot_pair, pack_print_redir, MIXED_ADD,
        MIXED_ADD_FIELD_TO_SLOT, MIXED_ADD_MUL_FIELDS_TO_SLOT, MIXED_ADD_SLOT_TO_SLOT,
        MIXED_ARRAY_COMPOUND, MIXED_ARRAY_DELETE_ALL, MIXED_ARRAY_DELETE_ELEM, MIXED_ARRAY_GET,
        MIXED_ARRAY_IN, MIXED_ARRAY_INCDEC, MIXED_ARRAY_SET, MIXED_BUILTIN_ARG, MIXED_BUILTIN_CALL,
        MIXED_CALL_USER_ARG, MIXED_CALL_USER_CALL, MIXED_COMPOUND_ASSIGN_FIELD, MIXED_CONCAT,
        MIXED_CONCAT_POOL, MIXED_DECR_SLOT, MIXED_DIV, MIXED_GETLINE_COPROC, MIXED_GETLINE_FILE,
        MIXED_GETLINE_INTO_RECORD, MIXED_GETLINE_PRIMARY, MIXED_GET_FIELD, MIXED_GET_SLOT,
        MIXED_GET_VAR, MIXED_GSUB_FIELD, MIXED_GSUB_INDEX, MIXED_GSUB_INDEX_STASH,
        MIXED_GSUB_RECORD, MIXED_GSUB_SLOT, MIXED_GSUB_VAR, MIXED_INCDEC_SLOT, MIXED_INCR_SLOT,
        MIXED_JOIN_ARRAY_KEY, MIXED_JOIN_KEY_ARG, MIXED_MATCH_BUILTIN, MIXED_MATCH_BUILTIN_ARR,
        MIXED_MOD, MIXED_MUL, MIXED_NEG, MIXED_NOT, MIXED_PATSPLIT, MIXED_PATSPLIT_FP,
        MIXED_PATSPLIT_FP_SEP, MIXED_PATSPLIT_FP_SEP_WIDE, MIXED_PATSPLIT_SEP,
        MIXED_PATSPLIT_STASH_SEPS, MIXED_POS, MIXED_PRINTF_FLUSH, MIXED_PRINTF_FLUSH_REDIR,
        MIXED_PRINT_ARG, MIXED_PRINT_FLUSH, MIXED_PRINT_FLUSH_REDIR, MIXED_PUSH_STR,
        MIXED_REGEX_MATCH, MIXED_REGEX_NOT_MATCH, MIXED_SET_FIELD, MIXED_SET_VAR,
        MIXED_SLOT_AS_NUMBER, MIXED_SPLIT, MIXED_SPLIT_WITH_FS, MIXED_SUB, MIXED_SUB_FIELD,
        MIXED_SUB_INDEX, MIXED_SUB_INDEX_STASH, MIXED_SUB_RECORD, MIXED_SUB_SLOT, MIXED_SUB_VAR,
        MIXED_TO_BOOL, MIXED_TRUTHINESS, MIXED_TYPEOF_ARRAY_ELEM, MIXED_TYPEOF_FIELD,
        MIXED_TYPEOF_SLOT, MIXED_TYPEOF_VALUE, MIXED_TYPEOF_VAR,
    };

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

    let slot_count: usize = if ops.iter().any(|op| {
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
        (max_slot(ops) + 1) as usize
    } else {
        0
    };

    let has_fields = needs_field_callback(ops);
    let jump_targets = collect_jump_targets(ops);
    let use_slot_ssa = !mixed
        && slot_count > 0
        && jump_targets.is_empty()
        && !ops_have_early_return_or_signal(ops);

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

        // Jump targets computed before this block (also used for slot SSA eligibility).
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

        // Single-block non-mixed chunks: keep each scalar slot in Cranelift `Variable` SSA form
        // and flush to `slots_ptr` once before return (no control-flow merges — phi-free).
        let slot_vars: Vec<Variable> = if use_slot_ssa {
            (0..slot_count)
                .map(|_| builder.declare_var(types::F64))
                .collect()
        } else {
            Vec::new()
        };
        if use_slot_ssa {
            let slots_ptr_init = builder.use_var(var_slots_ptr);
            for (i, &slot_var) in slot_vars.iter().enumerate() {
                let v = builder.ins().load(
                    types::F64,
                    MemFlags::trusted(),
                    slots_ptr_init,
                    (i as i32) * 8,
                );
                builder.def_var(slot_var, v);
            }
        }

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
                Op::PushStr(idx) => {
                    let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_PUSH_STR));
                    let a1 = builder.ins().iconst(types::I32, i64::from(idx));
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::Concat => {
                    let b = stack.pop().expect("Concat");
                    let a = stack.pop().expect("Concat");
                    let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_CONCAT));
                    let z = builder.ins().iconst(types::I32, 0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::ConcatPoolStr(idx) => {
                    let a = stack.pop().expect("ConcatPoolStr");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_CONCAT_POOL));
                    let a1 = builder.ins().iconst(types::I32, i64::from(idx));
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, a, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::RegexMatch => {
                    let pat = stack.pop().expect("RegexMatch pat");
                    let s = stack.pop().expect("RegexMatch s");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_REGEX_MATCH));
                    let z = builder.ins().iconst(types::I32, 0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, s, pat]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::RegexNotMatch => {
                    let pat = stack.pop().expect("RegexNotMatch pat");
                    let s = stack.pop().expect("RegexNotMatch s");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_REGEX_NOT_MATCH));
                    let z = builder.ins().iconst(types::I32, 0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, s, pat]);
                    stack.push(builder.inst_results(call)[0]);
                }

                // ── typeof (push string — mixed `val_dispatch`) ───────
                Op::TypeofVar(idx) => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_TYPEOF_VAR));
                    let a1 = builder.ins().iconst(types::I32, i64::from(idx));
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::TypeofSlot(slot) => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_TYPEOF_SLOT));
                    let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::TypeofArrayElem(arr) => {
                    let key = stack.pop().expect("TypeofArrayElem");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_TYPEOF_ARRAY_ELEM));
                    let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::TypeofField => {
                    let idx = stack.pop().expect("TypeofField");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_TYPEOF_FIELD));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, idx, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::TypeofValue => {
                    let v = stack.pop().expect("TypeofValue");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_TYPEOF_VALUE));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, v, z]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::Split { arr, has_fs } => {
                    let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                    let zf = builder.ins().f64const(0.0);
                    if has_fs {
                        let fs = stack.pop().expect("Split fs");
                        let s = stack.pop().expect("Split s");
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_SPLIT_WITH_FS));
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, s, fs]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let s = stack.pop().expect("Split s");
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SPLIT));
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, s, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::GetLine { var, source } => {
                    let a1_enc = if let Some(v) = var {
                        builder.ins().iconst(types::I32, i64::from(v))
                    } else {
                        builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_GETLINE_INTO_RECORD))
                    };
                    let zf = builder.ins().f64const(0.0);
                    match source {
                        GetlineSource::Primary => {
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GETLINE_PRIMARY));
                            builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1_enc, zf, zf],
                            );
                        }
                        GetlineSource::File => {
                            let path = stack.pop().expect("GetLine file path");
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GETLINE_FILE));
                            builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1_enc, path, zf],
                            );
                        }
                        GetlineSource::Coproc => {
                            let path = stack.pop().expect("GetLine coproc");
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GETLINE_COPROC));
                            builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1_enc, path, zf],
                            );
                        }
                    }
                }
                Op::Patsplit { arr, has_fp, seps } => {
                    let zf = builder.ins().f64const(0.0);
                    match (has_fp, seps) {
                        (false, None) => {
                            let s = stack.pop().expect("Patsplit s");
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_PATSPLIT));
                            let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, s, zf],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        (false, Some(sepi)) => {
                            let s = stack.pop().expect("Patsplit s");
                            let op_ps = builder.ins().iconst(types::I32, i64::from(MIXED_PUSH_STR));
                            let a1s = builder.ins().iconst(types::I32, i64::from(sepi));
                            let seps_box = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_ps, a1s, zf, zf],
                            );
                            let seps_val = builder.inst_results(seps_box)[0];
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_PATSPLIT_SEP));
                            let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, s, seps_val],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        (true, None) => {
                            let fp = stack.pop().expect("Patsplit fp");
                            let s = stack.pop().expect("Patsplit s");
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_PATSPLIT_FP));
                            let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, s, fp],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        (true, Some(sepi)) => {
                            let fp = stack.pop().expect("Patsplit fp");
                            let s = stack.pop().expect("Patsplit s");
                            if arr < 65536 && sepi < 65536 {
                                let packed = arr | (sepi << 16);
                                let op_c = builder
                                    .ins()
                                    .iconst(types::I32, i64::from(MIXED_PATSPLIT_FP_SEP));
                                let a1 = builder.ins().iconst(types::I32, i64::from(packed));
                                let call = builder.ins().call_indirect(
                                    val_sig_ir,
                                    val_fn_ptr,
                                    &[op_c, a1, s, fp],
                                );
                                stack.push(builder.inst_results(call)[0]);
                            } else {
                                let op_stash = builder
                                    .ins()
                                    .iconst(types::I32, i64::from(MIXED_PATSPLIT_STASH_SEPS));
                                let a1_seps = builder.ins().iconst(types::I32, i64::from(sepi));
                                builder.ins().call_indirect(
                                    val_sig_ir,
                                    val_fn_ptr,
                                    &[op_stash, a1_seps, zf, zf],
                                );
                                let op_c = builder
                                    .ins()
                                    .iconst(types::I32, i64::from(MIXED_PATSPLIT_FP_SEP_WIDE));
                                let a1_arr = builder.ins().iconst(types::I32, i64::from(arr));
                                let call = builder.ins().call_indirect(
                                    val_sig_ir,
                                    val_fn_ptr,
                                    &[op_c, a1_arr, s, fp],
                                );
                                stack.push(builder.inst_results(call)[0]);
                            }
                        }
                    }
                }
                Op::MatchBuiltin { arr } => {
                    let re = stack.pop().expect("MatchBuiltin re");
                    let s = stack.pop().expect("MatchBuiltin s");
                    let z = builder.ins().iconst(types::I32, 0);
                    let (op_c, a1) = if let Some(ai) = arr {
                        (
                            builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_MATCH_BUILTIN_ARR)),
                            builder.ins().iconst(types::I32, i64::from(ai)),
                        )
                    } else {
                        (
                            builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_MATCH_BUILTIN)),
                            z,
                        )
                    };
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, s, re]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::CallBuiltin(name_idx, argc_u) => {
                    let argc = argc_u as usize;
                    let mut arg_vals: Vec<_> = (0..argc)
                        .map(|_| stack.pop().expect("CallBuiltin"))
                        .collect();
                    arg_vals.reverse();
                    let zf = builder.ins().f64const(0.0);
                    for (i, v) in arg_vals.iter().enumerate() {
                        let op_arg = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_BUILTIN_ARG));
                        let a1 = builder.ins().iconst(types::I32, i64::from(i as u32));
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_arg, a1, *v, zf]);
                    }
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_BUILTIN_CALL));
                    let a1 = builder.ins().iconst(types::I32, i64::from(name_idx));
                    let a2 = builder.ins().f64const(argc as f64);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, a2, zf]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::CallUser(name_idx, argc_u) => {
                    let argc = argc_u as usize;
                    let mut arg_vals: Vec<_> =
                        (0..argc).map(|_| stack.pop().expect("CallUser")).collect();
                    arg_vals.reverse();
                    let zf = builder.ins().f64const(0.0);
                    for (i, v) in arg_vals.iter().enumerate() {
                        let op_arg = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_CALL_USER_ARG));
                        let a1 = builder.ins().iconst(types::I32, i64::from(i as u32));
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_arg, a1, *v, zf]);
                    }
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_CALL_USER_CALL));
                    let a1 = builder.ins().iconst(types::I32, i64::from(name_idx));
                    let a2 = builder.ins().f64const(argc as f64);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, a2, zf]);
                    stack.push(builder.inst_results(call)[0]);
                }
                Op::SubFn(target) => {
                    let z = builder.ins().iconst(types::I32, 0);
                    let zf = builder.ins().f64const(0.0);
                    match target {
                        SubTarget::Record => {
                            let repl = stack.pop().expect("SubFn repl");
                            let re = stack.pop().expect("SubFn re");
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_SUB_RECORD));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, z, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::Var(name_idx) => {
                            let repl = stack.pop().expect("SubFn repl");
                            let re = stack.pop().expect("SubFn re");
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SUB_VAR));
                            let a1 = builder.ins().iconst(types::I32, i64::from(name_idx));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::SlotVar(slot) => {
                            let repl = stack.pop().expect("SubFn repl");
                            let re = stack.pop().expect("SubFn re");
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SUB_SLOT));
                            let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::Field => {
                            let fi = stack.pop().expect("SubFn field");
                            let repl = stack.pop().expect("SubFn repl");
                            let re = stack.pop().expect("SubFn re");
                            let fi_i = builder.ins().fcvt_to_sint(types::I32, fi);
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SUB_FIELD));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, fi_i, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::Index(arr_idx) => {
                            let key = stack.pop().expect("SubFn key");
                            let repl = stack.pop().expect("SubFn repl");
                            let re = stack.pop().expect("SubFn re");
                            let st_op = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_SUB_INDEX_STASH));
                            builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[st_op, z, key, zf],
                            );
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SUB_INDEX));
                            let a1 = builder.ins().iconst(types::I32, i64::from(arr_idx));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                    }
                }
                Op::GsubFn(target) => {
                    let z = builder.ins().iconst(types::I32, 0);
                    let zf = builder.ins().f64const(0.0);
                    match target {
                        SubTarget::Record => {
                            let repl = stack.pop().expect("GsubFn repl");
                            let re = stack.pop().expect("GsubFn re");
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GSUB_RECORD));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, z, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::Var(name_idx) => {
                            let repl = stack.pop().expect("GsubFn repl");
                            let re = stack.pop().expect("GsubFn re");
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_GSUB_VAR));
                            let a1 = builder.ins().iconst(types::I32, i64::from(name_idx));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::SlotVar(slot) => {
                            let repl = stack.pop().expect("GsubFn repl");
                            let re = stack.pop().expect("GsubFn re");
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_GSUB_SLOT));
                            let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::Field => {
                            let fi = stack.pop().expect("GsubFn field");
                            let repl = stack.pop().expect("GsubFn repl");
                            let re = stack.pop().expect("GsubFn re");
                            let fi_i = builder.ins().fcvt_to_sint(types::I32, fi);
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GSUB_FIELD));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, fi_i, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                        SubTarget::Index(arr_idx) => {
                            let key = stack.pop().expect("GsubFn key");
                            let repl = stack.pop().expect("GsubFn repl");
                            let re = stack.pop().expect("GsubFn re");
                            let st_op = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GSUB_INDEX_STASH));
                            builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[st_op, z, key, zf],
                            );
                            let op_c = builder
                                .ins()
                                .iconst(types::I32, i64::from(MIXED_GSUB_INDEX));
                            let a1 = builder.ins().iconst(types::I32, i64::from(arr_idx));
                            let call = builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, re, repl],
                            );
                            stack.push(builder.inst_results(call)[0]);
                        }
                    }
                }

                // ── Slot access ────────────────────────────────────────
                Op::GetSlot(slot) => {
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_GET_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                        let z = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                        stack.push(builder.inst_results(call)[0]);
                    } else if use_slot_ssa {
                        stack.push(builder.use_var(slot_vars[slot as usize]));
                    } else {
                        let offset = (slot as i32) * 8;
                        let v =
                            builder
                                .ins()
                                .load(types::F64, MemFlags::trusted(), slots_ptr, offset);
                        stack.push(v);
                    }
                }
                Op::SetSlot(slot) => {
                    let v = *stack.last().expect("SetSlot: empty stack");
                    if use_slot_ssa {
                        builder.def_var(slot_vars[slot as usize], v);
                    } else {
                        let offset = (slot as i32) * 8;
                        builder
                            .ins()
                            .store(MemFlags::trusted(), v, slots_ptr, offset);
                    }
                }

                Op::GetVar(idx) => {
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_GET_VAR));
                        let a1 = builder.ins().iconst(types::I32, i64::from(idx));
                        let z = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let opv = builder.ins().iconst(types::I32, i64::from(JIT_VAR_OP_GET));
                        let ni = builder.ins().iconst(types::I32, idx as i64);
                        let z = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, z]);
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::SetVar(idx) => {
                    let v = *stack.last().expect("SetVar: empty stack");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SET_VAR));
                        let a1 = builder.ins().iconst(types::I32, i64::from(idx));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, v, z]);
                    } else {
                        let opv = builder.ins().iconst(types::I32, i64::from(JIT_VAR_OP_SET));
                        let ni = builder.ins().iconst(types::I32, idx as i64);
                        builder
                            .ins()
                            .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, v]);
                    }
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
                    let ni = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    let new_val = if mixed {
                        let op_get = builder.ins().iconst(types::I32, i64::from(MIXED_GET_VAR));
                        let call_old = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_get, ni, z, z],
                        );
                        let old = builder.inst_results(call_old)[0];
                        let mop = mixed_op_for_binop(bop);
                        let op_b = builder.ins().iconst(types::I32, i64::from(mop));
                        let z32 = builder.ins().iconst(types::I32, 0);
                        let call_op = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_b, z32, old, rhs],
                        );
                        let computed = builder.inst_results(call_op)[0];
                        let op_set = builder.ins().iconst(types::I32, i64::from(MIXED_SET_VAR));
                        builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_set, ni, computed, z],
                        );
                        computed
                    } else {
                        let cop = jit_var_op_for_compound(bop);
                        let opv = builder.ins().iconst(types::I32, i64::from(cop));
                        let call =
                            builder
                                .ins()
                                .call_indirect(var_sig_ir, var_fn_ptr, &[opv, ni, rhs]);
                        builder.inst_results(call)[0]
                    };
                    stack.push(new_val);
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
                    if mixed {
                        let bc = mixed_encode_field_compound_binop(bop);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_COMPOUND_ASSIGN_FIELD));
                        let a1 = builder.ins().iconst(types::I32, i64::from(bc));
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, idx_f, rhs],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
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
                    if mixed {
                        let idx_i32 = builder.ins().fcvt_to_sint_sat(types::I32, idx_f);
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SET_FIELD));
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, idx_i32, val, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
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
                }

                // ── Arithmetic ─────────────────────────────────────────
                Op::Add => {
                    let b = stack.pop().expect("Add");
                    let a = stack.pop().expect("Add");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_ADD));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        stack.push(builder.ins().fadd(a, b));
                    }
                }
                Op::Sub => {
                    let b = stack.pop().expect("Sub");
                    let a = stack.pop().expect("Sub");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_SUB));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        stack.push(builder.ins().fsub(a, b));
                    }
                }
                Op::Mul => {
                    let b = stack.pop().expect("Mul");
                    let a = stack.pop().expect("Mul");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_MUL));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        stack.push(builder.ins().fmul(a, b));
                    }
                }
                Op::Div => {
                    let b = stack.pop().expect("Div");
                    let a = stack.pop().expect("Div");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_DIV));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        stack.push(builder.ins().fdiv(a, b));
                    }
                }
                Op::Mod => {
                    let b = stack.pop().expect("Mod");
                    let a = stack.pop().expect("Mod");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_MOD));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let div = builder.ins().fdiv(a, b);
                        let trunc = builder.ins().trunc(div);
                        let prod = builder.ins().fmul(trunc, b);
                        stack.push(builder.ins().fsub(a, prod));
                    }
                }

                // ── Comparison ─────────────────────────────────────────
                Op::CmpEq | Op::CmpNe | Op::CmpLt | Op::CmpLe | Op::CmpGt | Op::CmpGe => {
                    let b = stack.pop().expect("cmp");
                    let a = stack.pop().expect("cmp");
                    if mixed {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(mixed_op_for_cmp(&ops[pc])));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, b]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        use cranelift_codegen::ir::condcodes::FloatCC;
                        let cc = match ops[pc] {
                            Op::CmpEq => FloatCC::Equal,
                            Op::CmpNe => FloatCC::NotEqual,
                            Op::CmpLt => FloatCC::LessThan,
                            Op::CmpLe => FloatCC::LessThanOrEqual,
                            Op::CmpGt => FloatCC::GreaterThan,
                            Op::CmpGe => FloatCC::GreaterThanOrEqual,
                            _ => unreachable!(),
                        };
                        let cmp = builder.ins().fcmp(cc, a, b);
                        let i = builder.ins().uextend(types::I32, cmp);
                        stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                    }
                }

                // ── Unary ──────────────────────────────────────────────
                Op::Neg => {
                    let a = stack.pop().expect("Neg");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_NEG));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        stack.push(builder.ins().fneg(a));
                    }
                }
                Op::Pos => {
                    if mixed {
                        let a = stack.pop().expect("Pos");
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_POS));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    }
                    // Non-mixed: identity, no stack effect (matches VM numeric fast path).
                }
                Op::Not => {
                    let a = stack.pop().expect("Not");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_NOT));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let zero = builder.ins().f64const(0.0);
                        let is_zero = builder.ins().fcmp(
                            cranelift_codegen::ir::condcodes::FloatCC::Equal,
                            a,
                            zero,
                        );
                        let i = builder.ins().uextend(types::I32, is_zero);
                        stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                    }
                }
                Op::ToBool => {
                    let a = stack.pop().expect("ToBool");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_TO_BOOL));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, a, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let zero = builder.ins().f64const(0.0);
                        let ne = builder.ins().fcmp(
                            cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                            a,
                            zero,
                        );
                        let i = builder.ins().uextend(types::I32, ne);
                        stack.push(builder.ins().fcvt_from_uint(types::F64, i));
                    }
                }

                // ── Control flow ───────────────────────────────────────
                Op::Jump(target) => {
                    let target_block = block_map[&target];
                    builder.ins().jump(target_block, &[]);
                    block_terminated = true;
                }
                Op::JumpIfFalsePop(target) => {
                    let v = stack.pop().expect("JumpIfFalsePop");
                    let cond = if mixed {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_TRUTHINESS));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, v, zf]);
                        let truth = builder.inst_results(call)[0];
                        let zero = builder.ins().f64const(0.0);
                        builder.ins().fcmp(
                            cranelift_codegen::ir::condcodes::FloatCC::Equal,
                            truth,
                            zero,
                        )
                    } else {
                        let zero = builder.ins().f64const(0.0);
                        builder.ins().fcmp(
                            cranelift_codegen::ir::condcodes::FloatCC::Equal,
                            v,
                            zero,
                        )
                    };
                    let target_block = block_map[&target];
                    let fall_through = builder.create_block();
                    builder
                        .ins()
                        .brif(cond, target_block, &[], fall_through, &[]);
                    builder.switch_to_block(fall_through);
                    stack.clear(); // stack doesn't survive branch
                }
                Op::JumpIfTruePop(target) => {
                    let v = stack.pop().expect("JumpIfTruePop");
                    let cond = if mixed {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_TRUTHINESS));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, v, zf]);
                        let truth = builder.inst_results(call)[0];
                        let zero = builder.ins().f64const(0.0);
                        builder.ins().fcmp(
                            cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                            truth,
                            zero,
                        )
                    } else {
                        let zero = builder.ins().f64const(0.0);
                        builder.ins().fcmp(
                            cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                            v,
                            zero,
                        )
                    };
                    let target_block = block_map[&target];
                    let fall_through = builder.create_block();
                    builder
                        .ins()
                        .brif(cond, target_block, &[], fall_through, &[]);
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
                    let old = if use_slot_ssa {
                        builder.use_var(slot_vars[slot as usize])
                    } else {
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset)
                    };
                    let new_val = if mixed {
                        let mop = mixed_op_for_binop(bop);
                        let op_c = builder.ins().iconst(types::I32, i64::from(mop));
                        let z = builder.ins().iconst(types::I32, 0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, z, old, rhs],
                        );
                        builder.inst_results(call)[0]
                    } else {
                        match bop {
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
                        }
                    };
                    if use_slot_ssa {
                        builder.def_var(slot_vars[slot as usize], new_val);
                    } else {
                        builder
                            .ins()
                            .store(MemFlags::trusted(), new_val, slots_ptr, offset);
                    }
                    stack.push(new_val);
                }

                // ── Inc/dec slot (expression context — pushes result) ──
                Op::IncDecSlot(slot, kind) => {
                    if mixed {
                        let enc = mixed_encode_slot_incdec(slot, kind);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_INCDEC_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(enc));
                        let z = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                        stack.push(builder.inst_results(call)[0]);
                    } else if use_slot_ssa {
                        let old = builder.use_var(slot_vars[slot as usize]);
                        let delta = match kind {
                            IncDecOp::PreInc | IncDecOp::PostInc => builder.ins().f64const(1.0),
                            IncDecOp::PreDec | IncDecOp::PostDec => builder.ins().f64const(-1.0),
                        };
                        let new_val = builder.ins().fadd(old, delta);
                        builder.def_var(slot_vars[slot as usize], new_val);
                        let push_val = match kind {
                            IncDecOp::PreInc | IncDecOp::PreDec => new_val,
                            IncDecOp::PostInc | IncDecOp::PostDec => old,
                        };
                        stack.push(push_val);
                    } else {
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
                }

                // ── Fused slot ops (statement context) ─────────────────
                Op::IncrSlot(slot) => {
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_INCR_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    } else if use_slot_ssa {
                        let old = builder.use_var(slot_vars[slot as usize]);
                        let one = builder.ins().f64const(1.0);
                        let new_val = builder.ins().fadd(old, one);
                        builder.def_var(slot_vars[slot as usize], new_val);
                    } else {
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
                }
                Op::DecrSlot(slot) => {
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_DECR_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    } else if use_slot_ssa {
                        let old = builder.use_var(slot_vars[slot as usize]);
                        let one = builder.ins().f64const(1.0);
                        let new_val = builder.ins().fsub(old, one);
                        builder.def_var(slot_vars[slot as usize], new_val);
                    } else {
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
                }
                Op::AddSlotToSlot { src, dst } => {
                    if mixed {
                        let enc = mixed_encode_slot_pair(src, dst);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ADD_SLOT_TO_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(enc));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    } else if use_slot_ssa {
                        let sv = builder.use_var(slot_vars[src as usize]);
                        let dv = builder.use_var(slot_vars[dst as usize]);
                        let sum = builder.ins().fadd(dv, sv);
                        builder.def_var(slot_vars[dst as usize], sum);
                    } else {
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
                }

                // ── Field access via callback ──────────────────────────
                Op::PushFieldNum(field) => {
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_GET_FIELD));
                        let z = builder.ins().iconst(types::I32, 0);
                        let fv = builder.ins().f64const(field as f64);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, fv, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let arg = builder.ins().iconst(types::I32, field as i64);
                        let call = builder
                            .ins()
                            .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                        let result = builder.inst_results(call)[0];
                        stack.push(result);
                    }
                }
                Op::GetField => {
                    let fv = stack.pop().expect("GetField");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_GET_FIELD));
                        let z = builder.ins().iconst(types::I32, 0);
                        let zf = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z, fv, zf]);
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        // Match VM: `ctx.pop().as_number() as i32` — use saturating float→int
                        // (same family of semantics as Rust’s `f64 as i32` on recent editions).
                        let idx_i32 = builder.ins().fcvt_to_sint_sat(types::I32, fv);
                        let call =
                            builder
                                .ins()
                                .call_indirect(field_sig_ir, field_fn_ptr, &[idx_i32]);
                        stack.push(builder.inst_results(call)[0]);
                    }
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
                    if mixed {
                        let enc = mixed_encode_field_slot(field, slot);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ADD_FIELD_TO_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(enc));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    } else if use_slot_ssa {
                        let arg = builder.ins().iconst(types::I32, field as i64);
                        let call = builder
                            .ins()
                            .call_indirect(field_sig_ir, field_fn_ptr, &[arg]);
                        let fv = builder.inst_results(call)[0];
                        let old = builder.use_var(slot_vars[slot as usize]);
                        let sum = builder.ins().fadd(old, fv);
                        builder.def_var(slot_vars[slot as usize], sum);
                    } else {
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
                }
                Op::AddMulFieldsToSlot { f1, f2, slot } => {
                    if mixed {
                        let enc = u32::from(f1) | (u32::from(f2) << 16);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ADD_MUL_FIELDS_TO_SLOT));
                        let a1 = builder.ins().iconst(types::I32, i64::from(enc));
                        let a2 = builder.ins().f64const(f64::from(slot));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, a2, z]);
                    } else if use_slot_ssa {
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
                        let old = builder.use_var(slot_vars[slot as usize]);
                        let sum = builder.ins().fadd(old, prod);
                        builder.def_var(slot_vars[slot as usize], sum);
                    } else {
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
                    let v = if mixed {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_SLOT_AS_NUMBER));
                        let a1 = builder.ins().iconst(types::I32, i64::from(slot));
                        let z = builder.ins().f64const(0.0);
                        let call =
                            builder
                                .ins()
                                .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                        builder.inst_results(call)[0]
                    } else {
                        let offset = (slot as i32) * 8;
                        builder
                            .ins()
                            .load(types::F64, MemFlags::trusted(), slots_ptr, offset)
                    };
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
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_IO_PRINT_FIELD));
                    let a1 = builder.ins().iconst(types::I32, field as i64);
                    let z = builder.ins().iconst(types::I32, 0);
                    builder
                        .ins()
                        .call_indirect(io_sig_ir, io_fn_ptr, &[op_c, a1, z, z]);
                }
                Op::PrintFieldSepField { f1, sep, f2 } => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_IO_PRINT_FIELD_SEP_FIELD));
                    let a1 = builder.ins().iconst(types::I32, f1 as i64);
                    let a2 = builder.ins().iconst(types::I32, sep as i64);
                    let a3 = builder.ins().iconst(types::I32, f2 as i64);
                    builder
                        .ins()
                        .call_indirect(io_sig_ir, io_fn_ptr, &[op_c, a1, a2, a3]);
                }
                Op::PrintThreeFieldsStdout { f1, f2, f3 } => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_IO_PRINT_THREE_FIELDS));
                    let a1 = builder.ins().iconst(types::I32, f1 as i64);
                    let a2 = builder.ins().iconst(types::I32, f2 as i64);
                    let a3 = builder.ins().iconst(types::I32, f3 as i64);
                    builder
                        .ins()
                        .call_indirect(io_sig_ir, io_fn_ptr, &[op_c, a1, a2, a3]);
                }
                Op::Print {
                    argc: 0,
                    redir: crate::bytecode::RedirKind::Stdout,
                } => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_IO_PRINT_RECORD));
                    let z = builder.ins().iconst(types::I32, 0);
                    builder
                        .ins()
                        .call_indirect(io_sig_ir, io_fn_ptr, &[op_c, z, z, z]);
                }
                Op::Print {
                    argc,
                    redir: crate::bytecode::RedirKind::Stdout,
                } if argc > 0 => {
                    let n = argc as usize;
                    if stack.len() < n {
                        return None;
                    }
                    let mut vals: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(n);
                    for _ in 0..n {
                        vals.push(stack.pop().expect("Print argc"));
                    }
                    vals.reverse();
                    for (i, v) in vals.iter().enumerate() {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_PRINT_ARG));
                        let a1 = builder.ins().iconst(types::I32, i64::try_from(i).unwrap());
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, *v, z]);
                    }
                    let op_f = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_PRINT_FLUSH));
                    let a1 = builder.ins().iconst(types::I32, i64::from(argc));
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_f, a1, z, z]);
                }
                Op::Printf {
                    argc,
                    redir: crate::bytecode::RedirKind::Stdout,
                } if argc > 0 => {
                    let n = argc as usize;
                    if stack.len() < n {
                        return None;
                    }
                    let mut vals: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(n);
                    for _ in 0..n {
                        vals.push(stack.pop().expect("Printf argc"));
                    }
                    vals.reverse();
                    for (i, v) in vals.iter().enumerate() {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_PRINT_ARG));
                        let a1 = builder.ins().iconst(types::I32, i64::try_from(i).unwrap());
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, *v, z]);
                    }
                    let op_pf = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_PRINTF_FLUSH));
                    let a1 = builder.ins().iconst(types::I32, i64::from(argc));
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_pf, a1, z, z]);
                }
                Op::Printf { argc, redir }
                    if argc > 0 && redir != crate::bytecode::RedirKind::Stdout =>
                {
                    let n = argc as usize;
                    if stack.len() < n + 1 {
                        return None;
                    }
                    let path = stack.pop().expect("Printf redir path");
                    let mut vals: Vec<_> =
                        (0..n).map(|_| stack.pop().expect("Printf arg")).collect();
                    vals.reverse();
                    let zf = builder.ins().f64const(0.0);
                    for (i, v) in vals.iter().enumerate() {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_PRINT_ARG));
                        let a1 = builder.ins().iconst(types::I32, i64::try_from(i).unwrap());
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, *v, zf]);
                    }
                    let op_pf = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_PRINTF_FLUSH_REDIR));
                    let a1 = builder
                        .ins()
                        .iconst(types::I32, i64::from(pack_print_redir(argc, redir)));
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_pf, a1, path, zf]);
                }
                Op::Print { argc, redir } if redir != crate::bytecode::RedirKind::Stdout => {
                    let n = argc as usize;
                    if stack.len() < n + 1 {
                        return None;
                    }
                    let path = stack.pop().expect("Print redir path");
                    let zf = builder.ins().f64const(0.0);
                    if n > 0 {
                        let mut vals: Vec<_> =
                            (0..n).map(|_| stack.pop().expect("Print arg")).collect();
                        vals.reverse();
                        for (i, v) in vals.iter().enumerate() {
                            let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_PRINT_ARG));
                            let a1 = builder.ins().iconst(types::I32, i64::try_from(i).unwrap());
                            builder.ins().call_indirect(
                                val_sig_ir,
                                val_fn_ptr,
                                &[op_c, a1, *v, zf],
                            );
                        }
                    }
                    let op_fr = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_PRINT_FLUSH_REDIR));
                    let a1 = builder
                        .ins()
                        .iconst(types::I32, i64::from(pack_print_redir(argc, redir)));
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_fr, a1, path, zf]);
                }

                // ── MatchRegexp (push 0/1) ────────────────────────────────
                Op::MatchRegexp(idx) => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_MATCH_REGEXP));
                    let a1 = builder.ins().iconst(types::I32, idx as i64);
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    stack.push(builder.inst_results(call)[0]);
                }

                // ── Flow signals ──────────────────────────────────────────
                Op::Next => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_SIGNAL_NEXT));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::NextFile => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_SIGNAL_NEXT_FILE));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::ExitDefault => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_SIGNAL_EXIT_DEFAULT));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::ExitWithCode => {
                    let code = stack.pop().expect("ExitWithCode");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_SIGNAL_EXIT_CODE));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, code, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }

                // ── Array ops ─────────────────────────────────────────────
                Op::GetArrayElem(arr) => {
                    let key = stack.pop().expect("GetArrayElem key");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_ARRAY_GET));
                        let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(JIT_VAL_ARRAY_GET));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::SetArrayElem(arr) => {
                    let val = stack.pop().expect("SetArrayElem val");
                    let key = stack.pop().expect("SetArrayElem key");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_ARRAY_SET));
                        let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, val],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(JIT_VAL_ARRAY_SET));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, val],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::InArray(arr) => {
                    let key = stack.pop().expect("InArray key");
                    if mixed {
                        let op_c = builder.ins().iconst(types::I32, i64::from(MIXED_ARRAY_IN));
                        let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(JIT_VAL_ARRAY_IN));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::DeleteElem(arr) => {
                    let key = stack.pop().expect("DeleteElem key");
                    if mixed {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ARRAY_DELETE_ELEM));
                        let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                    } else {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(JIT_VAL_ARRAY_DELETE_ELEM));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, key, z]);
                    }
                }
                Op::DeleteArray(arr) => {
                    if mixed {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ARRAY_DELETE_ALL));
                        let a1 = builder.ins().iconst(types::I32, i64::from(arr));
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    } else {
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(JIT_VAL_ARRAY_DELETE_ALL));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let z = builder.ins().f64const(0.0);
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                    }
                }
                Op::CompoundAssignIndex(arr, bop) => {
                    let rhs = stack.pop().expect("CompoundAssignIndex rhs");
                    let key = stack.pop().expect("CompoundAssignIndex key");
                    if mixed {
                        let enc = mixed_encode_array_compound(arr, bop);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ARRAY_COMPOUND));
                        let a1 = builder.ins().iconst(types::I32, i64::from(enc));
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, rhs],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let cop = jit_val_op_for_array_compound(bop);
                        let op_c = builder.ins().iconst(types::I32, i64::from(cop));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, rhs],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::IncDecIndex(arr, kind) => {
                    let key = stack.pop().expect("IncDecIndex key");
                    if mixed {
                        let enc = mixed_encode_array_incdec(arr, kind);
                        let op_c = builder
                            .ins()
                            .iconst(types::I32, i64::from(MIXED_ARRAY_INCDEC));
                        let a1 = builder.ins().iconst(types::I32, i64::from(enc));
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    } else {
                        let cop = jit_val_op_for_array_incdec(kind);
                        let op_c = builder.ins().iconst(types::I32, i64::from(cop));
                        let a1 = builder.ins().iconst(types::I32, arr as i64);
                        let z = builder.ins().f64const(0.0);
                        let call = builder.ins().call_indirect(
                            val_sig_ir,
                            val_fn_ptr,
                            &[op_c, a1, key, z],
                        );
                        stack.push(builder.inst_results(call)[0]);
                    }
                }
                Op::JoinArrayKey(n) => {
                    let n = n as usize;
                    let mut vals: Vec<cranelift_codegen::ir::Value> = Vec::with_capacity(n);
                    for _ in 0..n {
                        vals.push(stack.pop().expect("JoinArrayKey"));
                    }
                    vals.reverse();
                    let z0 = builder.ins().iconst(types::I32, 0);
                    let zf = builder.ins().f64const(0.0);
                    let op_arg = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_JOIN_KEY_ARG));
                    for v in vals {
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_arg, z0, v, zf]);
                    }
                    let op_join = builder
                        .ins()
                        .iconst(types::I32, i64::from(MIXED_JOIN_ARRAY_KEY));
                    let a1n = builder.ins().iconst(types::I32, i64::try_from(n).unwrap());
                    let call = builder.ins().call_indirect(
                        val_sig_ir,
                        val_fn_ptr,
                        &[op_join, a1n, zf, zf],
                    );
                    stack.push(builder.inst_results(call)[0]);
                }

                // ── Return signals ────────────────────────────────────────
                Op::ReturnVal => {
                    let val = stack.pop().expect("ReturnVal");
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_SIGNAL_RETURN_VAL));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, val, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }
                Op::ReturnEmpty => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_SIGNAL_RETURN_EMPTY));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
                    builder.ins().return_(&[z]);
                    block_terminated = true;
                }

                // ── ForIn iteration ───────────────────────────────────────
                Op::ForInStart(arr) => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_FORIN_START));
                    let a1 = builder.ins().iconst(types::I32, arr as i64);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
                }
                Op::ForInNext { var, end_jump } => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_FORIN_NEXT));
                    let a1 = builder.ins().iconst(types::I32, var as i64);
                    let z = builder.ins().f64const(0.0);
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, z, z]);
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
                    builder
                        .ins()
                        .brif(exhausted, end_block, &[], fall_through, &[]);
                    builder.switch_to_block(fall_through);
                    stack.clear();
                }
                Op::ForInEnd => {
                    let op_c = builder
                        .ins()
                        .iconst(types::I32, i64::from(JIT_VAL_FORIN_END));
                    let z32 = builder.ins().iconst(types::I32, 0);
                    let z = builder.ins().f64const(0.0);
                    builder
                        .ins()
                        .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, z32, z, z]);
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
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, d, z]);
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
                    let call =
                        builder
                            .ins()
                            .call_indirect(val_sig_ir, val_fn_ptr, &[op_c, a1, d, z]);
                    stack.push(builder.inst_results(call)[0]);
                }

                _ => unreachable!("filtered by is_jit_eligible"),
            }
            pc += 1;
        }

        // Return the top of stack, or 0.0 if empty.
        if !block_terminated {
            let slots_ptr_ret = builder.use_var(var_slots_ptr);
            emit_slot_ssa_flush_to_mem(&mut builder, use_slot_ssa, &slot_vars, slots_ptr_ret);
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
        slot_count: slot_count as u16,
        needs_fields: has_fields,
    })
}

// ── Public dispatch API ────────────────────────────────────────────────────

/// When `AWKRS_JIT` is exactly `0`, skip JIT and use the bytecode VM only.
///
/// Reads the environment each call so tests and embedders can toggle the flag without process restart.
#[inline]
pub(crate) fn jit_disabled_by_env() -> bool {
    matches!(
        std::env::var_os("AWKRS_JIT").as_deref(),
        Some(s) if s == "0"
    )
}

/// Run a previously compiled chunk (no opcode hash or compile-cache lookup).
pub(crate) fn try_jit_execute_cached(
    chunk: &Arc<JitChunk>,
    state: &mut JitRuntimeState<'_>,
) -> Option<f64> {
    if jit_disabled_by_env() {
        return None;
    }
    Some(chunk.execute(state))
}

/// Try to JIT-compile and execute a chunk. Returns `Some(f64)` on success.
///
/// The caller supplies [`JitRuntimeState`] (slots and the seven `extern "C"` callbacks).
/// Uses a thread-local map keyed by [`ops_hash`] (legacy callers without a [`Chunk`] cache).
pub fn try_jit_execute(
    ops: &[Op],
    state: &mut JitRuntimeState<'_>,
    cp: &CompiledProgram,
) -> Option<f64> {
    if jit_disabled_by_env() {
        return None;
    }
    let hash = ops_hash(ops);

    let cached = JIT_COMPILE_CACHE.with(|c| c.borrow().get(&hash).cloned());
    if let Some(entry) = cached {
        return entry.as_ref().map(|chunk| chunk.execute(state));
    }

    let chunk = try_compile(ops, cp).map(Arc::new);
    let result = chunk.as_ref().map(|c| c.execute(state));
    JIT_COMPILE_CACHE.with(|c| {
        c.borrow_mut().insert(hash, chunk);
    });
    result
}

// ── Legacy API (backward compat with existing VM integration) ──────────────

/// True if `ops` is a straight-line numeric expression ending with exactly one value.
/// (Legacy check — superseded by [`is_jit_eligible`] but kept for the public API.)
///
/// Supports the same straight-line stack discipline as [`is_jit_eligible`] for pure
/// numeric stack ops: constants, `+ - * / %`, comparisons, unary `+`/`-`, logical
/// [`Op::Not`] / [`Op::ToBool`], [`Op::Dup`], [`Op::Pop`], slot read/write ([`Op::GetSlot`],
/// [`Op::SetSlot`]), [`Op::CompoundAssignSlot`], [`Op::IncDecSlot`], fused slot/field
/// peepholes ([`Op::IncrSlot`], [`Op::DecrSlot`], [`Op::AddSlotToSlot`], [`Op::AddFieldToSlot`],
/// [`Op::AddMulFieldsToSlot`]), [`Op::GetField`], constant field reads ([`Op::PushFieldNum`]), and
/// [`Op::GetNR`] / [`Op::GetFNR`] / [`Op::GetNF`] (via the field callback — [`JitNumericChunk::call_f64`]
/// passes a stub that returns `0.0`; use the VM path for real field/NR semantics).
///
/// Control-flow opcodes ([`Op::Jump`], [`Op::JumpIfSlotGeNum`], …) stay excluded so the legacy
/// API remains a single straight-line sequence ending with one stack value.
pub fn is_numeric_stack_eligible(ops: &[Op]) -> bool {
    let mut depth: i32 = 0;
    for op in ops {
        match op {
            Op::PushNum(_) => depth += 1,
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::CmpEq | Op::CmpNe | Op::CmpLt | Op::CmpLe | Op::CmpGt | Op::CmpGe => {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            }
            Op::Neg | Op::Pos | Op::Not | Op::ToBool => {
                if depth < 1 {
                    return false;
                }
            }
            Op::Dup => {
                if depth < 1 {
                    return false;
                }
                depth += 1;
            }
            Op::Pop => {
                if depth < 1 {
                    return false;
                }
                depth -= 1;
            }
            Op::GetSlot(_) => depth += 1,
            Op::SetSlot(_) => {}
            Op::CompoundAssignSlot(_, bop) => {
                if depth < 1 {
                    return false;
                }
                match bop {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {}
                    _ => return false,
                }
            }
            Op::IncDecSlot(_, _) => depth += 1,
            Op::IncrSlot(_) | Op::DecrSlot(_) => {}
            Op::AddSlotToSlot { .. } => {}
            Op::AddFieldToSlot { .. } | Op::AddMulFieldsToSlot { .. } => {}
            Op::GetField => {
                if depth < 1 {
                    return false;
                }
            }
            Op::PushFieldNum(_) | Op::GetNR | Op::GetFNR | Op::GetNF => depth += 1,
            _ => return false,
        }
    }
    depth == 1
}

/// Upper bound on slot indices touched by [`is_numeric_stack_eligible`] bytecode (for
/// [`JitNumericChunk::call_f64`] backing storage).
pub fn numeric_stack_slot_words(ops: &[Op]) -> usize {
    let mut m = 0usize;
    for op in ops {
        match op {
            Op::GetSlot(s) | Op::SetSlot(s) => m = m.max(*s as usize + 1),
            Op::CompoundAssignSlot(s, _)
            | Op::IncDecSlot(s, _)
            | Op::IncrSlot(s)
            | Op::DecrSlot(s) => {
                m = m.max(*s as usize + 1);
            }
            Op::AddSlotToSlot { src, dst, .. } => {
                m = m.max(*src as usize + 1).max(*dst as usize + 1);
            }
            Op::AddFieldToSlot { slot, .. } | Op::AddMulFieldsToSlot { slot, .. } => {
                m = m.max(*slot as usize + 1);
            }
            _ => {}
        }
    }
    m
}

/// Compile a pure-numeric expression (legacy API).
pub fn try_compile_numeric_expr(ops: &[Op]) -> Option<JitNumericChunk> {
    if !is_numeric_stack_eligible(ops) {
        return None;
    }
    // Use the new compiler but wrap in legacy struct
    let chunk = try_compile(ops, &empty_compiled_program())?;
    Some(JitNumericChunk {
        inner: chunk,
        slot_words: numeric_stack_slot_words(ops),
    })
}

/// Legacy wrapper — holds a JIT chunk compiled from pure numeric ops.
pub struct JitNumericChunk {
    inner: JitChunk,
    slot_words: usize,
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
        let mut slots: Vec<f64> = if self.slot_words == 0 {
            Vec::new()
        } else {
            vec![0.0; self.slot_words]
        };
        let mut state = JitRuntimeState::new(
            &mut slots,
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

/// Legacy dispatch — if the chunk is pure-numeric, run via JIT.
pub fn try_jit_dispatch_numeric_chunk(ops: &[Op]) -> Option<f64> {
    if jit_disabled_by_env() {
        return None;
    }
    let jit = try_compile_numeric_expr(ops)?;
    Some(jit.call_f64())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BinOp;
    use crate::bytecode::GetlineSource;
    use std::cell::RefCell;

    thread_local! {
        static TEST_JIT_VARS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }

    thread_local! {
        static TEST_JIT_FIELDS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    }

    #[test]
    fn is_nan_str_recognizes_dyn_bit() {
        let h = nan_str_dyn(0);
        assert!(is_nan_str(h.to_bits()));
    }

    #[test]
    fn nan_uninit_distinct_from_string_handles() {
        let u = nan_uninit();
        assert!(is_nan_uninit(u.to_bits()));
        assert!(!is_nan_str(u.to_bits()));
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
        let chunk = try_compile(ops, &super::empty_compiled_program()).expect("compile failed");
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
        let chunk = try_compile(ops, &super::empty_compiled_program()).expect("compile failed");
        let mut slots = [0.0f64; 0];
        let mut state = JitRuntimeState::new(
            &mut slots,
            dummy_field,
            dummy_array,
            dummy_var,
            test_field_dispatch,
            dummy_io_dispatch,
            test_field_mixed_val_dispatch,
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

    /// Mixed `SetField` lowers to [`MIXED_SET_FIELD`] on `val_dispatch`; wire it for field tests.
    extern "C" fn test_field_mixed_val_dispatch(op: u32, a1: u32, a2: f64, a3: f64) -> f64 {
        if op == MIXED_SET_FIELD {
            let field_idx = a1 as i32;
            let i = field_idx.max(0) as usize;
            TEST_JIT_FIELDS.with(|cell| {
                let mut v = cell.borrow_mut();
                if v.len() <= i {
                    v.resize(i + 1, 0.0);
                }
                v[i] = a2;
                a2
            })
        } else {
            dummy_val_dispatch(op, a1, a2, a3)
        }
    }

    fn exec(ops: &[Op]) -> f64 {
        let chunk = try_compile(ops, &super::empty_compiled_program()).expect("compile failed");
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
        let chunk = try_compile(ops, &super::empty_compiled_program()).expect("compile failed");
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
        let chunk = try_compile(ops, &super::empty_compiled_program()).expect("compile failed");
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
    fn jit_numeric_expr_mod_dup_pos() {
        let ops_mod = [Op::PushNum(10.0), Op::PushNum(3.0), Op::Mod];
        let j = try_compile_numeric_expr(&ops_mod).expect("compile mod");
        assert!((j.call_f64() - 1.0).abs() < 1e-15);

        let ops_pos = [Op::PushNum(7.0), Op::Pos];
        let j = try_compile_numeric_expr(&ops_pos).expect("compile pos");
        assert!((j.call_f64() - 7.0).abs() < 1e-15);

        // 5 * 5 = 25 via Dup
        let ops_dup = [Op::PushNum(5.0), Op::Dup, Op::Mul];
        let j = try_compile_numeric_expr(&ops_dup).expect("compile dup mul");
        assert!((j.call_f64() - 25.0).abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_cmp_not_to_bool() {
        let ops_lt = [Op::PushNum(1.0), Op::PushNum(2.0), Op::CmpLt];
        let j = try_compile_numeric_expr(&ops_lt).expect("compile CmpLt");
        assert!((j.call_f64() - 1.0).abs() < 1e-15);

        let ops_not = [Op::PushNum(0.0), Op::Not];
        let j = try_compile_numeric_expr(&ops_not).expect("compile Not");
        assert!((j.call_f64() - 1.0).abs() < 1e-15);

        let ops_tb = [Op::PushNum(0.0), Op::ToBool];
        let j = try_compile_numeric_expr(&ops_tb).expect("compile ToBool");
        assert!(j.call_f64().abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_slots_get_set() {
        // Store 5 in slot 0, duplicate via GetSlot, add -> 10.
        let ops = [Op::PushNum(5.0), Op::SetSlot(0), Op::GetSlot(0), Op::Add];
        assert_eq!(numeric_stack_slot_words(&ops), 1);
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 10.0).abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_set_pop_get() {
        let ops = [Op::PushNum(5.0), Op::SetSlot(0), Op::Pop, Op::GetSlot(0)];
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 5.0).abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_get_slot_zero() {
        let ops = [Op::GetSlot(0)];
        assert_eq!(numeric_stack_slot_words(&ops), 1);
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!(j.call_f64().abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_get_field() {
        let ops = [Op::PushNum(2.0), Op::GetField];
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!(j.call_f64().abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_compound_assign_slot() {
        let ops = [Op::PushNum(5.0), Op::CompoundAssignSlot(0, BinOp::Add)];
        assert_eq!(numeric_stack_slot_words(&ops), 1);
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 5.0).abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_add_slot_to_slot() {
        let ops = [
            Op::PushNum(7.0),
            Op::SetSlot(0),
            Op::Pop,
            Op::PushNum(0.0),
            Op::SetSlot(1),
            Op::Pop,
            Op::AddSlotToSlot { src: 0, dst: 1 },
            Op::GetSlot(1),
        ];
        assert_eq!(numeric_stack_slot_words(&ops), 2);
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 7.0).abs() < 1e-15);
    }

    #[test]
    fn jit_numeric_expr_incr_slot_then_get() {
        let ops = [
            Op::PushNum(0.0),
            Op::SetSlot(0),
            Op::Pop,
            Op::IncrSlot(0),
            Op::GetSlot(0),
        ];
        assert_eq!(numeric_stack_slot_words(&ops), 1);
        let j = try_compile_numeric_expr(&ops).expect("compile");
        assert!((j.call_f64() - 1.0).abs() < 1e-15);
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
    fn jit_eligible_join_array_key() {
        let ops = [
            Op::PushNum(1.0),
            Op::PushNum(2.0),
            Op::JoinArrayKey(2),
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_eligible_typeof() {
        let t = [Op::PushNum(1.0), Op::TypeofValue, Op::ReturnVal];
        assert!(is_jit_eligible(&t));
        assert!(needs_mixed_mode(&t));
        assert!(is_jit_eligible(&[Op::TypeofVar(0), Op::ReturnVal]));
        assert!(needs_mixed_mode(&[Op::TypeofVar(0), Op::ReturnVal]));
    }

    #[test]
    fn jit_mixed_string_ops_eligible() {
        assert!(is_jit_eligible(&[Op::PushStr(0)]));
        assert!(needs_mixed_mode(&[Op::PushStr(0)]));
        let concat_ops = [
            Op::PushNum(1.0),
            Op::PushStr(0),
            Op::Concat,
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&concat_ops));
        assert!(needs_mixed_mode(&concat_ops));
    }

    #[test]
    fn jit_mixed_print_stdout_eligible() {
        let ops = [
            Op::PushNum(1.0),
            Op::Print {
                argc: 1,
                redir: crate::bytecode::RedirKind::Stdout,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_eligible_printf_stdout() {
        let ops = [
            Op::PushStr(0),
            Op::PushNum(42.0),
            Op::Printf {
                argc: 2,
                redir: crate::bytecode::RedirKind::Stdout,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
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
        let r = exec_with_test_field(&[Op::PushNum(1.0), Op::PushNum(42.0), Op::SetField]);
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
            Op::PrintFieldSepField {
                f1: 1,
                sep: 0,
                f2: 2
            },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_print_three_fields() {
        assert!(is_jit_eligible(&[
            Op::PrintThreeFieldsStdout {
                f1: 1,
                f2: 2,
                f3: 3
            },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_eligible_print_record() {
        assert!(is_jit_eligible(&[
            Op::Print {
                argc: 0,
                redir: crate::bytecode::RedirKind::Stdout
            },
            Op::PushNum(0.0),
        ]));
    }

    #[test]
    fn jit_mixed_print_with_args_eligible() {
        let ops = [
            Op::PushNum(1.0),
            Op::Print {
                argc: 1,
                redir: crate::bytecode::RedirKind::Stdout,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
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
        let ops = [
            Op::PrintThreeFieldsStdout {
                f1: 1,
                f2: 2,
                f3: 3,
            },
            Op::PushNum(0.0),
        ];
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

    // ── Array opcodes (mixed mode: NaN-boxed keys) ────────────────────────

    #[test]
    fn jit_mixed_array_get_eligible() {
        let ops = [Op::PushNum(1.0), Op::GetArrayElem(0), Op::PushNum(0.0)];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_mixed_array_set_eligible() {
        let ops = [
            Op::PushNum(1.0),
            Op::PushNum(42.0),
            Op::SetArrayElem(0),
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_mixed_in_array_eligible() {
        let ops = [Op::PushNum(1.0), Op::InArray(0), Op::PushNum(0.0)];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_mixed_compound_assign_index_eligible() {
        let ops = [
            Op::PushNum(1.0),
            Op::PushNum(5.0),
            Op::CompoundAssignIndex(0, BinOp::Add),
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_mixed_split_eligible() {
        let ops_default_fs = [
            Op::PushStr(0),
            Op::Split {
                arr: 1,
                has_fs: false,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops_default_fs));
        assert!(needs_mixed_mode(&ops_default_fs));

        let ops_explicit_fs = [
            Op::PushStr(0),
            Op::PushStr(0),
            Op::Split {
                arr: 1,
                has_fs: true,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops_explicit_fs));
        assert!(needs_mixed_mode(&ops_explicit_fs));
    }

    #[test]
    fn jit_eligible_call_user_and_gsub() {
        use crate::bytecode::SubTarget;
        let ops_user = [Op::PushNum(1.0), Op::CallUser(0, 1), Op::PushNum(0.0)];
        assert!(is_jit_eligible(&ops_user));
        assert!(needs_mixed_mode(&ops_user));

        let ops_gsub = [
            Op::PushStr(0),
            Op::PushStr(1),
            Op::GsubFn(SubTarget::Record),
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops_gsub));
        assert!(needs_mixed_mode(&ops_gsub));
    }

    #[test]
    fn jit_mixed_patsplit_eligible() {
        let ops = [
            Op::PushStr(0),
            Op::Patsplit {
                arr: 1,
                has_fp: false,
                seps: None,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));

        let ops_fp_sep = [
            Op::PushStr(0),
            Op::PushStr(0),
            Op::Patsplit {
                arr: 1,
                has_fp: true,
                seps: Some(2),
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops_fp_sep));
        assert!(needs_mixed_mode(&ops_fp_sep));

        let ops_fp_sep_large = [
            Op::PushStr(0),
            Op::PushStr(0),
            Op::Patsplit {
                arr: 70000,
                has_fp: true,
                seps: Some(2),
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops_fp_sep_large));
    }

    #[test]
    fn jit_mixed_match_builtin_eligible() {
        let ops = [
            Op::PushStr(0),
            Op::PushStr(0),
            Op::MatchBuiltin { arr: None },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));

        let ops_arr = [
            Op::PushStr(0),
            Op::PushStr(0),
            Op::MatchBuiltin { arr: Some(1) },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops_arr));
        assert!(needs_mixed_mode(&ops_arr));
    }

    #[test]
    fn jit_mixed_print_redir_eligible() {
        use crate::bytecode::RedirKind;
        let ops = [
            Op::PushStr(0),
            Op::PushStr(0),
            Op::Print {
                argc: 1,
                redir: RedirKind::Overwrite,
            },
            Op::PushNum(0.0),
        ];
        assert!(is_jit_eligible(&ops));
        assert!(needs_mixed_mode(&ops));
    }

    #[test]
    fn jit_mixed_getline_eligible() {
        let ops_primary = [
            Op::GetLine {
                var: None,
                source: GetlineSource::Primary,
            },
            Op::ReturnEmpty,
        ];
        assert!(is_jit_eligible(&ops_primary));
        assert!(needs_mixed_mode(&ops_primary));

        let ops_file = [
            Op::PushStr(0),
            Op::GetLine {
                var: Some(0),
                source: GetlineSource::File,
            },
            Op::ReturnEmpty,
        ];
        assert!(is_jit_eligible(&ops_file));
        assert!(needs_mixed_mode(&ops_file));
    }

    // ── Conditional Next ──────────────────────────────────────────────────

    #[test]
    fn jit_conditional_next() {
        // if (1) next; else fall through
        let ops = [
            Op::PushNum(1.0),
            Op::JumpIfFalsePop(3),
            Op::Next,          // signal raised — JIT returns immediately
            Op::PushNum(99.0), // not reached
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
            match i {
                1 => 10.0,
                2 => 20.0,
                _ => 0.0,
            }
        }
        let ops = [
            Op::PrintFieldStdout(1),                  // side-effect (dummy)
            Op::AddFieldToSlot { field: 2, slot: 0 }, // sum += $2
            Op::GetSlot(0),                           // return sum
        ];
        let mut slots = [5.0];
        let chunk = try_compile(&ops, &super::empty_compiled_program()).expect("compile");
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
            Op::ForInNext {
                var: 1,
                end_jump: 4
            },
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
            Op::ForInNext {
                var: 1,
                end_jump: 4,
            },
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
