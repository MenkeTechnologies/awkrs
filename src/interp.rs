use crate::ast::*;
use crate::error::{Error, Result};
use crate::runtime::{Runtime, Value};
use regex::Regex;

pub fn run_begin(prog: &Program, rt: &mut Runtime) -> Result<()> {
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::Begin) {
            for s in &rule.stmts {
                exec_stmt(s, rt)?;
            }
        }
    }
    Ok(())
}

pub fn run_end(prog: &Program, rt: &mut Runtime) -> Result<()> {
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::End) {
            for s in &rule.stmts {
                exec_stmt(s, rt)?;
            }
        }
    }
    Ok(())
}

pub fn run_rule_on_record(prog: &Program, rt: &mut Runtime, rule_idx: usize) -> Result<()> {
    let rule = &prog.rules[rule_idx];
    for s in &rule.stmts {
        exec_stmt(s, rt)?;
    }
    Ok(())
}

pub fn pattern_matches(pat: &Pattern, rt: &mut Runtime) -> Result<bool> {
    Ok(match pat {
        Pattern::Begin | Pattern::End => false,
        Pattern::Empty => true,
        Pattern::Regexp(re) => {
            let r = Regex::new(re).map_err(|e| Error::Runtime(e.to_string()))?;
            r.is_match(&rt.record)
        }
        Pattern::Expr(e) => truthy(&eval_expr(e, rt)?),
        Pattern::Range(_, _) => {
            return Err(Error::Runtime("range patterns are not implemented yet".into()));
        }
    })
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Num(n) => *n != 0.0,
        Value::Str(s) => !s.is_empty() && s.parse::<f64>().map(|n| n != 0.0).unwrap_or(true),
    }
}

fn exec_stmt(s: &Stmt, rt: &mut Runtime) -> Result<()> {
    match s {
        Stmt::If {
            cond,
            then_,
            else_,
        } => {
            if truthy(&eval_expr(cond, rt)?) {
                for t in then_ {
                    exec_stmt(t, rt)?;
                }
            } else {
                for t in else_ {
                    exec_stmt(t, rt)?;
                }
            }
        }
        Stmt::While { cond, body } => {
            while truthy(&eval_expr(cond, rt)?) {
                for t in body {
                    exec_stmt(t, rt)?;
                }
            }
        }
        Stmt::Block(ss) => {
            for t in ss {
                exec_stmt(t, rt)?;
            }
        }
        Stmt::Expr(e) => {
            eval_expr(e, rt)?;
        }
        Stmt::Print(args) => {
            let ofs = rt.vars.get("OFS").map(|v| v.as_str()).unwrap_or_else(|| " ".into());
            let ors = rt.vars.get("ORS").map(|v| v.as_str()).unwrap_or_else(|| "\n".into());
            let mut parts = Vec::new();
            for a in args {
                parts.push(eval_expr(a, rt)?.as_str());
            }
            let line = if parts.is_empty() {
                String::new()
            } else {
                parts.join(&ofs)
            };
            print!("{line}{ors}");
        }
    }
    Ok(())
}

pub fn eval_expr(e: &Expr, rt: &mut Runtime) -> Result<Value> {
    if let Expr::Binary { op, left, right } = e {
        if *op == BinOp::And {
            let lv = eval_expr(left, rt)?;
            if !truthy(&lv) {
                return Ok(Value::Num(0.0));
            }
            return Ok(Value::Num(if truthy(&eval_expr(right, rt)?) {
                1.0
            } else {
                0.0
            }));
        }
        if *op == BinOp::Or {
            let lv = eval_expr(left, rt)?;
            if truthy(&lv) {
                return Ok(Value::Num(1.0));
            }
            return Ok(Value::Num(if truthy(&eval_expr(right, rt)?) {
                1.0
            } else {
                0.0
            }));
        }
    }
    Ok(match e {
        Expr::Number(n) => Value::Num(*n),
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::Var(name) => rt
            .vars
            .get(name)
            .cloned()
            .unwrap_or_else(|| match name.as_str() {
                "NR" => Value::Num(rt.nr),
                "FNR" => Value::Num(rt.fnr),
                "NF" => Value::Num(rt.fields.len() as f64),
                "FILENAME" => Value::Str(rt.filename.clone()),
                _ => Value::Str(String::new()),
            }),
        Expr::Field(inner) => {
            let i = eval_expr(inner, rt)?.as_number() as i32;
            rt.field(i)
        }
        Expr::Binary { op, left, right } => eval_binary(*op, left, right, rt)?,
        Expr::Unary { op, expr } => eval_unary(*op, expr, rt)?,
        Expr::Assign { name, op, rhs } => {
            let v = eval_expr(rhs, rt)?;
            let newv = if let Some(bop) = op {
                let old = rt
                    .vars
                    .get(name)
                    .cloned()
                    .unwrap_or(Value::Num(0.0));
                apply_binop(*bop, &old, &v)?
            } else {
                v
            };
            rt.vars.insert(name.clone(), newv.clone());
            newv
        }
        Expr::AssignField { field, op, rhs } => {
            let idx = eval_expr(field, rt)?.as_number() as i32;
            let v = eval_expr(rhs, rt)?;
            let newv = if let Some(bop) = op {
                let old = Value::Str(rt.field(idx).as_str());
                apply_binop(*bop, &old, &v)?
            } else {
                v
            };
            let s = newv.as_str();
            rt.set_field(idx, &s);
            newv
        }
        Expr::Incr { .. } => Value::Num(0.0),
        Expr::Call { name, args } => eval_call(name, args, rt)?,
        Expr::Ternary { cond, then_, else_ } => {
            if truthy(&eval_expr(cond, rt)?) {
                eval_expr(then_, rt)?
            } else {
                eval_expr(else_, rt)?
            }
        }
    })
}

