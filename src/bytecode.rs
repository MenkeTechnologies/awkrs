//! Bytecode representation for the awk VM.
//!
//! The AST is compiled into flat [`Op`] instruction streams stored in [`Chunk`]s.
//! A [`CompiledProgram`] holds compiled rule bodies, function bodies, and a shared
//! [`StringPool`] that interns all string constants and variable names so the VM
//! can refer to them by cheap `u32` index.

use crate::ast::{BinOp, IncDecOp};
use crate::runtime::{AwkMap, Value};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

// ── Instruction set ──────────────────────────────────────────────────────────

/// Print/printf output target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirKind {
    Stdout,
    Overwrite,
    Append,
    Pipe,
    Coproc,
}

/// Source for `getline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetlineSource {
    Primary,
    File,
    Coproc,
    /// `expr | getline` — one line from `sh -c` with the command string.
    Pipe,
}

/// Lvalue target for `sub`/`gsub`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubTarget {
    /// Operate on `$0` (no third argument).
    Record,
    /// Named variable (string pool index, not slotted).
    Var(u32),
    /// Named variable (slot index, fast path).
    SlotVar(u16),
    /// `$expr` — field index is on stack.
    Field,
    /// `arr[key]` — key is on stack.
    Index(u32),
}

#[allow(dead_code)]
/// Single bytecode instruction.
///
/// All jump targets are **absolute** instruction indices within the chunk.
/// Each variant is `Copy` so the VM can read instructions without cloning.
#[derive(Debug, Clone, Copy)]
pub enum Op {
    // ── Constants ────────────────────────────────────────────────────────
    PushNum(f64),
    /// Push interned string by pool index.
    PushStr(u32),
    /// Push gawk-style regexp constant (`@/…/`) — [`crate::runtime::Value::Regexp`] with interned pattern.
    PushRegexp(u32),

    // ── Variable access ─────────────────────────────────────────────────
    /// Push variable value (name by pool index) — HashMap path for specials.
    GetVar(u32),
    /// Peek TOS, store in variable — HashMap path for specials.
    SetVar(u32),
    /// Push variable value — fast Vec-indexed path for user scalars.
    GetSlot(u16),
    /// Peek TOS, store in slot — fast Vec-indexed path for user scalars.
    SetSlot(u16),
    /// Pop field index, push `$idx`.
    GetField,
    /// Pop value, pop field index, store `$idx = val`, push `val`.
    SetField,
    /// Pop key, push `arr[key]`.
    GetArrayElem(u32),
    /// Pop value, pop key, store `arr[key] = val`, push `val`.
    SetArrayElem(u32),
    /// Push `length(SYMTAB)` — dynamic symbol count (gawk-style introspection).
    SymtabKeyCount,

    // ── Compound assignment ─────────────────────────────────────────────
    /// Pop rhs; compute `var op= rhs`; push result — HashMap path.
    CompoundAssignVar(u32, BinOp),
    /// Pop rhs; compute `slot op= rhs`; push result — fast Vec path.
    CompoundAssignSlot(u16, BinOp),
    /// Pop rhs, pop field idx; compute `$idx op= rhs`; push result.
    CompoundAssignField(BinOp),
    /// Pop rhs, pop key; compute `arr[key] op= rhs`; push result.
    CompoundAssignIndex(u32, BinOp),

    /// `++`/`--` on a named variable (HashMap path).
    IncDecVar(u32, IncDecOp),
    /// `i++`/`++i` statement (result discarded) on HashMap-path variable.
    IncrVar(u32),
    /// `i--`/`--i` statement (result discarded) on HashMap-path variable.
    DecrVar(u32),
    /// `++`/`--` on a slotted scalar.
    IncDecSlot(u16, IncDecOp),
    /// Pop field index; `++`/`--` `$n`; push resulting numeric value.
    IncDecField(IncDecOp),
    /// Pop key; `++`/`--` on `arr[key]`; push resulting numeric value.
    IncDecIndex(u32, IncDecOp),

