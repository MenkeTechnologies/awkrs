//! Bridge between awkrs's bytecode `Op` and fusevm's `fusevm::Op`.
//!
//! Translates an awkrs `Chunk` into a fusevm `fusevm::Chunk`, mapping:
//! - Universal ops (arithmetic, comparison, control flow, slots, stack) → direct fusevm ops
//! - AWK-specific ops (fields, arrays, regex, print, getline, etc.) → `fusevm::Op::Extended`
//!
//! This allows awkrs to share fusevm's interpreter and Cranelift JIT for the
//! universal hot path while keeping AWK semantics in extension handlers.

use crate::ast::BinOp;
use crate::bytecode::{self, GetlineSource, RedirKind};

// ── AWK extension op IDs for fusevm::Op::Extended(id, arg) ──

// Fields
/// `AWK_GET_FIELD` constant.
pub const AWK_GET_FIELD: u16 = 1000;
/// `AWK_SET_FIELD` constant.
pub const AWK_SET_FIELD: u16 = 1001;
/// `AWK_COMPOUND_ASSIGN_FIELD` constant.
pub const AWK_COMPOUND_ASSIGN_FIELD: u16 = 1002;
/// `AWK_INCDEC_FIELD` constant.
pub const AWK_INCDEC_FIELD: u16 = 1003;

// Variables (HashMap path, not slotted)
/// `AWK_COMPOUND_ASSIGN_VAR` constant.
pub const AWK_COMPOUND_ASSIGN_VAR: u16 = 1010;
/// `AWK_INCDEC_VAR` constant.
pub const AWK_INCDEC_VAR: u16 = 1011;
/// `AWK_INCR_VAR` constant.
pub const AWK_INCR_VAR: u16 = 1012;
/// `AWK_DECR_VAR` constant.
pub const AWK_DECR_VAR: u16 = 1013;

// Slot compound/incdec
/// `AWK_COMPOUND_ASSIGN_SLOT` constant.
pub const AWK_COMPOUND_ASSIGN_SLOT: u16 = 1020;
/// `AWK_INCDEC_SLOT` constant.
pub const AWK_INCDEC_SLOT: u16 = 1021;

// Array compound/incdec
/// `AWK_COMPOUND_ASSIGN_INDEX` constant.
pub const AWK_COMPOUND_ASSIGN_INDEX: u16 = 1030;
/// `AWK_INCDEC_INDEX` constant.
pub const AWK_INCDEC_INDEX: u16 = 1031;

// Regex
/// `AWK_PUSH_REGEXP` constant.
pub const AWK_PUSH_REGEXP: u16 = 1040;
/// `AWK_REGEX_MATCH` constant.
pub const AWK_REGEX_MATCH: u16 = 1041;
/// `AWK_REGEX_NOT_MATCH` constant.
pub const AWK_REGEX_NOT_MATCH: u16 = 1042;
/// `AWK_MATCH_REGEXP` constant.
pub const AWK_MATCH_REGEXP: u16 = 1043;

// Coercion
/// `AWK_POS` constant.
pub const AWK_POS: u16 = 1050;
/// `AWK_TO_BOOL` constant.
pub const AWK_TO_BOOL: u16 = 1051;

// Print/Printf
/// `AWK_PRINT` constant.
pub const AWK_PRINT: u16 = 1060;
/// `AWK_PRINTF` constant.
pub const AWK_PRINTF: u16 = 1061;

// Flow signals
/// `AWK_NEXT` constant.
pub const AWK_NEXT: u16 = 1070;
/// `AWK_NEXT_FILE` constant.
pub const AWK_NEXT_FILE: u16 = 1071;
/// `AWK_EXIT_CODE` constant.
pub const AWK_EXIT_CODE: u16 = 1072;
/// `AWK_EXIT_DEFAULT` constant.
pub const AWK_EXIT_DEFAULT: u16 = 1073;
/// `AWK_RETURN_EMPTY` constant.
pub const AWK_RETURN_EMPTY: u16 = 1074;

// Function calls
/// `AWK_CALL_BUILTIN` constant.
pub const AWK_CALL_BUILTIN: u16 = 1080;
/// `AWK_CALL_USER` constant.
pub const AWK_CALL_USER: u16 = 1081;
/// `AWK_CALL_INDIRECT` constant.
pub const AWK_CALL_INDIRECT: u16 = 1082;

// typeof
/// `AWK_TYPEOF_VAR` constant.
pub const AWK_TYPEOF_VAR: u16 = 1090;
/// `AWK_TYPEOF_SLOT` constant.
pub const AWK_TYPEOF_SLOT: u16 = 1091;
/// `AWK_TYPEOF_ARRAY_ELEM` constant.
pub const AWK_TYPEOF_ARRAY_ELEM: u16 = 1092;
/// `AWK_TYPEOF_FIELD` constant.
pub const AWK_TYPEOF_FIELD: u16 = 1093;
/// `AWK_TYPEOF_VALUE` constant.
pub const AWK_TYPEOF_VALUE: u16 = 1094;

// Arrays
/// `AWK_GET_ARRAY_ELEM` constant.
pub const AWK_GET_ARRAY_ELEM: u16 = 1100;
/// `AWK_SET_ARRAY_ELEM` constant.
pub const AWK_SET_ARRAY_ELEM: u16 = 1101;
/// `AWK_IN_ARRAY` constant.
pub const AWK_IN_ARRAY: u16 = 1102;
/// `AWK_DELETE_ARRAY` constant.
pub const AWK_DELETE_ARRAY: u16 = 1103;
/// `AWK_DELETE_ELEM` constant.
pub const AWK_DELETE_ELEM: u16 = 1104;
/// `AWK_JOIN_ARRAY_KEY` constant.
pub const AWK_JOIN_ARRAY_KEY: u16 = 1105;
/// `AWK_SYMTAB_KEY_COUNT` constant.
pub const AWK_SYMTAB_KEY_COUNT: u16 = 1106;

// ForIn
/// `AWK_FORIN_START` constant.
pub const AWK_FORIN_START: u16 = 1110;
/// `AWK_FORIN_NEXT` constant.
pub const AWK_FORIN_NEXT: u16 = 1111;
/// `AWK_FORIN_END` constant.
pub const AWK_FORIN_END: u16 = 1112;

// Getline
/// `AWK_GETLINE` constant.
pub const AWK_GETLINE: u16 = 1120;

// Sub/Gsub
/// `AWK_SUB_FN` constant.
pub const AWK_SUB_FN: u16 = 1130;
/// `AWK_GSUB_FN` constant.
pub const AWK_GSUB_FN: u16 = 1131;

// Split/Patsplit/Match
/// `AWK_SPLIT` constant.
pub const AWK_SPLIT: u16 = 1140;
/// `AWK_PATSPLIT` constant.
pub const AWK_PATSPLIT: u16 = 1141;
/// `AWK_MATCH_BUILTIN` constant.
pub const AWK_MATCH_BUILTIN: u16 = 1142;

