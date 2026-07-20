//! Stage 3 of the fusevm-native migration: compile the awk AST directly to
//! fusevm bytecode (`fusevm::Chunk`), the backend that replaces `compiler.rs` +
//! `vm.rs`. Built and validated alongside the existing interpreter — it grows to
//! cover the whole language one construct at a time, each gated end-to-end on
//! `fusevm::VM` + the [`crate::fusevm_host`] `AwkHost`, before execution flips
//! over to it (stage 4) and vm.rs is removed (stage 5).
//!
//! Coverage so far: `print`/`printf`, string/number literals, the full operator
//! set (arithmetic / comparison-with-strnum / `~`/`!~` / `&&`/`||` / ternary /
//! unary / mod / pow / concat), host-backed scalars (user + special vars), simple
//! and compound assignment, `$n` field read + assignment, arrays incl. multi-dim
//! SUBSEP keys (`a[i,j]`, `k in a`, `delete a[k]`), all control flow (`if`/`else`,
//! `while`, `do-while`, C-style `for`, `next`/`nextfile`/`exit` via `Op::AwkSignal`),
//! `++`/`--`, builtin calls routed to first-class `Op::Awk*` ops, `for (k in a)`,
//! pattern-action rules (empty / expression / `/re/` / `start,end` range), and
//! user functions (recursion, frame-slot params, global access). Unsupported
//! constructs return an error — there is no silent miscompile.
//!
//! Full programs run via [`run_compiled_files`]: a Rust-driven main loop that
//! compiles `BEGIN` / each per-record rule / `END` to separate chunks and drives
//! records through them over the ARGV file list (`run_program_on_input` is the
//! test-only single-string wrapper).
//!
//! Variable model: all awk scalars (user globals and specials) are host-backed
//! through `Op::AwkSpecial*` → `Runtime.symtab_elem_get/set`, so they persist
//! across the separately-run BEGIN / per-record / END chunks and will be shared
//! with user functions. (Frame slots would reset at every chunk boundary.)

use std::collections::HashMap;

use crate::ast::{
    BinOp, Expr, FunctionDef, IncDecOp, IncDecTarget, Pattern, Program, Rule, Stmt, UnaryOp,
};
use crate::error::{Error, Result};

// awk control-flow signal codes carried by `Op::AwkSignal` and read back from
// `vm.awk_signal()` by the main-loop driver (these have no `fusevm::Value` form).
const SIG_NEXT: u8 = 0;
const SIG_EXIT: u8 = 1;
const SIG_NEXTFILE: u8 = 2;
/// Terminates a chunk's main body before appended function code. A top-level
/// `Op::Return` makes fusevm re-execute the whole chunk (its host side effects
/// fire twice), whereas `Op::AwkSignal` halts cleanly; this sentinel code is
/// ignored by the main-loop driver.
const SIG_HALT: u8 = 255;

struct Compiler {
    b: fusevm::ChunkBuilder,
    /// All user functions' parameter lists (name → params): used for call-site
    /// arity padding and, inside a body, param → slot resolution.
    func_params: HashMap<String, Vec<String>>,
    /// Parameters of the function currently being compiled (empty at top level).
    /// These resolve to frame slots (recursion-safe); every other scalar is
    /// host-backed.
    params: Vec<String>,
    /// Next free frame slot for compiler temporaries (e.g. `for (k in a)` loop
    /// state). Each chunk runs in a single frame, so a monotonically increasing
    /// index per chunk is safe and supports nesting.
    next_slot: u16,
}

impl Compiler {
    fn new(func_params: HashMap<String, Vec<String>>) -> Self {
        Self {
            b: fusevm::ChunkBuilder::new(),
            func_params,
            params: Vec::new(),
            next_slot: 0,
        }
    }

    /// Allocate a fresh temporary frame slot.
    fn alloc_slot(&mut self) -> u16 {
        let s = self.next_slot;
        self.next_slot += 1;
        s
    }

    /// Push an array subscript key onto the stack. A single subscript is its raw
    /// value (the host applies CONVFMT). Multi-dimensional subscripts (`a[i,j]`)
    /// are joined with `SUBSEP` into one string key, per awk.
    fn compile_index_key(&mut self, indices: &[Expr]) -> Result<()> {
        let Some((first, rest)) = indices.split_first() else {
            return Err(Error::Runtime(
                "fusevm_compile: empty array subscript".into(),
            ));
        };
        self.compile_expr(first)?;
        for idx in rest {
            self.emit_var_get("SUBSEP");
            self.emit_concat();
            self.compile_expr(idx)?;
            self.emit_concat();
        }
        Ok(())
    }

    /// Emit a CONVFMT-aware string concatenation of the top two stack values.
    fn emit_concat(&mut self) {
        self.b.emit(
            fusevm::Op::CallBuiltin(crate::fusevm_host::BUILTIN_AWK_CONCAT, 2),
            0,
        );
    }

    /// Compile the regex operand of `~`/`!~`: a literal `/re/` becomes its source
    /// string constant; any other expression is used as a dynamic regex string.
    fn compile_regex_operand(&mut self, e: &Expr) -> Result<()> {
        match e {
            Expr::RegexpLiteral(re) => {
                let c = self.b.add_constant(fusevm::Value::str(re.clone()));
                self.b.emit(fusevm::Op::LoadConst(c), 0);
                Ok(())
            }
            other => self.compile_expr(other),
        }
    }

    /// Push a boolean for a pattern used as a condition (range endpoint).
    /// `/re/` becomes `$0 ~ /re/`; an expression pattern is its own truth value.
    fn compile_pattern_cond(&mut self, p: &Pattern) -> Result<()> {
        match p {
            Pattern::Regexp(re) => {
                self.b.emit(fusevm::Op::LoadInt(0), 0);
                self.b.emit(fusevm::Op::AwkFieldGet, 0);
                let c = self.b.add_constant(fusevm::Value::str(re.clone()));
                self.b.emit(fusevm::Op::LoadConst(c), 0);
                self.b.emit(fusevm::Op::RegexMatch, 0);
                Ok(())
            }
            Pattern::Expr(e) => self.compile_expr(e),
            other => Err(Error::Runtime(format!(
                "fusevm_compile: unsupported range endpoint pattern: {other:?}"
            ))),
        }
    }

    /// Compile a rule action (default `print $0` when the action is empty).
    fn compile_action_stmts(&mut self, stmts: &[Stmt]) -> Result<()> {
        if stmts.is_empty() {
            self.compile_stmt(&Stmt::Print {
                args: vec![],
                redir: None,
            })
        } else {
            for s in stmts {
                self.compile_stmt(s)?;
            }
            Ok(())
        }
    }

    /// Compile a range-pattern rule `start,end { action }`. The in-range state is
    /// a hidden host-backed flag (`flag`, a name no user var can collide with) so
    /// it persists across the per-record chunk runs. Semantics: while in range,
    /// run the action and leave the range when `end` matches; when not in range,
    /// enter and run the action when `start` matches (also leaving immediately if
    /// `end` matches the same record).
    fn compile_range_rule(
        &mut self,
        start: &Pattern,
        end: &Pattern,
        stmts: &[Stmt],
        flag: &str,
    ) -> Result<()> {
        self.emit_var_get(flag);
        let jf_not = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);

