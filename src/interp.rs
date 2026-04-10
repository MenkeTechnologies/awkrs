use crate::ast::{IncDecOp, IncDecTarget, *};
use crate::builtins;
use crate::error::{Error, Result};
use crate::format;
use crate::runtime::{sorted_in_mode, Runtime, SortedInMode, Value};
use regex::Regex;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::Write;

/// Control flow from executing statements (loops, rules, functions).
#[derive(Debug)]
pub enum Flow {
    Normal,
    Break,
    Continue,
    Next,
    /// Skip to the next input file (invalid in `BEGIN`/`END`/`BEGINFILE`/`ENDFILE`).
    NextFile,
    Return(Value),
    /// POSIX: run `END`, then exit with `Runtime.exit_code`.
    ExitPending,
}

pub struct ExecCtx<'a> {
    pub prog: &'a Program,
    pub rt: &'a mut Runtime,
    pub locals: Vec<HashMap<String, Value>>,
    pub in_function: bool,
    /// When set, `print` / `printf` append here instead of stdout (parallel record mode).
    pub print_out: Option<&'a mut Vec<String>>,
}

impl<'a> ExecCtx<'a> {
    pub fn new(prog: &'a Program, rt: &'a mut Runtime) -> Self {
        Self {
            prog,
            rt,
            locals: Vec::new(),
            in_function: false,
            print_out: None,
        }
    }

    pub fn with_print_capture(
        prog: &'a Program,
        rt: &'a mut Runtime,
        out: &'a mut Vec<String>,
    ) -> Self {
        Self {
            prog,
            rt,
            locals: Vec::new(),
            in_function: false,
            print_out: Some(out),
        }
    }

    pub fn emit_print(&mut self, s: &str) {
        if let Some(buf) = self.print_out.as_mut() {
            buf.push(s.to_string());
        } else {
            print!("{s}");
        }
    }

    /// Flush stdout when not capturing prints (parallel workers buffer; flush is a no-op there).
    pub fn emit_flush(&mut self) -> Result<()> {
        if self.print_out.is_none() {
            std::io::stdout().flush().map_err(Error::Io)?;
        }
        Ok(())
    }

    fn get_var(&self, name: &str) -> Value {
        for frame in self.locals.iter().rev() {
            if let Some(v) = frame.get(name) {
                return v.clone();
            }
        }
        self.rt
            .get_global_var(name)
            .cloned()
            .unwrap_or_else(|| match name {
                "NR" => Value::Num(self.rt.nr),
                "FNR" => Value::Num(self.rt.fnr),
                "NF" => Value::Num(self.rt.fields.len() as f64),
                "FILENAME" => Value::Str(self.rt.filename.clone()),
                _ => Value::Uninit,
            })
    }

    fn set_var(&mut self, name: &str, val: Value) {
        for frame in self.locals.iter_mut().rev() {
            if let Some(v) = frame.get_mut(name) {
                *v = val;
                return;
            }
        }
        match self.rt.vars.get_mut(name) {
            Some(v) => *v = val,
            None => {
                self.rt.vars.insert(name.to_string(), val);
            }
        }
    }
}

