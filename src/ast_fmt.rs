//! Reformatted awk-like source from the AST (`-o` / `--pretty-print`), not `Debug` dumps.

use crate::ast::{
    BinOp, Expr, FunctionDef, IncDecOp, IncDecTarget, Pattern, PrintRedir, Program, Rule, Stmt,
    SwitchArm, SwitchLabel, UnaryOp,
};

fn format_field_expr(inner: &Expr) -> String {
    match inner {
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => {
            format!("${}", *n as i64)
        }
        _ => format!("$({})", format_expr(inner)),
    }
}

/// Pretty-print a program as readable awk-like text (best-effort; not byte-identical to input).
pub fn format_program(prog: &Program) -> String {
    let mut out = String::new();
    for r in &prog.rules {
        out.push_str(&format_rule(r));
        out.push('\n');
    }
    let mut fnames: Vec<&String> = prog.funcs.keys().collect();
    fnames.sort();
    for name in fnames {
        out.push_str(&format_function(name, &prog.funcs[name]));
        out.push('\n');
    }
    out
}

fn format_rule(r: &Rule) -> String {
    let mut s = String::new();
    match &r.pattern {
        Pattern::Begin => s.push_str("BEGIN"),
        Pattern::End => s.push_str("END"),
        Pattern::BeginFile => s.push_str("BEGINFILE"),
        Pattern::EndFile => s.push_str("ENDFILE"),
        Pattern::Expr(e) => s.push_str(&format_expr(e)),
        Pattern::Regexp(re) => {
            s.push('/');
            s.push_str(&escape_regex_slash(re));
            s.push('/');
        }
        Pattern::Range(a, b) => {
            s.push_str(&format_pattern_inner(a));
            s.push(',');
            s.push_str(&format_pattern_inner(b));
        }
        Pattern::Empty => {}
    }
    if r.stmts.is_empty() {
        if !matches!(r.pattern, Pattern::Empty) {
            s.push_str(" { }");
        }
        return s;
    }
    s.push_str(" {\n");
    for st in &r.stmts {
        for line in format_stmt(st, 1).lines() {
            s.push_str("    ");
            s.push_str(line);
            s.push('\n');
        }
    }
    s.push('}');
    s
}

fn format_pattern_inner(p: &Pattern) -> String {
    match p {
        Pattern::Expr(e) => format_expr(e),
        Pattern::Regexp(re) => format!("/{}/", escape_regex_slash(re)),
        _ => format!("({p:?})"),
    }
}

fn format_function(name: &str, fd: &FunctionDef) -> String {
    let mut s = format!("function {}({}) {{\n", name, fd.params.join(", "));
    for st in &fd.body {
        for line in format_stmt(st, 1).lines() {
            s.push_str("    ");
            s.push_str(line);
            s.push('\n');
        }
    }
    s.push('}');
    s
}

fn indent(n: usize) -> String {
    "    ".repeat(n)
}