    // ── Arithmetic (pop 2, push 1) ──────────────────────────────────────
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// Exponentiation (`^` / `**`); pop exponent, pop base, push `pow(base, exp)` (right-assoc in parser).
    Pow,

    // ── Comparison (pop 2, push Num 0/1) — POSIX-aware ──────────────────
    CmpEq,
    CmpNe,
    CmpLt,
    CmpLe,
    CmpGt,
    CmpGe,

    // ── String / regex (pop 2, push result) ─────────────────────────────
    Concat,
    RegexMatch,
    RegexNotMatch,

    // ── Unary (pop 1, push 1) ───────────────────────────────────────────
    Neg,
    Pos,
    Not,

    /// Convert TOS to `Num(0.0)` or `Num(1.0)`.
    ToBool,

    // ── Control flow ────────────────────────────────────────────────────
    /// Unconditional jump to absolute instruction index.
    Jump(usize),
    /// Pop TOS; if falsy, jump.
    JumpIfFalsePop(usize),
    /// Pop TOS; if truthy, jump.
    JumpIfTruePop(usize),

    // ── Print / Printf ─────────────────────────────────────────────────
    /// Pop `argc` values (+ redir path if not Stdout). No stack result.
    Print {
        argc: u16,
        redir: RedirKind,
    },
    Printf {
        argc: u16,
        redir: RedirKind,
    },

    // ── Flow signals (cause VM to return) ───────────────────────────────
    Next,
    /// Skip remaining records in the current file; run `ENDFILE` then open the next file.
    NextFile,
    /// Pop exit code from stack.
    ExitWithCode,
    /// Exit with code 0.
    ExitDefault,
    /// Pop return value from stack.
    ReturnVal,
    /// Return empty string.
    ReturnEmpty,

    // ── Function calls ──────────────────────────────────────────────────
    /// Pop `argc` args, call builtin by name index, push result.
    CallBuiltin(u32, u16),
    /// Pop `argc` args, call user function by name index, push result.
    CallUser(u32, u16),
    /// Pop `argc` args then callee name (TOS), resolve builtin or user function, push result.
    CallIndirect(u16),

    /// `typeof(var)` — interned name; push `"string"` / `"number"` / `"array"` / `"uninitialized"`.
    TypeofVar(u32),
    /// `typeof` for a scalar in a slot (same semantics as [`TypeofVar`]).
    TypeofSlot(u16),
    /// Pop key, `typeof(arr[key])` for existing array or `"uninitialized"`.
    TypeofArrayElem(u32),
    /// Pop field index, `typeof($n)` — fields beyond `NF` are `"uninitialized"`.
    TypeofField,
    /// Pop any value; `typeof` never reports `"uninitialized"` (only from lvalue forms above).
    TypeofValue,

    // ── Array operations ────────────────────────────────────────────────
    /// Pop key, push `Num(1)` if key in array, else `Num(0)`.
    InArray(u32),
    /// Delete entire array.
    DeleteArray(u32),
    /// Pop key, delete `arr[key]`.
    DeleteElem(u32),

    // ── Multi-dimensional array key ─────────────────────────────────────
    /// Pop `n` values, join with SUBSEP, push combined key string.
    JoinArrayKey(u16),

    // ── Getline ─────────────────────────────────────────────────────────
    /// `var` is optional variable name index. File/Coproc/Pipe pop an expr from stack when applicable.
    /// `push_result`: expression `getline` pushes `1`/`0`/`-1`; statement form uses `false`.
    GetLine {
        var: Option<u32>,
        source: GetlineSource,
        push_result: bool,
    },

    // ── Sub / Gsub with lvalue info ─────────────────────────────────────
    /// Pop re, pop repl [, pop field_idx/key]; push substitution count.
    SubFn(SubTarget),
    GsubFn(SubTarget),