        // ── in range ──
        self.compile_action_stmts(stmts)?;
        self.compile_pattern_cond(end)?;
        let jf_e1 = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
        self.clear_flag(flag);
        let l_e1 = self.b.current_pos();
        self.b.patch_jump(jf_e1, l_e1);
        let jmp_done = self.b.emit(fusevm::Op::Jump(0), 0);

        // ── not in range ──
        let not_pos = self.b.current_pos();
        self.b.patch_jump(jf_not, not_pos);
        self.compile_pattern_cond(start)?;
        let jf_start = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
        self.b.emit(fusevm::Op::LoadInt(1), 0);
        self.emit_var_set(flag);
        self.compile_action_stmts(stmts)?;
        self.compile_pattern_cond(end)?;
        let jf_e2 = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
        self.clear_flag(flag);
        let l_e2 = self.b.current_pos();
        self.b.patch_jump(jf_e2, l_e2);

        // ── done ──
        let done = self.b.current_pos();
        self.b.patch_jump(jf_start, done);
        self.b.patch_jump(jmp_done, done);
        Ok(())
    }

    fn clear_flag(&mut self, flag: &str) {
        self.b.emit(fusevm::Op::LoadInt(0), 0);
        self.emit_var_set(flag);
    }

    /// `++`/`--` on a var, field, or array element. The current value is read,
    /// the delta applied, the result written back; pre forms leave the new value
    /// on the stack, post forms leave the old.
    fn compile_incdec(&mut self, op: &IncDecOp, target: &IncDecTarget) -> Result<()> {
        let is_pre = matches!(op, IncDecOp::PreInc | IncDecOp::PreDec);
        let delta = match op {
            IncDecOp::PreInc | IncDecOp::PostInc => fusevm::Op::Add,
            IncDecOp::PreDec | IncDecOp::PostDec => fusevm::Op::Sub,
        };
        // With `old` already on the stack, leave [result, new] so the writer pops
        // `new` and leaves `result` (pre → new, post → old).
        let apply = |b: &mut fusevm::ChunkBuilder| {
            if is_pre {
                b.emit(fusevm::Op::LoadInt(1), 0);
                b.emit(delta, 0);
                b.emit(fusevm::Op::Dup, 0);
            } else {
                b.emit(fusevm::Op::Dup, 0);
                b.emit(fusevm::Op::LoadInt(1), 0);
                b.emit(delta, 0);
            }
        };
        match target {
            IncDecTarget::Var(name) => {
                self.emit_var_get(name);
                apply(&mut self.b);
                self.emit_var_set(name);
            }
            IncDecTarget::Field(idx) => {
                self.compile_expr(idx)?;
                self.b.emit(fusevm::Op::AwkFieldGet, 0);
                apply(&mut self.b);
                self.compile_expr(idx)?;
                self.b.emit(fusevm::Op::AwkFieldSet, 0);
            }
            IncDecTarget::Index { name, indices } => {
                self.compile_index_key(indices)?;
                let ni = self.b.add_name(name);
                self.b.emit(fusevm::Op::AwkArrayGet(ni), 0);
                apply(&mut self.b);
                self.compile_index_key(indices)?;
                let ni2 = self.b.add_name(name);
                self.b.emit(fusevm::Op::AwkArraySet(ni2), 0);
            }
        }
        Ok(())
    }

    /// Emit a read of scalar `name`. All awk scalars (user globals and specials
    /// alike) are host-backed via `AwkSpecialGet` → `Runtime.symtab_elem_get`, so
    /// they persist across the separately-run BEGIN / per-record / END chunks and
    /// are shared with user functions (frame slots would not survive a chunk
    /// boundary).
    fn emit_var_get(&mut self, name: &str) {
        if let Some(slot) = self.params.iter().position(|p| p == name) {
            self.b.emit(fusevm::Op::GetSlot(slot as u16), 0);
        } else {
            let n = self.b.add_name(name);
            self.b.emit(fusevm::Op::AwkSpecialGet(n), 0);
        }
    }

    /// Emit a write of the top-of-stack value into scalar `name`. A function
    /// parameter writes its frame slot; any other scalar is host-backed.
    fn emit_var_set(&mut self, name: &str) {
        if let Some(slot) = self.params.iter().position(|p| p == name) {
            self.b.emit(fusevm::Op::SetSlot(slot as u16), 0);
        } else {
            let n = self.b.add_name(name);
            self.b.emit(fusevm::Op::AwkSpecialSet(n), 0);
        }
    }

    /// Emit the fusevm op(s) for a binary operator (operands already on the
    /// stack). Relational operators go through the host strnum comparator
    /// (`BUILTIN_AWK_CMP` → -1/0/1) then a numeric compare against 0, so e.g.
    /// `$1 == "x"` compares as strings while `NR == 2` compares numerically.
    fn emit_binop(&mut self, op: &BinOp) -> Result<()> {
        let rel = match op {
            BinOp::Eq => Some(fusevm::Op::NumEq),
            BinOp::Ne => Some(fusevm::Op::NumNe),
            BinOp::Lt => Some(fusevm::Op::NumLt),
            BinOp::Le => Some(fusevm::Op::NumLe),
            BinOp::Gt => Some(fusevm::Op::NumGt),
            BinOp::Ge => Some(fusevm::Op::NumGe),
            _ => None,
        };
        if let Some(numop) = rel {
            self.b.emit(
                fusevm::Op::CallBuiltin(crate::fusevm_host::BUILTIN_AWK_CMP, 2),
                0,
            );
            self.b.emit(fusevm::Op::LoadInt(0), 0);
            self.b.emit(numop, 0);
            return Ok(());
        }
        // Concatenation goes through the host so numbers stringify via CONVFMT.
        if matches!(op, BinOp::Concat) {
            self.emit_concat();
            return Ok(());
        }
        let fop = match op {
            BinOp::Add => fusevm::Op::Add,
            BinOp::Sub => fusevm::Op::Sub,
            BinOp::Mul => fusevm::Op::Mul,
            BinOp::Div => fusevm::Op::Div,
            BinOp::Mod => fusevm::Op::Mod,
            BinOp::Pow => fusevm::Op::Pow,
            other => {
                return Err(Error::Runtime(format!(
                    "fusevm_compile: unsupported binary operator: {other:?}"
                )))
            }
        };
        self.b.emit(fop, 0);
        Ok(())
    }

    /// Compile a builtin function call to its first-class fusevm awk op. Args are
    /// pushed in source order; the dispatch pops them per the op's protocol.
    /// Unknown / not-yet-supported builtins error out (no silent miscompile).
    fn compile_call(&mut self, name: &str, args: &[Expr]) -> Result<()> {
        // User function call: push args, pad missing params with "" so the callee
        // prologue always pops a fixed count, then Call.
        if let Some(nparams) = self.func_params.get(name).map(|p| p.len()) {
            for a in args {
                self.compile_expr(a)?;
            }
            for _ in args.len()..nparams {
                let c = self.b.add_constant(fusevm::Value::str(""));
                self.b.emit(fusevm::Op::LoadConst(c), 0);
            }
            let ni = self.b.add_name(name);
            self.b.emit(fusevm::Op::Call(ni, nparams as u8), 0);
            return Ok(());
        }

        // sub/gsub take an lvalue target whose *name* (not value) is passed; the
        // host writes the result back. Default target is `$0`.
        if name == "sub" || name == "gsub" {
            return self.compile_sub_gsub(name, args);
        }

        for a in args {
            self.compile_expr(a)?;
        }
        let argc = args.len() as u8;
        let op = match name {
            "length" => fusevm::Op::AwkLength(argc),
            "substr" => fusevm::Op::AwkSubstr(argc),
            "sprintf" => fusevm::Op::AwkSprintf(argc),
            "index" => fusevm::Op::AwkIndex,
            "match" => fusevm::Op::AwkMatch,
            "tolower" => fusevm::Op::AwkToLower,
            "toupper" => fusevm::Op::AwkToUpper,
            "int" => fusevm::Op::AwkInt,
            "sqrt" => fusevm::Op::AwkSqrt,
            "sin" => fusevm::Op::AwkSin,
            "cos" => fusevm::Op::AwkCos,
            "exp" => fusevm::Op::AwkExp,
            "log" => fusevm::Op::AwkLog,
            "atan2" => fusevm::Op::AwkAtan2,
            other => {
                return Err(Error::Runtime(format!(
                    "fusevm_compile: unsupported builtin call: {other}"
                )))
            }
        };
        self.b.emit(op, 0);
        Ok(())
    }

    /// Append every user function's code after the chunk's main body. The main
    /// body is terminated with `Op::AwkSignal(SIG_HALT)` (a clean halt — a
    /// top-level `Op::Return` would re-execute the chunk) so it can't fall into
    /// function code. Each function gets a `sub_entry`, a prologue that pops the
    /// arity-padded args into slots (last param first), its body, and a default
    /// `""` return. Appended to every chunk so calls resolve regardless of block.
    fn append_functions(&mut self, funcs: &HashMap<String, FunctionDef>) -> Result<()> {
        if funcs.is_empty() {
            return Ok(());
        }
        self.b.emit(fusevm::Op::AwkSignal(SIG_HALT), 0);
        let mut names: Vec<&String> = funcs.keys().collect();
        names.sort();
        for name in names {
            let fd = &funcs[name];
            let entry = self.b.current_pos();
            let ni = self.b.add_name(name);
            self.b.add_sub_entry(ni, entry);
            for i in (0..fd.params.len()).rev() {
                self.b.emit(fusevm::Op::SetSlot(i as u16), 0);
            }
            // Params occupy slots 0..nparams; temporaries (for-in) allocate above.
            self.params = fd.params.clone();
            self.next_slot = fd.params.len() as u16;
            for s in &fd.body {
                self.compile_stmt(s)?;
            }
            self.params.clear();
            self.next_slot = 0;
            let c = self.b.add_constant(fusevm::Value::str(""));
            self.b.emit(fusevm::Op::LoadConst(c), 0);
            self.b.emit(fusevm::Op::ReturnValue, 0);
        }
        Ok(())
    }

    /// `sub(re, repl [, target])` / `gsub(...)`. Pushes `re`, `repl`, and (for the
    /// 3-arg form) the target variable's *name* string; the host substitutes and
    /// writes back. Returns the replacement count. Default target is `$0`; field /
    /// array-element targets aren't lowered yet.
    fn compile_sub_gsub(&mut self, name: &str, args: &[Expr]) -> Result<()> {
        if !(2..=3).contains(&args.len()) {
            return Err(Error::Runtime(format!(
                "fusevm_compile: {name} expects 2 or 3 arguments"
            )));
        }
        self.compile_regex_operand(&args[0])?;
        self.compile_expr(&args[1])?;
        if let Some(target) = args.get(2) {
            match target {
                Expr::Var(vname) => {
                    let c = self.b.add_constant(fusevm::Value::str(vname.clone()));
                    self.b.emit(fusevm::Op::LoadConst(c), 0);
                }
                _ => {
                    return Err(Error::Runtime(format!(
                        "fusevm_compile: {name} with a field/array-element target not yet supported"
                    )))
                }
            }
        }
        let argc = args.len() as u8;
        let op = if name == "sub" {
            fusevm::Op::AwkSub(argc)
        } else {
            fusevm::Op::AwkGsub(argc)
        };
        self.b.emit(op, 0);
        Ok(())
    }

    fn compile_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            // Debug line markers never reach the fusevm backend (debug runs on vm.rs).
            Stmt::SrcLine(_) => Ok(()),
            Stmt::Print { args, redir: None } => {
                if args.is_empty() {
                    // bare `print` ≡ `print $0`
                    self.b.emit(fusevm::Op::LoadInt(0), 0);
                    self.b.emit(fusevm::Op::AwkFieldGet, 0);
                    self.b.emit(fusevm::Op::AwkPrint(1), 0);
                } else {
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.b.emit(fusevm::Op::AwkPrint(args.len() as u8), 0);
                }
                Ok(())
            }
            Stmt::Expr(e) => {
                self.compile_expr(e)?;
                self.b.emit(fusevm::Op::Pop, 0);
                Ok(())
            }
            // `printf fmt, …` — args pushed in source order (fmt first); the host
            // formats and emits with no trailing ORS.
            Stmt::Printf { args, redir: None } => {
                for a in args {
                    self.compile_expr(a)?;
                }
                self.b.emit(fusevm::Op::AwkPrintf(args.len() as u8), 0);
                Ok(())
            }
            Stmt::If { cond, then_, else_ } => {
                self.compile_expr(cond)?;
                let jf = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
                for s in then_ {
                    self.compile_stmt(s)?;
                }
                if else_.is_empty() {
                    let end = self.b.current_pos();
                    self.b.patch_jump(jf, end);
                } else {
                    let jend = self.b.emit(fusevm::Op::Jump(0), 0);
                    let else_start = self.b.current_pos();
                    self.b.patch_jump(jf, else_start);
                    for s in else_ {
                        self.compile_stmt(s)?;
                    }
                    let end = self.b.current_pos();
                    self.b.patch_jump(jend, end);
                }
                Ok(())
            }
            Stmt::While { cond, body } => {
                let loop_start = self.b.current_pos();
                self.compile_expr(cond)?;
                let jf = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.b.emit(fusevm::Op::Jump(loop_start), 0);
                let end = self.b.current_pos();
                self.b.patch_jump(jf, end);
                Ok(())
            }
            Stmt::ForC {
                init,
                cond,
                iter,
                body,
            } => {
                if let Some(e) = init {
                    self.compile_expr(e)?;
                    self.b.emit(fusevm::Op::Pop, 0);
                }
                let loop_start = self.b.current_pos();
                let jf = match cond {
                    Some(c) => {
                        self.compile_expr(c)?;
                        Some(self.b.emit(fusevm::Op::JumpIfFalse(0), 0))
                    }
                    None => None,
                };
                for s in body {
                    self.compile_stmt(s)?;
                }
                if let Some(e) = iter {
                    self.compile_expr(e)?;
                    self.b.emit(fusevm::Op::Pop, 0);
                }
                self.b.emit(fusevm::Op::Jump(loop_start), 0);
                if let Some(jf) = jf {
                    let end = self.b.current_pos();
                    self.b.patch_jump(jf, end);
                }
                Ok(())
            }
            // `for (k in a) body` — materialize the keys as a fusevm Array (host
            // builtin), then iterate by index. Loop temps are frame slots; the
            // loop variable `k` is host-backed like any scalar.
            Stmt::ForIn { var, arr, body } => {
                let keys_slot = self.alloc_slot();
                let len_slot = self.alloc_slot();
                let i_slot = self.alloc_slot();

                let c = self.b.add_constant(fusevm::Value::str(arr.clone()));
                self.b.emit(fusevm::Op::LoadConst(c), 0);
                self.b.emit(
                    fusevm::Op::CallBuiltin(crate::fusevm_host::BUILTIN_AWK_KEYS, 1),
                    0,
                );
                self.b.emit(fusevm::Op::Dup, 0);
                self.b.emit(fusevm::Op::SetSlot(keys_slot), 0);
                self.b.emit(
                    fusevm::Op::CallBuiltin(crate::fusevm_host::BUILTIN_ARRAY_LEN, 1),
                    0,
                );
                self.b.emit(fusevm::Op::SetSlot(len_slot), 0);
                self.b.emit(fusevm::Op::LoadInt(0), 0);
                self.b.emit(fusevm::Op::SetSlot(i_slot), 0);

                let loop_start = self.b.current_pos();
                self.b.emit(fusevm::Op::GetSlot(i_slot), 0);
                self.b.emit(fusevm::Op::GetSlot(len_slot), 0);
                self.b.emit(fusevm::Op::NumLt, 0);
                let jf = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);

                self.b.emit(fusevm::Op::GetSlot(i_slot), 0);
                self.b.emit(fusevm::Op::SlotArrayGet(keys_slot), 0);
                self.emit_var_set(var);
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.b.emit(fusevm::Op::GetSlot(i_slot), 0);
                self.b.emit(fusevm::Op::LoadInt(1), 0);
                self.b.emit(fusevm::Op::Add, 0);
                self.b.emit(fusevm::Op::SetSlot(i_slot), 0);
                self.b.emit(fusevm::Op::Jump(loop_start), 0);

                let end = self.b.current_pos();
                self.b.patch_jump(jf, end);
                Ok(())
            }
            Stmt::DoWhile { body, cond } => {
                let loop_start = self.b.current_pos();
                for s in body {
                    self.compile_stmt(s)?;
                }
                self.compile_expr(cond)?;
                self.b.emit(fusevm::Op::JumpIfTrue(loop_start), 0);
                Ok(())
            }
            // Control-flow signals: the chunk halts and the driver reacts.
            Stmt::Next => {
                self.b.emit(fusevm::Op::AwkSignal(SIG_NEXT), 0);
                Ok(())
            }
            Stmt::NextFile => {
                self.b.emit(fusevm::Op::AwkSignal(SIG_NEXTFILE), 0);
                Ok(())
            }
            // `exit [code]` — the exit code is not yet threaded (signal only).
            Stmt::Exit(_code) => {
                self.b.emit(fusevm::Op::AwkSignal(SIG_EXIT), 0);
                Ok(())
            }
            // `return [expr]` — bare return yields "" (awk).
            Stmt::Return(expr) => {
                match expr {
                    Some(e) => self.compile_expr(e)?,
                    None => {
                        let c = self.b.add_constant(fusevm::Value::str(""));
                        self.b.emit(fusevm::Op::LoadConst(c), 0);
                    }
                }
                self.b.emit(fusevm::Op::ReturnValue, 0);
                Ok(())
            }
            // `delete a[k]` (single key). `delete a` (whole array) needs a clear
            // op and is deferred.
            Stmt::Delete {
                name,
                indices: Some(idxs),
            } => {
                self.compile_index_key(idxs)?;
                let ni = self.b.add_name(name);
                self.b.emit(fusevm::Op::AwkArrayDelete(ni), 0);
                Ok(())
            }
            // `delete a` — clear the whole array.
            Stmt::Delete {
                name,
                indices: None,
            } => {
                let ni = self.b.add_name(name);
                self.b.emit(fusevm::Op::AwkArrayClear(ni), 0);
                Ok(())
            }
            other => Err(Error::Runtime(format!(
                "fusevm_compile: unsupported statement: {other:?}"
            ))),
        }
    }

    fn compile_expr(&mut self, e: &Expr) -> Result<()> {
        match e {
            Expr::Str(s) => {
                let c = self.b.add_constant(fusevm::Value::str(s.clone()));
                self.b.emit(fusevm::Op::LoadConst(c), 0);
            }
            Expr::Number(n) => {
                self.b.emit(fusevm::Op::LoadFloat(*n), 0);
            }
            // awk has one numeric type (double); load integer literals as floats
            // too so `/` keeps awk semantics.
            Expr::IntegerLiteral(s) => {
                let n: f64 = s.parse().unwrap_or(0.0);
                self.b.emit(fusevm::Op::LoadFloat(n), 0);
            }
            Expr::Var(name) => self.emit_var_get(name),
            Expr::Field(idx) => {
                self.compile_expr(idx)?;
                self.b.emit(fusevm::Op::AwkFieldGet, 0);
            }
            // Assignment `name = rhs` and compound `name op= rhs`. The assigned
            // value stays on the stack so `y = (x = 1)` works; statement context
            // pops it.
            Expr::Assign { name, op, rhs } => {
                if let Some(bop) = op {
                    self.emit_var_get(name);
                    self.compile_expr(rhs)?;
                    self.emit_binop(bop)?;
                } else {
                    self.compile_expr(rhs)?;
                }
                self.b.emit(fusevm::Op::Dup, 0);
                self.emit_var_set(name);
            }
            // `lhs ~ /re/` and `lhs !~ /re/` → fusevm RegexMatch (boolean) via the
            // shell-host regex backing. `!~` negates to a numeric 0/1.
            Expr::Binary {
                op: op @ (BinOp::Match | BinOp::NotMatch),
                left,
                right,
            } => {
                self.compile_expr(left)?;
                self.compile_regex_operand(right)?;
                self.b.emit(fusevm::Op::RegexMatch, 0);
                if matches!(op, BinOp::NotMatch) {
                    self.b.emit(fusevm::Op::LoadInt(0), 0);
                    self.b.emit(fusevm::Op::NumEq, 0);
                }
            }
            // Bare `/re/` in expression context ≡ `$0 ~ /re/`.
            Expr::RegexpLiteral(re) => {
                self.b.emit(fusevm::Op::LoadInt(0), 0);
                self.b.emit(fusevm::Op::AwkFieldGet, 0);
                let c = self.b.add_constant(fusevm::Value::str(re.clone()));
                self.b.emit(fusevm::Op::LoadConst(c), 0);
                self.b.emit(fusevm::Op::RegexMatch, 0);
            }
            // Short-circuit `&&` / `||`, producing a numeric 0/1.
            Expr::Binary {
                op: BinOp::And,
                left,
                right,
            } => {
                self.compile_expr(left)?;
                let jf1 = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
                self.compile_expr(right)?;
                let jf2 = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
                self.b.emit(fusevm::Op::LoadInt(1), 0);
                let jend = self.b.emit(fusevm::Op::Jump(0), 0);
                let lfalse = self.b.current_pos();
                self.b.patch_jump(jf1, lfalse);
                self.b.patch_jump(jf2, lfalse);
                self.b.emit(fusevm::Op::LoadInt(0), 0);
                let lend = self.b.current_pos();
                self.b.patch_jump(jend, lend);
            }
            Expr::Binary {
                op: BinOp::Or,
                left,
                right,
            } => {
                self.compile_expr(left)?;
                let jt1 = self.b.emit(fusevm::Op::JumpIfTrue(0), 0);
                self.compile_expr(right)?;
                let jt2 = self.b.emit(fusevm::Op::JumpIfTrue(0), 0);
                self.b.emit(fusevm::Op::LoadInt(0), 0);
                let jend = self.b.emit(fusevm::Op::Jump(0), 0);
                let ltrue = self.b.current_pos();
                self.b.patch_jump(jt1, ltrue);
                self.b.patch_jump(jt2, ltrue);
                self.b.emit(fusevm::Op::LoadInt(1), 0);
                let lend = self.b.current_pos();
                self.b.patch_jump(jend, lend);
            }
            Expr::Binary { op, left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.emit_binop(op)?;
            }
            // Unary `-x` / `+x` (numeric coercion) / `!x` (logical not → 0/1).
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => {
                    self.compile_expr(expr)?;
                    self.b.emit(fusevm::Op::Negate, 0);
                }
                UnaryOp::Pos => {
                    self.b.emit(fusevm::Op::LoadFloat(0.0), 0);
                    self.compile_expr(expr)?;
                    self.b.emit(fusevm::Op::Add, 0);
                }
                UnaryOp::Not => {
                    self.compile_expr(expr)?;
                    let jf = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
                    self.b.emit(fusevm::Op::LoadInt(0), 0);
                    let jend = self.b.emit(fusevm::Op::Jump(0), 0);
                    let ltrue = self.b.current_pos();
                    self.b.patch_jump(jf, ltrue);
                    self.b.emit(fusevm::Op::LoadInt(1), 0);
                    let lend = self.b.current_pos();
                    self.b.patch_jump(jend, lend);
                }
            },
            // `cond ? then : else`
            Expr::Ternary { cond, then_, else_ } => {
                self.compile_expr(cond)?;
                let jf = self.b.emit(fusevm::Op::JumpIfFalse(0), 0);
                self.compile_expr(then_)?;
                let jend = self.b.emit(fusevm::Op::Jump(0), 0);
                let lelse = self.b.current_pos();
                self.b.patch_jump(jf, lelse);
                self.compile_expr(else_)?;
                let lend = self.b.current_pos();
                self.b.patch_jump(jend, lend);
            }
            // `++x` / `x++` / `--x` / `x--` on a var, field, or array element.
            // Pre forms leave the new value; post forms leave the old value.
            Expr::IncDec { op, target } => self.compile_incdec(op, target)?,
            Expr::Call { name, args } => self.compile_call(name, args)?,
            // `a[k]` read. Single subscript only for now (multi-dim SUBSEP join
            // is a later refinement).
            Expr::Index { name, indices } => {
                self.compile_index_key(indices)?;
                let ni = self.b.add_name(name);
                self.b.emit(fusevm::Op::AwkArrayGet(ni), 0);
            }
            // `a[k] = rhs` / `a[k] op= rhs`. Leaves the assigned value on the
            // stack (via Dup) so it's usable as an expression; statement context
            // pops it.
            Expr::AssignIndex {
                name,
                indices,
                op,
                rhs,
            } => {
                if let Some(bop) = op {
                    self.compile_index_key(indices)?;
                    let ni = self.b.add_name(name);
                    self.b.emit(fusevm::Op::AwkArrayGet(ni), 0);
                    self.compile_expr(rhs)?;
                    self.emit_binop(bop)?;
                } else {
                    self.compile_expr(rhs)?;
                }
                self.b.emit(fusevm::Op::Dup, 0);
                self.compile_index_key(indices)?;
                let ni = self.b.add_name(name);
                self.b.emit(fusevm::Op::AwkArraySet(ni), 0);
            }
            // `k in a` → Bool.
            Expr::In { key, arr } => {
                self.compile_expr(key)?;
                let ni = self.b.add_name(arr);
                self.b.emit(fusevm::Op::AwkArrayExists(ni), 0);
            }
            // `$n = rhs` / `$n op= rhs` (`$0 = …` resplits via the host). Leaves
            // the assigned value on the stack; statement context pops it. The
            // host's `field_set` pops the index then the value.
            Expr::AssignField { field, op, rhs } => {
                if let Some(bop) = op {
                    self.compile_expr(field)?;
                    self.b.emit(fusevm::Op::AwkFieldGet, 0);
                    self.compile_expr(rhs)?;
                    self.emit_binop(bop)?;
                } else {
                    self.compile_expr(rhs)?;
                }
                self.b.emit(fusevm::Op::Dup, 0);
                self.compile_expr(field)?;
                self.b.emit(fusevm::Op::AwkFieldSet, 0);
            }
            other => {
                return Err(Error::Runtime(format!(
                    "fusevm_compile: unsupported expression: {other:?}"
                )))
            }
        }
        Ok(())
    }
}