fn format_stmt(st: &Stmt, depth: usize) -> String {
    let ind = indent(depth);
    match st {
        Stmt::If { cond, then_, else_ } => {
            let mut s = format!("{ind}if ({}) {{\n", format_expr(cond));
            for t in then_ {
                s.push_str(&format_stmt(t, depth + 1));
            }
            s.push_str(&ind);
            s.push('}');
            if !else_.is_empty() {
                s.push_str(" else {\n");
                for t in else_ {
                    s.push_str(&format_stmt(t, depth + 1));
                }
                s.push_str(&ind);
                s.push('}');
            }
            s.push('\n');
            s
        }
        Stmt::While { cond, body } => {
            let mut s = format!("{ind}while ({}) {{\n", format_expr(cond));
            for t in body {
                s.push_str(&format_stmt(t, depth + 1));
            }
            s.push_str(&ind);
            s.push_str("}\n");
            s
        }
        Stmt::DoWhile { body, cond } => {
            let mut s = format!("{ind}do {{\n");
            for t in body {
                s.push_str(&format_stmt(t, depth + 1));
            }
            s.push_str(&ind);
            s.push_str("} while (");
            s.push_str(&format_expr(cond));
            s.push_str(");\n");
            s
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            let mut s = format!("{ind}for (");
            if let Some(e) = init {
                s.push_str(&format_expr(e));
            }
            s.push(';');
            if let Some(e) = cond {
                s.push_str(&format_expr(e));
            }
            s.push(';');
            if let Some(e) = iter {
                s.push_str(&format_expr(e));
            }
            s.push_str(") {\n");
            for t in body {
                s.push_str(&format_stmt(t, depth + 1));
            }
            s.push_str(&ind);
            s.push_str("}\n");
            s
        }
        Stmt::ForIn { var, arr, body } => {
            let mut s = format!("{ind}for ({var} in {arr}) {{\n");
            for t in body {
                s.push_str(&format_stmt(t, depth + 1));
            }
            s.push_str(&ind);
            s.push_str("}\n");
            s
        }
        Stmt::Block(ss) => {
            let mut s = String::new();
            for t in ss {
                s.push_str(&format_stmt(t, depth));
            }
            s
        }
        Stmt::Expr(e) => format!("{ind}{};\n", format_expr(e)),
        Stmt::Print { args, redir } => {
            let mut s = format!("{ind}print");
            if !args.is_empty() {
                s.push(' ');
                s.push_str(&args.iter().map(format_expr).collect::<Vec<_>>().join(", "));
            }
            if let Some(r) = redir {
                s.push_str(&format_print_redir(r));
            }
            s.push_str(";\n");
            s
        }
        Stmt::Printf { args, redir } => {
            let mut s = format!("{ind}printf ");
            if !args.is_empty() {
                s.push_str(&args.iter().map(format_expr).collect::<Vec<_>>().join(", "));
            }
            if let Some(r) = redir {
                s.push_str(&format_print_redir(r));
            }
            s.push_str(";\n");
            s
        }
        Stmt::Break => format!("{ind}break;\n"),
        Stmt::Continue => format!("{ind}continue;\n"),
        Stmt::Next => format!("{ind}next;\n"),
        Stmt::NextFile => format!("{ind}nextfile;\n"),
        Stmt::Exit(e) => {
            if let Some(ex) = e {
                format!("{ind}exit {};\n", format_expr(ex))
            } else {
                format!("{ind}exit;\n")
            }
        }
        Stmt::Return(e) => {
            if let Some(ex) = e {
                format!("{ind}return {};\n", format_expr(ex))
            } else {
                format!("{ind}return;\n")
            }
        }
        Stmt::Delete { name, indices } => match indices {
            None => format!("{ind}delete {name};\n"),
            Some(ix) => {
                let parts: Vec<_> = ix.iter().map(format_expr).collect();
                format!("{ind}delete {name}[{}];\n", parts.join(", "))
            }
        },
        Stmt::GetLine {
            pipe_cmd,
            var,
            redir,
        } => {
            let mut s = ind.to_string();
            if let Some(cmd) = pipe_cmd {
                s.push_str(&format_expr(cmd));
                s.push_str(" | ");
            }
            s.push_str("getline");
            if let Some(v) = var {
                s.push(' ');
                s.push_str(v);
            }
            use crate::ast::GetlineRedir;
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) => {
                    s.push_str(" < ");
                    s.push_str(&format_expr(e));
                }
                GetlineRedir::Coproc(e) => {
                    s.push_str(" <& ");
                    s.push_str(&format_expr(e));
                }
            }
            s.push_str(";\n");
            s
        }
        Stmt::Switch { expr, arms } => {
            let mut s = format!("{ind}switch ({}) {{\n", format_expr(expr));
            for a in arms {
                match a {
                    SwitchArm::Case { label, stmts } => {
                        match label {
                            SwitchLabel::Expr(e) => {
                                s.push_str(&indent(depth + 1));
                                s.push_str(&format!("case {}:\n", format_expr(e)));
                            }
                            SwitchLabel::Regexp(re) => {
                                s.push_str(&indent(depth + 1));
                                s.push_str(&format!("case /{}/:\n", escape_regex_slash(re)));
                            }
                        }
                        for st in stmts {
                            s.push_str(&format_stmt(st, depth + 2));
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        s.push_str(&indent(depth + 1));
                        s.push_str("default:\n");
                        for st in stmts {
                            s.push_str(&format_stmt(st, depth + 2));
                        }
                    }
                }
            }
            s.push_str(&ind);
            s.push_str("}\n");
            s
        }
    }
}

fn format_print_redir(r: &PrintRedir) -> String {
    match r {
        PrintRedir::Overwrite(e) => format!(" > {}", format_expr(e)),
        PrintRedir::Append(e) => format!(" >> {}", format_expr(e)),
        PrintRedir::Pipe(e) => format!(" | {}", format_expr(e)),
        PrintRedir::Coproc(e) => format!(" |& {}", format_expr(e)),
    }
}

fn escape_regex_slash(s: &str) -> String {
    s.replace('\\', "\\\\").replace('/', "\\/")
}

