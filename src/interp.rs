use crate::ast::*;
use crate::builtins;
use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use regex::Regex;
use std::collections::HashMap;

/// Control flow from executing statements (loops, rules, functions).
#[derive(Debug)]
pub enum Flow {
    Normal,
    Break,
    Continue,
    Next,
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

    fn get_var(&self, name: &str) -> Value {
        for frame in self.locals.iter().rev() {
            if let Some(v) = frame.get(name) {
                return v.clone();
            }
        }
        self.rt
            .vars
            .get(name)
            .cloned()
            .unwrap_or_else(|| match name {
                "NR" => Value::Num(self.rt.nr),
                "FNR" => Value::Num(self.rt.fnr),
                "NF" => Value::Num(self.rt.fields.len() as f64),
                "FILENAME" => Value::Str(self.rt.filename.clone()),
                _ => Value::Str(String::new()),
            })
    }

    fn set_var(&mut self, name: &str, val: Value) {
        for frame in self.locals.iter_mut().rev() {
            if frame.contains_key(name) {
                frame.insert(name.to_string(), val);
                return;
            }
        }
        self.rt.vars.insert(name.to_string(), val);
    }
}

pub fn run_begin(prog: &Program, rt: &mut Runtime) -> Result<()> {
    let mut ctx = ExecCtx::new(prog, rt);
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::Begin) {
            for s in &rule.stmts {
                match exec_stmt(s, &mut ctx)? {
                    Flow::Next => return Err(Error::Runtime("`next` is invalid in BEGIN".into())),
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
        Pattern::Begin | Pattern::End => false,
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
        Pattern::Begin | Pattern::End => false,
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

/// Range pattern: `state` is false until `p1` matches, then true until `p2` matches after a run.
pub fn range_step(
    state: &mut bool,
    p1: &Pattern,
    p2: &Pattern,
    rt: &mut Runtime,
    prog: &Program,
) -> Result<bool> {
    if !*state {
        if match_pattern(p1, rt, prog)? {
            *state = true;
        }
    }
    if *state {
        let run = true;
        if match_pattern(p2, rt, prog)? {
            *state = false;
        }
        return Ok(run);
    }
    Ok(false)
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
            while truthy(&eval_expr(cond, ctx)?) {
                for t in body {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        Flow::Break => break,
                        Flow::Continue => continue,
                        f @ (Flow::Next | Flow::Return(_) | Flow::ExitPending) => return Ok(f),
                    }
                }
            }
        }
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
                        f @ (Flow::Next | Flow::Return(_) | Flow::ExitPending) => return Ok(f),
                    }
                }
                if let Some(it) = iter {
                    eval_expr(it, ctx)?;
                }
            }
        }
        Stmt::ForIn { var, arr, body } => {
            let keys = ctx.rt.array_keys(arr);
            'outer: for k in keys {
                ctx.set_var(var, Value::Str(k.clone()));
                for t in body {
                    match exec_stmt(t, ctx)? {
                        Flow::Normal => {}
                        Flow::Break => break 'outer,
                        Flow::Continue => continue 'outer,
                        f @ (Flow::Next | Flow::Return(_) | Flow::ExitPending) => return Ok(f),
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
        Stmt::Print(args) => {
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
            ctx.emit_print(&chunk);
        }
        Stmt::Break => return Ok(Flow::Break),
        Stmt::Continue => return Ok(Flow::Continue),
        Stmt::Next => {
            if ctx.in_function {
                return Err(Error::Runtime("`next` used inside a function".into()));
            }
            return Ok(Flow::Next);
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
                }
                match &redir {
                    GetlineRedir::Primary => {
                        ctx.rt.nr += 1.0;
                        ctx.rt.fnr += 1.0;
                    }
                    GetlineRedir::File(_) => {}
                }
            }
            ctx.rt
                .vars
                .insert("NF".into(), Value::Num(ctx.rt.fields.len() as f64));
        }
        Stmt::Delete { name, index } => {
            if let Some(ix) = index {
                let k = eval_expr(ix, ctx)?.as_str();
                ctx.rt.array_delete(name, Some(&k));
            } else {
                ctx.rt.array_delete(name, None);
            }
        }
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
        Expr::Index { name, index } => {
            let k = eval_expr(index, ctx)?.as_str();
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
                apply_binop(*bop, &old, &v)?
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
                apply_binop(*bop, &old, &v)?
            } else {
                v
            };
            let s = newv.as_str();
            ctx.rt.set_field(idx, &s);
            newv
        }
        Expr::AssignIndex {
            name,
            index,
            op,
            rhs,
        } => {
            let k = eval_expr(index, ctx)?.as_str();
            let v = eval_expr(rhs, ctx)?;
            let newv = if let Some(bop) = op {
                let old = ctx.rt.array_get(name, &k);
                apply_binop(*bop, &old, &v)?
            } else {
                v
            };
            ctx.rt.array_set(name, k, newv.clone());
            newv
        }
        Expr::Call { name, args } => eval_call(name, args, ctx)?,
        Expr::Ternary { cond, then_, else_ } => {
            if truthy(&eval_expr(cond, ctx)?) {
                eval_expr(then_, ctx)?
            } else {
                eval_expr(else_, ctx)?
            }
        }
    })
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