    // ── Split / Patsplit / Match ────────────────────────────────────────
    /// `split(s, arr [, fs])`. Pop fs if `has_fs`, pop s. Push count.
    Split {
        arr: u32,
        has_fs: bool,
    },
    /// `patsplit(s, arr [, fp [, seps]])`. Pop fp if `has_fp`, pop s. Push count.
    Patsplit {
        arr: u32,
        has_fp: bool,
        seps: Option<u32>,
    },
    /// `match(s, re [, arr])`. Pop re, pop s. Push RSTART.
    MatchBuiltin {
        arr: Option<u32>,
    },

    // ── ForIn iteration ─────────────────────────────────────────────────
    /// Collect keys of array into iterator stack.
    ForInStart(u32),
    /// Store next key in var; if exhausted jump to `end_jump`.
    ForInNext {
        var: u32,
        end_jump: usize,
    },
    /// Pop iterator from stack.
    ForInEnd,

    // ── Stack manipulation ──────────────────────────────────────────────
    Pop,
    /// Duplicate top of stack (for `switch` / multi-branch compare).
    Dup,

    // ── gawk array sort ───────────────────────────────────────────────────
    /// `asort(src [, dest])` — sort by value; string pool indices for array names.
    Asort {
        src: u32,
        dest: Option<u32>,
    },
    /// `asorti(src [, dest])` — sort indices lexicographically.
    Asorti {
        src: u32,
        dest: Option<u32>,
    },

    // ── Pattern helpers ─────────────────────────────────────────────────
    /// Test regex (by pool index) against `$0`, push `Num(0/1)`.
    MatchRegexp(u32),

    // ── Fused opcodes (peephole) ────────────────────────────────────────
    /// `s += $N` fused: read field N as number, add to slot, discard result.
    /// Eliminates: PushNum + GetField + CompoundAssignSlot(Add) + Pop.
    AddFieldToSlot {
        field: u16,
        slot: u16,
    },
    /// `s .. "lit"` fused: append interned string to TOS in-place, no clone.
    /// Eliminates: PushStr(idx) + Concat (2 ops → 1).
    ConcatPoolStr(u32),
    /// `print $N` to stdout fused: write field N bytes directly to print_buf.
    /// Eliminates: PushNum + GetField + Print{1,Stdout} (3 ops → 1).
    PrintFieldStdout(u16),
    /// `i = i + 1` or `i++` fused: increment slot by 1.0 in-place.
    /// Eliminates: GetSlot + PushNum(1) + Add + SetSlot + Pop (5 ops → 1),
    /// or IncDecSlot(PostInc/PreInc) + Pop (2 ops → 1).
    IncrSlot(u16),
    /// `i--` / `--i` fused: decrement slot by 1.0 in-place (statement context, result discarded).
    DecrSlot(u16),
    /// `s += i` fused: add src slot value to dst slot, discard result.
    /// Eliminates: GetSlot + CompoundAssignSlot(Add) + Pop (3 ops → 1).
    AddSlotToSlot {
        src: u16,
        dst: u16,
    },
    /// `$N` as number: push field N parsed as f64 directly, no String allocation.
    /// Eliminates: PushNum(N) + GetField when followed by arithmetic.
    PushFieldNum(u16),
    /// Push NR directly as Value::Num — avoids HashMap lookup for special variable.
    GetNR,
    /// Push FNR directly as Value::Num.
    GetFNR,
    /// Push NF directly as Value::Num.
    GetNF,
    /// `if (slot < limit) goto target` fused loop condition.
    /// Eliminates: GetSlot + PushNum(limit) + CmpLt + JumpIfFalsePop (4 ops → 1).
    JumpIfSlotGeNum {
        slot: u16,
        limit: f64,
        target: usize,
    },
    /// `sum += $f1 * $f2` fused.
    AddMulFieldsToSlot {
        f1: u16,
        f2: u16,
        slot: u16,
    },
    /// `a[$field] += delta` with numeric delta (common `a[$5] += 1`).
    ArrayFieldAddConst {
        arr: u32,
        field: u16,
        delta: f64,
    },
    /// `print $f1 sep $f2` to stdout (sep is interned string pool index).
    PrintFieldSepField {
        f1: u16,
        sep: u32,
        f2: u16,
    },
    /// `print $f1, $f2, $f3` to stdout (three fields, OFS between).
    PrintThreeFieldsStdout {
        f1: u16,
        f2: u16,
        f3: u16,
    },
}

