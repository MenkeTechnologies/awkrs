//! Bridge between awkrs's bytecode [`Op`] and fusevm's [`fusevm::Op`].
//!
//! Translates an awkrs [`Chunk`] into a fusevm [`fusevm::Chunk`], mapping:
//! - Universal ops (arithmetic, comparison, control flow, slots, stack) → direct fusevm ops
//! - AWK-specific ops (fields, arrays, regex, print, getline, etc.) → `fusevm::Op::Extended`
//!
//! This allows awkrs to share fusevm's interpreter and Cranelift JIT for the
//! universal hot path while keeping AWK semantics in extension handlers.

use crate::ast::BinOp;
use crate::bytecode::{self, GetlineSource, RedirKind, SubTarget};

// ── AWK extension op IDs for fusevm::Op::Extended(id, arg) ──

// Fields
pub const AWK_GET_FIELD: u16 = 1000;
pub const AWK_SET_FIELD: u16 = 1001;
pub const AWK_COMPOUND_ASSIGN_FIELD: u16 = 1002;
pub const AWK_INCDEC_FIELD: u16 = 1003;

// Variables (HashMap path, not slotted)
pub const AWK_COMPOUND_ASSIGN_VAR: u16 = 1010;
pub const AWK_INCDEC_VAR: u16 = 1011;
pub const AWK_INCR_VAR: u16 = 1012;
pub const AWK_DECR_VAR: u16 = 1013;

// Slot compound/incdec
pub const AWK_COMPOUND_ASSIGN_SLOT: u16 = 1020;
pub const AWK_INCDEC_SLOT: u16 = 1021;

// Array compound/incdec
pub const AWK_COMPOUND_ASSIGN_INDEX: u16 = 1030;
pub const AWK_INCDEC_INDEX: u16 = 1031;

// Regex
pub const AWK_PUSH_REGEXP: u16 = 1040;
pub const AWK_REGEX_MATCH: u16 = 1041;
pub const AWK_REGEX_NOT_MATCH: u16 = 1042;
pub const AWK_MATCH_REGEXP: u16 = 1043;

// Coercion
pub const AWK_POS: u16 = 1050;
pub const AWK_TO_BOOL: u16 = 1051;

// Print/Printf
pub const AWK_PRINT: u16 = 1060;
pub const AWK_PRINTF: u16 = 1061;

// Flow signals
pub const AWK_NEXT: u16 = 1070;
pub const AWK_NEXT_FILE: u16 = 1071;
pub const AWK_EXIT_CODE: u16 = 1072;
pub const AWK_EXIT_DEFAULT: u16 = 1073;
pub const AWK_RETURN_EMPTY: u16 = 1074;

// Function calls
pub const AWK_CALL_BUILTIN: u16 = 1080;
pub const AWK_CALL_USER: u16 = 1081;
pub const AWK_CALL_INDIRECT: u16 = 1082;

// typeof
pub const AWK_TYPEOF_VAR: u16 = 1090;
pub const AWK_TYPEOF_SLOT: u16 = 1091;
pub const AWK_TYPEOF_ARRAY_ELEM: u16 = 1092;
pub const AWK_TYPEOF_FIELD: u16 = 1093;
pub const AWK_TYPEOF_VALUE: u16 = 1094;

// Arrays
pub const AWK_GET_ARRAY_ELEM: u16 = 1100;
pub const AWK_SET_ARRAY_ELEM: u16 = 1101;
pub const AWK_IN_ARRAY: u16 = 1102;
pub const AWK_DELETE_ARRAY: u16 = 1103;
pub const AWK_DELETE_ELEM: u16 = 1104;
pub const AWK_JOIN_ARRAY_KEY: u16 = 1105;
pub const AWK_SYMTAB_KEY_COUNT: u16 = 1106;

// ForIn
pub const AWK_FORIN_START: u16 = 1110;
pub const AWK_FORIN_NEXT: u16 = 1111;
pub const AWK_FORIN_END: u16 = 1112;

// Getline
pub const AWK_GETLINE: u16 = 1120;

// Sub/Gsub
pub const AWK_SUB_FN: u16 = 1130;
pub const AWK_GSUB_FN: u16 = 1131;

// Split/Patsplit/Match
pub const AWK_SPLIT: u16 = 1140;
pub const AWK_PATSPLIT: u16 = 1141;
pub const AWK_MATCH_BUILTIN: u16 = 1142;

// Sort
pub const AWK_ASORT: u16 = 1150;
pub const AWK_ASORTI: u16 = 1151;

// Fused peephole ops
pub const AWK_ADD_FIELD_TO_SLOT: u16 = 1200;
pub const AWK_ADD_MUL_FIELDS_TO_SLOT: u16 = 1201;
pub const AWK_CONCAT_POOL_STR: u16 = 1202;
pub const AWK_PRINT_FIELD_STDOUT: u16 = 1203;
pub const AWK_PRINT_FIELD_SEP_FIELD: u16 = 1204;
pub const AWK_PRINT_THREE_FIELDS: u16 = 1205;
pub const AWK_PUSH_FIELD_NUM: u16 = 1206;
pub const AWK_GET_NR: u16 = 1207;
pub const AWK_GET_FNR: u16 = 1208;
pub const AWK_GET_NF: u16 = 1209;
pub const AWK_ARRAY_FIELD_ADD_CONST: u16 = 1210;
pub const AWK_PUSH_NUM_DECIMAL_STR: u16 = 1211;
pub const AWK_ADD_SLOT_TO_SLOT: u16 = 1212;
pub const AWK_JUMP_IF_SLOT_GE_NUM: u16 = 1213;

