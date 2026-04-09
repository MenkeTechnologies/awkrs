//! Compile an AST [`Program`] into a [`CompiledProgram`] of flat bytecode.

use crate::ast::*;
use crate::bytecode::*;
use std::collections::{HashMap, HashSet};

/// Variables with special awk semantics — accessed by Runtime methods or computed
/// from Runtime fields. These bypass the slot system and use the HashMap path.
const SPECIAL_VARS: &[&str] = &[
    "NR", "FNR", "NF", "FILENAME", "FS", "OFS", "ORS", "SUBSEP", "OFMT", "FPAT", "RSTART",
    "RLENGTH", "ENVIRON", "ARGC", "ARGV",
];

/// Loop or `switch` — both support `break`; only loops support `continue`.
enum StructuralKind {
    Loop {
        break_patches: Vec<usize>,
        continue_patches: Vec<usize>,
    },
    Switch {
        break_patches: Vec<usize>,
    },
}

pub struct Compiler {
    pub strings: StringPool,
    structural_stack: Vec<StructuralKind>,
    /// Variable name → slot index (only non-special, non-array scalars).
    var_slots: HashMap<String, u16>,
    next_slot: u16,
    /// Names used in array contexts anywhere in the program (excluded from slots).
    array_names: HashSet<String>,
    /// Parameter names of the function currently being compiled (if any).
    current_func_params: HashSet<String>,
}

impl Compiler {
    pub fn compile_program(prog: &Program) -> CompiledProgram {
        // Pre-pass: collect all names used in array contexts.
        let array_names = collect_array_names(prog);

        let mut c = Compiler {
            strings: StringPool::default(),
            structural_stack: Vec::new(),
            var_slots: HashMap::new(),
            next_slot: 0,
            array_names,
            current_func_params: HashSet::new(),
        };

        let mut begin_chunks = Vec::new();
        let mut end_chunks = Vec::new();
        let mut beginfile_chunks = Vec::new();
        let mut endfile_chunks = Vec::new();
        let mut record_rules = Vec::new();

        for (i, rule) in prog.rules.iter().enumerate() {
            match &rule.pattern {
                Pattern::Begin => begin_chunks.push(c.compile_chunk(&rule.stmts)),
                Pattern::End => end_chunks.push(c.compile_chunk(&rule.stmts)),
                Pattern::BeginFile => beginfile_chunks.push(c.compile_chunk(&rule.stmts)),
                Pattern::EndFile => endfile_chunks.push(c.compile_chunk(&rule.stmts)),
                pat => {
                    let cpat = c.compile_pattern(pat);
                    let body = c.compile_chunk(&rule.stmts);
                    record_rules.push(CompiledRule {
                        pattern: cpat,
                        body,
                        original_index: i,
                    });
                }
            }
        }

        let mut functions = HashMap::new();
        for (name, fd) in &prog.funcs {
            // Set current function params so the compiler uses GetVar for them.
            c.current_func_params = fd.params.iter().cloned().collect();
            let body = c.compile_chunk(&fd.body);
            c.current_func_params.clear();
            functions.insert(
                name.clone(),
                CompiledFunc {
                    params: fd.params.clone(),
                    body,
                },
            );
        }

        let slot_count = c.next_slot;
        let mut slot_names = vec![String::new(); slot_count as usize];
        let mut slot_map = HashMap::new();
        for (name, &idx) in &c.var_slots {
            slot_names[idx as usize] = name.clone();
            slot_map.insert(name.clone(), idx);
        }

        CompiledProgram {
            begin_chunks,
            end_chunks,
            beginfile_chunks,
            endfile_chunks,
            record_rules,
            functions,
            strings: c.strings,
            slot_count,
            slot_names,
            slot_map,
        }
    }

    /// Get or assign a slot index for a variable. Returns `None` for specials,
    /// array names, and variables that are parameters of the current function.
    fn var_slot(&mut self, name: &str) -> Option<u16> {
        if SPECIAL_VARS.contains(&name) {
            return None;
        }
        if self.array_names.contains(name) {
            return None;
        }
        if self.current_func_params.contains(name) {
            return None;
        }
        if let Some(&idx) = self.var_slots.get(name) {
            return Some(idx);
        }
        let idx = self.next_slot;
        self.next_slot += 1;
        self.var_slots.insert(name.to_string(), idx);
        Some(idx)
    }

    fn compile_chunk(&mut self, stmts: &[Stmt]) -> Chunk {
        let mut ops = Vec::new();
        self.compile_stmts(stmts, &mut ops);
        peephole_optimize(&mut ops);
        Chunk::from_ops(ops)
    }

    fn compile_pattern(&mut self, pat: &Pattern) -> CompiledPattern {
        match pat {
            Pattern::Empty => CompiledPattern::Always,
            Pattern::Regexp(re) => {
                let idx = self.strings.intern(re);
                if is_literal_regex(re) {
                    CompiledPattern::LiteralRegexp(idx)
                } else {
                    CompiledPattern::Regexp(idx)
                }
            }
            Pattern::Expr(e) => {
                let mut ops = Vec::new();
                self.compile_expr(e, &mut ops);
                peephole_optimize(&mut ops);
                CompiledPattern::Expr(Chunk::from_ops(ops))
            }
            Pattern::Range(_, _) => CompiledPattern::Range,
            Pattern::Begin | Pattern::End | Pattern::BeginFile | Pattern::EndFile => {
                CompiledPattern::Always
            }
        }
    }

    // ── Statements ──────────────────────────────────────────────────────────

    fn compile_stmts(&mut self, stmts: &[Stmt], ops: &mut Vec<Op>) {
        for s in stmts {
            self.compile_stmt(s, ops);
        }
    }