/// Compile the `BEGIN` actions of `prog` to a fusevm chunk. (Scope grows each
/// migration step; non-`BEGIN` rules and unsupported nodes error out for now.)
pub(crate) fn compile_begin_only(prog: &Program) -> Result<fusevm::Chunk> {
    let mut c = Compiler::new(func_param_map(prog));
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::Begin) {
            for s in &rule.stmts {
                c.compile_stmt(s)?;
            }
        }
    }
    c.append_functions(&prog.funcs)?;
    Ok(c.b.build())
}

/// Name → parameter list for every user function (call-site arity + resolution).
fn func_param_map(prog: &Program) -> HashMap<String, Vec<String>> {
    prog.funcs
        .iter()
        .map(|(k, fd)| (k.clone(), fd.params.clone()))
        .collect()
}

/// Compile a statement list (a `BEGIN`/`END` block) to a chunk, with the
/// program's functions appended.
fn compile_action(stmts: &[Stmt], funcs: &HashMap<String, FunctionDef>) -> Result<fusevm::Chunk> {
    let mut c = Compiler::new(
        funcs
            .iter()
            .map(|(k, fd)| (k.clone(), fd.params.clone()))
            .collect(),
    );
    for s in stmts {
        c.compile_stmt(s)?;
    }
    c.append_functions(funcs)?;
    Ok(c.b.build())
}