pub fn run_begin(prog: &Program, rt: &mut Runtime) -> Result<()> {
    let mut ctx = ExecCtx::new(prog, rt);
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::Begin) {
            for s in &rule.stmts {
                match exec_stmt(s, &mut ctx)? {
                    Flow::Next => return Err(Error::Runtime("`next` is invalid in BEGIN".into())),
                    Flow::NextFile => {
                        return Err(Error::Runtime("`nextfile` is invalid in BEGIN".into()));
                    }
                    Flow::Return(_) => {
                        return Err(Error::Runtime("`return` outside function".into()));
                    }
                    Flow::ExitPending => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

pub fn run_end(prog: &Program, rt: &mut Runtime) -> Result<()> {
    let mut ctx = ExecCtx::new(prog, rt);
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::End) {
            for s in &rule.stmts {
                match exec_stmt(s, &mut ctx)? {
                    Flow::Next => return Err(Error::Runtime("`next` is invalid in END".into())),
                    Flow::NextFile => {
                        return Err(Error::Runtime("`nextfile` is invalid in END".into()));
                    }
                    Flow::Return(_) => {
                        return Err(Error::Runtime("`return` outside function".into()));
                    }
                    Flow::ExitPending => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

pub fn run_beginfile(prog: &Program, rt: &mut Runtime) -> Result<()> {
    let mut ctx = ExecCtx::new(prog, rt);
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::BeginFile) {
            for s in &rule.stmts {
                match exec_stmt(s, &mut ctx)? {
                    Flow::Next => {
                        return Err(Error::Runtime("`next` is invalid in BEGINFILE".into()));
                    }
                    Flow::NextFile => {
                        return Err(Error::Runtime("`nextfile` is invalid in BEGINFILE".into()));
                    }
                    Flow::Return(_) => {
                        return Err(Error::Runtime("`return` outside function".into()));
                    }
                    Flow::ExitPending => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

pub fn run_endfile(prog: &Program, rt: &mut Runtime) -> Result<()> {
    let mut ctx = ExecCtx::new(prog, rt);
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::EndFile) {
            for s in &rule.stmts {
                match exec_stmt(s, &mut ctx)? {
                    Flow::Next => {
                        return Err(Error::Runtime("`next` is invalid in ENDFILE".into()));
                    }
                    Flow::NextFile => {
                        return Err(Error::Runtime("`nextfile` is invalid in ENDFILE".into()));
                    }
                    Flow::Return(_) => {
                        return Err(Error::Runtime("`return` outside function".into()));
                    }
                    Flow::ExitPending => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

pub fn run_rule_on_record(
    prog: &Program,
    rt: &mut Runtime,
    rule_idx: usize,
    print_out: Option<&mut Vec<String>>,
) -> Result<Flow> {
    let mut ctx = match print_out {
        Some(buf) => ExecCtx::with_print_capture(prog, rt, buf),
        None => ExecCtx::new(prog, rt),
    };
    let rule = &prog.rules[rule_idx];
    for s in &rule.stmts {
        match exec_stmt(s, &mut ctx)? {
            Flow::Normal => {}
            f @ (Flow::Break
            | Flow::Continue
            | Flow::Next
            | Flow::NextFile
            | Flow::Return(_)
            | Flow::ExitPending) => {
                return Ok(f);
            }
        }
    }
    Ok(Flow::Normal)
}

/// Whether a record rule pattern matches (not used for `Range` — handled in `main`).
pub fn pattern_matches(pat: &Pattern, rt: &mut Runtime, prog: &Program) -> Result<bool> {
    let mut ctx = ExecCtx::new(prog, rt);
    Ok(match pat {
        Pattern::Begin | Pattern::End | Pattern::BeginFile | Pattern::EndFile => false,
        Pattern::Range(_, _) => false,
        Pattern::Empty => true,
        Pattern::Regexp(re) => {
            let r = Regex::new(re).map_err(|e| Error::Runtime(e.to_string()))?;
            r.is_match(&ctx.rt.record)
        }
        Pattern::Expr(e) => truthy(&eval_expr(e, &mut ctx)?),
    })
}

/// Match any pattern kind (used for range endpoints).
pub fn match_pattern(pat: &Pattern, rt: &mut Runtime, prog: &Program) -> Result<bool> {
    let mut ctx = ExecCtx::new(prog, rt);
    Ok(match pat {
        Pattern::Begin | Pattern::End | Pattern::BeginFile | Pattern::EndFile => false,
        Pattern::Empty => true,
        Pattern::Regexp(re) => {
            let r = Regex::new(re).map_err(|e| Error::Runtime(e.to_string()))?;
            r.is_match(&rt.record)
        }
        Pattern::Expr(e) => truthy(&eval_expr(e, &mut ctx)?),
        Pattern::Range(_, _) => {
            return Err(Error::Runtime("nested range pattern".into()));
        }
    })
}

fn truthy(v: &Value) -> bool {
    v.truthy()
}

fn exec_stmt(s: &Stmt, ctx: &mut ExecCtx<'_>) -> Result<Flow> {
    match s {
        Stmt::If { cond, then_, else_ } => {
            if truthy(&eval_expr(cond, ctx)?) {
                for t in then_ {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        f => return Ok(f),
                    }
                }
            } else {
                for t in else_ {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        f => return Ok(f),
                    }
                }
            }
        }
        Stmt::While { cond, body } => {
            'outer: while truthy(&eval_expr(cond, ctx)?) {
                for t in body {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        Flow::Break => break 'outer,
                        Flow::Continue => continue 'outer,
                        f @ (Flow::Next | Flow::NextFile | Flow::Return(_) | Flow::ExitPending) => {
                            return Ok(f);
                        }
                    }
                }
            }
        }
        Stmt::DoWhile { body, cond } => 'outer: loop {
            for t in body {
                match exec_stmt(t, ctx)? {
                    Flow::Normal => {}
                    Flow::Break => break 'outer,
                    Flow::Continue => {
                        if !truthy(&eval_expr(cond, ctx)?) {
                            break 'outer;
                        }
                        continue 'outer;
                    }
                    f @ (Flow::Next | Flow::NextFile | Flow::Return(_) | Flow::ExitPending) => {
                        return Ok(f);
                    }
                }
            }
            if !truthy(&eval_expr(cond, ctx)?) {
                break;
            }
        },
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                eval_expr(e, ctx)?;
            }
            'outer: loop {
                if let Some(c) = cond {
                    if !truthy(&eval_expr(c, ctx)?) {
                        break 'outer;
                    }
                }
                for t in body {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        Flow::Break => break 'outer,
                        Flow::Continue => {
                            if let Some(it) = iter {
                                eval_expr(it, ctx)?;
                            }
                            continue 'outer;
                        }
                        f @ (Flow::Next | Flow::NextFile | Flow::Return(_) | Flow::ExitPending) => {
                            return Ok(f);
                        }
                    }
                }
                if let Some(it) = iter {
                    eval_expr(it, ctx)?;
                }
            }
        }
        Stmt::ForIn { var, arr, body } => {
            let keys = interp_for_in_keys(ctx, arr)?;
            'outer: for k in keys {
                ctx.set_var(var, Value::Str(k));
                for t in body {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        Flow::Break => break 'outer,
                        Flow::Continue => continue 'outer,
                        f @ (Flow::Next | Flow::NextFile | Flow::Return(_) | Flow::ExitPending) => {
                            return Ok(f);
                        }
                    }
                }
            }
        }
        Stmt::Block(ss) => {
            for t in ss {
                match exec_stmt(t, ctx)? {
                    Flow::Normal => {}
                    f => return Ok(f),
                }
            }
        }
        Stmt::Expr(e) => {
            eval_expr(e, ctx)?;
        }
        Stmt::Print { args, redir } => {
            let ofs = ctx
                .rt
                .vars
                .get("OFS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| " ".into());
            let ors = ctx
                .rt
                .vars
                .get("ORS")
                .map(|v| v.as_str())
                .unwrap_or_else(|| "\n".into());
            let mut parts = Vec::new();
            for a in args {
                parts.push(eval_expr(a, ctx)?.as_str());
            }
            let line = if parts.is_empty() {
                ctx.rt.record.clone()
            } else {
                parts.join(&ofs)
            };
            let chunk = format!("{line}{ors}");
            match redir {
                None => ctx.emit_print(&chunk),
                Some(PrintRedir::Overwrite(e)) => {
                    let path = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_output_line(&path, &chunk, false)?;
                }
                Some(PrintRedir::Append(e)) => {
                    let path = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_output_line(&path, &chunk, true)?;
                }
                Some(PrintRedir::Pipe(e)) => {
                    let cmd = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_pipe_line(&cmd, &chunk)?;
                }
                Some(PrintRedir::Coproc(e)) => {
                    let cmd = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_coproc_line(&cmd, &chunk)?;
                }
            }
        }
        Stmt::Printf { args, redir } => {
            if args.is_empty() {
                return Err(Error::Runtime("`printf` needs a format string".into()));
            }
            let fmt = eval_expr(&args[0], ctx)?.as_str();
            let vals: Vec<Value> = args[1..]
                .iter()
                .map(|e| eval_expr(e, ctx))
                .collect::<Result<_>>()?;
            let out = sprintf_simple(&fmt, &vals, ctx.rt.numeric_decimal)?;
            let s = out.as_str();
            match redir {
                None => ctx.emit_print(&s),
                Some(PrintRedir::Overwrite(e)) => {
                    let path = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_output_line(&path, &s, false)?;
                }
                Some(PrintRedir::Append(e)) => {
                    let path = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_output_line(&path, &s, true)?;
                }
                Some(PrintRedir::Pipe(e)) => {
                    let cmd = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_pipe_line(&cmd, &s)?;
                }
                Some(PrintRedir::Coproc(e)) => {
                    let cmd = eval_expr(e, ctx)?.as_str();
                    ctx.rt.write_coproc_line(&cmd, &s)?;
                }
            }
        }
        Stmt::Break => return Ok(Flow::Break),
        Stmt::Continue => return Ok(Flow::Continue),
        Stmt::Next => {
            if ctx.in_function {
                return Err(Error::Runtime("`next` used inside a function".into()));
            }
            return Ok(Flow::Next);
        }
        Stmt::NextFile => {
            if ctx.in_function {
                return Err(Error::Runtime("`nextfile` used inside a function".into()));
            }
            return Ok(Flow::NextFile);
        }
        Stmt::Switch { expr, arms } => {
            let v = eval_expr(expr, ctx)?;
            for arm in arms {
                match arm {
                    SwitchArm::Case { label, stmts } => {
                        let matched = match label {
                            SwitchLabel::Expr(e) => {
                                let ev = eval_expr(e, ctx)?;
                                switch_value_eq(&v, &ev, ctx.rt)
                            }
                            SwitchLabel::Regexp(re) => {
                                let r =
                                    Regex::new(re).map_err(|e| Error::Runtime(e.to_string()))?;
                                r.is_match(&v.as_str())
                            }
                        };
                        if matched {
                            for s in stmts {
                                match exec_stmt(s, ctx)? {
                                    Flow::Normal => {}
                                    // `break` exits the switch only (not an enclosing loop).
                                    Flow::Break => return Ok(Flow::Normal),
                                    f => return Ok(f),
                                }
                            }
                            return Ok(Flow::Normal);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for s in stmts {
                            match exec_stmt(s, ctx)? {
                                Flow::Normal => {}
                                Flow::Break => return Ok(Flow::Normal),
                                f => return Ok(f),
                            }
                        }
                        return Ok(Flow::Normal);
                    }
                }
            }
        }
        Stmt::Exit(e) => {
            let code = if let Some(x) = e {
                eval_expr(x, ctx)?.as_number() as i32
            } else {
                0
            };
            ctx.rt.exit_code = code;
            ctx.rt.exit_pending = true;
            return Ok(Flow::ExitPending);
        }
        Stmt::GetLine { var, redir } => {
            let line = match &redir {
                GetlineRedir::Primary => ctx.rt.read_line_primary()?,
                GetlineRedir::File(path_expr) => {
                    let path = eval_expr(path_expr, ctx)?.as_str();
                    ctx.rt.read_line_file(&path)?
                }
                GetlineRedir::Coproc(cmd_expr) => {
                    let cmd = eval_expr(cmd_expr, ctx)?.as_str();
                    ctx.rt.read_line_coproc(&cmd)?
                }
            };
            if let Some(l) = line {
                let trimmed = l.trim_end_matches(['\n', '\r']).to_string();
                if let Some(ref name) = var {
                    ctx.set_var(name, Value::Str(trimmed));
                } else {
                    let fs = ctx
                        .rt
                        .vars
                        .get("FS")
                        .map(|v| v.as_str())
                        .unwrap_or_else(|| " ".into());
                    ctx.rt.set_field_sep_split(&fs, &trimmed);
                    ctx.rt.ensure_fields_split();
                    let nf = ctx.rt.nf() as f64;
                    ctx.rt.vars.insert("NF".into(), Value::Num(nf));
                }
                match &redir {
                    GetlineRedir::Primary => {
                        ctx.rt.nr += 1.0;
                        ctx.rt.fnr += 1.0;
                    }
                    GetlineRedir::File(_) | GetlineRedir::Coproc(_) => {}
                }
            }
        }
        Stmt::Delete { name, indices } => match indices {
            None => ctx.rt.array_delete(name, None),
            Some(ixs) => {
                let k = array_key(ctx, ixs)?;
                ctx.rt.array_delete(name, Some(&k));
            }
        },
        Stmt::Return(e) => {
            if !ctx.in_function {
                return Err(Error::Runtime("`return` outside function".into()));
            }
            let v = if let Some(ex) = e {
                eval_expr(ex, ctx)?
            } else {
                Value::Str(String::new())
            };
            return Ok(Flow::Return(v));
        }
    }
    Ok(Flow::Normal)
}

fn array_key(ctx: &mut ExecCtx<'_>, indices: &[Expr]) -> Result<String> {
    if indices.is_empty() {
        return Err(Error::Runtime("empty array index".into()));
    }
    let sep = ctx
        .rt
        .vars
        .get("SUBSEP")
        .map(|v| v.as_str())
        .unwrap_or_else(|| "\x1c".into());
    let mut acc = eval_expr(&indices[0], ctx)?.as_str();
    for ix in &indices[1..] {
        acc.push_str(&sep);
        acc.push_str(&eval_expr(ix, ctx)?.as_str());
    }
    Ok(acc)
}

pub fn eval_expr(e: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    if let Expr::Binary { op, left, right } = e {
        if *op == BinOp::And {
            let lv = eval_expr(left, ctx)?;
            if !truthy(&lv) {
                return Ok(Value::Num(0.0));
            }
            return Ok(Value::Num(if truthy(&eval_expr(right, ctx)?) {
                1.0
            } else {
                0.0
            }));
        }
        if *op == BinOp::Or {
            let lv = eval_expr(left, ctx)?;
            if truthy(&lv) {
                return Ok(Value::Num(1.0));
            }
            return Ok(Value::Num(if truthy(&eval_expr(right, ctx)?) {
                1.0
            } else {
                0.0
            }));
        }
    }
    Ok(match e {
        Expr::Number(n) => Value::Num(*n),
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::Var(name) => ctx.get_var(name),
        Expr::Index { name, indices } => {
            let k = array_key(ctx, indices)?;
            ctx.rt.array_get(name, &k)
        }
        Expr::Field(inner) => {
            let i = eval_expr(inner, ctx)?.as_number() as i32;
            ctx.rt.field(i)
        }
        Expr::Binary { op, left, right } => eval_binary(*op, left, right, ctx)?,
        Expr::Unary { op, expr } => eval_unary(*op, expr, ctx)?,
        Expr::Assign { name, op, rhs } => {
            let v = eval_expr(rhs, ctx)?;
            let newv = if let Some(bop) = op {
                let old = ctx.get_var(name);
                apply_binop(*bop, &old, &v, ctx.rt.bignum, ctx.rt)?
            } else {
                v
            };
            ctx.set_var(name, newv.clone());
            newv
        }
        Expr::AssignField { field, op, rhs } => {
            let idx = eval_expr(field, ctx)?.as_number() as i32;
            let v = eval_expr(rhs, ctx)?;
            let newv = if let Some(bop) = op {
                let old = Value::Str(ctx.rt.field(idx).as_str());
                apply_binop(*bop, &old, &v, ctx.rt.bignum, ctx.rt)?
            } else {
                v
            };
            let s = newv.as_str();
            ctx.rt.set_field(idx, &s);
            newv
        }
        Expr::AssignIndex {
            name,
            indices,
            op,
            rhs,
        } => {
            let k = array_key(ctx, indices)?;
            let v = eval_expr(rhs, ctx)?;
            let newv = if let Some(bop) = op {
                let old = ctx.rt.array_get(name, &k);
                apply_binop(*bop, &old, &v, ctx.rt.bignum, ctx.rt)?
            } else {
                v
            };
            ctx.rt.array_set(name, k, newv.clone());
            newv
        }
        Expr::Call { name, args } => eval_call(name, args, ctx)?,
        Expr::IndirectCall { callee, args } => {
            let fname = eval_expr(callee, ctx)?.as_str();
            eval_call(&fname, args, ctx)?
        }
        Expr::Ternary { cond, then_, else_ } => {
            if truthy(&eval_expr(cond, ctx)?) {
                eval_expr(then_, ctx)?
            } else {
                eval_expr(else_, ctx)?
            }
        }
        Expr::In { key, arr } => {
            let k = eval_expr(key, ctx)?.as_str();
            Value::Num(if ctx.rt.array_has(arr, &k) { 1.0 } else { 0.0 })
        }
        Expr::IncDec { op, target } => eval_inc_dec(*op, target, ctx)?,
    })
}

fn eval_inc_dec(op: IncDecOp, target: &IncDecTarget, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let delta = match op {
        IncDecOp::PreInc | IncDecOp::PostInc => 1.0,
        IncDecOp::PreDec | IncDecOp::PostDec => -1.0,
    };
    let ret_old = matches!(op, IncDecOp::PostInc | IncDecOp::PostDec);
    match target {
        IncDecTarget::Var(name) => {
            let old_n = ctx.get_var(name).as_number();
            let new_n = old_n + delta;
            ctx.set_var(name, Value::Num(new_n));
            Ok(Value::Num(if ret_old { old_n } else { new_n }))
        }
        IncDecTarget::Field(field) => {
            let idx = eval_expr(field, ctx)?.as_number() as i32;
            let old_n = ctx.rt.field(idx).as_number();
            let new_n = old_n + delta;
            let s = Value::Num(new_n).as_str();
            ctx.rt.set_field(idx, &s);
            Ok(Value::Num(if ret_old { old_n } else { new_n }))
        }
        IncDecTarget::Index { name, indices } => {
            let k = array_key(ctx, indices)?;
            let old_n = ctx.rt.array_get(name, &k).as_number();
            let new_n = old_n + delta;
            let newv = Value::Num(new_n);
            ctx.rt.array_set(name, k, newv);
            Ok(Value::Num(if ret_old { old_n } else { new_n }))
        }
    }
}

fn eval_binary(op: BinOp, left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    if op == BinOp::Concat {
        let a = eval_expr(left, ctx)?.as_str();
        let b = eval_expr(right, ctx)?.as_str();
        return Ok(Value::Str(format!("{a}{b}")));
    }
    if op == BinOp::Match || op == BinOp::NotMatch {
        let s = eval_expr(left, ctx)?.as_str();
        let pat = eval_expr(right, ctx)?.as_str();
        let r = Regex::new(&pat).map_err(|e| Error::Runtime(e.to_string()))?;
        let m = r.is_match(&s);
        let res = if op == BinOp::Match { m } else { !m };
        return Ok(Value::Num(if res { 1.0 } else { 0.0 }));
    }
    if op == BinOp::Eq {
        return awk_eq(left, right, ctx);
    }
    if op == BinOp::Ne {
        return awk_ne(left, right, ctx);
    }
    if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
        return awk_rel(op, left, right, ctx);
    }
    if ctx.rt.bignum
        && matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
        )
    {
        let av = eval_expr(left, ctx)?;
        let bv = eval_expr(right, ctx)?;
        return crate::runtime::awk_binop_values(op, &av, &bv, true, ctx.rt);
    }
    let a = eval_expr(left, ctx)?.as_number();
    let b = eval_expr(right, ctx)?.as_number();
    let n = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Mod => a % b,
        BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::Concat
        | BinOp::Match
        | BinOp::NotMatch
        | BinOp::And
        | BinOp::Or => {
            unreachable!()
        }
    };
    Ok(Value::Num(n))
}