// Sort
/// `AWK_ASORT` constant.
pub const AWK_ASORT: u16 = 1150;
/// `AWK_ASORTI` constant.
pub const AWK_ASORTI: u16 = 1151;

// Fused peephole ops
/// `AWK_ADD_FIELD_TO_SLOT` constant.
pub const AWK_ADD_FIELD_TO_SLOT: u16 = 1200;
/// `AWK_ADD_MUL_FIELDS_TO_SLOT` constant.
pub const AWK_ADD_MUL_FIELDS_TO_SLOT: u16 = 1201;
/// `AWK_CONCAT_POOL_STR` constant.
pub const AWK_CONCAT_POOL_STR: u16 = 1202;
/// `AWK_PRINT_FIELD_STDOUT` constant.
pub const AWK_PRINT_FIELD_STDOUT: u16 = 1203;
/// `AWK_PRINT_FIELD_SEP_FIELD` constant.
pub const AWK_PRINT_FIELD_SEP_FIELD: u16 = 1204;
/// `AWK_PRINT_THREE_FIELDS` constant.
pub const AWK_PRINT_THREE_FIELDS: u16 = 1205;
/// `AWK_PUSH_FIELD_NUM` constant.
pub const AWK_PUSH_FIELD_NUM: u16 = 1206;
/// `AWK_GET_NR` constant.
pub const AWK_GET_NR: u16 = 1207;
/// `AWK_GET_FNR` constant.
pub const AWK_GET_FNR: u16 = 1208;
/// `AWK_GET_NF` constant.
pub const AWK_GET_NF: u16 = 1209;
/// `AWK_ARRAY_FIELD_ADD_CONST` constant.
pub const AWK_ARRAY_FIELD_ADD_CONST: u16 = 1210;
/// `AWK_PUSH_NUM_DECIMAL_STR` constant.
pub const AWK_PUSH_NUM_DECIMAL_STR: u16 = 1211;
/// `AWK_ADD_SLOT_TO_SLOT` constant.
pub const AWK_ADD_SLOT_TO_SLOT: u16 = 1212;
/// `AWK_JUMP_IF_SLOT_GE_NUM` constant.
pub const AWK_JUMP_IF_SLOT_GE_NUM: u16 = 1213;

// ── BinOp encoding ──
/// `binop_to_u8` — see implementation for the contract.
pub fn binop_to_u8(op: BinOp) -> u8 {
    match op {
        BinOp::Add => 0,
        BinOp::Sub => 1,
        BinOp::Mul => 2,
        BinOp::Div => 3,
        BinOp::Mod => 4,
        BinOp::Pow => 5,
        BinOp::Concat => 6,
        BinOp::Match => 7,
        BinOp::NotMatch => 8,
        BinOp::Eq => 9,
        BinOp::Ne => 10,
        BinOp::Lt => 11,
        BinOp::Le => 12,
        BinOp::Gt => 13,
        BinOp::Ge => 14,
        BinOp::And => 15,
        BinOp::Or => 16,
    }
}

// ── RedirKind encoding ──
/// `redir_to_u8` — see implementation for the contract.
pub fn redir_to_u8(redir: RedirKind) -> u8 {
    match redir {
        RedirKind::Stdout => 0,
        RedirKind::Overwrite => 1,
        RedirKind::Append => 2,
        RedirKind::Pipe => 3,
        RedirKind::Coproc => 4,
    }
}

// ── GetlineSource encoding ──
/// `getline_source_to_u8` — see implementation for the contract.
pub fn getline_source_to_u8(source: GetlineSource) -> u8 {
    match source {
        GetlineSource::Primary => 0,
        GetlineSource::File => 1,
        GetlineSource::Coproc => 2,
        GetlineSource::Pipe => 3,
    }
}