fn eval_binary(op: BinOp, left: &Expr, right: &Expr, rt: &mut Runtime) -> Result<Value> {
    if op == BinOp::Concat {
        let a = eval_expr(left, rt)?.as_str();
        let b = eval_expr(right, rt)?.as_str();
        return Ok(Value::Str(format!("{a}{b}")));
    }
    if op == BinOp::Match || op == BinOp::NotMatch {
        let s = eval_expr(left, rt)?.as_str();
        let pat = eval_expr(right, rt)?.as_str();
        let r = Regex::new(&pat).map_err(|e| Error::Runtime(e.to_string()))?;
        let m = r.is_match(&s);
        let res = if op == BinOp::Match { m } else { !m };
        return Ok(Value::Num(if res { 1.0 } else { 0.0 }));
    }
    if op == BinOp::Eq {
        let ls = eval_expr(left, rt)?.as_str();
        let rs = eval_expr(right, rt)?.as_str();
        return Ok(Value::Num(if ls == rs { 1.0 } else { 0.0 }));
    }
    if op == BinOp::Ne {
        let ls = eval_expr(left, rt)?.as_str();
        let rs = eval_expr(right, rt)?.as_str();
        return Ok(Value::Num(if ls != rs { 1.0 } else { 0.0 }));
    }
    let a = eval_expr(left, rt)?.as_number();
    let b = eval_expr(right, rt)?.as_number();
    let n = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Mod => a % b,
        BinOp::Lt => return Ok(Value::Num(if a < b { 1.0 } else { 0.0 })),
        BinOp::Le => return Ok(Value::Num(if a <= b { 1.0 } else { 0.0 })),
        BinOp::Gt => return Ok(Value::Num(if a > b { 1.0 } else { 0.0 })),
        BinOp::Ge => return Ok(Value::Num(if a >= b { 1.0 } else { 0.0 })),
        BinOp::Eq | BinOp::Ne | BinOp::Concat | BinOp::Match | BinOp::NotMatch | BinOp::And | BinOp::Or => {
            unreachable!()
        }
    };
    Ok(Value::Num(n))
}

fn eval_unary(op: UnaryOp, expr: &Expr, rt: &mut Runtime) -> Result<Value> {
    let v = eval_expr(expr, rt)?;
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

fn eval_call(name: &str, args: &[Expr], rt: &mut Runtime) -> Result<Value> {
    match name {
        "length" => {
            let s = if args.is_empty() {
                rt.record.clone()
            } else {
                eval_expr(&args[0], rt)?.as_str()
            };
            Ok(Value::Num(s.chars().count() as f64))
        }
        "index" if args.len() == 2 => {
            let hay = eval_expr(&args[0], rt)?.as_str();
            let needle = eval_expr(&args[1], rt)?.as_str();
            if needle.is_empty() {
                return Ok(Value::Num(0.0));
            }
            let pos = hay.find(&needle).map(|i| i + 1).unwrap_or(0);
            Ok(Value::Num(pos as f64))
        }
        "substr" => {
            let s = eval_expr(args.first().ok_or_else(|| Error::Runtime("substr".into()))?, rt)?.as_str();
            let start = eval_expr(args.get(1).ok_or_else(|| Error::Runtime("substr".into()))?, rt)?.as_number() as usize;
            let len = if let Some(e) = args.get(2) {
                eval_expr(e, rt)?.as_number() as usize
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
        "split" => Err(Error::Runtime(
            "`split` is not implemented yet".into(),
        )),
        _ => Err(Error::Runtime(format!("unknown function `{name}`"))),
    }
}