/// POSIX string compare: `strcoll` when available (honors process `LC_*`), else Rust byte order.
fn locale_str_cmp(a: &str, b: &str) -> Ordering {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        match (CString::new(a), CString::new(b)) {
            (Ok(ca), Ok(cb)) => unsafe {
                let r = libc::strcoll(ca.as_ptr(), cb.as_ptr());
                r.cmp(&0)
            },
            _ => a.cmp(b),
        }
    }
    #[cfg(not(unix))]
    {
        a.cmp(b)
    }
}

fn awk_eq(left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let lv = eval_expr(left, ctx)?;
    let rv = eval_expr(right, ctx)?;
    if ctx.rt.bignum && lv.is_numeric_str() && rv.is_numeric_str() {
        let prec = ctx.rt.mpfr_prec_bits();
        let fa = crate::runtime::value_to_float(&lv, prec);
        let fb = crate::runtime::value_to_float(&rv, prec);
        return Ok(Value::Num(if fa == fb { 1.0 } else { 0.0 }));
    }
    if lv.is_numeric_str() && rv.is_numeric_str() {
        let a = lv.as_number();
        let b = rv.as_number();
        return Ok(Value::Num(if (a - b).abs() < f64::EPSILON {
            1.0
        } else {
            0.0
        }));
    }
    Ok(Value::Num(
        if locale_str_cmp(&lv.as_str(), &rv.as_str()) == Ordering::Equal {
            1.0
        } else {
            0.0
        },
    ))
}