/// Translate a single awkrs `bytecode::Op` into fusevm ops.
/// Returns a vec because some awkrs ops may expand to multiple fusevm ops.
pub fn translate_op(op: &bytecode::Op, line: u32) -> Vec<(fusevm::Op, u32)> {
    use bytecode::Op as A;
    use fusevm::Op as F;

    match op {
        // ── Direct mappings: arithmetic ──
        A::Add => vec![(F::Add, line)],
        A::Sub => vec![(F::Sub, line)],
        A::Mul => vec![(F::Mul, line)],
        A::Div => vec![(F::Div, line)],
        A::Mod => vec![(F::Mod, line)],
        A::Pow => vec![(F::Pow, line)],

        // ── Direct mappings: unary ──
        A::Neg => vec![(F::Negate, line)],
        A::Not => vec![(F::LogNot, line)],

        // ── Direct mappings: stack ──
        A::Pop => vec![(F::Pop, line)],
        A::Dup => vec![(F::Dup, line)],

        // ── Direct mappings: constants ──
        A::PushNum(f) => vec![(F::LoadFloat(*f), line)],

        // ── Direct mappings: variables ──
        A::GetVar(idx) => vec![(F::GetVar(*idx as u16), line)],
        A::SetVar(idx) => vec![(F::SetVar(*idx as u16), line)],
        A::GetSlot(slot) => vec![(F::GetSlot(*slot), line)],
        A::SetSlot(slot) => vec![(F::SetSlot(*slot), line)],

        // ── Direct mappings: control flow ──
        A::Jump(target) => vec![(F::Jump(*target), line)],
        A::JumpIfFalsePop(target) => vec![(F::JumpIfFalse(*target), line)],
        A::JumpIfTruePop(target) => vec![(F::JumpIfTrue(*target), line)],

        // ── Direct mappings: comparison (POSIX-aware → numeric for now) ──
        A::CmpEq => vec![(F::NumEq, line)],
        A::CmpNe => vec![(F::NumNe, line)],
        A::CmpLt => vec![(F::NumLt, line)],
        A::CmpLe => vec![(F::NumLe, line)],
        A::CmpGt => vec![(F::NumGt, line)],
        A::CmpGe => vec![(F::NumGe, line)],

        // ── Direct mappings: string ──
        A::Concat => vec![(F::Concat, line)],

        // ── Direct mappings: return ──
        A::ReturnVal => vec![(F::ReturnValue, line)],

        // ── AWK-specific → Extended ──
        A::GetField => vec![(F::Extended(AWK_GET_FIELD, 0), line)],
        A::SetField => vec![(F::Extended(AWK_SET_FIELD, 0), line)],
        A::PushStr(_idx) => vec![(F::Extended(AWK_PUSH_NUM_DECIMAL_STR, 0), line)], // pool string
        A::PushRegexp(_idx) => vec![(F::Extended(AWK_PUSH_REGEXP, 0), line)],
        A::PushNumDecimalStr(_idx) => vec![(F::Extended(AWK_PUSH_NUM_DECIMAL_STR, 0), line)],

        A::CompoundAssignVar(_idx, bop) => vec![(
            F::Extended(AWK_COMPOUND_ASSIGN_VAR, binop_to_u8(*bop)),
            line,
        )],
        A::CompoundAssignSlot(_slot, bop) => vec![(
            F::Extended(AWK_COMPOUND_ASSIGN_SLOT, binop_to_u8(*bop)),
            line,
        )],
        A::CompoundAssignField(bop) => vec![(
            F::Extended(AWK_COMPOUND_ASSIGN_FIELD, binop_to_u8(*bop)),
            line,
        )],
        A::CompoundAssignIndex(_idx, bop) => vec![(
            F::Extended(AWK_COMPOUND_ASSIGN_INDEX, binop_to_u8(*bop)),
            line,
        )],

        A::IncDecVar(_idx, _kind) => vec![(F::Extended(AWK_INCDEC_VAR, 0), line)],
        A::IncrVar(_idx) => vec![(F::Extended(AWK_INCR_VAR, 0), line)],
        A::DecrVar(_idx) => vec![(F::Extended(AWK_DECR_VAR, 0), line)],
        A::IncDecSlot(_slot, _kind) => vec![(F::Extended(AWK_INCDEC_SLOT, 0), line)],
        A::IncDecField(_kind) => vec![(F::Extended(AWK_INCDEC_FIELD, 0), line)],
        A::IncDecIndex(_idx, _kind) => vec![(F::Extended(AWK_INCDEC_INDEX, 0), line)],

        A::RegexMatch => vec![(F::Extended(AWK_REGEX_MATCH, 0), line)],
        A::RegexNotMatch => vec![(F::Extended(AWK_REGEX_NOT_MATCH, 0), line)],
        A::Pos => vec![(F::Extended(AWK_POS, 0), line)],
        A::ToBool => vec![(F::Extended(AWK_TO_BOOL, 0), line)],

        A::Print { argc: _, redir } => vec![(F::Extended(AWK_PRINT, redir_to_u8(*redir)), line)],
        A::Printf { argc: _, redir } => vec![(F::Extended(AWK_PRINTF, redir_to_u8(*redir)), line)],

        A::Next => vec![(F::Extended(AWK_NEXT, 0), line)],
        A::NextFile => vec![(F::Extended(AWK_NEXT_FILE, 0), line)],
        A::ExitWithCode => vec![(F::Extended(AWK_EXIT_CODE, 0), line)],
        A::ExitDefault => vec![(F::Extended(AWK_EXIT_DEFAULT, 0), line)],
        A::ReturnEmpty => vec![(F::Extended(AWK_RETURN_EMPTY, 0), line)],

        A::CallBuiltin(_name, argc) => vec![(F::Extended(AWK_CALL_BUILTIN, *argc as u8), line)],
        A::CallUser(_name, argc) => vec![(F::Extended(AWK_CALL_USER, *argc as u8), line)],
        A::CallIndirect(argc) => vec![(F::Extended(AWK_CALL_INDIRECT, *argc as u8), line)],

        A::TypeofVar(_) => vec![(F::Extended(AWK_TYPEOF_VAR, 0), line)],
        A::TypeofSlot(s) => vec![(F::Extended(AWK_TYPEOF_SLOT, *s as u8), line)],
        A::TypeofArrayElem(_) => vec![(F::Extended(AWK_TYPEOF_ARRAY_ELEM, 0), line)],
        A::TypeofField => vec![(F::Extended(AWK_TYPEOF_FIELD, 0), line)],
        A::TypeofValue => vec![(F::Extended(AWK_TYPEOF_VALUE, 0), line)],

        A::GetArrayElem(_) => vec![(F::Extended(AWK_GET_ARRAY_ELEM, 0), line)],
        A::SetArrayElem(_) => vec![(F::Extended(AWK_SET_ARRAY_ELEM, 0), line)],
        A::InArray(_) => vec![(F::Extended(AWK_IN_ARRAY, 0), line)],
        A::DeleteArray(_) => vec![(F::Extended(AWK_DELETE_ARRAY, 0), line)],
        A::DeleteElem(_) => vec![(F::Extended(AWK_DELETE_ELEM, 0), line)],
        A::JoinArrayKey(n) => vec![(F::Extended(AWK_JOIN_ARRAY_KEY, *n as u8), line)],
        A::SymtabKeyCount => vec![(F::Extended(AWK_SYMTAB_KEY_COUNT, 0), line)],

        A::ForInStart(_) => vec![(F::Extended(AWK_FORIN_START, 0), line)],
        A::ForInNext { var: _, end_jump } => {
            vec![(F::ExtendedWide(AWK_FORIN_NEXT, *end_jump), line)]
        }
        A::ForInEnd => vec![(F::Extended(AWK_FORIN_END, 0), line)],

        A::GetLine {
            var: _,
            source,
            push_result,
        } => {
            let arg = getline_source_to_u8(*source) | if *push_result { 0x10 } else { 0 };
            vec![(F::Extended(AWK_GETLINE, arg), line)]
        }

        A::SubFn(_) => vec![(F::Extended(AWK_SUB_FN, 0), line)],
        A::GsubFn(_) => vec![(F::Extended(AWK_GSUB_FN, 0), line)],

        A::Split {
            arr: _,
            has_fs,
            seps: _,
        } => vec![(F::Extended(AWK_SPLIT, if *has_fs { 1 } else { 0 }), line)],
        A::Patsplit {
            arr: _,
            has_fp,
            seps: _,
        } => vec![(F::Extended(AWK_PATSPLIT, if *has_fp { 1 } else { 0 }), line)],
        A::MatchBuiltin { arr } => vec![(
            F::Extended(AWK_MATCH_BUILTIN, if arr.is_some() { 1 } else { 0 }),
            line,
        )],

        A::Asort { src: _, dest } => vec![(
            F::Extended(AWK_ASORT, if dest.is_some() { 1 } else { 0 }),
            line,
        )],
        A::Asorti { src: _, dest } => vec![(
            F::Extended(AWK_ASORTI, if dest.is_some() { 1 } else { 0 }),
            line,
        )],

        // Catch-all for any remaining/fused ops — keep as Extended with debug info
        _ => vec![(F::Extended(0xFFFF, 0), line)], // unmapped — will trap at runtime
    }
}