// ── Compiled structures ─────────────────────────────────────────────────────

type JitChunkCache = Mutex<Option<Result<Arc<crate::jit::JitChunk>, ()>>>;

/// A flat sequence of bytecode instructions.
///
/// [`Self::jit_lock`] caches the result of the first JIT attempt for this chunk:
/// `None` = not yet tried, `Some(Err(()))` = use interpreter, `Some(Ok(arc))` = native code.
///
/// [`Self::jit_invocation_count`] supports tiered JIT: the VM runs the interpreter until this
/// chunk has been entered enough times (see [`crate::jit::jit_min_invocations_before_compile`]),
/// avoiding compile cost on cold paths (e.g. one-shot `BEGIN` blocks).
#[derive(Clone)]
pub struct Chunk {
    pub ops: Vec<Op>,
    pub(crate) jit_lock: Arc<JitChunkCache>,
    pub(crate) jit_invocation_count: Arc<AtomicU32>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            ops: Vec::new(),
            jit_lock: Arc::new(Mutex::new(None)),
            jit_invocation_count: Arc::new(AtomicU32::new(0)),
        }
    }
}

impl fmt::Debug for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chunk")
            .field("ops", &self.ops)
            .field("jit_lock", &"<cached JIT>")
            .field(
                "jit_invocation_count",
                &self.jit_invocation_count.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl Chunk {
    pub fn from_ops(ops: Vec<Op>) -> Self {
        Self {
            ops,
            jit_lock: Arc::new(Mutex::new(None)),
            jit_invocation_count: Arc::new(AtomicU32::new(0)),
        }
    }
}

/// Interned string pool shared across the entire compiled program.
#[derive(Debug, Clone, Default)]
pub struct StringPool {
    strings: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringPool {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.index.get(s) {
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), idx);
        idx
    }

    pub fn get(&self, idx: u32) -> &str {
        &self.strings[idx as usize]
    }
}

/// A fully compiled awk program, ready for VM execution.
#[derive(Debug, Clone)]
pub struct CompiledProgram {
    pub begin_chunks: Vec<Chunk>,
    pub end_chunks: Vec<Chunk>,
    pub beginfile_chunks: Vec<Chunk>,
    pub endfile_chunks: Vec<Chunk>,
    pub record_rules: Vec<CompiledRule>,
    pub functions: HashMap<String, CompiledFunc>,
    pub strings: StringPool,
    /// Number of variable slots (size of the `Runtime::slots` Vec).
    pub slot_count: u16,
    /// `slot_names[i]` = variable name for slot `i`.
    pub slot_names: Vec<String>,
    /// Reverse map: variable name → slot index (used by cold-path `get_var`/`set_var`).
    pub slot_map: HashMap<String, u16>,
}

impl CompiledProgram {
    /// Create the initial slots Vec from the runtime's current variable state.
    pub fn init_slots(&self, vars: &AwkMap<String, Value>) -> Vec<Value> {
        let mut slots = vec![Value::Uninit; self.slot_count as usize];
        for (i, name) in self.slot_names.iter().enumerate() {
            if let Some(v) = vars.get(name) {
                slots[i] = v.clone();
            }
        }
        slots
    }
}

/// One compiled record-processing rule (pattern + action body).
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub pattern: CompiledPattern,
    pub body: Chunk,
    /// Index into the original `Program.rules` vec (used for range-state tracking).
    pub original_index: usize,
}