#[inline]
fn switch_value_eq(lv: &Value, rv: &Value, rt: &crate::runtime::Runtime) -> bool {
    if matches!(lv, Value::Mpfr(_)) || matches!(rv, Value::Mpfr(_)) {
        let prec = rt.mpfr_prec_bits();
        let fa = crate::runtime::value_to_float(lv, prec);
        let fb = crate::runtime::value_to_float(rv, prec);
        return fa == fb;
    }
    if lv.is_numeric_str() && rv.is_numeric_str() {
        let a = lv.as_number();
        let b = rv.as_number();
        return (a - b).abs() < f64::EPSILON;
    }
    locale_str_cmp(&lv.as_str(), &rv.as_str()) == Ordering::Equal
}

fn awk_ne(left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let v = awk_eq(left, right, ctx)?;
    Ok(Value::Num(if v.as_number() != 0.0 { 0.0 } else { 1.0 }))
}

fn awk_rel(op: BinOp, left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let lv = eval_expr(left, ctx)?;
    let rv = eval_expr(right, ctx)?;
    if ctx.rt.bignum && lv.is_numeric_str() && rv.is_numeric_str() {
        let prec = ctx.rt.mpfr_prec_bits();
        let fa = crate::runtime::value_to_float(&lv, prec);
        let fb = crate::runtime::value_to_float(&rv, prec);
        let ord = fa.partial_cmp(&fb).unwrap_or(Ordering::Equal);
        let ok = match op {
            BinOp::Lt => ord == Ordering::Less,
            BinOp::Le => matches!(ord, Ordering::Less | Ordering::Equal),
            BinOp::Gt => ord == Ordering::Greater,
            BinOp::Ge => matches!(ord, Ordering::Greater | Ordering::Equal),
            _ => unreachable!(),
        };
        return Ok(Value::Num(if ok { 1.0 } else { 0.0 }));
    }
    if lv.is_numeric_str() && rv.is_numeric_str() {
        let a = lv.as_number();
        let b = rv.as_number();
        let ok = match op {
            BinOp::Lt => a < b,
            BinOp::Le => a <= b,
            BinOp::Gt => a > b,
            BinOp::Ge => a >= b,
            _ => unreachable!(),
        };
        return Ok(Value::Num(if ok { 1.0 } else { 0.0 }));
    }
    let ls = lv.as_str();
    let rs = rv.as_str();
    let ord = locale_str_cmp(&ls, &rs);
    let ok = match op {
        BinOp::Lt => ord == Ordering::Less,
        BinOp::Le => matches!(ord, Ordering::Less | Ordering::Equal),
        BinOp::Gt => ord == Ordering::Greater,
        BinOp::Ge => matches!(ord, Ordering::Greater | Ordering::Equal),
        _ => unreachable!(),
    };
    Ok(Value::Num(if ok { 1.0 } else { 0.0 }))
}