/// Whether every op in `ops` is part of the universal numeric subset that
/// fusevm's interpreter + JIT can execute directly (no AWK extension
/// handler required). Returns `false` for bignum mode or an empty chunk.
///
/// This is the gate for [`build_numeric_chunk`]; the two must stay in sync.
pub fn is_fusevm_eligible<'s>(
    ops: &[bytecode::Op],
    bignum: bool,
    resolve_name: impl Fn(u32) -> &'s str,
) -> bool {
    use bytecode::Op;
    if bignum || ops.is_empty() {
        return false;
    }
    for op in ops {
        match op {
            Op::PushNum(_)
            | Op::PushNumDecimalStr(_)
            | Op::Add
            | Op::Sub
            | Op::Mul
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
            | Op::CompoundAssignSlot(_, BinOp::Pow)
            | Op::AddSlotToSlot { .. }
            | Op::JumpIfSlotGeNum { .. }
            | Op::IncDecSlot(_, _) => continue,
            // `/` and `%` lower to `fusevm::Op::AwkDivJit`/`AwkModJit`: awk-
            // semantic div/mod that trap on a zero divisor (faithful fatal) AND
            // are block-JIT-eligible via a Cranelift guarded early-exit (the
            // divisor is compared to 0.0; on zero a libcall records the trap and
            // the block returns, the VM raising the awk error). So a div/mod
            // numeric chunk now native-JITs through fusevm instead of falling to
            // an interpreter — both trapping correctly. Compound `/=`/`%=` lower
            // the same way (see `build_numeric_chunk`).
            Op::Div
            | Op::Mod
            | Op::CompoundAssignSlot(_, BinOp::Div)
            | Op::CompoundAssignSlot(_, BinOp::Mod) => continue,
            // `int(x)` lowers to a native fusevm `Op::AwkInt` (Cranelift
            // `trunc`), so the whole numeric chunk can still block/trace-JIT.
            // Only the 1-arg `int` builtin is admitted — every other builtin
            // needs host (Runtime) state and stays interpreter-side. Bignum is
            // already rejected above, matching awkrs's non-bignum
            // `Value::Num(as_number().trunc())`.
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "int" => continue,
            // `mkbool(x)` — gawk extension: 1 if x is truthy (numeric != 0,
            // including NaN/inf), else 0. Lowers to a native `Op::AwkMkbool`
            // (Cranelift `fcmp ne, 0.0` + `select`), no libcall or host state.
            // No trap path — chunk stays disk-cacheable.
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "mkbool" => continue,
            // `sqrt(x)` / `log(x)`: warn on negative arg with the gawk-style
            // "received negative argument <x>" message; non-negative path is
            // `f64::sqrt` / `f64::ln`. Lower to `Op::AwkSqrtJit` / `Op::AwkLogJit`
            // (fusevm 0.13.6+) — interpreter on fusevm now, block-JIT codegen
            // pending.
            Op::CallBuiltin(idx, 1) if matches!(resolve_name(*idx), "sqrt" | "log") => continue,
            // `lshift(a, n)` / `rshift(a, n)`: fatal "negative values are not
            // allowed" when either operand is negative; non-negative path is
            // `(a as i64) << (n & 0x3f)` / `>> (n & 0x3f)`. Lower to
            // `Op::AwkLshiftJit` / `Op::AwkRshiftJit`.
            Op::CallBuiltin(idx, 2) if matches!(resolve_name(*idx), "lshift" | "rshift") => {
                continue
            }
            // `compl(a)`: fatal "negative value is not allowed" on negative arg;
            // non-negative path is `!(a as i64)`. Lower to `Op::AwkComplJit`.
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "compl" => continue,
            // `$N` numeric field read with compile-time N. Lowers to
            // `fusevm::Op::AwkGetFieldNum(N)`, which calls the thread-local
            // host hook installed by [`crate::vm::try_fusevm_dispatch`] right
            // before each chunk invocation. The hook reads the active record's
            // field via the awkrs Runtime, so the chunk stays disk-cacheable
            // (the libcall symbol is stable; the per-invocation context flows
            // through TLS, not through the chunk bytecode).
            Op::PushFieldNum(_) => continue,
            // Transcendental math builtins lower to native fusevm libcall ops
            // (`Op::AwkSin`/`AwkCos`/`AwkExp`/`AwkAtan2`) that canonicalize NaN
            // to `+nan`, matching awkrs's `Value::Num(if r.is_nan(){NAN}else{r})`
            // sign normalization. sin/cos/exp are 1-arg; atan2 is 2-arg. sqrt/log
            // stay interpreter-side (they emit a host stderr warning on negative
            // args, which a native op cannot reproduce faithfully).
            Op::CallBuiltin(idx, 1) if matches!(resolve_name(*idx), "sin" | "cos" | "exp") => {
                continue
            }
            Op::CallBuiltin(idx, 2) if resolve_name(*idx) == "atan2" => continue,
            // `and`/`or`/`xor` are variadic (≥2 args) pure-integer bitwise folds
            // — operands truncated+saturated to i64 (matching awkrs's
            // `num_to_u64`), no host state, no value-dependent trap. They lower
            // to native fusevm `Op::AwkAnd`/`AwkOr`/`AwkXor` (Cranelift
            // band/bor/bxor) so the chunk stays block-JIT-eligible. `lshift`/
            // `rshift`/`compl` are NOT admitted: they raise a fatal on negative
            // args, which a pure native op cannot reproduce (same reason as
            // div/mod). The arg count must fit fusevm's `u8` payload.
            Op::CallBuiltin(idx, argc)
                if *argc >= 2
                    && *argc <= u8::MAX as u16
                    && matches!(resolve_name(*idx), "and" | "or" | "xor") =>
            {
                continue
            }
            _ => return false,
        }
    }
    true
}

/// Value-stack depth change of an eligible op, or `None` if the op is not in
/// the fusevm-eligible numeric subset. Mirrors the interpreter semantics in
/// `vm.rs::execute`: `SetSlot`/`CompoundAssignSlot` peek-or-replace (net 0),
/// `IncDecSlot` leaves the read value (+1), the fused slot ops touch slots
/// only (0). The admitted builtins (`int`/`sin`/`cos`/`exp` pop 1 push 1 → 0;
/// `atan2` pops 2 → -1; variadic `and`/`or`/`xor` pop `argc` push 1 → `1-argc`)
/// need `resolve_name`, so this must stay consistent with `is_fusevm_eligible`.
/// Used by [`eligible_loop_prefix`] to find statement boundaries.
fn stack_delta<'s>(op: &bytecode::Op, resolve_name: impl Fn(u32) -> &'s str) -> Option<i32> {
    use bytecode::Op;
    Some(match op {
        Op::PushNum(_)
        | Op::PushNumDecimalStr(_)
        | Op::GetSlot(_)
        | Op::PushFieldNum(_)
        | Op::Dup
        | Op::IncDecSlot(_, _) => 1,
        // Div/Mod now lower to the block-JIT-eligible `AwkDivJit`/`AwkModJit`
        // (guarded zero-divisor trap), so they are admitted here AND in
        // `is_fusevm_eligible`. Binary `/`/`%` pop two, push one (-1); compound
        // `/=`/`%=` are stack-neutral (0).
        Op::Add
        | Op::Sub
        | Op::Mul
        | Op::Pow
        | Op::Div
        | Op::Mod
        | Op::CmpEq
        | Op::CmpNe
        | Op::CmpLt
        | Op::CmpLe
        | Op::CmpGt
        | Op::CmpGe
        | Op::Pop
        | Op::JumpIfFalsePop(_)
        | Op::JumpIfTruePop(_) => -1,
        Op::Neg
        | Op::Not
        | Op::Pos
        | Op::ToBool
        | Op::SetSlot(_)
        | Op::IncrSlot(_)
        | Op::DecrSlot(_)
        | Op::AddSlotToSlot { .. }
        | Op::JumpIfSlotGeNum { .. }
        | Op::Jump(_) => 0,
        Op::CompoundAssignSlot(
            _,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Pow | BinOp::Div | BinOp::Mod,
        ) => 0,
        // Admitted numeric builtins (must match `is_fusevm_eligible`). Each
        // pops `argc` and pushes one result.
        Op::CallBuiltin(idx, 1)
            if matches!(
                resolve_name(*idx),
                "int" | "mkbool" | "sqrt" | "log" | "compl" | "sin" | "cos" | "exp"
            ) =>
        {
            0
        }
        Op::CallBuiltin(idx, 2) if matches!(resolve_name(*idx), "lshift" | "rshift") => -1,
        Op::CallBuiltin(idx, 2) if resolve_name(*idx) == "atan2" => -1,
        Op::CallBuiltin(idx, argc)
            if *argc >= 2
                && *argc <= u8::MAX as u16
                && matches!(resolve_name(*idx), "and" | "or" | "xor") =>
        {
            1 - *argc as i32
        }
        _ => return None,
    })
}