/// Compile a per-record rule. Empty pattern = every record; an expression or
/// `/re/` pattern guards the action; `start,end` is a stateful range. `rule_id`
/// makes a range rule's hidden in-range flag unique. The action defaults to
/// `print $0` when empty.
fn compile_main_rule(
    rule: &Rule,
    funcs: &HashMap<String, FunctionDef>,
    rule_id: usize,
) -> Result<fusevm::Chunk> {
    let mut c = Compiler::new(
        funcs
            .iter()
            .map(|(k, fd)| (k.clone(), fd.params.clone()))
            .collect(),
    );

    if let Pattern::Range(start, end) = &rule.pattern {
        // The flag name uses a control-char prefix no user variable can have.
        let flag = format!("\u{1}range{rule_id}");
        c.compile_range_rule(start, end, &rule.stmts, &flag)?;
        c.append_functions(funcs)?;
        return Ok(c.b.build());
    }

    let jf = match &rule.pattern {
        Pattern::Empty => None,
        Pattern::Expr(e) => {
            c.compile_expr(e)?;
            Some(c.b.emit(fusevm::Op::JumpIfFalse(0), 0))
        }
        // `/re/ { … }` ≡ `$0 ~ /re/`.
        Pattern::Regexp(_) => {
            c.compile_pattern_cond(&rule.pattern)?;
            Some(c.b.emit(fusevm::Op::JumpIfFalse(0), 0))
        }
        other => {
            return Err(Error::Runtime(format!(
                "fusevm_compile: unsupported pattern: {other:?}"
            )))
        }
    };
    c.compile_action_stmts(&rule.stmts)?;
    if let Some(jf) = jf {
        let end = c.b.current_pos();
        c.b.patch_jump(jf, end);
    }
    c.append_functions(funcs)?;
    Ok(c.b.build())
}

