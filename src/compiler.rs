//! Compile an AST [`Program`] into a [`CompiledProgram`] of flat bytecode.

use crate::ast::*;
use crate::bytecode::*;
use std::collections::{HashMap, HashSet};

/// Variables with special awk semantics — accessed by Runtime methods or computed
/// from Runtime fields. These bypass the slot system and use the HashMap path.
const SPECIAL_VARS: &[&str] = &[
    "NR", "FNR", "NF", "FILENAME", "FS", "OFS", "ORS", "SUBSEP", "OFMT", "FPAT", "RSTART",
    "RLENGTH", "ENVIRON",
];

/// Tracks break/continue jump patches for loops.
struct LoopInfo {
    break_patches: Vec<usize>,
    continue_patches: Vec<usize>,
}

pub struct Compiler {
    pub strings: StringPool,
    loop_stack: Vec<LoopInfo>,
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
            loop_stack: Vec::new(),
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
        Chunk { ops }
    }

    fn compile_pattern(&mut self, pat: &Pattern) -> CompiledPattern {
        match pat {
            Pattern::Empty => CompiledPattern::Always,
            Pattern::Regexp(re) => {
                let idx = self.strings.intern(re);
                CompiledPattern::Regexp(idx)
            }
            Pattern::Expr(e) => {
                let mut ops = Vec::new();
                self.compile_expr(e, &mut ops);
                CompiledPattern::Expr(Chunk { ops })
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
                self.loop_stack.push(LoopInfo {
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

                let info = self.loop_stack.pop().unwrap();
                for pos in info.break_patches {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.continue_patches {
                    ops[pos] = Op::Jump(loop_start);
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
                self.loop_stack.push(LoopInfo {
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

                let info = self.loop_stack.pop().unwrap();
                for pos in info.break_patches {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.continue_patches {
                    ops[pos] = Op::Jump(continue_target);
                }
            }

            Stmt::ForIn { var, arr, body } => {
                let arr_idx = self.strings.intern(arr);
                let var_idx = self.strings.intern(var);

                ops.push(Op::ForInStart(arr_idx));

                let loop_top = ops.len();
                self.loop_stack.push(LoopInfo {
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

                let info = self.loop_stack.pop().unwrap();
                for pos in info.break_patches {
                    ops[pos] = Op::Jump(after_loop);
                }
                for pos in info.continue_patches {
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
                if let Some(info) = self.loop_stack.last_mut() {
                    info.break_patches.push(pos);
                }
            }

            Stmt::Continue => {
                let pos = ops.len();
                ops.push(Op::Jump(0));
                if let Some(info) = self.loop_stack.last_mut() {
                    info.continue_patches.push(pos);
                }
            }

            Stmt::Next => {
                ops.push(Op::Next);
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
                if let Some(slot) = self.var_slot(name) {
                    ops.push(Op::GetSlot(slot));
                } else {
                    let idx = self.strings.intern(name);
                    ops.push(Op::GetVar(idx));
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
        }
    }

    fn compile_call(&mut self, name: &str, args: &[Expr], ops: &mut Vec<Op>) {
        match name {
            "sub" => self.compile_sub_gsub(args, false, ops),
            "gsub" => self.compile_sub_gsub(args, true, ops),
            "split" => self.compile_split(args, ops),
            "patsplit" => self.compile_patsplit(args, ops),
            "match" => self.compile_match(args, ops),
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
        Stmt::Break | Stmt::Continue | Stmt::Next | Stmt::Exit(None) | Stmt::Return(None) => {}
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
        Expr::Number(_) | Expr::Str(_) | Expr::Var(_) => {}
    }
}

/// Peephole optimizer: fuse common multi-op sequences into single opcodes.
/// Runs in a single pass over the instruction stream after compilation.
fn peephole_optimize(ops: &mut Vec<Op>) {
    let mut i = 0;
    while i + 3 < ops.len() {
        // Pattern: PushNum(N) + GetField + Print{argc:1, Stdout} → PrintFieldStdout(N)
        // where N is a small positive integer (field index).
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
                ops[i] = Op::PrintFieldStdout(field);
                ops.remove(i + 2);
                ops.remove(i + 1);
                continue;
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
                    ops[i] = Op::AddFieldToSlot { field, slot };
                    ops.remove(i + 3);
                    ops.remove(i + 2);
                    ops.remove(i + 1);
                    continue;
                }
            }
        }

        i += 1;
    }
}