fn eval_unary(op: UnaryOp, expr: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let v = eval_expr(expr, ctx)?;
    Ok(match op {
        UnaryOp::Neg => {
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = crate::runtime::value_to_float(&v, prec);
                Value::Mpfr(rug::Float::with_val_round(prec, -f, round).0)
            } else {
                Value::Num(-v.as_number())
            }
        }
        UnaryOp::Pos => {
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = crate::runtime::value_to_float(&v, prec);
                Value::Mpfr(rug::Float::with_val_round(prec, f, round).0)
            } else {
                Value::Num(v.as_number())
            }
        }
        UnaryOp::Not => Value::Num(if truthy(&v) { 0.0 } else { 1.0 }),
    })
}

fn apply_binop(
    op: BinOp,
    old: &Value,
    new: &Value,
    bignum: bool,
    rt: &crate::runtime::Runtime,
) -> Result<Value> {
    crate::runtime::awk_binop_values(op, old, new, bignum, rt)
}

fn interp_typeof_scalar(ctx: &ExecCtx<'_>, name: &str) -> &'static str {
    match name {
        "NR" | "FNR" | "NF" => return "number",
        "FILENAME" => return "string",
        _ => {}
    }
    for frame in ctx.locals.iter().rev() {
        if let Some(v) = frame.get(name) {
            return builtins::awk_typeof_value(v);
        }
    }
    if let Some(v) = ctx.rt.get_global_var(name) {
        return builtins::awk_typeof_value(v);
    }
    "uninitialized"
}

fn eval_call(name: &str, args: &[Expr], ctx: &mut ExecCtx<'_>) -> Result<Value> {
    if let Some(fd) = ctx.prog.funcs.get(name) {
        return call_user(fd, args, ctx);
    }
    match name {
        "length" => {
            if args.is_empty() {
                let n = if ctx.rt.characters_as_bytes {
                    ctx.rt.record.len()
                } else {
                    ctx.rt.record.chars().count()
                };
                Ok(Value::Num(n as f64))
            } else {
                let v = eval_expr(&args[0], ctx)?;
                match &v {
                    Value::Array(a) => Ok(Value::Num(a.len() as f64)),
                    _ => {
                        let s = v.as_str();
                        let n = if ctx.rt.characters_as_bytes {
                            s.len()
                        } else {
                            s.chars().count()
                        };
                        Ok(Value::Num(n as f64))
                    }
                }
            }
        }
        "index" if args.len() == 2 => {
            let hay = eval_expr(&args[0], ctx)?.as_str();
            let needle = eval_expr(&args[1], ctx)?.as_str();
            if needle.is_empty() {
                return Ok(Value::Num(0.0));
            }
            let pos = if let Some(b) = hay.find(&needle) {
                if ctx.rt.characters_as_bytes {
                    b + 1
                } else {
                    hay[..b].chars().count() + 1
                }
            } else {
                0
            };
            Ok(Value::Num(pos as f64))
        }
        "substr" => {
            let s = eval_expr(
                args.first()
                    .ok_or_else(|| Error::Runtime("substr".into()))?,
                ctx,
            )?
            .as_str();
            let start = eval_expr(
                args.get(1).ok_or_else(|| Error::Runtime("substr".into()))?,
                ctx,
            )?
            .as_number() as usize;
            let len = if let Some(e) = args.get(2) {
                eval_expr(e, ctx)?.as_number() as usize
            } else {
                usize::MAX
            };
            if start < 1 {
                return Ok(Value::Str(String::new()));
            }
            let start0 = start - 1;
            let slice = if ctx.rt.characters_as_bytes {
                s.as_bytes()
                    .get(start0..)
                    .map(|rest| {
                        let take = len.min(rest.len());
                        String::from_utf8_lossy(&rest[..take]).into_owned()
                    })
                    .unwrap_or_default()
            } else {
                s.chars().skip(start0).take(len).collect()
            };
            Ok(Value::Str(slice))
        }
        "gsub" => {
            let re = eval_expr(&args[0], ctx)?.as_str();
            let rep = eval_expr(&args[1], ctx)?.as_str();
            if args.len() >= 3 {
                match &args[2] {
                    Expr::Var(name) => {
                        let mut s = ctx.get_var(name).as_str();
                        let n = builtins::gsub(ctx.rt, &re, &rep, Some(&mut s))?;
                        ctx.set_var(name, Value::Str(s));
                        return Ok(Value::Num(n));
                    }
                    Expr::Field(inner) => {
                        let i = eval_expr(inner, ctx)?.as_number() as i32;
                        let mut s = ctx.rt.field(i).as_str();
                        let n = builtins::gsub(ctx.rt, &re, &rep, Some(&mut s))?;
                        ctx.rt.set_field(i, &s);
                        return Ok(Value::Num(n));
                    }
                    Expr::Index { name, indices } => {
                        let k = array_key(ctx, indices)?;
                        let mut s = ctx.rt.array_get(name, &k).as_str();
                        let n = builtins::gsub(ctx.rt, &re, &rep, Some(&mut s))?;
                        ctx.rt.array_set(name, k, Value::Str(s));
                        return Ok(Value::Num(n));
                    }
                    _ => {
                        return Err(Error::Runtime(
                            "gsub: third argument must be variable, field, or array element".into(),
                        ));
                    }
                }
            }
            let n = builtins::gsub(ctx.rt, &re, &rep, None)?;
            Ok(Value::Num(n))
        }
        "sub" => {
            let re = eval_expr(&args[0], ctx)?.as_str();
            let rep = eval_expr(&args[1], ctx)?.as_str();
            if args.len() >= 3 {
                match &args[2] {
                    Expr::Var(name) => {
                        let mut s = ctx.get_var(name).as_str();
                        let n = builtins::sub_fn(ctx.rt, &re, &rep, Some(&mut s))?;
                        ctx.set_var(name, Value::Str(s));
                        return Ok(Value::Num(n));
                    }
                    Expr::Field(inner) => {
                        let i = eval_expr(inner, ctx)?.as_number() as i32;
                        let mut s = ctx.rt.field(i).as_str();
                        let n = builtins::sub_fn(ctx.rt, &re, &rep, Some(&mut s))?;
                        ctx.rt.set_field(i, &s);
                        return Ok(Value::Num(n));
                    }
                    Expr::Index { name, indices } => {
                        let k = array_key(ctx, indices)?;
                        let mut s = ctx.rt.array_get(name, &k).as_str();
                        let n = builtins::sub_fn(ctx.rt, &re, &rep, Some(&mut s))?;
                        ctx.rt.array_set(name, k, Value::Str(s));
                        return Ok(Value::Num(n));
                    }
                    _ => {
                        return Err(Error::Runtime(
                            "sub: third argument must be variable, field, or array element".into(),
                        ));
                    }
                }
            }
            let n = builtins::sub_fn(ctx.rt, &re, &rep, None)?;
            Ok(Value::Num(n))
        }
        "match" => {
            let s = eval_expr(&args[0], ctx)?.as_str();
            let re = eval_expr(&args[1], ctx)?.as_str();
            let arr = args.get(2).and_then(|e| {
                if let Expr::Var(n) = e {
                    Some(n.as_str())
                } else {
                    None
                }
            });
            let r = builtins::match_fn(ctx.rt, &s, &re, arr)?;
            Ok(Value::Num(r))
        }
        "tolower" if args.len() == 1 => {
            let s = eval_expr(&args[0], ctx)?.as_str();
            Ok(Value::Str(s.to_lowercase()))
        }
        "toupper" if args.len() == 1 => {
            let s = eval_expr(&args[0], ctx)?.as_str();
            Ok(Value::Str(s.to_uppercase()))
        }
        "int" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.trunc()))
        }
        "intdiv" if args.len() == 2 => {
            let b = eval_expr(&args[1], ctx)?.as_number();
            if b == 0.0 {
                return Err(Error::Runtime("intdiv: division by zero".into()));
            }
            let a = eval_expr(&args[0], ctx)?.as_number() as i64;
            let bi = b as i64;
            Ok(Value::Num((a / bi) as f64))
        }
        "mkbool" if args.len() == 1 => {
            let v = eval_expr(&args[0], ctx)?;
            Ok(Value::Num(if truthy(&v) { 1.0 } else { 0.0 }))
        }
        "sqrt" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.sqrt()))
        }
        "sin" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.sin()))
        }
        "cos" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.cos()))
        }
        "atan2" if args.len() == 2 => {
            let y = eval_expr(&args[0], ctx)?.as_number();
            let x = eval_expr(&args[1], ctx)?.as_number();
            Ok(Value::Num(y.atan2(x)))
        }
        "exp" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.exp()))
        }
        "log" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.ln()))
        }
        "systime" if args.is_empty() => Ok(Value::Num(builtins::awk_systime())),
        "strftime" => {
            let vals: Vec<Value> = args
                .iter()
                .map(|e| eval_expr(e, ctx))
                .collect::<Result<_>>()?;
            builtins::awk_strftime(&vals).map_err(Error::Runtime)
        }
        "mktime" if args.len() == 1 => {
            let s = eval_expr(&args[0], ctx)?.as_str();
            Ok(Value::Num(builtins::awk_mktime(&s)))
        }
        "rand" if args.is_empty() => Ok(Value::Num(ctx.rt.rand())),
        "srand" => {
            let n = if let Some(e) = args.first() {
                Some(eval_expr(e, ctx)?.as_number() as u32)
            } else {
                None
            };
            Ok(Value::Num(ctx.rt.srand(n)))
        }
        "system" if args.len() == 1 => {
            use std::process::Command;
            let cmd = eval_expr(&args[0], ctx)?.as_str();
            let st = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .status()
                .map_err(Error::Io)?;
            Ok(Value::Num(st.code().unwrap_or(-1) as f64))
        }
        "close" if args.len() == 1 => {
            let path = eval_expr(&args[0], ctx)?.as_str();
            Ok(Value::Num(ctx.rt.close_handle(&path)))
        }
        "fflush" => match args.len() {
            0 => {
                ctx.emit_flush()?;
                Ok(Value::Num(0.0))
            }
            1 => {
                let path = eval_expr(&args[0], ctx)?.as_str();
                if path.is_empty() {
                    ctx.emit_flush()?;
                } else {
                    ctx.rt.flush_redirect_target(&path)?;
                }
                Ok(Value::Num(0.0))
            }
            _ => Err(Error::Runtime("fflush: expected 0 or 1 arguments".into())),
        },
        "patsplit" => {
            if !(2..=4).contains(&args.len()) {
                return Err(Error::Runtime(
                    "patsplit: expected 2, 3, or 4 arguments".into(),
                ));
            }
            let s = eval_expr(&args[0], ctx)?.as_str();
            let arr_name = match &args[1] {
                Expr::Var(n) => n.clone(),
                _ => {
                    return Err(Error::Runtime(
                        "patsplit: second argument must be array name".into(),
                    ));
                }
            };
            let fp = if args.len() >= 3 {
                Some(eval_expr(&args[2], ctx)?.as_str())
            } else {
                None
            };
            let seps = if args.len() == 4 {
                match &args[3] {
                    Expr::Var(n) => Some(n.as_str()),
                    _ => {
                        return Err(Error::Runtime(
                            "patsplit: fourth argument must be array name".into(),
                        ));
                    }
                }
            } else {
                None
            };
            let n = builtins::patsplit(ctx.rt, &s, &arr_name, fp.as_deref(), seps)?;
            Ok(Value::Num(n))
        }
        "split" => {
            let s = eval_expr(
                args.first().ok_or_else(|| Error::Runtime("split".into()))?,
                ctx,
            )?
            .as_str();
            let arr_name = match &args.get(1) {
                Some(Expr::Var(n)) => n.clone(),
                _ => {
                    return Err(Error::Runtime(
                        "split: second argument must be array name".into(),
                    ));
                }
            };
            let fs = if let Some(e) = args.get(2) {
                eval_expr(e, ctx)?.as_str()
            } else {
                ctx.rt
                    .vars
                    .get("FS")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| " ".into())
            };
            let parts: Vec<String> = if fs.is_empty() {
                s.chars().map(|c| c.to_string()).collect()
            } else if fs == " " {
                s.split_whitespace().map(String::from).collect()
            } else {
                s.split(&fs).map(String::from).collect()
            };
            let n = parts.len();
            ctx.rt.split_into_array(&arr_name, &parts);
            Ok(Value::Num(n as f64))
        }
        "sprintf" => {
            if args.is_empty() {
                return Err(Error::Runtime("sprintf: need format".into()));
            }
            let fmt = eval_expr(&args[0], ctx)?.as_str();
            let vals: Vec<Value> = args[1..]
                .iter()
                .map(|e| eval_expr(e, ctx))
                .collect::<Result<_>>()?;
            sprintf_simple(&fmt, &vals, ctx.rt.numeric_decimal)
        }
        "and" if args.len() == 2 => {
            let a = eval_expr(&args[0], ctx)?.as_number();
            let b = eval_expr(&args[1], ctx)?.as_number();
            Ok(Value::Num(builtins::awk_and(a, b)))
        }
        "or" if args.len() == 2 => {
            let a = eval_expr(&args[0], ctx)?.as_number();
            let b = eval_expr(&args[1], ctx)?.as_number();
            Ok(Value::Num(builtins::awk_or(a, b)))
        }
        "xor" if args.len() == 2 => {
            let a = eval_expr(&args[0], ctx)?.as_number();
            let b = eval_expr(&args[1], ctx)?.as_number();
            Ok(Value::Num(builtins::awk_xor(a, b)))
        }
        "lshift" if args.len() == 2 => {
            let a = eval_expr(&args[0], ctx)?.as_number();
            let b = eval_expr(&args[1], ctx)?.as_number();
            Ok(Value::Num(builtins::awk_lshift(a, b)))
        }
        "rshift" if args.len() == 2 => {
            let a = eval_expr(&args[0], ctx)?.as_number();
            let b = eval_expr(&args[1], ctx)?.as_number();
            Ok(Value::Num(builtins::awk_rshift(a, b)))
        }
        "compl" if args.len() == 1 => {
            let a = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(builtins::awk_compl(a)))
        }
        "strtonum" if args.len() == 1 => {
            let s = eval_expr(&args[0], ctx)?.as_str();
            Ok(Value::Num(builtins::awk_strtonum(&s)))
        }
        "typeof" => {
            if args.len() != 1 {
                return Err(Error::Runtime("`typeof` expects one argument".into()));
            }
            match &args[0] {
                Expr::Var(name) => Ok(Value::Str(interp_typeof_scalar(ctx, name).into())),
                Expr::Index { name, indices } => {
                    let k = array_key(ctx, indices)?;
                    let t = builtins::awk_typeof_array_elem(ctx.rt, name, &k);
                    Ok(Value::Str(t.into()))
                }
                Expr::Field(inner) => {
                    let i = eval_expr(inner, ctx)?.as_number() as i32;
                    let t = if ctx.rt.field_is_unassigned(i) {
                        "uninitialized"
                    } else {
                        "string"
                    };
                    Ok(Value::Str(t.into()))
                }
                e => {
                    let v = eval_expr(e, ctx)?;
                    Ok(Value::Str(builtins::awk_typeof_value(&v).into()))
                }
            }
        }
        "asort" => {
            let src = match args.first() {
                Some(Expr::Var(n)) => n.as_str(),
                _ => {
                    return Err(Error::Runtime(
                        "asort: first argument must be array name".into(),
                    ));
                }
            };
            let dest = if let Some(e) = args.get(1) {
                match e {
                    Expr::Var(n) => Some(n.as_str()),
                    _ => {
                        return Err(Error::Runtime(
                            "asort: second argument must be array name".into(),
                        ));
                    }
                }
            } else {
                None
            };
            let n = builtins::asort(ctx.rt, src, dest)?;
            Ok(Value::Num(n))
        }
        "asorti" => {
            let src = match args.first() {
                Some(Expr::Var(n)) => n.as_str(),
                _ => {
                    return Err(Error::Runtime(
                        "asorti: first argument must be array name".into(),
                    ));
                }
            };
            let dest = if let Some(e) = args.get(1) {
                match e {
                    Expr::Var(n) => Some(n.as_str()),
                    _ => {
                        return Err(Error::Runtime(
                            "asorti: second argument must be array name".into(),
                        ));
                    }
                }
            } else {
                None
            };
            let n = builtins::asorti(ctx.rt, src, dest)?;
            Ok(Value::Num(n))
        }
        "printf" => {
            if args.is_empty() {
                return Err(Error::Runtime("printf: need format".into()));
            }
            let fmt = eval_expr(&args[0], ctx)?.as_str();
            let vals: Vec<Value> = args[1..]
                .iter()
                .map(|e| eval_expr(e, ctx))
                .collect::<Result<_>>()?;
            let s = sprintf_simple(&fmt, &vals, ctx.rt.numeric_decimal)?.as_str();
            ctx.emit_print(&s);
            Ok(Value::Num(0.0))
        }
        _ => Err(Error::Runtime(format!("unknown function `{name}`"))),
    }
}