fn is_main_rule(r: &Rule) -> bool {
    !matches!(r.pattern, Pattern::Begin | Pattern::End)
}

/// Run a chunk on a fresh fusevm VM with awkrs's host installed. Returns any awk
/// control-flow signal the chunk raised (`next`/`exit`/…), propagating a host
/// fatal as `Err`.
fn run_chunk(rt: &mut crate::runtime::Runtime, chunk: &fusevm::Chunk) -> Result<Option<u8>> {
    let mut vm = fusevm::VM::new(chunk.clone());
    crate::fusevm_host::install_awk_host(&mut vm);
    {
        let _g = crate::fusevm_host::RuntimeGuard::enter(rt);
        let _ = vm.run();
    }
    let signal = vm.awk_signal();
    match crate::fusevm_host::take_host_error() {
        Some(e) => Err(e),
        None => Ok(signal),
    }
}

/// A fully compiled awk program: `BEGIN` blocks, per-record rules, and `END`
/// blocks as separate fusevm chunks. Produced before any input is read, so the
/// caller can detect unsupported constructs (a compile error) and fall back to
/// the interpreter without having consumed stdin.
pub(crate) struct NativeProgram {
    begin: Vec<fusevm::Chunk>,
    main: Vec<fusevm::Chunk>,
    end: Vec<fusevm::Chunk>,
}