pub(crate) fn format_expr(e: &Expr) -> String {
    match e {
        Expr::Number(n) => {
            if n.is_finite() {
                format!("{n}")
            } else {
                "0".into()
            }
        }
        Expr::Str(s) => format_string_literal(s),
        Expr::RegexpLiteral(s) => format!("@/{}/", escape_regex_slash(s)),
        Expr::Var(v) => v.clone(),
        Expr::Field(inner) => format_field_expr(inner),
        Expr::Index { name, indices } => {
            let parts: Vec<_> = indices.iter().map(format_expr).collect();
            format!("{name}[{}]", parts.join(", "))
        }
        Expr::Binary { op, left, right } => {
            format!(
                "({} {} {})",
                format_expr(left),
                binop_str(*op),
                format_expr(right)
            )
        }
        Expr::Unary { op, expr } => match op {
            UnaryOp::Neg => format!("(-{})", format_expr(expr)),
            UnaryOp::Pos => format!("(+{})", format_expr(expr)),
            UnaryOp::Not => format!("(!{})", format_expr(expr)),
        },
        Expr::Assign { name, op, rhs } => {
            if let Some(bop) = op {
                format!("{name} {} {}", binop_str(*bop), format_expr(rhs))
            } else {
                format!("{name} = {}", format_expr(rhs))
            }
        }
        Expr::AssignField { field, op, rhs } => {
            let lhs = format_field_expr(field);
            if let Some(bop) = op {
                format!("{lhs} {} {}", binop_str(*bop), format_expr(rhs))
            } else {
                format!("{lhs} = {}", format_expr(rhs))
            }
        }
        Expr::AssignIndex {
            name,
            indices,
            op,
            rhs,
        } => {
            let ix: Vec<_> = indices.iter().map(format_expr).collect();
            let lhs = format!("{name}[{}]", ix.join(", "));
            if let Some(bop) = op {
                format!("{lhs} {} {}", binop_str(*bop), format_expr(rhs))
            } else {
                format!("{lhs} = {}", format_expr(rhs))
            }
        }
        Expr::Call { name, args } => {
            let a: Vec<_> = args.iter().map(format_expr).collect();
            format!("{name}({})", a.join(", "))
        }
        Expr::IndirectCall { callee, args } => {
            let a: Vec<_> = args.iter().map(format_expr).collect();
            format!("@{}({})", format_expr(callee), a.join(", "))
        }
        Expr::Ternary { cond, then_, else_ } => format!(
            "({} ? {} : {})",
            format_expr(cond),
            format_expr(then_),
            format_expr(else_)
        ),
        Expr::In { key, arr } => format!("{} in {}", format_expr(key), arr),
        Expr::IncDec { op, target } => match target {
            IncDecTarget::Var(n) => format_incdec_var(op, n),
            IncDecTarget::Field(inner) => format_incdec_field(op, inner),
            IncDecTarget::Index { name, indices } => {
                let ix: Vec<_> = indices.iter().map(format_expr).collect();
                format_incdec_index(op, &format!("{name}[{}]", ix.join(", ")))
            }
        },
        Expr::GetLine {
            pipe_cmd,
            var,
            redir,
        } => {
            let mut s = String::new();
            if let Some(cmd) = pipe_cmd {
                s.push_str(&format_expr(cmd));
                s.push_str(" | ");
            }
            s.push_str("getline");
            if let Some(v) = var {
                s.push(' ');
                s.push_str(v);
            }
            use crate::ast::GetlineRedir;
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) => {
                    s.push_str(" < ");
                    s.push_str(&format_expr(e));
                }
                GetlineRedir::Coproc(e) => {
                    s.push_str(" <& ");
                    s.push_str(&format_expr(e));
                }
            }
            s
        }
    }
}

fn format_incdec_var(op: &IncDecOp, name: &str) -> String {
    match op {
        IncDecOp::PreInc => format!("++{name}"),
        IncDecOp::PostInc => format!("{name}++"),
        IncDecOp::PreDec => format!("--{name}"),
        IncDecOp::PostDec => format!("{name}--"),
    }
}

fn format_incdec_field(op: &IncDecOp, inner: &Expr) -> String {
    let f = format!("$({})", format_expr(inner));
    format_incdec_index(op, &f)
}

fn format_incdec_index(op: &IncDecOp, lhs: &str) -> String {
    match op {
        IncDecOp::PreInc => format!("++{lhs}"),
        IncDecOp::PostInc => format!("{lhs}++"),
        IncDecOp::PreDec => format!("--{lhs}"),
        IncDecOp::PostDec => format!("{lhs}--"),
    }
}

fn format_string_literal(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "^",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Match => "~",
        BinOp::NotMatch => "!~",
        BinOp::Concat => " ",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_program;

    #[test]
    fn pretty_print_roundtrip_shape() {
        let src = "BEGIN { x = 1; print x + 2 }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("BEGIN"));
        assert!(out.contains("x = 1"));
        assert!(out.contains("print"));
    }
}
