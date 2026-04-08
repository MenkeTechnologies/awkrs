//! Compile an AST [`Program`] into a [`CompiledProgram`] of flat bytecode.

use crate::ast::*;
use crate::bytecode::*;

/// Tracks break/continue jump patches for loops.
struct LoopInfo {
    break_patches: Vec<usize>,
    continue_patches: Vec<usize>,
}

pub struct Compiler {
    pub strings: StringPool,
    loop_stack: Vec<LoopInfo>,
}

impl Compiler {
    pub fn compile_program(prog: &Program) -> CompiledProgram {
        let mut c = Compiler {
            strings: StringPool::default(),
            loop_stack: Vec::new(),
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

        let mut functions = std::collections::HashMap::new();
        for (name, fd) in &prog.funcs {
            let body = c.compile_chunk(&fd.body);
            functions.insert(
                name.clone(),
                CompiledFunc {
                    params: fd.params.clone(),
                    body,
                },
            );
        }

        CompiledProgram {
            begin_chunks,
            end_chunks,
            beginfile_chunks,
            endfile_chunks,
            record_rules,
            functions,
            strings: c.strings,
        }
    }

    fn compile_chunk(&mut self, stmts: &[Stmt]) -> Chunk {
        let mut ops = Vec::new();
        self.compile_stmts(stmts, &mut ops);
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
                ops.push(Op::JumpIfFalsePop(0)); // placeholder

                self.compile_stmts(then_, ops);

                if else_.is_empty() {
                    let after = ops.len();
                    ops[jump_else] = Op::JumpIfFalsePop(after);
                } else {
                    let jump_end = ops.len();
                    ops.push(Op::Jump(0)); // placeholder
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
                    end_jump: 0, // placeholder
                });

                self.compile_stmts(body, ops);
                ops.push(Op::Jump(loop_top));

                let after_loop = ops.len();
                ops.push(Op::ForInEnd);
                let cleanup_done = ops.len();

                // Patch ForInNext to jump past ForInEnd
                ops[next_pos] = Op::ForInNext {
                    var: var_idx,
                    end_jump: after_loop,
                };

                let info = self.loop_stack.pop().unwrap();
                for pos in info.break_patches {
                    // Break: jump to ForInEnd (cleanup)
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
                ops.push(Op::Jump(0)); // placeholder
                if let Some(info) = self.loop_stack.last_mut() {
                    info.break_patches.push(pos);
                }
            }

            Stmt::Continue => {
                let pos = ops.len();
                ops.push(Op::Jump(0)); // placeholder
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

        let (rk, has_redir_expr) = match redir {
            None => (RedirKind::Stdout, false),
            Some(PrintRedir::Overwrite(e)) => {
                self.compile_expr(e, ops);
                (RedirKind::Overwrite, true)
            }
            Some(PrintRedir::Append(e)) => {
                self.compile_expr(e, ops);
                (RedirKind::Append, true)
            }
            Some(PrintRedir::Pipe(e)) => {
                self.compile_expr(e, ops);
                (RedirKind::Pipe, true)
            }
            Some(PrintRedir::Coproc(e)) => {
                self.compile_expr(e, ops);
                (RedirKind::Coproc, true)
            }
        };
        let _ = has_redir_expr;

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
                let idx = self.strings.intern(name);
                ops.push(Op::GetVar(idx));
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
                ops.push(Op::JumpIfFalsePop(0)); // placeholder
                self.compile_expr(right, ops);
                ops.push(Op::ToBool);
                let end_jump = ops.len();
                ops.push(Op::Jump(0)); // placeholder
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
                ops.push(Op::JumpIfTruePop(0)); // placeholder
                self.compile_expr(right, ops);
                ops.push(Op::ToBool);
                let end_jump = ops.len();
                ops.push(Op::Jump(0)); // placeholder
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
                let var_idx = self.strings.intern(name);
                self.compile_expr(rhs, ops);
                if let Some(bop) = op {
                    ops.push(Op::CompoundAssignVar(var_idx, *bop));
                } else {
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

    /// Compile function calls — dispatches special builtins (sub/gsub/split/etc.)
    /// vs generic builtins vs user functions.
    fn compile_call(&mut self, name: &str, args: &[Expr], ops: &mut Vec<Op>) {
        match name {
            "sub" => self.compile_sub_gsub(args, false, ops),
            "gsub" => self.compile_sub_gsub(args, true, ops),
            "split" => self.compile_split(args, ops),
            "patsplit" => self.compile_patsplit(args, ops),
            "match" => self.compile_match(args, ops),
            _ => {
                // Generic: push all args, then call
                for a in args {
                    self.compile_expr(a, ops);
                }
                let name_idx = self.strings.intern(name);
                let argc = args.len() as u16;
                // Check if it's a user function — the VM resolves at runtime, but
                // we use CallUser for names that *aren't* known builtins so the VM
                // can try user functions first.
                ops.push(Op::CallBuiltin(name_idx, argc));
            }
        }
    }

    fn compile_sub_gsub(&mut self, args: &[Expr], is_global: bool, ops: &mut Vec<Op>) {
        // args[0] = re, args[1] = repl, args[2] = optional lvalue target
        self.compile_expr(&args[0], ops);
        self.compile_expr(&args[1], ops);

        let target = if args.len() >= 3 {
            match &args[2] {
                Expr::Var(name) => SubTarget::Var(self.strings.intern(name)),
                Expr::Field(inner) => {
                    self.compile_expr(inner, ops);
                    SubTarget::Field
                }
                Expr::Index { name, indices } => {
                    let arr_idx = self.strings.intern(name);
                    self.compile_array_key(indices, ops);
                    SubTarget::Index(arr_idx)
                }
                _ => SubTarget::Record, // shouldn't happen, but fallback
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
        // split(string, array [, fieldsep])
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
        ops.push(Op::Split { arr: arr_idx, has_fs });
    }

    fn compile_patsplit(&mut self, args: &[Expr], ops: &mut Vec<Op>) {
        // patsplit(string, array [, fieldpat [, seps]])
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
        // match(string, re [, arr])
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

    // ── Helpers ─────────────────────────────────────────────────────────────

    fn compile_array_key(&mut self, indices: &[Expr], ops: &mut Vec<Op>) {
        for ix in indices {
            self.compile_expr(ix, ops);
        }
        if indices.len() > 1 {
            ops.push(Op::JoinArrayKey(indices.len() as u16));
        }
    }
}