fn sprintf_simple(fmt: &str, vals: &[Value], dec: char) -> Result<Value> {
    let s = if dec == '.' {
        format::awk_sprintf(fmt, vals)
    } else {
        format::awk_sprintf_with_decimal(fmt, vals, dec)
    };
    s.map(Value::Str).map_err(Error::Runtime)
}

fn call_user_with_values(name: &str, mut vals: Vec<Value>, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let fd = ctx
        .prog
        .funcs
        .get(name)
        .ok_or_else(|| Error::Runtime(format!("unknown function `{name}`")))?;
    while vals.len() < fd.params.len() {
        vals.push(Value::Uninit);
    }
    vals.truncate(fd.params.len());
    let mut frame = HashMap::new();
    for (p, v) in fd.params.iter().zip(vals.into_iter()) {
        frame.insert(p.clone(), v);
    }
    ctx.locals.push(frame);
    let was_fn = ctx.in_function;
    ctx.in_function = true;
    let mut result = Value::Str(String::new());
    for s in &fd.body {
        match exec_stmt(s, ctx) {
            Ok(Flow::Normal) => {}
            Ok(Flow::Return(v)) => {
                result = v;
                break;
            }
            Ok(Flow::Next) | Ok(Flow::NextFile) | Ok(Flow::Break) | Ok(Flow::Continue) => {
                ctx.locals.pop();
                ctx.in_function = was_fn;
                return Err(Error::Runtime(
                    "invalid jump out of function (break/continue/next/nextfile)".into(),
                ));
            }
            Ok(Flow::ExitPending) => {
                ctx.locals.pop();
                ctx.in_function = was_fn;
                return Err(Error::Exit(ctx.rt.exit_code));
            }
            Err(e) => {
                ctx.locals.pop();
                ctx.in_function = was_fn;
                return Err(e);
            }
        }
    }
    ctx.locals.pop();
    ctx.in_function = was_fn;
    Ok(result)
}