/// Compile a whole program to fusevm chunks. Returns an `Err` (message prefixed
/// `fusevm_compile:`) for any construct the backend doesn't support yet.
pub(crate) fn compile_program_native(prog: &Program) -> Result<NativeProgram> {
    Ok(NativeProgram {
        begin: prog
            .rules
            .iter()
            .filter(|r| matches!(r.pattern, Pattern::Begin))
            .map(|r| compile_action(&r.stmts, &prog.funcs))
            .collect::<Result<_>>()?,
        main: prog
            .rules
            .iter()
            .filter(|r| is_main_rule(r))
            .enumerate()
            .map(|(i, r)| compile_main_rule(r, &prog.funcs, i))
            .collect::<Result<_>>()?,
        end: prog
            .rules
            .iter()
            .filter(|r| matches!(r.pattern, Pattern::End))
            .map(|r| compile_action(&r.stmts, &prog.funcs))
            .collect::<Result<_>>()?,
    })
}

/// Disassemble a whole program to a fusevm bytecode listing: every `BEGIN`
/// block, per-record rule, and `END` block as a labelled section, via the shared
/// `fusevm::Chunk::disassemble`. Returns an `Err` (prefixed `fusevm_compile:`)
/// for any construct the backend doesn't support yet.
pub(crate) fn disassemble_program(prog: &Program) -> Result<String> {
    let np = compile_program_native(prog)?;
    let mut out = String::new();
    for (i, ch) in np.begin.iter().enumerate() {
        out.push_str(&format!("; awkrs fusevm — BEGIN[{i}]\n{}\n", ch.disassemble()));
    }
    for (i, ch) in np.main.iter().enumerate() {
        out.push_str(&format!("; awkrs fusevm — rule[{i}]\n{}\n", ch.disassemble()));
    }
    for (i, ch) in np.end.iter().enumerate() {
        out.push_str(&format!("; awkrs fusevm — END[{i}]\n{}\n", ch.disassemble()));
    }
    Ok(out)
}

/// Run a compiled program over `input` (records split on '\n') on the given
/// Runtime: `BEGIN`, then every record through the main rules, then `END`.
/// Returns the bytes written to the record stream. Test-only thin wrapper over
/// [`run_compiled_files`] (the production driver takes the ARGV file list).
#[cfg(test)]
pub(crate) fn run_compiled(
    p: &NativeProgram,
    input: &str,
    rt: &mut crate::runtime::Runtime,
) -> Result<Vec<u8>> {
    run_compiled_files(p, &[(String::new(), input.to_string())], rt)
}

/// Run a compiled program over a sequence of `(filename, content)` input sources
/// (the awk ARGV file list). `NR` is cumulative; `FNR` and `FILENAME` reset at
/// each file. `next` skips the record's remaining rules, `nextfile` advances to
/// the next file, `exit` stops the record loop and runs `END`.
pub(crate) fn run_compiled_files(
    p: &NativeProgram,
    sources: &[(String, String)],
    rt: &mut crate::runtime::Runtime,
) -> Result<Vec<u8>> {
    let (begin, main, end) = (&p.begin, &p.main, &p.end);
    let mut exiting = false;
    for ch in begin {
        if run_chunk(rt, ch)? == Some(SIG_EXIT) {
            exiting = true;
            break;
        }
    }

    if !exiting && (!main.is_empty() || !end.is_empty()) {
        'files: for (fname, content) in sources {
            rt.filename = fname.clone();
            rt.fnr = 0.0;
            'records: for line in content.lines() {
                rt.nr += 1.0;
                rt.fnr += 1.0;
                // FS is read each record so a rule changing it affects the next.
                let fs = rt
                    .vars
                    .get("FS")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| " ".to_string());
                rt.set_field_sep_split(&fs, line);
                for ch in main {
                    match run_chunk(rt, ch)? {
                        Some(SIG_NEXT) => continue 'records,
                        Some(SIG_NEXTFILE) => continue 'files,
                        Some(SIG_EXIT) => break 'files,
                        _ => {}
                    }
                }
            }
        }
    }

    // END always runs (even after `exit`); `exit` inside END just stops it.
    for ch in end {
        if run_chunk(rt, ch)? == Some(SIG_EXIT) {
            break;
        }
    }
    Ok(rt.print_buf.clone())
}