/// Find the longest *prefix* of `ops` that is (a) entirely fusevm-eligible,
/// (b) stack-neutral at its end (a statement boundary), (c) self-contained
/// (every jump target stays within the prefix), and (d) contains at least one
/// backward jump (a loop — the only case where offloading to fusevm's JIT can
/// pay for the per-dispatch setup). Returns the exclusive end index `k`, or
/// `None` if no such prefix exists.
///
/// This lets the JIT engage on compute-in-`BEGIN`/`END` chunks like
/// `BEGIN{s=0;for(i=1;i<=N;i++)s+=i;print s}` whose trailing `print` makes the
/// *whole* chunk ineligible: the numeric prefix (init + loop) runs on fusevm,
/// then the awkrs interpreter resumes at `k` for the `print`.
pub fn eligible_loop_prefix<'s>(
    ops: &[bytecode::Op],
    bignum: bool,
    resolve_name: impl Fn(u32) -> &'s str,
) -> Option<usize> {
    use bytecode::Op;
    if bignum || ops.is_empty() {
        return None;
    }
    let mut depth: i32 = 0;
    let mut last_neutral: usize = 0;
    let mut i: usize = 0;
    while i < ops.len() {
        let delta = match stack_delta(&ops[i], &resolve_name) {
            Some(d) => d,
            None => break,
        };
        depth += delta;
        if depth < 0 {
            return None;
        }
        i += 1;
        if depth == 0 {
            last_neutral = i;
        }
    }
    let k = last_neutral;
    if k == 0 {
        return None;
    }
    // Validate self-containment + require a backward jump within [0, k).
    let mut saw_backjump = false;
    for (j, op) in ops.iter().enumerate().take(k) {
        let target = match op {
            Op::Jump(t) | Op::JumpIfFalsePop(t) | Op::JumpIfTruePop(t) => Some(*t),
            Op::JumpIfSlotGeNum { target, .. } => Some(*target),
            _ => None,
        };
        if let Some(t) = target {
            if t > k {
                return None;
            }
            if t <= j {
                saw_backjump = true;
            }
        }
    }
    if !saw_backjump {
        return None;
    }
    Some(k)
}