    fn compile_stmt(&mut self, stmt: &Stmt, ops: &mut Vec<Op>) {
        match stmt {
            Stmt::Expr(e) => {
                self.compile_expr(e, ops);
                ops.push(Op::Pop);
            }

            Stmt::Print { args, redir } => {
                self.compile_print(args, redir, false, ops);
            }

            Stmt::Printf { args, redir } => {
                self.compile_print(args, redir, true, ops);
            }

            Stmt::If { cond, then_, else_ } => {
                self.compile_expr(cond, ops);
                let jump_else = ops.len();
                ops.push(Op::JumpIfFalsePop(0));

                self.compile_stmts(then_, ops);

                if else_.is_empty() {
                    let after = ops.len();
                    ops[jump_else] = Op::JumpIfFalsePop(after);
                } else {
                    let jump_end = ops.len();
                    ops.push(Op::Jump(0));
                    let else_start = ops.len();
                    ops[jump_else] = Op::JumpIfFalsePop(else_start);
                    self.compile_stmts(else_, ops);
                    let after = ops.len();
                    ops[jump_end] = Op::Jump(after);
                }
            }

            Stmt::While { cond, body } => {
                let loop_start = ops.len();
                self.structural_stack.push(StructuralKind::Loop {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                self.compile_expr(cond, ops);
                let cond_jump = ops.len();
                ops.push(Op::JumpIfFalsePop(0));

                self.compile_stmts(body, ops);

                ops.push(Op::Jump(loop_start));
                let after_loop = ops.len();
                ops[cond_jump] = Op::JumpIfFalsePop(after_loop);

                let info = match self.structural_stack.pop().unwrap() {
                    StructuralKind::Loop {
                        break_patches,
                        continue_patches,
                    } => (break_patches, continue_patches),
                    StructuralKind::Switch { .. } => unreachable!(),
                };
                for pos in info.0 {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.1 {
                    ops[pos] = Op::Jump(loop_start);
                }
            }

            Stmt::DoWhile { body, cond } => {
                let body_start = ops.len();
                self.structural_stack.push(StructuralKind::Loop {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                self.compile_stmts(body, ops);

                let cond_start = ops.len();
                self.compile_expr(cond, ops);
                ops.push(Op::JumpIfTruePop(body_start));

                let after_loop = ops.len();

                let info = match self.structural_stack.pop().unwrap() {
                    StructuralKind::Loop {
                        break_patches,
                        continue_patches,
                    } => (break_patches, continue_patches),
                    StructuralKind::Switch { .. } => unreachable!(),
                };
                for pos in info.0 {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.1 {
                    ops[pos] = Op::Jump(cond_start);
                }
            }

            Stmt::ForC {
                init,
                cond,
                iter,
                body,
            } => {
                if let Some(e) = init {
                    self.compile_expr(e, ops);
                    ops.push(Op::Pop);
                }

                let loop_start = ops.len();
                self.structural_stack.push(StructuralKind::Loop {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                let mut cond_jump = None;
                if let Some(c) = cond {
                    self.compile_expr(c, ops);
                    cond_jump = Some(ops.len());
                    ops.push(Op::JumpIfFalsePop(0));
                }

                self.compile_stmts(body, ops);

                let continue_target = ops.len();
                if let Some(it) = iter {
                    self.compile_expr(it, ops);
                    ops.push(Op::Pop);
                }
                ops.push(Op::Jump(loop_start));

                let after_loop = ops.len();
                if let Some(cj) = cond_jump {
                    ops[cj] = Op::JumpIfFalsePop(after_loop);
                }

                let info = match self.structural_stack.pop().unwrap() {
                    StructuralKind::Loop {
                        break_patches,
                        continue_patches,
                    } => (break_patches, continue_patches),
                    StructuralKind::Switch { .. } => unreachable!(),
                };
                for pos in info.0 {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.1 {
                    ops[pos] = Op::Jump(continue_target);
                }
            }

            Stmt::ForIn { var, arr, body } => {
                let arr_idx = self.strings.intern(arr);
                let var_idx = self.strings.intern(var);

                ops.push(Op::ForInStart(arr_idx));

                let loop_top = ops.len();
                self.structural_stack.push(StructuralKind::Loop {
                    break_patches: Vec::new(),
                    continue_patches: Vec::new(),
                });

                let next_pos = ops.len();
                ops.push(Op::ForInNext {
                    var: var_idx,
                    end_jump: 0,
                });

                self.compile_stmts(body, ops);
                ops.push(Op::Jump(loop_top));

                let after_loop = ops.len();
                ops.push(Op::ForInEnd);
                let cleanup_done = ops.len();

                ops[next_pos] = Op::ForInNext {
                    var: var_idx,
                    end_jump: after_loop,
                };

                let info = match self.structural_stack.pop().unwrap() {
                    StructuralKind::Loop {
                        break_patches,
                        continue_patches,
                    } => (break_patches, continue_patches),
                    StructuralKind::Switch { .. } => unreachable!(),
                };
                for pos in info.0 {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.1 {
                    ops[pos] = Op::Jump(loop_top);
                }
                let _ = cleanup_done;
            }

            Stmt::Block(stmts) => {
                self.compile_stmts(stmts, ops);
            }

            Stmt::Break => {
                let pos = ops.len();
                ops.push(Op::Jump(0));
                match self.structural_stack.last_mut() {
                    Some(StructuralKind::Loop { break_patches, .. }) => break_patches.push(pos),
                    Some(StructuralKind::Switch { break_patches }) => break_patches.push(pos),
                    None => {}
                }
            }

            Stmt::Continue => {
                let pos = ops.len();
                ops.push(Op::Jump(0));
                for ctx in self.structural_stack.iter_mut().rev() {
                    if let StructuralKind::Loop {
                        continue_patches, ..
                    } = ctx
                    {
                        continue_patches.push(pos);
                        break;
                    }
                }
            }

            Stmt::Next => {
                ops.push(Op::Next);
            }

            Stmt::NextFile => {
                ops.push(Op::NextFile);
            }

            Stmt::Exit(e) => {
                if let Some(ex) = e {
                    self.compile_expr(ex, ops);
                    ops.push(Op::ExitWithCode);
                } else {
                    ops.push(Op::ExitDefault);
                }
            }

            Stmt::Return(e) => {
                if let Some(ex) = e {
                    self.compile_expr(ex, ops);
                    ops.push(Op::ReturnVal);
                } else {
                    ops.push(Op::ReturnEmpty);
                }
            }

            Stmt::Delete { name, indices } => {
                let arr_idx = self.strings.intern(name);
                match indices {
                    None => ops.push(Op::DeleteArray(arr_idx)),
                    Some(ixs) => {
                        self.compile_array_key(ixs, ops);
                        ops.push(Op::DeleteElem(arr_idx));
                    }
                }
            }

            Stmt::Switch { expr, arms } => {
                self.compile_switch(expr, arms, ops);
            }

            Stmt::GetLine { var, redir } => {
                let var_idx = var.as_ref().map(|v| self.strings.intern(v));
                match redir {
                    GetlineRedir::Primary => {
                        ops.push(Op::GetLine {
                            var: var_idx,
                            source: GetlineSource::Primary,
                        });
                    }
                    GetlineRedir::File(e) => {
                        self.compile_expr(e, ops);
                        ops.push(Op::GetLine {
                            var: var_idx,
                            source: GetlineSource::File,
                        });
                    }
                    GetlineRedir::Coproc(e) => {
                        self.compile_expr(e, ops);
                        ops.push(Op::GetLine {
                            var: var_idx,
                            source: GetlineSource::Coproc,
                        });
                    }
                }
            }
        }
    }

    fn compile_print(
        &mut self,
        args: &[Expr],
        redir: &Option<PrintRedir>,
        is_printf: bool,
        ops: &mut Vec<Op>,
    ) {
        for a in args {
            self.compile_expr(a, ops);
        }
        let argc = args.len() as u16;

        let rk = match redir {
            None => RedirKind::Stdout,
            Some(PrintRedir::Overwrite(e)) => {
                self.compile_expr(e, ops);
                RedirKind::Overwrite
            }
            Some(PrintRedir::Append(e)) => {
                self.compile_expr(e, ops);
                RedirKind::Append
            }
            Some(PrintRedir::Pipe(e)) => {
                self.compile_expr(e, ops);
                RedirKind::Pipe
            }
            Some(PrintRedir::Coproc(e)) => {
                self.compile_expr(e, ops);
                RedirKind::Coproc
            }
        };

        if is_printf {
            ops.push(Op::Printf { argc, redir: rk });
        } else {
            ops.push(Op::Print { argc, redir: rk });
        }
    }

    // ── Expressions ─────────────────────────────────────────────────────────

    fn compile_expr(&mut self, expr: &Expr, ops: &mut Vec<Op>) {
        match expr {
            Expr::Number(n) => ops.push(Op::PushNum(*n)),
            Expr::Str(s) => {
                let idx = self.strings.intern(s);
                ops.push(Op::PushStr(idx));
            }
            Expr::Var(name) => {
                // Direct opcodes for frequently-accessed special variables.
                match name.as_str() {
                    "NR" => ops.push(Op::GetNR),
                    "FNR" => ops.push(Op::GetFNR),
                    "NF" => ops.push(Op::GetNF),
                    _ => {
                        if let Some(slot) = self.var_slot(name) {
                            ops.push(Op::GetSlot(slot));
                        } else {
                            let idx = self.strings.intern(name);
                            ops.push(Op::GetVar(idx));
                        }
                    }
                }
            }
            Expr::Field(inner) => {
                self.compile_expr(inner, ops);
                ops.push(Op::GetField);
            }
            Expr::Index { name, indices } => {
                let arr_idx = self.strings.intern(name);
                self.compile_array_key(indices, ops);
                ops.push(Op::GetArrayElem(arr_idx));
            }

            // Short-circuit logical operators
            Expr::Binary {
                op: BinOp::And,
                left,
                right,
            } => {
                self.compile_expr(left, ops);
                let false_jump = ops.len();
                ops.push(Op::JumpIfFalsePop(0));
                self.compile_expr(right, ops);
                ops.push(Op::ToBool);
                let end_jump = ops.len();
                ops.push(Op::Jump(0));
                let false_branch = ops.len();
                ops.push(Op::PushNum(0.0));
                let after = ops.len();
                ops[false_jump] = Op::JumpIfFalsePop(false_branch);
                ops[end_jump] = Op::Jump(after);
            }
            Expr::Binary {
                op: BinOp::Or,
                left,
                right,
            } => {
                self.compile_expr(left, ops);
                let true_jump = ops.len();
                ops.push(Op::JumpIfTruePop(0));
                self.compile_expr(right, ops);
                ops.push(Op::ToBool);
                let end_jump = ops.len();
                ops.push(Op::Jump(0));
                let true_branch = ops.len();
                ops.push(Op::PushNum(1.0));
                let after = ops.len();
                ops[true_jump] = Op::JumpIfTruePop(true_branch);
                ops[end_jump] = Op::Jump(after);
            }

            Expr::Binary { op, left, right } => {
                self.compile_expr(left, ops);
                self.compile_expr(right, ops);
                ops.push(match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Eq => Op::CmpEq,
                    BinOp::Ne => Op::CmpNe,
                    BinOp::Lt => Op::CmpLt,
                    BinOp::Le => Op::CmpLe,
                    BinOp::Gt => Op::CmpGt,
                    BinOp::Ge => Op::CmpGe,
                    BinOp::Concat => Op::Concat,
                    BinOp::Match => Op::RegexMatch,
                    BinOp::NotMatch => Op::RegexNotMatch,
                    BinOp::And | BinOp::Or => unreachable!("handled above"),
                });
            }

            Expr::Unary { op, expr: inner } => {
                self.compile_expr(inner, ops);
                ops.push(match op {
                    UnaryOp::Neg => Op::Neg,
                    UnaryOp::Pos => Op::Pos,
                    UnaryOp::Not => Op::Not,
                });
            }

            Expr::Assign { name, op, rhs } => {
                self.compile_expr(rhs, ops);
                if let Some(bop) = op {
                    if let Some(slot) = self.var_slot(name) {
                        ops.push(Op::CompoundAssignSlot(slot, *bop));
                    } else {
                        let var_idx = self.strings.intern(name);
                        ops.push(Op::CompoundAssignVar(var_idx, *bop));
                    }
                } else if let Some(slot) = self.var_slot(name) {
                    ops.push(Op::SetSlot(slot));
                } else {
                    let var_idx = self.strings.intern(name);
                    ops.push(Op::SetVar(var_idx));
                }
            }

            Expr::AssignField { field, op, rhs } => {
                self.compile_expr(field, ops);
                self.compile_expr(rhs, ops);
                if let Some(bop) = op {
                    ops.push(Op::CompoundAssignField(*bop));
                } else {
                    ops.push(Op::SetField);
                }
            }

            Expr::AssignIndex {
                name,
                indices,
                op,
                rhs,
            } => {
                let arr_idx = self.strings.intern(name);
                self.compile_array_key(indices, ops);
                self.compile_expr(rhs, ops);
                if let Some(bop) = op {
                    ops.push(Op::CompoundAssignIndex(arr_idx, *bop));
                } else {
                    ops.push(Op::SetArrayElem(arr_idx));
                }
            }

            Expr::Call { name, args } => {
                self.compile_call(name, args, ops);
            }

            Expr::Ternary { cond, then_, else_ } => {
                self.compile_expr(cond, ops);
                let jump_else = ops.len();
                ops.push(Op::JumpIfFalsePop(0));
                self.compile_expr(then_, ops);
                let jump_end = ops.len();
                ops.push(Op::Jump(0));
                let else_start = ops.len();
                self.compile_expr(else_, ops);
                let after = ops.len();
                ops[jump_else] = Op::JumpIfFalsePop(else_start);
                ops[jump_end] = Op::Jump(after);
            }

            Expr::In { key, arr } => {
                let arr_idx = self.strings.intern(arr);
                self.compile_expr(key, ops);
                ops.push(Op::InArray(arr_idx));
            }

            Expr::IncDec { op, target } => match target {
                IncDecTarget::Var(name) => {
                    if let Some(slot) = self.var_slot(name) {
                        ops.push(Op::IncDecSlot(slot, *op));
                    } else {
                        let idx = self.strings.intern(name);
                        ops.push(Op::IncDecVar(idx, *op));
                    }
                }
                IncDecTarget::Field(inner) => {
                    self.compile_expr(inner, ops);
                    ops.push(Op::IncDecField(*op));
                }
                IncDecTarget::Index { name, indices } => {
                    let arr_idx = self.strings.intern(name);
                    self.compile_array_key(indices, ops);
                    ops.push(Op::IncDecIndex(arr_idx, *op));
                }
            },
        }
    }

    fn compile_call(&mut self, name: &str, args: &[Expr], ops: &mut Vec<Op>) {
        match name {
            "sub" => self.compile_sub_gsub(args, false, ops),
            "gsub" => self.compile_sub_gsub(args, true, ops),
            "split" => self.compile_split(args, ops),
            "patsplit" => self.compile_patsplit(args, ops),
            "match" => self.compile_match(args, ops),
            "asort" => self.compile_asort(args, ops),
            "asorti" => self.compile_asorti(args, ops),
            "typeof" => {
                if args.len() != 1 {
                    for a in args {
                        self.compile_expr(a, ops);
                    }
                    let name_idx = self.strings.intern("typeof");
                    ops.push(Op::CallBuiltin(name_idx, args.len() as u16));
                    return;
                }
                match &args[0] {
                    Expr::Var(name) => {
                        if let Some(slot) = self.var_slot(name) {
                            ops.push(Op::TypeofSlot(slot));
                        } else {
                            let idx = self.strings.intern(name);
                            ops.push(Op::TypeofVar(idx));
                        }
                    }
                    Expr::Index { name, indices } => {
                        let arr_idx = self.strings.intern(name);
                        self.compile_array_key(indices, ops);
                        ops.push(Op::TypeofArrayElem(arr_idx));
                    }
                    Expr::Field(inner) => {
                        self.compile_expr(inner, ops);
                        ops.push(Op::TypeofField);
                    }
                    other => {
                        self.compile_expr(other, ops);
                        ops.push(Op::TypeofValue);
                    }
                }
            }
            _ => {
                for a in args {
                    self.compile_expr(a, ops);
                }
                let name_idx = self.strings.intern(name);
                let argc = args.len() as u16;
                ops.push(Op::CallBuiltin(name_idx, argc));
            }
        }
    }

    fn compile_sub_gsub(&mut self, args: &[Expr], is_global: bool, ops: &mut Vec<Op>) {
        self.compile_expr(&args[0], ops);
        self.compile_expr(&args[1], ops);

        let target = if args.len() >= 3 {
            match &args[2] {
                Expr::Var(name) => {
                    if let Some(slot) = self.var_slot(name) {
                        SubTarget::SlotVar(slot)
                    } else {
                        SubTarget::Var(self.strings.intern(name))
                    }
                }
                Expr::Field(inner) => {
                    self.compile_expr(inner, ops);
                    SubTarget::Field
                }
                Expr::Index { name, indices } => {
                    let arr_idx = self.strings.intern(name);
                    self.compile_array_key(indices, ops);
                    SubTarget::Index(arr_idx)
                }
                _ => SubTarget::Record,
            }
        } else {
            SubTarget::Record
        };

        if is_global {
            ops.push(Op::GsubFn(target));
        } else {
            ops.push(Op::SubFn(target));
        }
    }

    fn compile_split(&mut self, args: &[Expr], ops: &mut Vec<Op>) {
        self.compile_expr(&args[0], ops);
        let arr_name = match &args[1] {
            Expr::Var(n) => n.as_str(),
            _ => "",
        };
        let arr_idx = self.strings.intern(arr_name);
        let has_fs = args.len() >= 3;
        if has_fs {
            self.compile_expr(&args[2], ops);
        }
        ops.push(Op::Split {
            arr: arr_idx,
            has_fs,
        });
    }

    fn compile_patsplit(&mut self, args: &[Expr], ops: &mut Vec<Op>) {
        self.compile_expr(&args[0], ops);
        let arr_name = match &args[1] {
            Expr::Var(n) => n.as_str(),
            _ => "",
        };
        let arr_idx = self.strings.intern(arr_name);
        let has_fp = args.len() >= 3;
        if has_fp {
            self.compile_expr(&args[2], ops);
        }
        let seps = if args.len() >= 4 {
            match &args[3] {
                Expr::Var(n) => Some(self.strings.intern(n)),
                _ => None,
            }
        } else {
            None
        };
        ops.push(Op::Patsplit {
            arr: arr_idx,
            has_fp,
            seps,
        });
    }

    fn compile_match(&mut self, args: &[Expr], ops: &mut Vec<Op>) {
        self.compile_expr(&args[0], ops);
        self.compile_expr(&args[1], ops);
        let arr = if args.len() >= 3 {
            match &args[2] {
                Expr::Var(n) => Some(self.strings.intern(n)),
                _ => None,
            }
        } else {
            None
        };
        ops.push(Op::MatchBuiltin { arr });
    }

    fn compile_asort(&mut self, args: &[Expr], ops: &mut Vec<Op>) {
        let src = match args.first() {
            Some(Expr::Var(n)) => self.strings.intern(n),
            _ => self.strings.intern(""),
        };
        let dest = if args.len() >= 2 {
            match &args[1] {
                Expr::Var(n) => Some(self.strings.intern(n)),
                _ => None,
            }
        } else {
            None
        };
        ops.push(Op::Asort { src, dest });
    }

    fn compile_asorti(&mut self, args: &[Expr], ops: &mut Vec<Op>) {
        let src = match args.first() {
            Some(Expr::Var(n)) => self.strings.intern(n),
            _ => self.strings.intern(""),
        };
        let dest = if args.len() >= 2 {
            match &args[1] {
                Expr::Var(n) => Some(self.strings.intern(n)),
                _ => None,
            }
        } else {
            None
        };
        ops.push(Op::Asorti { src, dest });
    }

    fn compile_switch(&mut self, expr: &Expr, arms: &[SwitchArm], ops: &mut Vec<Op>) {
        self.structural_stack.push(StructuralKind::Switch {
            break_patches: Vec::new(),
        });
        self.compile_expr(expr, ops);
        if arms.is_empty() {
            ops.push(Op::Pop);
            let _ = self.structural_stack.pop();
            return;
        }
        let mut pending_jfail: Option<usize> = None;
        let mut end_jumps: Vec<usize> = Vec::new();
        for arm in arms {
            match arm {
                SwitchArm::Case { label, stmts } => {
                    if let Some(p) = pending_jfail.take() {
                        ops[p] = Op::JumpIfFalsePop(ops.len());
                    }
                    ops.push(Op::Dup);
                    match label {
                        SwitchLabel::Expr(e) => {
                            self.compile_expr(e, ops);
                            ops.push(Op::CmpEq);
                        }
                        SwitchLabel::Regexp(re) => {
                            let idx = self.strings.intern(re);
                            ops.push(Op::PushStr(idx));
                            ops.push(Op::RegexMatch);
                        }
                    }
                    let jfail = ops.len();
                    ops.push(Op::JumpIfFalsePop(0));
                    pending_jfail = Some(jfail);
                    ops.push(Op::Pop);
                    ops.push(Op::Pop);
                    self.compile_stmts(stmts, ops);
                    let jend = ops.len();
                    ops.push(Op::Jump(0));
                    end_jumps.push(jend);
                }
                SwitchArm::Default { stmts } => {
                    if let Some(p) = pending_jfail.take() {
                        ops[p] = Op::JumpIfFalsePop(ops.len());
                    }
                    ops.push(Op::Pop);
                    self.compile_stmts(stmts, ops);
                }
            }
        }
        if let Some(p) = pending_jfail {
            let pop_pos = ops.len();
            ops.push(Op::Pop);
            ops[p] = Op::JumpIfFalsePop(pop_pos);
        }
        let end = ops.len();
        for j in end_jumps {
            ops[j] = Op::Jump(end);
        }
        if let StructuralKind::Switch { break_patches } = self.structural_stack.pop().unwrap() {
            for bp in break_patches {
                ops[bp] = Op::Jump(end);
            }
        }
    }

    fn compile_array_key(&mut self, indices: &[Expr], ops: &mut Vec<Op>) {
        for ix in indices {
            self.compile_expr(ix, ops);
        }
        if indices.len() > 1 {
            ops.push(Op::JoinArrayKey(indices.len() as u16));
        }
    }
}

// ── Pre-pass: collect array names ───────────────────────────────────────────

fn collect_array_names(prog: &Program) -> HashSet<String> {
    let mut names = HashSet::new();
    for rule in &prog.rules {
        for s in &rule.stmts {
            collect_array_names_stmt(s, &mut names);
        }
    }
    for fd in prog.funcs.values() {
        for s in &fd.body {
            collect_array_names_stmt(s, &mut names);
        }
    }
    names
}

fn collect_array_names_stmt(s: &Stmt, names: &mut HashSet<String>) {
    match s {
        Stmt::If { cond, then_, else_ } => {
            collect_array_names_expr(cond, names);
            for t in then_ {
                collect_array_names_stmt(t, names);
            }
            for t in else_ {
                collect_array_names_stmt(t, names);
            }
        }
        Stmt::While { cond, body } => {
            collect_array_names_expr(cond, names);
            for t in body {
                collect_array_names_stmt(t, names);
            }
        }
        Stmt::DoWhile { cond, body } => {
            collect_array_names_expr(cond, names);
            for t in body {
                collect_array_names_stmt(t, names);
            }
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                collect_array_names_expr(e, names);
            }
            if let Some(e) = cond {
                collect_array_names_expr(e, names);
            }
            if let Some(e) = iter {
                collect_array_names_expr(e, names);
            }
            for t in body {
                collect_array_names_stmt(t, names);
            }
        }
        Stmt::ForIn { arr, body, .. } => {
            names.insert(arr.clone());
            for t in body {
                collect_array_names_stmt(t, names);
            }
        }
        Stmt::Block(stmts) => {
            for t in stmts {
                collect_array_names_stmt(t, names);
            }
        }
        Stmt::Expr(e) => collect_array_names_expr(e, names),
        Stmt::Print { args, redir } | Stmt::Printf { args, redir } => {
            for a in args {
                collect_array_names_expr(a, names);
            }
            if let Some(r) = redir {
                match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => collect_array_names_expr(e, names),
                }
            }
        }
        Stmt::Delete { name, indices } => {
            names.insert(name.clone());
            if let Some(ixs) = indices {
                for e in ixs {
                    collect_array_names_expr(e, names);
                }
            }
        }
        Stmt::Exit(Some(e)) | Stmt::Return(Some(e)) => collect_array_names_expr(e, names),
        Stmt::GetLine { redir, .. } => match redir {
            GetlineRedir::File(e) | GetlineRedir::Coproc(e) => collect_array_names_expr(e, names),
            GetlineRedir::Primary => {}
        },
        Stmt::Switch { expr, arms } => {
            collect_array_names_expr(expr, names);
            for arm in arms {
                match arm {
                    SwitchArm::Case { label, stmts } => {
                        if let SwitchLabel::Expr(e) = label {
                            collect_array_names_expr(e, names);
                        }
                        for t in stmts {
                            collect_array_names_stmt(t, names);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for t in stmts {
                            collect_array_names_stmt(t, names);
                        }
                    }
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::Next
        | Stmt::NextFile
        | Stmt::Exit(None)
        | Stmt::Return(None) => {}
    }
}

fn collect_array_names_expr(e: &Expr, names: &mut HashSet<String>) {
    match e {
        Expr::Index { name, indices } => {
            names.insert(name.clone());
            for ix in indices {
                collect_array_names_expr(ix, names);
            }
        }
        Expr::AssignIndex {
            name, indices, rhs, ..
        } => {
            names.insert(name.clone());
            for ix in indices {
                collect_array_names_expr(ix, names);
            }
            collect_array_names_expr(rhs, names);
        }
        Expr::In { arr, key } => {
            names.insert(arr.clone());
            collect_array_names_expr(key, names);
        }
        Expr::Binary { left, right, .. } => {
            collect_array_names_expr(left, names);
            collect_array_names_expr(right, names);
        }
        Expr::Unary { expr, .. } => collect_array_names_expr(expr, names),
        Expr::Assign { rhs, .. } => collect_array_names_expr(rhs, names),
        Expr::AssignField { field, rhs, .. } => {
            collect_array_names_expr(field, names);
            collect_array_names_expr(rhs, names);
        }
        Expr::Field(inner) => collect_array_names_expr(inner, names),
        Expr::Call { args, .. } => {
            for a in args {
                collect_array_names_expr(a, names);
            }
        }
        Expr::Ternary { cond, then_, else_ } => {
            collect_array_names_expr(cond, names);
            collect_array_names_expr(then_, names);
            collect_array_names_expr(else_, names);
        }
        Expr::IncDec { target, .. } => match target {
            IncDecTarget::Index { name, indices } => {
                names.insert(name.clone());
                for ix in indices {
                    collect_array_names_expr(ix, names);
                }
            }
            IncDecTarget::Field(e) => collect_array_names_expr(e, names),
            IncDecTarget::Var(_) => {}
        },
        Expr::Number(_) | Expr::Str(_) | Expr::Var(_) => {}
    }
}

/// Peephole optimizer: fuse common multi-op sequences into single opcodes.
/// Runs in a single pass, recording removals, then adjusting jump targets.
fn peephole_optimize(ops: &mut Vec<Op>) {
    // Phase 1: identify fusions. We build a list of (position, replacement_op, count_removed).
    // To avoid invalidating indices, we scan and collect, then apply in reverse order.
    let mut fusions: Vec<(usize, Op, usize)> = Vec::new(); // (pos, new_op, ops_removed_after_pos)

    let mut i = 0;
    while i < ops.len() {
        // Pattern: PushNum(N) + GetField + Print{argc:1, Stdout} → PrintFieldStdout(N)
        if i + 3 <= ops.len() {
            if let (
                Op::PushNum(n),
                Op::GetField,
                Op::Print {
                    argc: 1,
                    redir: RedirKind::Stdout,
                },
            ) = (ops[i], ops[i + 1], ops[i + 2])
            {
                let field = n as u16;
                if n >= 0.0 && n == field as f64 {
                    fusions.push((i, Op::PrintFieldStdout(field), 2));
                    i += 3;
                    continue;
                }
            }
        }

        // Pattern: PushNum(N) + GetField + CompoundAssignSlot(slot, Add) + Pop
        //        → AddFieldToSlot { field: N, slot }
        if i + 4 <= ops.len() {
            if let (
                Op::PushNum(n),
                Op::GetField,
                Op::CompoundAssignSlot(slot, BinOp::Add),
                Op::Pop,
            ) = (ops[i], ops[i + 1], ops[i + 2], ops[i + 3])
            {
                let field = n as u16;
                if n >= 0.0 && n == field as f64 {
                    fusions.push((i, Op::AddFieldToSlot { field, slot }, 3));
                    i += 4;
                    continue;
                }
            }
        }

        // Pattern: PushNum(N) + GetField → PushFieldNum(N)
        // when field value is used as a number (followed by arithmetic/comparison).
        if i + 2 <= ops.len() {
            if let (Op::PushNum(n), Op::GetField) = (ops[i], ops[i + 1]) {
                let field = n as u16;
                if n >= 0.0 && n == field as f64 {
                    // Check if next op consumes the value numerically
                    let next = if i + 2 < ops.len() {
                        Some(ops[i + 2])
                    } else {
                        None
                    };
                    let is_numeric_consumer = matches!(
                        next,
                        Some(Op::Add)
                            | Some(Op::Sub)
                            | Some(Op::Mul)
                            | Some(Op::Div)
                            | Some(Op::Mod)
                            | Some(Op::CmpLt)
                            | Some(Op::CmpLe)
                            | Some(Op::CmpGt)
                            | Some(Op::CmpGe)
                            | Some(Op::CompoundAssignSlot(_, _))
                            | Some(Op::CompoundAssignVar(_, _))
                    );
                    if is_numeric_consumer {
                        fusions.push((i, Op::PushFieldNum(field), 1));
                        i += 2;
                        continue;
                    }
                }
            }
        }

        // Pattern: GetSlot(src) + CompoundAssignSlot(dst, Add) + Pop
        //        → AddSlotToSlot { src, dst }
        if i + 3 <= ops.len() {
            if let (Op::GetSlot(src), Op::CompoundAssignSlot(dst, BinOp::Add), Op::Pop) =
                (ops[i], ops[i + 1], ops[i + 2])
            {
                fusions.push((i, Op::AddSlotToSlot { src, dst }, 2));
                i += 3;
                continue;
            }
        }

        // Pattern: IncDecSlot(slot, Pre/PostInc) + Pop → IncrSlot(slot)
        // Pattern: IncDecSlot(slot, Pre/PostDec) + Pop → DecrSlot(slot)
        if i + 2 <= ops.len() {
            if let (Op::IncDecSlot(slot, kind), Op::Pop) = (ops[i], ops[i + 1]) {
                let fused = match kind {
                    IncDecOp::PreInc | IncDecOp::PostInc => Op::IncrSlot(slot),
                    IncDecOp::PreDec | IncDecOp::PostDec => Op::DecrSlot(slot),
                };
                fusions.push((i, fused, 1));
                i += 2;
                continue;
            }
            // Pattern: IncDecVar(idx, Pre/PostInc) + Pop → IncrVar(idx)
            // Pattern: IncDecVar(idx, Pre/PostDec) + Pop → DecrVar(idx)
            if let (Op::IncDecVar(idx, kind), Op::Pop) = (ops[i], ops[i + 1]) {
                let fused = match kind {
                    IncDecOp::PreInc | IncDecOp::PostInc => Op::IncrVar(idx),
                    IncDecOp::PreDec | IncDecOp::PostDec => Op::DecrVar(idx),
                };
                fusions.push((i, fused, 1));
                i += 2;
                continue;
            }
            // Pattern: PushStr(idx) + Concat → ConcatPoolStr(idx)
            if let (Op::PushStr(idx), Op::Concat) = (ops[i], ops[i + 1]) {
                fusions.push((i, Op::ConcatPoolStr(idx), 1));
                i += 2;
                continue;
            }
        }

        // Pattern: GetSlot(s) + PushNum(1.0) + Add + SetSlot(s) + Pop
        //        → IncrSlot(s)
        if i + 5 <= ops.len() {
            if let (Op::GetSlot(s1), Op::PushNum(n), Op::Add, Op::SetSlot(s2), Op::Pop) =
                (ops[i], ops[i + 1], ops[i + 2], ops[i + 3], ops[i + 4])
            {
                if s1 == s2 && n == 1.0 {
                    fusions.push((i, Op::IncrSlot(s1), 4));
                    i += 5;
                    continue;
                }
            }
        }

        // Pattern: GetSlot(s) + PushNum(limit) + CmpLt + JumpIfFalsePop(target)
        //        → JumpIfSlotGeNum { slot: s, limit, target }
        if i + 4 <= ops.len() {
            if let (Op::GetSlot(slot), Op::PushNum(limit), Op::CmpLt, Op::JumpIfFalsePop(target)) =
                (ops[i], ops[i + 1], ops[i + 2], ops[i + 3])
            {
                fusions.push((
                    i,
                    Op::JumpIfSlotGeNum {
                        slot,
                        limit,
                        target,
                    },
                    3,
                ));
                i += 4;
                continue;
            }
        }

        // `sum += $f1 * $f2` when fields are still `PushNum`+`GetField` (rhs is `$1 * $2`, not yet fused to PushFieldNum).
        if i + 7 <= ops.len() {
            if let (
                Op::PushNum(n1),
                Op::GetField,
                Op::PushNum(n2),
                Op::GetField,
                Op::Mul,
                Op::CompoundAssignSlot(slot, BinOp::Add),
                Op::Pop,
            ) = (
                ops[i],
                ops[i + 1],
                ops[i + 2],
                ops[i + 3],
                ops[i + 4],
                ops[i + 5],
                ops[i + 6],
            ) {
                let f1 = n1 as u16;
                let f2 = n2 as u16;
                if n1 >= 0.0 && n1 == f1 as f64 && n2 >= 0.0 && n2 == f2 as f64 {
                    fusions.push((i, Op::AddMulFieldsToSlot { f1, f2, slot }, 6));
                    i += 7;
                    continue;
                }
            }
        }

        // `a[$n] += delta` with `$n` as GetField (array subscript is a field expr, not numeric PushFieldNum).
        if i + 5 <= ops.len() {
            if let (
                Op::PushNum(n),
                Op::GetField,
                Op::PushNum(delta),
                Op::CompoundAssignIndex(arr, BinOp::Add),
                Op::Pop,
            ) = (ops[i], ops[i + 1], ops[i + 2], ops[i + 3], ops[i + 4])
            {
                let field = n as u16;
                if n >= 0.0 && n == field as f64 {
                    fusions.push((i, Op::ArrayFieldAddConst { arr, field, delta }, 4));
                    i += 5;
                    continue;
                }
            }
        }

        // `print $a, $b, $c` with three `PushNum`+`GetField` (not PushFieldNum — print uses string form).
        if i + 7 <= ops.len() {
            if let (
                Op::PushNum(n1),
                Op::GetField,
                Op::PushNum(n2),
                Op::GetField,
                Op::PushNum(n3),
                Op::GetField,
                Op::Print {
                    argc: 3,
                    redir: RedirKind::Stdout,
                },
            ) = (
                ops[i],
                ops[i + 1],
                ops[i + 2],
                ops[i + 3],
                ops[i + 4],
                ops[i + 5],
                ops[i + 6],
            ) {
                let f1 = n1 as u16;
                let f2 = n2 as u16;
                let f3 = n3 as u16;
                if n1 >= 0.0
                    && n1 == f1 as f64
                    && n2 >= 0.0
                    && n2 == f2 as f64
                    && n3 >= 0.0
                    && n3 == f3 as f64
                {
                    fusions.push((i, Op::PrintThreeFieldsStdout { f1, f2, f3 }, 6));
                    i += 7;
                    continue;
                }
            }
        }

        // `print $f1 sep $f2` with GetField+Concat form.
        if i + 8 <= ops.len() {
            if let (
                Op::PushNum(n1),
                Op::GetField,
                Op::PushStr(sep),
                Op::Concat,
                Op::PushNum(n2),
                Op::GetField,
                Op::Concat,
                Op::Print {
                    argc: 1,
                    redir: RedirKind::Stdout,
                },
            ) = (
                ops[i],
                ops[i + 1],
                ops[i + 2],
                ops[i + 3],
                ops[i + 4],
                ops[i + 5],
                ops[i + 6],
                ops[i + 7],
            ) {
                let f1 = n1 as u16;
                let f2 = n2 as u16;
                if n1 >= 0.0 && n1 == f1 as f64 && n2 >= 0.0 && n2 == f2 as f64 {
                    fusions.push((i, Op::PrintFieldSepField { f1, sep, f2 }, 7));
                    i += 8;
                    continue;
                }
            }
        }

        // `sum += $f1 * $f2` → AddMulFieldsToSlot (PushFieldNum form)
        if i + 5 <= ops.len() {
            if let (
                Op::PushFieldNum(f1),
                Op::PushFieldNum(f2),
                Op::Mul,
                Op::CompoundAssignSlot(slot, BinOp::Add),
                Op::Pop,
            ) = (ops[i], ops[i + 1], ops[i + 2], ops[i + 3], ops[i + 4])
            {
                fusions.push((i, Op::AddMulFieldsToSlot { f1, f2, slot }, 4));
                i += 5;
                continue;
            }
        }

        // `a[$field] += delta` (delta constant) → ArrayFieldAddConst
        if i + 4 <= ops.len() {
            if let (
                Op::PushFieldNum(field),
                Op::PushNum(delta),
                Op::CompoundAssignIndex(arr, BinOp::Add),
                Op::Pop,
            ) = (ops[i], ops[i + 1], ops[i + 2], ops[i + 3])
            {
                fusions.push((i, Op::ArrayFieldAddConst { arr, field, delta }, 3));
                i += 4;
                continue;
            }
        }

        // `print $f1, $f2, $f3` → PrintThreeFieldsStdout
        if i + 4 <= ops.len() {
            if let (
                Op::PushFieldNum(f1),
                Op::PushFieldNum(f2),
                Op::PushFieldNum(f3),
                Op::Print {
                    argc: 3,
                    redir: RedirKind::Stdout,
                },
            ) = (ops[i], ops[i + 1], ops[i + 2], ops[i + 3])
            {
                fusions.push((i, Op::PrintThreeFieldsStdout { f1, f2, f3 }, 3));
                i += 4;
                continue;
            }
        }

        // `print $f1 sep $f2` → PrintFieldSepField
        if i + 6 <= ops.len() {
            if let (
                Op::PushFieldNum(f1),
                Op::PushStr(sep),
                Op::Concat,
                Op::PushFieldNum(f2),
                Op::Concat,
                Op::Print {
                    argc: 1,
                    redir: RedirKind::Stdout,
                },
            ) = (
                ops[i],
                ops[i + 1],
                ops[i + 2],
                ops[i + 3],
                ops[i + 4],
                ops[i + 5],
            ) {
                fusions.push((i, Op::PrintFieldSepField { f1, sep, f2 }, 5));
                i += 6;
                continue;
            }
        }

        i += 1;
    }

    if fusions.is_empty() {
        return;
    }

    // Phase 2: build index mapping (old position → new position).
    // Each fusion at pos removes `removed` ops starting at pos+1.
    let old_len = ops.len();
    let mut offset_map = vec![0usize; old_len + 1]; // +1 for end-of-chunk targets
    let mut adjustment: usize = 0;
    let mut fi = 0;
    #[allow(clippy::needless_range_loop)]
    for pos in 0..=old_len {
        if fi < fusions.len() {
            let (fpos, _, removed) = fusions[fi];
            // The removed ops are at positions fpos+1 .. fpos+removed (inclusive).
            if pos > fpos && pos <= fpos + removed {
                // This position is being removed — map to the fused op position.
                offset_map[pos] = fpos - adjustment;
                if pos == fpos + removed {
                    adjustment += removed;
                    fi += 1;
                }
                continue;
            }
            if pos == fpos + removed {
                adjustment += removed;
                fi += 1;
            }
        }
        offset_map[pos] = pos - adjustment;
    }

    // Phase 3: apply fusions in reverse order to preserve indices.
    for &(pos, ref new_op, removed) in fusions.iter().rev() {
        ops[pos] = *new_op;
        for _ in 0..removed {
            ops.remove(pos + 1);
        }
    }

    // Phase 4: adjust all jump targets using the offset map.
    for op in ops.iter_mut() {
        match op {
            Op::Jump(ref mut t) => *t = offset_map[*t],
            Op::JumpIfFalsePop(ref mut t) => *t = offset_map[*t],
            Op::JumpIfTruePop(ref mut t) => *t = offset_map[*t],
            Op::ForInNext {
                ref mut end_jump, ..
            } => *end_jump = offset_map[*end_jump],
            Op::JumpIfSlotGeNum { ref mut target, .. } => *target = offset_map[*target],
            _ => {}
        }
    }
}

/// Check if a regex pattern is a plain literal (no metacharacters).
fn is_literal_regex(pat: &str) -> bool {
    !pat.bytes().any(|b| {
        matches!(
            b,
            b'.' | b'*'
                | b'+'
                | b'?'
                | b'['
                | b']'
                | b'('
                | b')'
                | b'{'
                | b'}'
                | b'|'
                | b'^'
                | b'$'
                | b'\\'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_program;

    #[test]
    fn compile_begin_print_constant() {
        let prog = parse_program("BEGIN { print 42 }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks.is_empty());
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }

    #[test]
    fn compile_record_rule_field_access() {
        let prog = parse_program("{ print $1 + $2 }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert_eq!(cp.record_rules.len(), 1);
        assert!(!cp.record_rules[0].body.ops.is_empty());
    }

    #[test]
    fn compile_user_function_has_body() {
        let prog = parse_program("function dbl(n){ return n*2 } { print dbl(3) }").unwrap();
        let cp = Compiler::compile_program(&prog);
        let f = cp.functions.get("dbl").expect("dbl");
        assert_eq!(f.params, vec!["n".to_string()]);
        assert!(!f.body.ops.is_empty());
    }

    #[test]
    fn compile_empty_record_pattern_still_emits_rule() {
        let prog = parse_program("{ }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert_eq!(cp.record_rules.len(), 1);
    }

    #[test]
    fn compile_end_block() {
        let prog = parse_program("END { print \"x\" }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.end_chunks.is_empty());
    }

    #[test]
    fn compile_beginfile_endfile() {
        let prog = parse_program("BEGINFILE { print \"bf\" } ENDFILE { print \"ef\" }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.beginfile_chunks.is_empty());
        assert!(!cp.endfile_chunks.is_empty());
    }

    #[test]
    fn compile_begin_and_end_together() {
        let prog = parse_program("BEGIN { a=1 } { print $0 } END { print a }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks.is_empty());
        assert!(!cp.end_chunks.is_empty());
        assert_eq!(cp.record_rules.len(), 1);
    }

    #[test]
    fn compile_while_loop() {
        let prog = parse_program("BEGIN { i=0; while (i<3) i=i+1 }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }

    #[test]
    fn compile_if_else() {
        let prog = parse_program("BEGIN { if (1) print 1; else print 0 }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }

    #[test]
    fn compile_delete_array_elem() {
        let prog = parse_program("BEGIN { delete a[\"k\"] }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }

    #[test]
    fn compile_two_functions() {
        let prog = parse_program(
            "function a(){ return 1 } function b(){ return 2 } BEGIN { print a()+b() }",
        )
        .unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(cp.functions.contains_key("a"));
        assert!(cp.functions.contains_key("b"));
    }

    #[test]
    fn compile_do_while_loop() {
        let prog = parse_program("BEGIN { do { x = 1 } while (0) }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }

    #[test]
    fn compile_for_in_loop() {
        let prog = parse_program("BEGIN { for (k in a) print k }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }

    #[test]
    fn compile_record_rule_with_next() {
        let prog = parse_program("{ next }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert_eq!(cp.record_rules.len(), 1);
        assert!(!cp.record_rules[0].body.ops.is_empty());
    }

    #[test]
    fn compile_delete_multidimensional_element() {
        let prog = parse_program("BEGIN { delete a[1,2] }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!cp.begin_chunks[0].ops.is_empty());
    }
}