/// Test/helper entry: compile `prog` and run it over `input` with a fresh
/// Runtime, returning the record-stream output.
#[cfg(test)]
pub(crate) fn run_program_on_input(prog: &Program, input: &str) -> Result<Vec<u8>> {
    let np = compile_program_native(prog)?;
    let mut rt = crate::runtime::Runtime::new();
    run_compiled(&np, input, &mut rt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fusevm_host::{install_awk_host, RuntimeGuard};
    use crate::runtime::Runtime;

    fn run_with(src: &str, setup: impl FnOnce(&mut Runtime)) -> Vec<u8> {
        let prog = crate::parser::parse_program(src).unwrap();
        let chunk = compile_begin_only(&prog).unwrap();
        let mut vm = fusevm::VM::new(chunk);
        install_awk_host(&mut vm);
        let mut rt = Runtime::new();
        setup(&mut rt);
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let _ = vm.run();
        }
        rt.print_buf.clone()
    }

    fn run_begin(src: &str) -> Vec<u8> {
        run_with(src, |_| {})
    }

    #[test]
    fn compiles_begin_print_string_and_arithmetic() {
        assert_eq!(run_begin(r#"BEGIN { print "hello" }"#), b"hello\n");
        assert_eq!(run_begin("BEGIN { print 2 + 3 }"), b"5\n");
        assert_eq!(run_begin("BEGIN { print 10 - 4 * 2 }"), b"2\n");
        assert_eq!(run_begin("BEGIN { print 9 / 2 }"), b"4.5\n");
    }

    #[test]
    fn compiles_scalar_variables() {
        assert_eq!(
            run_begin("BEGIN { x = 5; print x; print x + 3 }"),
            b"5\n8\n"
        );
    }

    #[test]
    fn compiles_special_var_assignment_affects_print() {
        // OFS set via AwkSpecialSet must change how a multi-arg print joins.
        assert_eq!(run_begin(r#"BEGIN { OFS = "-"; print 1, 2 }"#), b"1-2\n");
    }

    #[test]
    fn compiles_field_reads() {
        // Pre-load a record so `$2` resolves; BEGIN reads it through AwkFieldGet.
        let out = run_with("BEGIN { print $2 }", |rt| {
            rt.set_field_sep_split(" ", "alpha beta gamma");
        });
        assert_eq!(out, b"beta\n");
    }

    #[test]
    fn compiles_if_else() {
        assert_eq!(
            run_begin("BEGIN { if (1 < 2) print \"y\"; else print \"n\" }"),
            b"y\n"
        );
        assert_eq!(
            run_begin("BEGIN { if (3 < 2) print \"y\"; else print \"n\" }"),
            b"n\n"
        );
    }

    #[test]
    fn compiles_while_loop_with_compound_assign() {
        // sum 1..3 via a while loop and `+=`
        let out = run_begin("BEGIN { i = 1; s = 0; while (i <= 3) { s += i; i += 1 } print s }");
        assert_eq!(out, b"6\n");
    }

    #[test]
    fn compiles_builtin_calls() {
        assert_eq!(run_begin(r#"BEGIN { print length("hello") }"#), b"5\n");
        assert_eq!(
            run_begin(r#"BEGIN { print substr("hello", 2, 3) }"#),
            b"ell\n"
        );
        assert_eq!(run_begin(r#"BEGIN { print toupper("abc") }"#), b"ABC\n");
        assert_eq!(run_begin(r#"BEGIN { print index("hello", "ll") }"#), b"3\n");
        assert_eq!(run_begin("BEGIN { print sqrt(9) }"), b"3\n");
    }

    #[test]
    fn compiles_field_assignment() {
        // $2 = "X" rebuilds the record
        let out = run_with(r#"BEGIN { $2 = "X"; print $0 }"#, |rt| {
            rt.set_field_sep_split(" ", "a b c");
        });
        assert_eq!(out, b"a X c\n");
        // $0 = ... resplits, so $1 reads the new first field
        assert_eq!(run_begin(r#"BEGIN { $0 = "x y z"; print $1 }"#), b"x\n");
    }

    #[test]
    fn compiles_printf_statement() {
        assert_eq!(run_begin(r#"BEGIN { printf "%d-%s", 5, "hi" }"#), b"5-hi");
    }

    #[test]
    fn compiles_array_get_set_in_delete() {
        assert_eq!(run_begin(r#"BEGIN { a["x"] = 5; print a["x"] }"#), b"5\n");
        assert_eq!(
            run_begin("BEGIN { a[1] = 10; a[1] += 5; print a[1] }"),
            b"15\n"
        );
        assert_eq!(
            run_begin(r#"BEGIN { a["k"] = 1; if ("k" in a) print "yes" }"#),
            b"yes\n"
        );
        assert_eq!(
            run_begin(
                r#"BEGIN { a["k"] = 1; delete a["k"]; if ("k" in a) print "y"; else print "n" }"#
            ),
            b"n\n"
        );
    }

    #[test]
    fn incr_decr_var_field_array() {
        // pre vs post on a var
        assert_eq!(run_begin("BEGIN { x = 5; print x++, x, ++x }"), b"5 6 7\n");
        assert_eq!(run_begin("BEGIN { x = 5; print x--, --x }"), b"5 3\n");
        // post-increment on an array element used as a counter
        let out = run_prog(
            "{ seen[$1]++ } END { print seen[\"a\"], seen[\"b\"] }",
            "a\nb\na\na\n",
        );
        assert_eq!(out, b"3 1\n");
        // increment a field
        assert_eq!(run_prog("{ $1++; print $1 }", "10\n20\n"), b"11\n21\n");
    }

    #[test]
    fn for_in_array_iteration() {
        // sum values over all keys (order-independent)
        let out = run_begin(
            "BEGIN { a[\"x\"]=10; a[\"y\"]=20; a[\"z\"]=30; s=0; for (k in a) s += a[k]; print s }",
        );
        assert_eq!(out, b"60\n");
        // count distinct keys (++ not yet implemented; use += 1)
        let out2 = run_prog(
            "{ seen[$1] += 1 } END { n = 0; for (k in seen) n += 1; print n }",
            "a\nb\na\nc\n",
        );
        assert_eq!(out2, b"3\n");
    }

    #[test]
    fn sub_gsub_and_delete_array() {
        // gsub on $0 (default target) mutates the record
        assert_eq!(
            run_prog("{ gsub(/o/, \"0\"); print }", "foo\nbox\n"),
            b"f00\nb0x\n"
        );
        // sub on a named variable target
        assert_eq!(
            run_begin(r#"BEGIN { s = "aaa"; sub(/a/, "b", s); print s }"#),
            b"baa\n"
        );
        // delete whole array: old keys gone, new key present
        let out = run_begin(
            r#"BEGIN { a[1]=1; a[2]=2; delete a; a[3]=3; print (1 in a), (2 in a), (3 in a) }"#,
        );
        assert_eq!(out, b"0 0 1\n");
    }

    #[test]
    fn range_pattern() {
        // print lines from the one matching /start/ through the one matching /end/
        let out = run_prog("/start/,/end/ { print }", "a\nstart\nb\nc\nend\nd\n");
        assert_eq!(out, b"start\nb\nc\nend\n");
    }

    #[test]
    fn range_pattern_reopens() {
        // a second range on the same rule re-activates after closing
        let out = run_prog("/o/,/c/ { print }", "x\no1\nc1\ny\no2\nc2\nz\n");
        assert_eq!(out, b"o1\nc1\no2\nc2\n");
    }

    #[test]
    fn multidim_array_subscripts() {
        // a[i,j] keys join with SUBSEP; same (i,j) addresses the same cell
        let out = run_prog(
            "{ a[$1, $2] = $3 } END { print a[\"x\", \"y\"], a[\"p\", \"q\"] }",
            "x y 1\np q 2\n",
        );
        assert_eq!(out, b"1 2\n");
    }

    #[test]
    fn compiles_for_loop() {
        let out = run_begin("BEGIN { s = 0; for (i = 1; i <= 3; i += 1) s += i; print s }");
        assert_eq!(out, b"6\n");
    }

    fn run_prog(src: &str, input: &str) -> Vec<u8> {
        let prog = crate::parser::parse_program(src).unwrap();
        run_program_on_input(&prog, input).unwrap()
    }

    #[test]
    fn main_loop_per_record_field_print() {
        assert_eq!(run_prog("{ print $1 }", "a b\nc d\n"), b"a\nc\n");
    }

    #[test]
    fn main_loop_bare_print_is_whole_record() {
        assert_eq!(run_prog("{ print }", "hello\nworld\n"), b"hello\nworld\n");
    }

    #[test]
    fn main_loop_pattern_filters_records() {
        assert_eq!(run_prog("NR == 2 { print $2 }", "a b\nc d\ne f\n"), b"d\n");
    }

    #[test]
    fn begin_main_end_sequence_and_nr() {
        let out = run_prog(
            r#"BEGIN { print "start" } { print $1 } END { print "end"; print NR }"#,
            "x y\nz w\n",
        );
        assert_eq!(out, b"start\nx\nz\nend\n2\n");
    }

    #[test]
    fn main_loop_accumulator_across_records() {
        // sum the first field of each record, print the total in END
        let out = run_prog("{ s += $1 } END { print s }", "10\n20\n30\n");
        assert_eq!(out, b"60\n");
    }

    #[test]
    fn short_circuit_and_or() {
        assert_eq!(
            run_begin("BEGIN { print (1 && 1), (1 && 0), (0 || 1), (0 || 0) }"),
            b"1 0 1 0\n"
        );
        assert_eq!(
            run_begin(r#"BEGIN { if (1 && 1) print "a"; if (1 && 0) print "b" }"#),
            b"a\n"
        );
    }

    #[test]
    fn ternary_and_unary() {
        assert_eq!(
            run_begin(r#"BEGIN { x = 5; print (x > 3 ? "big" : "small") }"#),
            b"big\n"
        );
        assert_eq!(run_begin("BEGIN { print -5 }"), b"-5\n");
        assert_eq!(run_begin("BEGIN { print !0, !5 }"), b"1 0\n");
    }

    #[test]
    fn string_vs_numeric_comparison_strnum() {
        // string equality (non-numeric) must compare as strings, not all-equal-0
        assert_eq!(
            run_prog(r#"$1 == "b" { print "hit" }"#, "a\nb\nc\n"),
            b"hit\n"
        );
        // numeric comparison still works
        assert_eq!(run_prog("$1 > 5 { print $1 }", "3\n8\n1\n10\n"), b"8\n10\n");
        // numeric-looking strings compare numerically: "10" > "9"
        assert_eq!(run_prog("$1 > 9 { print $1 }", "8\n10\n9\n"), b"10\n");
    }

    #[test]
    fn do_while_loop() {
        assert_eq!(
            run_begin("BEGIN { i = 0; do { print i; i += 1 } while (i < 3) }"),
            b"0\n1\n2\n"
        );
    }

    #[test]
    fn next_skips_remaining_rules() {
        assert_eq!(
            run_prog(r#"{ if ($1 == "skip") next; print $1 }"#, "a\nskip\nb\n"),
            b"a\nb\n"
        );
    }

    #[test]
    fn exit_stops_records_then_runs_end() {
        let out = run_prog(
            r#"{ if (NR == 2) exit; print $1 } END { print "done" }"#,
            "a\nb\nc\n",
        );
        assert_eq!(out, b"a\ndone\n");
    }

    #[test]
    fn regexp_pattern_filters_records() {
        assert_eq!(
            run_prog("/o/ { print $1 }", "foo bar\nbaz qux\nbox cat\n"),
            b"foo\nbox\n"
        );
    }

    #[test]
    fn tilde_match_operators() {
        assert_eq!(
            run_prog(r#"$1 ~ /^a/ { print $2 }"#, "abc x\nxyz y\nax z\n"),
            b"x\nz\n"
        );
        assert_eq!(
            run_prog(r#"$1 !~ /^a/ { print $2 }"#, "abc x\nxyz y\n"),
            b"y\n"
        );
    }

    #[test]
    fn user_function_with_return() {
        assert_eq!(
            run_begin("function add(a, b) { return a + b } BEGIN { print add(3, 4) }"),
            b"7\n"
        );
    }

    #[test]
    fn user_function_recursion() {
        // factorial — exercises frame-local params + recursion
        let src = "function fact(n) { if (n <= 1) return 1; return n * fact(n - 1) } \
                   BEGIN { print fact(5) }";
        assert_eq!(run_begin(src), b"120\n");
    }

    #[test]
    fn user_function_extra_params_are_locals() {
        // `t` is an extra param used as a local accumulator (starts empty/0)
        let src = "function sum3(a, b, c,    t) { t = a + b + c; return t } \
                   BEGIN { print sum3(1, 2, 3) }";
        assert_eq!(run_begin(src), b"6\n");
    }

    #[test]
    fn user_function_called_from_main_rule_and_globals() {
        // a function called per-record, mutating a global accumulator
        let src = "function add(x) { total += x } { add($1) } END { print total }";
        assert_eq!(run_prog(src, "10\n20\n30\n"), b"60\n");
    }

    #[test]
    fn unsupported_construct_errors_not_miscompiles() {
        // `system` isn't compiled yet — must error, never silently wrong.
        let prog = crate::parser::parse_program(r#"BEGIN { system("true") }"#).unwrap();
        assert!(compile_begin_only(&prog).is_err());
    }
}