/// Translate an eligible awkrs numeric chunk into a runnable `fusevm::Chunk`,
/// emitting a `PushFrame` + per-slot initialization preamble seeded from
/// `slot_init` (the awkrs runtime slot values coerced to `f64`).
///
/// Returns `None` when the chunk is not [`is_fusevm_eligible`] or contains an
/// op the translator does not handle. Multi-op expansions (incr/decr,
/// compound-assign) shift jump targets, so a two-pass ip-remap is performed.
///
/// Single source of truth for awkrs's numeric fusevm tier; `vm.rs`'s
/// `try_fusevm_dispatch` marshals slots in/out around this builder.
///
/// `resolve_num` maps a `PushNumDecimalStr` string-pool index to its `f64`
/// value (non-bignum mode only — eligibility rejects bignum), matching the
/// interpreter's `str.parse::<f64>().unwrap_or(0.0)`.
pub fn build_numeric_chunk<'s>(
    ops: &[bytecode::Op],
    bignum: bool,
    resolve_num: impl Fn(u32) -> f64,
    resolve_name: impl Fn(u32) -> &'s str,
) -> Option<fusevm::Chunk> {
    use crate::ast::IncDecOp;
    use bytecode::Op;

    if !is_fusevm_eligible(ops, bignum, &resolve_name) {
        return None;
    }

    // Pass 1: map each awkrs ip to its fusevm ip, accounting for multi-op
    // expansions (incr/decr, compound-assign). The chunk is *stable*: slot
    // seeds are NOT baked in as `LoadFloat` constants (that would make the
    // chunk's `op_hash` vary every record, defeating fusevm's op_hash-keyed
    // block-JIT warmup and on-disk cache). Instead the caller pre-seeds the
    // VM's base-frame slots as data, and the chunk operates on that frame
    // directly (no `PushFrame`), so identical programs hash identically and
    // the JIT-compiled native code is reused across records and processes.
    let mut ip_map: Vec<usize> = Vec::with_capacity(ops.len() + 1);
    let mut fusevm_ip = 0;
    for op in ops {
        ip_map.push(fusevm_ip);
        fusevm_ip += match op {
            Op::IncrSlot(_) | Op::DecrSlot(_) => 4,
            Op::SetSlot(_) => 2,
            Op::CompoundAssignSlot(_, BinOp::Sub | BinOp::Div | BinOp::Mod | BinOp::Pow) => 5,
            Op::CompoundAssignSlot(_, BinOp::Add | BinOp::Mul) => 4,
            Op::Not => 2,
            Op::Pos => 2,
            Op::ToBool => 2,
            Op::AddSlotToSlot { .. } => 4,
            Op::JumpIfSlotGeNum { .. } => 4,
            Op::IncDecSlot(_, _) => 5,
            _ => 1,
        };
    }
    ip_map.push(fusevm_ip); // sentinel: one past the end

    let remap = |t: usize| -> usize {
        if t < ip_map.len() {
            ip_map[t]
        } else {
            ip_map[ip_map.len() - 1]
        }
    };

    let mut builder = fusevm::ChunkBuilder::new();

    for op in ops.iter() {
        match op {
            Op::PushNum(n) => {
                builder.emit(fusevm::Op::LoadFloat(*n), 0);
            }
            Op::PushNumDecimalStr(idx) => {
                builder.emit(fusevm::Op::LoadFloat(resolve_num(*idx)), 0);
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
            // `/` and `%` lower to the trapping, block-JIT-eligible
            // AwkDivJit/AwkModJit (guarded zero-divisor early-exit in fusevm's
            // Cranelift block JIT — see `fusevm::Op::AwkDivJit`). Reachable now:
            // `is_fusevm_eligible` admits div/mod.
            Op::Div => {
                builder.emit(fusevm::Op::AwkDivJit, 0);
            }
            Op::Mod => {
                builder.emit(fusevm::Op::AwkModJit, 0);
            }
            Op::Pow => {
                builder.emit(fusevm::Op::Pow, 0);
            }
            Op::Neg => {
                builder.emit(fusevm::Op::Negate, 0);
            }
            // AWK !x returns Num(0/1): x == 0.0 yields the same for numbers.
            Op::Not => {
                builder.emit(fusevm::Op::LoadFloat(0.0), 0);
                builder.emit(fusevm::Op::NumEq, 0);
            }
            // Unary +: coerce to number (x + 0.0).
            Op::Pos => {
                builder.emit(fusevm::Op::LoadFloat(0.0), 0);
                builder.emit(fusevm::Op::Add, 0);
            }
            // Convert to 0.0/1.0: (x != 0.0).
            Op::ToBool => {
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
            // awkrs `SetSlot` PEEKS (stores top, leaves it); fusevm `SetSlot`
            // POPS. Emit `Dup; SetSlot` so the value survives, matching awkrs
            // semantics (the awkrs source pairs this with a trailing `Pop` in
            // statement context, or consumes the value in expression context).
            Op::SetSlot(s) => {
                builder.emit(fusevm::Op::Dup, 0);
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
                builder.emit(fusevm::Op::Jump(remap(*t)), 0);
            }
            Op::JumpIfFalsePop(t) => {
                builder.emit(fusevm::Op::JumpIfFalse(remap(*t)), 0);
            }
            Op::JumpIfTruePop(t) => {
                builder.emit(fusevm::Op::JumpIfTrue(remap(*t)), 0);
            }
            Op::IncrSlot(s) => {
                builder.emit(fusevm::Op::GetSlot(*s), 0);
                builder.emit(fusevm::Op::LoadFloat(1.0), 0);
                builder.emit(fusevm::Op::Add, 0);
                builder.emit(fusevm::Op::SetSlot(*s), 0);
            }
            Op::DecrSlot(s) => {
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
                builder.emit(fusevm::Op::Swap, 0);
                builder.emit(fusevm::Op::AwkDivJit, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Mod) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Swap, 0);
                builder.emit(fusevm::Op::AwkModJit, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::CompoundAssignSlot(s, BinOp::Pow) => {
                let slot = *s;
                builder.emit(fusevm::Op::GetSlot(slot), 0);
                builder.emit(fusevm::Op::Swap, 0);
                builder.emit(fusevm::Op::Pow, 0);
                builder.emit(fusevm::Op::Dup, 0);
                builder.emit(fusevm::Op::SetSlot(slot), 0);
            }
            Op::IncDecSlot(s, kind) => {
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
                builder.emit(fusevm::Op::GetSlot(*slot), 0);
                builder.emit(fusevm::Op::LoadFloat(*limit), 0);
                builder.emit(fusevm::Op::NumGe, 0);
                builder.emit(fusevm::Op::JumpIfTrue(remap(*target)), 0);
            }
            // `int(x)`: pop the f64 argument, push its truncation. Native
            // fusevm op (Cranelift `trunc`) — keeps the chunk JIT-eligible.
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "int" => {
                builder.emit(fusevm::Op::AwkInt, 0);
            }
            // `mkbool(x)`: pop the f64 argument, push 1.0 if nonzero (NaN/inf
            // included), else 0.0. Native fusevm op (Cranelift `fcmp ne, 0.0`
            // + `select`) added in fusevm 0.13.5 — disk-cacheable.
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "mkbool" => {
                builder.emit(fusevm::Op::AwkMkbool, 0);
            }
            // sqrt/log negative-arg handling lives entirely inside fusevm's
            // AwkSqrtJit/AwkLogJit (warns + NaN). Generic "awk: warning: ..."
            // text instead of the awkrs-specific "awkrs: warning: ..." prefix
            // on the JIT-eligible path — documented divergence (no existing
            // parity tests or integration tests inspect the prefix).
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "sqrt" => {
                builder.emit(fusevm::Op::AwkSqrtJit, 0);
            }
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "log" => {
                builder.emit(fusevm::Op::AwkLogJit, 0);
            }
            // lshift/rshift fatal-trap on negative; awkrs's existing message
            // format ("lshift(<a>, <n>): ...") differs from the JIT path's
            // generic "lshift: negative values are not allowed". Documented.
            Op::CallBuiltin(idx, 2) if resolve_name(*idx) == "lshift" => {
                builder.emit(fusevm::Op::AwkLshiftJit, 0);
            }
            Op::CallBuiltin(idx, 2) if resolve_name(*idx) == "rshift" => {
                builder.emit(fusevm::Op::AwkRshiftJit, 0);
            }
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "compl" => {
                builder.emit(fusevm::Op::AwkComplJit, 0);
            }
            // `$N` numeric read with compile-time N: emit `AwkGetFieldNum(N)`.
            // The active Runtime is exposed to the libcall via the thread-local
            // hook installed by `try_fusevm_dispatch`.
            Op::PushFieldNum(field) => {
                builder.emit(fusevm::Op::AwkGetFieldNum(*field), 0);
            }
            // Transcendental math: native fusevm libcall ops (NaN→`+nan`).
            // sin/cos/exp are 1-arg; atan2 is 2-arg (awkrs pushes y then x, so
            // x is on top — matches fusevm `Op::AwkAtan2`'s pop order).
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "sin" => {
                builder.emit(fusevm::Op::AwkSin, 0);
            }
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "cos" => {
                builder.emit(fusevm::Op::AwkCos, 0);
            }
            Op::CallBuiltin(idx, 1) if resolve_name(*idx) == "exp" => {
                builder.emit(fusevm::Op::AwkExp, 0);
            }
            Op::CallBuiltin(idx, 2) if resolve_name(*idx) == "atan2" => {
                builder.emit(fusevm::Op::AwkAtan2, 0);
            }
            // Bitwise and/or/xor: variadic (≥2 args), pure integer fold, no host
            // state and no trap → native fusevm fold ops (block-JIT-eligible).
            Op::CallBuiltin(idx, argc)
                if *argc >= 2
                    && *argc <= u8::MAX as u16
                    && matches!(resolve_name(*idx), "and" | "or" | "xor") =>
            {
                let n = *argc as u8;
                let fop = match resolve_name(*idx) {
                    "and" => fusevm::Op::AwkAnd(n),
                    "or" => fusevm::Op::AwkOr(n),
                    _ => fusevm::Op::AwkXor(n),
                };
                builder.emit(fop, 0);
            }
            _ => return None,
        }
    }

    Some(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    #[test]
    fn translate_arithmetic_ops() {
        let ops = translate_op(&Op::Add, 1);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0].0, fusevm::Op::Add));
    }

    #[test]
    fn translate_push_num() {
        let ops = translate_op(&Op::PushNum(42.0), 1);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0].0, fusevm::Op::LoadFloat(n) if n == 42.0));
    }

    #[test]
    fn translate_compound_assign_field() {
        let ops = translate_op(&Op::CompoundAssignField(BinOp::Add), 1);
        assert_eq!(ops.len(), 1);
        match ops[0].0 {
            fusevm::Op::Extended(id, arg) => {
                assert_eq!(id, AWK_COMPOUND_ASSIGN_FIELD);
                assert_eq!(arg, binop_to_u8(BinOp::Add));
            }
            _ => panic!("Expected Extended op"),
        }
    }

    #[test]
    fn binop_encoding_values() {
        assert_eq!(binop_to_u8(BinOp::Add), 0);
        assert_eq!(binop_to_u8(BinOp::Sub), 1);
        assert_eq!(binop_to_u8(BinOp::Or), 16);
    }

    #[test]
    fn translate_pop_dup_v2() {
        assert!(matches!(translate_op(&Op::Pop, 1)[0].0, fusevm::Op::Pop));
        assert!(matches!(translate_op(&Op::Dup, 1)[0].0, fusevm::Op::Dup));
    }

    #[test]
    fn translate_control_flow_v2() {
        assert!(matches!(
            translate_op(&Op::Jump(10), 1)[0].0,
            fusevm::Op::Jump(10)
        ));
        assert!(matches!(
            translate_op(&Op::JumpIfFalsePop(20), 1)[0].0,
            fusevm::Op::JumpIfFalse(20)
        ));
    }

    #[test]
    fn translate_comparisons_v2() {
        assert!(matches!(
            translate_op(&Op::CmpEq, 1)[0].0,
            fusevm::Op::NumEq
        ));
        assert!(matches!(
            translate_op(&Op::CmpLt, 1)[0].0,
            fusevm::Op::NumLt
        ));
    }

    #[test]
    fn redir_encoding_v2() {
        assert_eq!(redir_to_u8(RedirKind::Stdout), 0);
        assert_eq!(redir_to_u8(RedirKind::Overwrite), 1);
    }

    #[test]
    fn getline_source_encoding_v2() {
        assert_eq!(getline_source_to_u8(GetlineSource::Primary), 0);
        assert_eq!(getline_source_to_u8(GetlineSource::File), 1);
    }

    #[test]
    fn translate_math_v28() {
        assert!(matches!(translate_op(&Op::Add, 1)[0].0, fusevm::Op::Add));
    }
    #[test]
    fn translate_neg_v28() {
        assert!(matches!(translate_op(&Op::Neg, 1)[0].0, fusevm::Op::Negate));
    }

    #[test]
    fn numeric_chunk_rejects_non_universal_ops() {
        // PushStr is outside the universal numeric subset → not eligible.
        assert!(build_numeric_chunk(&[Op::PushStr(0)], false, |_| 0.0, |_| "").is_none());
    }

    #[test]
    fn numeric_chunk_rejects_bignum() {
        assert!(build_numeric_chunk(&[Op::Add], true, |_| 0.0, |_| "").is_none());
    }

    #[test]
    fn numeric_chunk_executes_slot_increment() {
        // slot0 seeded to 5.0 (as VM data); IncrSlot(0) → 6.0; GetSlot(0) leaves
        // 6.0 on stack. The chunk no longer self-seeds — the caller seeds the
        // base-frame slot so the chunk's op_hash stays stable across records.
        let chunk = build_numeric_chunk(&[Op::IncrSlot(0), Op::GetSlot(0)], false, |_| 0.0, |_| "")
            .unwrap();
        let mut vm = fusevm::VM::new(chunk);
        vm.set_slot(0, fusevm::Value::Float(5.0));
        match vm.run() {
            fusevm::VMResult::Ok(fusevm::Value::Float(f)) => assert_eq!(f, 6.0),
            fusevm::VMResult::Ok(fusevm::Value::Int(n)) => assert_eq!(n, 6),
            other => panic!("expected Ok(6.0), got {other:?}"),
        }
    }

    // Regression: awkrs `SetSlot` PEEKS (leaves the value) and is paired with a
    // trailing `Pop` in statement context, but fusevm `SetSlot` POPS. The bridge
    // must emit `Dup; SetSlot` so the value survives for the trailing `Pop`;
    // emitting a bare `SetSlot` underflowed the stack (silently tolerated by the
    // fusevm interpreter's saturating pop, but rejected by the strict block JIT).
    #[test]
    fn numeric_chunk_setslot_peeks_via_dup() {
        // `x = 7` as a statement: PushNum(7); SetSlot(0); Pop. Then read it back.
        let chunk = build_numeric_chunk(
            &[Op::PushNum(7.0), Op::SetSlot(0), Op::Pop, Op::GetSlot(0)],
            false,
            |_| 0.0,
            |_| "",
        )
        .unwrap();

        // The awkrs `SetSlot` must lower to `Dup` immediately followed by
        // fusevm `SetSlot` (peek semantics), not a bare `SetSlot`.
        let dup_then_set = chunk
            .ops
            .windows(2)
            .any(|w| matches!(w[0], fusevm::Op::Dup) && matches!(w[1], fusevm::Op::SetSlot(0)));
        assert!(
            dup_then_set,
            "SetSlot must lower to `Dup; SetSlot`, ops = {:?}",
            chunk.ops
        );

        // And it must execute without underflow, leaving slot 0 = 7.0 on top.
        let mut vm = fusevm::VM::new(chunk);
        vm.set_slot(0, fusevm::Value::Float(0.0));
        match vm.run() {
            fusevm::VMResult::Ok(fusevm::Value::Float(f)) => assert_eq!(f, 7.0),
            fusevm::VMResult::Ok(fusevm::Value::Int(n)) => assert_eq!(n, 7),
            other => panic!("expected Ok(7.0), got {other:?}"),
        }
    }

    // `int(x)` is the one builtin admitted into the numeric chunk: it lowers to
    // the native fusevm `Op::AwkInt` (Cranelift `trunc`) so an `int()`-bearing
    // numeric loop stays block/trace-JIT-eligible and cacheable.
    #[test]
    fn numeric_chunk_lowers_int_builtin_to_awk_int() {
        // `int(3.7)` → PushNum(3.7); CallBuiltin("int", 1). Name idx 0 → "int".
        let chunk = build_numeric_chunk(
            &[Op::PushNum(3.7), Op::CallBuiltin(0, 1)],
            false,
            |_| 0.0,
            |_| "int",
        )
        .unwrap();
        assert!(
            chunk.ops.iter().any(|o| matches!(o, fusevm::Op::AwkInt)),
            "int() must lower to fusevm::Op::AwkInt, ops = {:?}",
            chunk.ops
        );
        let mut vm = fusevm::VM::new(chunk);
        match vm.run() {
            // Truncation toward zero; result is an integral value.
            fusevm::VMResult::Ok(fusevm::Value::Float(f)) => assert_eq!(f, 3.0),
            fusevm::VMResult::Ok(fusevm::Value::Int(n)) => assert_eq!(n, 3),
            other => panic!("expected int(3.7)==3, got {other:?}"),
        }
    }

    // A builtin other than `int` (e.g. `sqrt`, which needs host warning state)
    // must keep the chunk ineligible so it stays on the awkrs interpreter.
    // (As of fusevm 0.13.6, sqrt/log/lshift/rshift/compl ARE admitted via the
    //  AwkSqrtJit / AwkLogJit / AwkLshiftJit / AwkRshiftJit / AwkComplJit ops,
    //  so this regression test now uses `length` — a string-touching builtin
    //  that's never going to be JIT-lowered.)
    #[test]
    fn numeric_chunk_rejects_non_int_builtin() {
        assert!(build_numeric_chunk(
            &[Op::PushNum(4.0), Op::CallBuiltin(0, 1)],
            false,
            |_| 0.0,
            |_| "length",
        )
        .is_none());
    }

    // `$N` numeric reads (Op::PushFieldNum) are admitted into fusevm chunks
    // since fusevm 0.13.9. The bridge translates them to
    // `fusevm::Op::AwkGetFieldNum(N)`, which the active dispatch wires to a
    // thread-local hook over the awkrs Runtime. Verify both the eligibility
    // gate and the emit pattern. Sum-of-three-fields is the canonical hot
    // pattern (`{ sum = $1 + $2 + $3 }`) — previously force-dropped, now
    // stays JIT-eligible.
    #[test]
    fn field_num_chunk_lowers_to_awk_get_field_num() {
        let ops = &[
            Op::PushFieldNum(1),
            Op::PushFieldNum(2),
            Op::Add,
            Op::PushFieldNum(3),
            Op::Add,
            Op::SetSlot(0),
        ];
        assert!(is_fusevm_eligible(ops, false, |_| ""));
        let chunk = build_numeric_chunk(ops, false, |_| 0.0, |_| "").unwrap();
        let n_field_reads = chunk
            .ops
            .iter()
            .filter(|o| matches!(o, fusevm::Op::AwkGetFieldNum(_)))
            .count();
        assert_eq!(n_field_reads, 3);
        // Field indices preserved in order.
        let indices: Vec<u16> = chunk
            .ops
            .iter()
            .filter_map(|o| match o {
                fusevm::Op::AwkGetFieldNum(i) => Some(*i),
                _ => None,
            })
            .collect();
        assert_eq!(indices, vec![1, 2, 3]);
    }

    // `/`, `%`, `/=`, `%=` chunks ARE now offloaded to fusevm: they lower to the
    // trapping, block-JIT-eligible AwkDivJit/AwkModJit (guarded zero-divisor
    // early-exit in fusevm's Cranelift block JIT). `is_fusevm_eligible` admits
    // them and `build_numeric_chunk` emits AwkDivJit/AwkModJit; the chunk still
    // raises the POSIX fatal on a zero divisor (verified via the VM below).
    #[test]
    fn div_mod_chunks_lower_to_awk_jit_ops() {
        // Binary `/` and `%` are eligible and lower to AwkDivJit/AwkModJit.
        let div = &[Op::PushNum(2.0), Op::Div];
        let mod_ = &[Op::PushNum(2.0), Op::Mod];
        assert!(is_fusevm_eligible(div, false, |_| ""));
        assert!(is_fusevm_eligible(mod_, false, |_| ""));

        let dchunk = build_numeric_chunk(div, false, |_| 0.0, |_| "").unwrap();
        assert!(
            dchunk
                .ops
                .iter()
                .any(|o| matches!(o, fusevm::Op::AwkDivJit)),
            "`/` must lower to AwkDivJit, ops = {:?}",
            dchunk.ops
        );
        let mchunk = build_numeric_chunk(mod_, false, |_| 0.0, |_| "").unwrap();
        assert!(
            mchunk
                .ops
                .iter()
                .any(|o| matches!(o, fusevm::Op::AwkModJit)),
            "`%` must lower to AwkModJit, ops = {:?}",
            mchunk.ops
        );

        // Compound `/=` and `%=` are eligible and lower with `Swap; AwkDivJit`
        // / `Swap; AwkModJit` (so the op computes `slot OP rhs`, not reversed).
        let cdiv = &[
            Op::PushNum(4.0),
            Op::CompoundAssignSlot(0, BinOp::Div),
            Op::Pop,
        ];
        let cmod = &[
            Op::PushNum(4.0),
            Op::CompoundAssignSlot(0, BinOp::Mod),
            Op::Pop,
        ];
        assert!(is_fusevm_eligible(cdiv, false, |_| ""));
        assert!(is_fusevm_eligible(cmod, false, |_| ""));
        let cdchunk = build_numeric_chunk(cdiv, false, |_| 0.0, |_| "").unwrap();
        assert!(
            cdchunk
                .ops
                .windows(2)
                .any(|w| matches!(w[0], fusevm::Op::Swap) && matches!(w[1], fusevm::Op::AwkDivJit)),
            "`/=` must lower to `Swap; AwkDivJit`, ops = {:?}",
            cdchunk.ops
        );

        // A pure mul/pow chunk (no div/mod) stays eligible + offloads.
        let mul = &[
            Op::PushNum(2.0),
            Op::CompoundAssignSlot(0, BinOp::Mul),
            Op::Pop,
        ];
        assert!(is_fusevm_eligible(mul, false, |_| ""));
        assert!(build_numeric_chunk(mul, false, |_| 0.0, |_| "").is_some());
    }

    // A div-by-zero numeric chunk lowered to AwkDivJit raises the POSIX fatal in
    // the fusevm interpreter (the block JIT's guarded early-exit has the same
    // observable behavior; this exercises the interpreter arm directly).
    #[test]
    fn div_by_zero_chunk_traps_in_fusevm() {
        // slot0 / 0.0 → AwkDivJit with a zero divisor.
        let chunk = build_numeric_chunk(
            &[Op::GetSlot(0), Op::PushNum(0.0), Op::Div, Op::Pop],
            false,
            |_| 0.0,
            |_| "",
        )
        .unwrap();
        let mut vm = fusevm::VM::new(chunk);
        vm.set_slot(0, fusevm::Value::Float(1.0));
        match vm.run() {
            fusevm::VMResult::Error(msg) => {
                assert!(msg.contains("division by zero"), "unexpected error: {msg}")
            }
            other => panic!("expected div-by-zero Error, got {other:?}"),
        }
    }
}