fn awk_eq(left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let lv = eval_expr(left, ctx)?;
    let rv = eval_expr(right, ctx)?;
    if lv.is_numeric_str() && rv.is_numeric_str() {
        let a = lv.as_number();
        let b = rv.as_number();
        return Ok(Value::Num(if (a - b).abs() < f64::EPSILON {
            1.0
        } else {
            0.0
        }));
    }
    Ok(Value::Num(if lv.as_str() == rv.as_str() {
        1.0
    } else {
        0.0
    }))
}

fn awk_ne(left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let v = awk_eq(left, right, ctx)?;
    Ok(Value::Num(if v.as_number() != 0.0 { 0.0 } else { 1.0 }))
}

fn awk_rel(op: BinOp, left: &Expr, right: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let lv = eval_expr(left, ctx)?;
    let rv = eval_expr(right, ctx)?;
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
    let ok = match op {
        BinOp::Lt => ls < rs,
        BinOp::Le => ls <= rs,
        BinOp::Gt => ls > rs,
        BinOp::Ge => ls >= rs,
        _ => unreachable!(),
    };
    Ok(Value::Num(if ok { 1.0 } else { 0.0 }))
}

fn eval_unary(op: UnaryOp, expr: &Expr, ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let v = eval_expr(expr, ctx)?;
    Ok(match op {
        UnaryOp::Neg => Value::Num(-v.as_number()),
        UnaryOp::Pos => Value::Num(v.as_number()),
        UnaryOp::Not => Value::Num(if truthy(&v) { 0.0 } else { 1.0 }),
    })
}

fn apply_binop(op: BinOp, old: &Value, new: &Value) -> Result<Value> {
    let a = old.as_number();
    let b = new.as_number();
    let n = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Mod => a % b,
        _ => return Err(Error::Runtime("invalid compound assignment op".into())),
    };
    Ok(Value::Num(n))
}

fn eval_call(name: &str, args: &[Expr], ctx: &mut ExecCtx<'_>) -> Result<Value> {
    if let Some(fd) = ctx.prog.funcs.get(name) {
        return call_user(fd, args, ctx);
    }
    match name {
        "length" => {
            let s = if args.is_empty() {
                ctx.rt.record.clone()
            } else {
                eval_expr(&args[0], ctx)?.as_str()
            };
            Ok(Value::Num(s.chars().count() as f64))
        }
        "index" if args.len() == 2 => {
            let hay = eval_expr(&args[0], ctx)?.as_str();
            let needle = eval_expr(&args[1], ctx)?.as_str();
            if needle.is_empty() {
                return Ok(Value::Num(0.0));
            }
            let pos = hay.find(&needle).map(|i| i + 1).unwrap_or(0);
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
            let slice: String = s.chars().skip(start0).take(len).collect();
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
                    Expr::Index { name, index } => {
                        let k = eval_expr(index, ctx)?.as_str();
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
                    Expr::Index { name, index } => {
                        let k = eval_expr(index, ctx)?.as_str();
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
        "sqrt" if args.len() == 1 => {
            let n = eval_expr(&args[0], ctx)?.as_number();
            Ok(Value::Num(n.sqrt()))
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
            sprintf_simple(&fmt, &vals)
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
            let s = sprintf_simple(&fmt, &vals)?.as_str();
            ctx.emit_print(&s);
            Ok(Value::Num(0.0))
        }
        _ => Err(Error::Runtime(format!("unknown function `{name}`"))),
    }
}

fn sprintf_simple(fmt: &str, vals: &[Value]) -> Result<Value> {
    let mut out = String::new();
    let mut vi = 0usize;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                Some('s') => {
                    let v = vals
                        .get(vi)
                        .ok_or_else(|| Error::Runtime("sprintf: not enough args".into()))?;
                    vi += 1;
                    out.push_str(&v.as_str());
                }
                Some('d') | Some('i') => {
                    let v = vals
                        .get(vi)
                        .ok_or_else(|| Error::Runtime("sprintf: not enough args".into()))?;
                    vi += 1;
                    out.push_str(&format!("{}", v.as_number() as i64));
                }
                Some('f') | Some('g') | Some('e') => {
                    let v = vals
                        .get(vi)
                        .ok_or_else(|| Error::Runtime("sprintf: not enough args".into()))?;
                    vi += 1;
                    out.push_str(&format!("{}", v.as_number()));
                }
                Some(x) => {
                    return Err(Error::Runtime(format!(
                        "unsupported sprintf conversion %{x}"
                    )));
                }
                None => return Err(Error::Runtime("truncated format".into())),
            }
        } else {
            out.push(c);
        }
    }
    Ok(Value::Str(out))
}

fn call_user(fd: &FunctionDef, args: &[Expr], ctx: &mut ExecCtx<'_>) -> Result<Value> {
    let mut vals: Vec<Value> = args
        .iter()
        .map(|e| eval_expr(e, ctx))
        .collect::<Result<_>>()?;
    while vals.len() < fd.params.len() {
        vals.push(Value::Str(String::new()));
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
            Ok(Flow::Next) | Ok(Flow::Break) | Ok(Flow::Continue) => {
                ctx.locals.pop();
                ctx.in_function = was_fn;
                return Err(Error::Runtime(
                    "invalid jump out of function (break/continue/next)".into(),
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