// ── BinOp encoding ──

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

pub fn getline_source_to_u8(source: GetlineSource) -> u8 {
    match source {
        GetlineSource::Primary => 0,
        GetlineSource::File => 1,
        GetlineSource::Coproc => 2,
        GetlineSource::Pipe => 3,
    }
}

/// Translate a single awkrs [`bytecode::Op`] into fusevm ops.
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
        A::PushStr(idx) => vec![(F::Extended(AWK_PUSH_NUM_DECIMAL_STR, 0), line)], // pool string
        A::PushRegexp(idx) => vec![(F::Extended(AWK_PUSH_REGEXP, 0), line)],
        A::PushNumDecimalStr(idx) => vec![(F::Extended(AWK_PUSH_NUM_DECIMAL_STR, 0), line)],

        A::CompoundAssignVar(idx, bop) => vec![(F::Extended(AWK_COMPOUND_ASSIGN_VAR, binop_to_u8(*bop)), line)],
        A::CompoundAssignSlot(slot, bop) => vec![(F::Extended(AWK_COMPOUND_ASSIGN_SLOT, binop_to_u8(*bop)), line)],
        A::CompoundAssignField(bop) => vec![(F::Extended(AWK_COMPOUND_ASSIGN_FIELD, binop_to_u8(*bop)), line)],
        A::CompoundAssignIndex(idx, bop) => vec![(F::Extended(AWK_COMPOUND_ASSIGN_INDEX, binop_to_u8(*bop)), line)],

        A::IncDecVar(idx, kind) => vec![(F::Extended(AWK_INCDEC_VAR, 0), line)],
        A::IncrVar(idx) => vec![(F::Extended(AWK_INCR_VAR, 0), line)],
        A::DecrVar(idx) => vec![(F::Extended(AWK_DECR_VAR, 0), line)],
        A::IncDecSlot(slot, kind) => vec![(F::Extended(AWK_INCDEC_SLOT, 0), line)],
        A::IncDecField(kind) => vec![(F::Extended(AWK_INCDEC_FIELD, 0), line)],
        A::IncDecIndex(idx, kind) => vec![(F::Extended(AWK_INCDEC_INDEX, 0), line)],

        A::RegexMatch => vec![(F::Extended(AWK_REGEX_MATCH, 0), line)],
        A::RegexNotMatch => vec![(F::Extended(AWK_REGEX_NOT_MATCH, 0), line)],
        A::Pos => vec![(F::Extended(AWK_POS, 0), line)],
        A::ToBool => vec![(F::Extended(AWK_TO_BOOL, 0), line)],

        A::Print { argc, redir } => vec![(F::Extended(AWK_PRINT, redir_to_u8(*redir)), line)],
        A::Printf { argc, redir } => vec![(F::Extended(AWK_PRINTF, redir_to_u8(*redir)), line)],

        A::Next => vec![(F::Extended(AWK_NEXT, 0), line)],
        A::NextFile => vec![(F::Extended(AWK_NEXT_FILE, 0), line)],
        A::ExitWithCode => vec![(F::Extended(AWK_EXIT_CODE, 0), line)],
        A::ExitDefault => vec![(F::Extended(AWK_EXIT_DEFAULT, 0), line)],
        A::ReturnEmpty => vec![(F::Extended(AWK_RETURN_EMPTY, 0), line)],

        A::CallBuiltin(name, argc) => vec![(F::Extended(AWK_CALL_BUILTIN, *argc as u8), line)],
        A::CallUser(name, argc) => vec![(F::Extended(AWK_CALL_USER, *argc as u8), line)],
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
        A::ForInNext { var, end_jump } => vec![(F::ExtendedWide(AWK_FORIN_NEXT, *end_jump), line)],
        A::ForInEnd => vec![(F::Extended(AWK_FORIN_END, 0), line)],

        A::GetLine { var, source, push_result } => {
            let arg = getline_source_to_u8(*source) | if *push_result { 0x10 } else { 0 };
            vec![(F::Extended(AWK_GETLINE, arg), line)]
        }

        A::SubFn(_) => vec![(F::Extended(AWK_SUB_FN, 0), line)],
        A::GsubFn(_) => vec![(F::Extended(AWK_GSUB_FN, 0), line)],

        A::Split { arr, has_fs } => vec![(F::Extended(AWK_SPLIT, if *has_fs { 1 } else { 0 }), line)],
        A::Patsplit { arr, has_fp, seps } => vec![(F::Extended(AWK_PATSPLIT, if *has_fp { 1 } else { 0 }), line)],
        A::MatchBuiltin { arr } => vec![(F::Extended(AWK_MATCH_BUILTIN, if arr.is_some() { 1 } else { 0 }), line)],

        A::Asort { src, dest } => vec![(F::Extended(AWK_ASORT, if dest.is_some() { 1 } else { 0 }), line)],
        A::Asorti { src, dest } => vec![(F::Extended(AWK_ASORTI, if dest.is_some() { 1 } else { 0 }), line)],

        // Catch-all for any remaining/fused ops — keep as Extended with debug info
        _ => vec![(F::Extended(0xFFFF, 0), line)], // unmapped — will trap at runtime
    }
}
