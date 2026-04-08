//! Bytecode representation for the awk VM.
//!
//! The AST is compiled into flat [`Op`] instruction streams stored in [`Chunk`]s.
//! A [`CompiledProgram`] holds compiled rule bodies, function bodies, and a shared
//! [`StringPool`] that interns all string constants and variable names so the VM
//! can refer to them by cheap `u32` index.

use crate::ast::BinOp;
use std::collections::HashMap;

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
}

/// Lvalue target for `sub`/`gsub`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubTarget {
    /// Operate on `$0` (no third argument).
    Record,
    /// Named variable.
    Var(u32),
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

    // ── Variable access ─────────────────────────────────────────────────
    /// Push variable value (name by pool index).
    GetVar(u32),
    /// Peek TOS, store in variable. Stack unchanged (assignment is an expression).
    SetVar(u32),
    /// Pop field index, push `$idx`.
    GetField,
    /// Pop value, pop field index, store `$idx = val`, push `val`.
    SetField,
    /// Pop key, push `arr[key]`.
    GetArrayElem(u32),
    /// Pop value, pop key, store `arr[key] = val`, push `val`.
    SetArrayElem(u32),

    // ── Compound assignment ─────────────────────────────────────────────
    /// Pop rhs; compute `var op= rhs`; push result. Peek TOS = result.
    CompoundAssignVar(u32, BinOp),
    /// Pop rhs, pop field idx; compute `$idx op= rhs`; push result.
    CompoundAssignField(BinOp),
    /// Pop rhs, pop key; compute `arr[key] op= rhs`; push result.
    CompoundAssignIndex(u32, BinOp),

    // ── Arithmetic (pop 2, push 1) ──────────────────────────────────────
    Add,
    Sub,
    Mul,
    Div,
    Mod,

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
    /// `var` is optional variable name index. File/Coproc pop an expr from stack.
    GetLine {
        var: Option<u32>,
        source: GetlineSource,
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

    // ── Pattern helpers ─────────────────────────────────────────────────
    /// Test regex (by pool index) against `$0`, push `Num(0/1)`.
    MatchRegexp(u32),
}

// ── Compiled structures ─────────────────────────────────────────────────────

/// A flat sequence of bytecode instructions.
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub ops: Vec<Op>,
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
}

/// One compiled record-processing rule (pattern + action body).
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub pattern: CompiledPattern,
    pub body: Chunk,
    /// Index into the original `Program.rules` vec (used for range-state tracking).
    pub original_index: usize,
}

/// Compiled form of a rule pattern.
#[derive(Debug, Clone)]
pub enum CompiledPattern {
    /// Matches every record.
    Always,
    /// Regex literal tested against `$0`.
    Regexp(u32),
    /// Arbitrary expression; truthy → match.
    Expr(Chunk),
    /// Range pattern — state tracked externally by `original_index`.
    Range,
}

/// A compiled user-defined function.
#[derive(Debug, Clone)]
pub struct CompiledFunc {
    pub params: Vec<String>,
    pub body: Chunk,
}