fn call_user(fd: &FunctionDef, args: &[Expr], ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let vals: Vec<Value> = args
        .iter()
        .map(|e| eval_expr(e, ctx))
        .collect::<Result<_>>()?;
    call_user_with_values(&fd.name, vals, ctx)
}

fn interp_sort_keys_custom(ctx: &mut ExecCtx<'_>, keys: &mut [String], fname: &str) -> Result<()> {
    if !ctx.prog.funcs.contains_key(fname) {
        return Err(Error::Runtime(format!(
            "sorted_in: unknown function `{fname}`"
        )));
    }
    let err: RefCell<Option<Error>> = RefCell::new(None);
    keys.sort_by(|a, b| {
        if err.borrow().is_some() {
            return Ordering::Equal;
        }
        match call_user_with_values(
            fname,
            vec![Value::Str(a.clone()), Value::Str(b.clone())],
            ctx,
        ) {
            Ok(v) => {
                let n = v.as_number();
                if n < 0.0 {
                    Ordering::Less
                } else if n > 0.0 {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            Err(e) => {
                *err.borrow_mut() = Some(e);
                Ordering::Equal
            }
        }
    });
    if let Some(e) = err.into_inner() {
        return Err(e);
    }
    Ok(())
}

fn interp_for_in_keys(ctx: &mut ExecCtx<'_>, arr_name: &str) -> Result<Vec<String>> {
    if let SortedInMode::CustomFn(fname) = sorted_in_mode(ctx.rt) {
        if arr_name == "SYMTAB" {
            let mut keys = ctx.rt.symtab_keys_reflect();
            if ctx.rt.posix {
                return Ok(keys);
            }
            interp_sort_keys_custom(ctx, &mut keys, fname.as_str())?;
            return Ok(keys);
        }
        let Some(Value::Array(a)) = ctx.rt.get_global_var(arr_name) else {
            return Ok(Vec::new());
        };
        let mut keys: Vec<String> = a.keys().cloned().collect();
        if ctx.rt.posix {
            return Ok(keys);
        }
        interp_sort_keys_custom(ctx, &mut keys, fname.as_str())?;
        return Ok(keys);
    }
    Ok(ctx.rt.array_keys(arr_name))
}

#[cfg(test)]
mod tests {
    use super::{match_pattern, pattern_matches, run_begin};
    use crate::ast::{Expr, Pattern};
    use crate::bytecode::CompiledPattern;
    use crate::compiler::Compiler;
    use crate::parser::parse_program;
    use crate::runtime::Runtime;
    use crate::vm::vm_range_step;

    #[test]
    fn pattern_empty_matches() {
        let prog = parse_program("").unwrap();
        let mut rt = Runtime::new();
        rt.set_record_from_line("anything");
        assert!(pattern_matches(&Pattern::Empty, &mut rt, &prog).unwrap());
    }

    #[test]
    fn pattern_regexp_respects_record() {
        let prog = parse_program("").unwrap();
        let mut rt = Runtime::new();
        rt.set_record_from_line("hello");
        assert!(pattern_matches(&Pattern::Regexp("ell".into()), &mut rt, &prog).unwrap());
        assert!(!pattern_matches(&Pattern::Regexp("^z".into()), &mut rt, &prog).unwrap());
    }

    #[test]
    fn match_pattern_rejects_nested_range() {
        let prog = parse_program("").unwrap();
        let mut rt = Runtime::new();
        let p = Pattern::Range(
            Box::new(Pattern::Regexp("a".into())),
            Box::new(Pattern::Regexp("b".into())),
        );
        assert!(match_pattern(&p, &mut rt, &prog).is_err());
    }

    #[test]
    fn range_step_enters_after_start_pattern() {
        let prog = parse_program("/start/,/end/ { print }").unwrap();
        let cp = Compiler::compile_program(&prog);
        let rule = &cp.record_rules[0];
        let CompiledPattern::Range { start, end } = &rule.pattern else {
            panic!("expected range pattern");
        };
        let mut rt = Runtime::new();
        rt.set_record_from_line("start");
        let mut state = false;
        assert!(vm_range_step(&mut state, start, end, &cp, &mut rt).unwrap());
        assert!(state);
    }

    #[test]
    fn run_begin_executes_assignments() {
        let prog = parse_program("BEGIN { answer = 42 }").unwrap();
        let mut rt = Runtime::new();
        run_begin(&prog, &mut rt).unwrap();
        assert_eq!(rt.vars.get("answer").unwrap().as_number(), 42.0);
    }

    #[test]
    fn pattern_expr_matches_truthy_numeric() {
        let prog = parse_program("").unwrap();
        let mut rt = Runtime::new();
        rt.set_record_from_line("anything");
        let p = Pattern::Expr(Expr::Number(1.0));
        assert!(pattern_matches(&p, &mut rt, &prog).unwrap());
    }

    #[test]
    fn pattern_expr_matches_falsy_numeric_zero() {
        let prog = parse_program("").unwrap();
        let mut rt = Runtime::new();
        rt.set_record_from_line("anything");
        let p = Pattern::Expr(Expr::Number(0.0));
        assert!(!pattern_matches(&p, &mut rt, &prog).unwrap());
    }

    #[test]
    fn match_pattern_accepts_truthy_expr() {
        let prog = parse_program("").unwrap();
        let mut rt = Runtime::new();
        rt.set_record_from_line("z");
        let p = Pattern::Expr(Expr::Number(2.0));
        assert!(match_pattern(&p, &mut rt, &prog).unwrap());
    }
}