/// One endpoint of a range pattern (`pat1, pat2 { … }`).
#[derive(Debug, Clone)]
pub enum CompiledRangeEndpoint {
    /// `Pattern::Empty` — always matches.
    Always,
    /// `BEGIN` / `END` / `BEGINFILE` / `ENDFILE` as an endpoint — never matches.
    Never,
    /// Nested `pat1, pat2` as an endpoint — [`crate::vm::vm_match_range_endpoint`] returns `Err`.
    NestedRangeError,
    /// Regex tested against `$0`.
    Regexp(u32),
    /// Literal string — `str::contains` on `$0`.
    LiteralRegexp(u32),
    /// Expression chunk; truthy → match.
    Expr(Chunk),
}

/// Compiled form of a rule pattern.
#[derive(Debug, Clone)]
pub enum CompiledPattern {
    /// Matches every record.
    Always,
    /// Regex literal tested against `$0`.
    Regexp(u32),
    /// Literal string pattern — uses `str::contains` instead of regex engine.
    LiteralRegexp(u32),
    /// Arbitrary expression; truthy → match.
    Expr(Chunk),
    /// Inclusive range — state tracked externally by [`CompiledRule::original_index`].
    Range {
        start: CompiledRangeEndpoint,
        end: CompiledRangeEndpoint,
    },
}

/// A compiled user-defined function.
#[derive(Debug, Clone)]
pub struct CompiledFunc {
    pub params: Vec<String>,
    pub body: Chunk,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Value;

    #[test]
    fn string_pool_intern_dedupes() {
        let mut p = StringPool::default();
        let a = p.intern("hello");
        let b = p.intern("hello");
        let c = p.intern("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(p.get(a), "hello");
        assert_eq!(p.get(c), "world");
    }

    #[test]
    fn init_slots_seeds_from_vars_map() {
        let mut vars = AwkMap::default();
        vars.insert("x".into(), Value::Num(7.0));
        let cp = CompiledProgram {
            begin_chunks: vec![],
            end_chunks: vec![],
            beginfile_chunks: vec![],
            endfile_chunks: vec![],
            record_rules: vec![],
            functions: HashMap::new(),
            strings: StringPool::default(),
            slot_count: 1,
            slot_names: vec!["x".into()],
            slot_map: HashMap::from([("x".into(), 0u16)]),
        };
        let slots = cp.init_slots(&vars);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].as_number(), 7.0);
    }

    #[test]
    fn string_pool_intern_preserves_order() {
        let mut p = StringPool::default();
        let i0 = p.intern("first");
        let i1 = p.intern("second");
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(p.get(i0), "first");
        assert_eq!(p.get(i1), "second");
    }

    #[test]
    fn string_pool_many_distinct_strings() {
        let mut p = StringPool::default();
        let mut idx = Vec::new();
        for i in 0..32 {
            let s = format!("k{i}");
            idx.push(p.intern(&s));
        }
        for (i, id) in idx.iter().copied().enumerate() {
            assert_eq!(p.get(id), format!("k{i}"));
        }
    }

    #[test]
    fn init_slots_missing_var_uses_empty_string() {
        let cp = CompiledProgram {
            begin_chunks: vec![],
            end_chunks: vec![],
            beginfile_chunks: vec![],
            endfile_chunks: vec![],
            record_rules: vec![],
            functions: HashMap::new(),
            strings: StringPool::default(),
            slot_count: 2,
            slot_names: vec!["x".into(), "y".into()],
            slot_map: HashMap::from([("x".into(), 0u16), ("y".into(), 1u16)]),
        };
        let mut vars = AwkMap::default();
        vars.insert("x".into(), Value::Num(1.0));
        let slots = cp.init_slots(&vars);
        assert_eq!(slots[0].as_number(), 1.0);
        assert_eq!(slots[1].as_str(), "");
    }
}
