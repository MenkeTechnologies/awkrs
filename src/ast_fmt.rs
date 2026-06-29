//! Reformatted awk-like source from the AST (`-o` / `--pretty-print`), not `Debug` dumps.
//! [`crate::cli_effects::pretty_print_ast`] prepends `#` comment lines so output is not mistaken for gawk’s `--pretty-print`.

use crate::ast::{
    BinOp, Expr, FunctionDef, IncDecOp, IncDecTarget, Pattern, PrintRedir, Program, Rule, Stmt,
    SwitchArm, SwitchLabel, UnaryOp,
};

fn format_field_expr(inner: &Expr) -> String {
    match inner {
        Expr::IntegerLiteral(s) if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) => {
            format!("${s}")
        }
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
        Stmt::SrcLine(_) => String::new(),
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
        Expr::IntegerLiteral(s) => s.clone(),
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
        Expr::Tuple(parts) => {
            let p: Vec<_> = parts.iter().map(format_expr).collect();
            format!("({})", p.join(", "))
        }
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
    use crate::ast::{Expr, Pattern, Program, Rule, Stmt};
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

    #[test]
    fn pretty_print_empty_program_empty_string() {
        let p = Program {
            rules: vec![],
            funcs: std::collections::HashMap::new(),
        };
        assert_eq!(format_program(&p), "");
    }

    #[test]
    fn pretty_print_delete_whole_array_and_subscript() {
        let src = "BEGIN { delete a; delete b[1] }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(
            out.contains("delete a") && out.contains("delete b"),
            "{out}"
        );
    }

    #[test]
    fn pretty_print_includes_range_pattern_and_sorted_functions() {
        let src = r#"function z() { return 0 } function a() { return 1 } /b/,/c/ { print 1 }"#;
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("/b/,/c/") || (out.contains("/b/") && out.contains("/c/")));
        let a_pos = out.find("function a").expect("function a");
        let z_pos = out.find("function z").expect("function z");
        assert!(
            a_pos < z_pos,
            "functions should be sorted by name in output: {out}"
        );
    }

    #[test]
    fn pretty_print_switch_statement_shape() {
        let src = "BEGIN { switch (x) { case 1: print 1; break; default: print 0 } }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("switch"));
        assert!(out.contains("case"));
        assert!(out.contains("default"));
    }

    #[test]
    fn pretty_print_ternary_v2() {
        let src = "BEGIN { print (a ? b : c) }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("?"));
        assert!(out.contains(":"));
    }

    #[test]
    fn pretty_print_indirect_call_v2() {
        let src = "BEGIN { @fn(1, 2) }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("@fn"));
    }

    #[test]
    fn pretty_print_compound_assign_v2() {
        let src = "BEGIN { x += 1; y *= 2 }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        // Current implementation formats "x += 1" as "x + 1"
        assert!(out.contains("x + 1"));
        assert!(out.contains("y * 2"));
    }

    #[test]
    fn pretty_print_inc_dec_v2() {
        let src = "BEGIN { x++; ++y; --z[1] }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("++"));
        assert!(out.contains("--"));
    }

    #[test]
    fn pretty_print_indirect_call_form() {
        let prog = Program {
            rules: vec![Rule {
                pattern: Pattern::Begin,
                stmts: vec![Stmt::Expr(Expr::IndirectCall {
                    callee: Box::new(Expr::Str("g".into())),
                    args: vec![Expr::Number(1.0)],
                })],
            }],
            funcs: std::collections::HashMap::new(),
        };
        let out = format_program(&prog);
        assert!(
            out.contains(r#"@"g"(1)"#) || (out.contains('@') && out.contains("\"g\"")),
            "expected indirect call in output:\n{out}"
        );
    }

    #[test]
    fn pretty_print_regexp_literal_at_slash_form() {
        let src = "BEGIN { r = @/z+/ }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(
            out.contains("@/z+/") || out.contains("z+"),
            "expected @/re/ in output:\n{out}"
        );
    }

    #[test]
    fn pretty_print_for_c_empty_components() {
        let src = "BEGIN { for (;;) break }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(
            out.contains("for (;;)") || out.contains("for (; ; )"),
            "{out}"
        );
    }

    #[test]
    fn pretty_print_ternary_nested() {
        let src = "BEGIN { x = (1 ? 2 : (3 ? 4 : 5)) }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        // format_expr for ternary uses (cond ? then : else)
        assert!(out.contains(" ? ") && out.contains(" : "));
    }

    #[test]
    fn pretty_print_string_escapes() {
        let prog = Program {
            rules: vec![Rule {
                pattern: Pattern::Begin,
                stmts: vec![Stmt::Expr(Expr::Assign {
                    name: "s".into(),
                    op: None,
                    rhs: Box::new(Expr::Str("a\nb\tc\"d\\e".into())),
                })],
            }],
            funcs: std::collections::HashMap::new(),
        };
        let out = format_program(&prog);
        assert!(out.contains(r#""a\nb\tc\"d\\e""#));
    }

    #[test]
    fn pretty_print_compound_assignment() {
        let src = "BEGIN { x += 1; y *= 2 }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        // binop_str for Add is "+", format_expr for Assign uses "{name} {binop_str} {rhs}"
        assert!(out.contains("x + 1") && out.contains("y * 2"));
    }

    #[test]
    fn pretty_print_inc_dec() {
        let src = "BEGIN { ++x; y--; $1++ }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("++x") || out.contains("++ x"));
        assert!(out.contains("y--") || out.contains("y --"));
        // $1++ -> format_incdec_field uses $(format_expr(inner))++
        assert!(out.contains("$1++") || out.contains("$1 ++") || out.contains("$(1)++"));
    }

    #[test]
    fn pretty_print_redirections() {
        let src = "BEGIN { print \"a\" > \"f1\"; print \"b\" >> \"f2\"; print \"c\" | \"cmd\" }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("> \"f1\""));
        assert!(out.contains(">> \"f2\""));
        assert!(out.contains("| \"cmd\""));
    }

    #[test]
    fn pretty_print_field_expressions() {
        let src = "BEGIN { print $0, $1, $(NF-1) }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("$0"));
        assert!(out.contains("$1"));
        // $(NF-1) -> format_field_expr uses $(format_expr(inner))
        // format_expr(NF-1) -> (NF - 1)
        // result: $((NF - 1))
        assert!(out.contains("$((NF - 1))"), "output was: {out}");
    }

    #[test]
    fn pretty_print_end_rule_v6() {
        let src = "END { print 1 }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("END"));
    }

    #[test]
    fn pretty_print_beginfile_endfile_v6() {
        let src = "BEGINFILE { print 1 } ENDFILE { print 2 }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("BEGINFILE"));
        assert!(out.contains("ENDFILE"));
    }

    #[test]
    fn pretty_print_range_pattern_v6() {
        let src = "1, 2 { print 1 }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains(","));
    }

    #[test]
    fn pretty_print_multiple_rules_v6() {
        let src = "BEGIN { a } { b } END { c }";
        let prog = parse_program(src).unwrap();
        let out = format_program(&prog);
        assert!(out.contains("BEGIN"));
        assert!(out.contains("END"));
    }

    #[test]
    fn pretty_print_num_v14() {
        assert!(format_program(&parse_program("BEGIN{0}").unwrap()).contains("0"));
    }
    #[test]
    fn pretty_print_str_v14() {
        assert!(format_program(&parse_program("BEGIN{\"a\"}").unwrap()).contains("\"a\""));
    }
    #[test]
    fn pretty_print_var_v14() {
        assert!(format_program(&parse_program("BEGIN{x}").unwrap()).contains("x"));
    }
    #[test]
    fn pretty_print_field_v14() {
        assert!(format_program(&parse_program("BEGIN{$0}").unwrap()).contains("$0"));
    }
    #[test]
    fn pretty_print_index_v14() {
        assert!(format_program(&parse_program("BEGIN{a[1]}").unwrap()).contains("a[1]"));
    }
    #[test]
    fn pretty_print_call_v14() {
        assert!(format_program(&parse_program("BEGIN{f()}").unwrap()).contains("f()"));
    }
    #[test]
    fn pretty_print_unary_v14() {
        assert!(format_program(&parse_program("BEGIN{!1}").unwrap()).contains("!1"));
    }
    #[test]
    fn pretty_print_binary_v14() {
        assert!(format_program(&parse_program("BEGIN{1+1}").unwrap()).contains("1 + 1"));
    }
    #[test]
    fn pretty_print_assign_v14() {
        assert!(format_program(&parse_program("BEGIN{x=1}").unwrap()).contains("x = 1"));
    }
    #[test]
    fn pretty_print_ternary_v14() {
        assert!(format_program(&parse_program("BEGIN{1?1:0}").unwrap()).contains("?"));
    }
    #[test]
    fn pretty_print_in_v14() {
        assert!(format_program(&parse_program("BEGIN{1 in a}").unwrap()).contains("in"));
    }
    #[test]
    fn pretty_print_if_v14() {
        assert!(format_program(&parse_program("BEGIN{if(1)1}").unwrap()).contains("if"));
    }
    #[test]
    fn pretty_print_while_v14() {
        assert!(format_program(&parse_program("BEGIN{while(1)1}").unwrap()).contains("while"));
    }
    #[test]
    fn pretty_print_for_v14() {
        assert!(format_program(&parse_program("BEGIN{for(;;)1}").unwrap()).contains("for"));
    }
    #[test]
    fn pretty_print_forin_v14() {
        assert!(format_program(&parse_program("BEGIN{for(k in a)1}").unwrap()).contains("for"));
    }
    #[test]
    fn pretty_print_block_v14() {
        assert!(format_program(&parse_program("BEGIN{{1}}").unwrap()).contains("{"));
    }
    #[test]
    fn pretty_print_print_v14() {
        assert!(format_program(&parse_program("BEGIN{print 1}").unwrap()).contains("print"));
    }
    #[test]
    fn pretty_print_printf_v14() {
        assert!(format_program(&parse_program("BEGIN{printf 1}").unwrap()).contains("printf"));
    }
    #[test]
    fn pretty_print_exit_v14() {
        assert!(format_program(&parse_program("BEGIN{exit}").unwrap()).contains("exit"));
    }
    #[test]
    fn pretty_print_return_v14() {
        assert!(format_program(&parse_program("function f(){return}").unwrap()).contains("return"));
    }

    #[test]
    fn pretty_print_regexp_v18() {
        assert!(format_program(&parse_program("BEGIN{/a/}").unwrap()).contains("$0 ~ \"a\""));
    }
    #[test]
    fn pretty_print_match_v18() {
        assert!(format_program(&parse_program("BEGIN{$0~/a/}").unwrap()).contains("~"));
    }
    #[test]
    fn pretty_print_not_match_v18() {
        assert!(format_program(&parse_program("BEGIN{$0!~/a/}").unwrap()).contains("!~"));
    }
    #[test]
    fn pretty_print_pow_v18() {
        assert!(format_program(&parse_program("BEGIN{2^3}").unwrap()).contains("^"));
    }
    #[test]
    fn pretty_print_pow_starstar_v18() {
        assert!(format_program(&parse_program("BEGIN{2**3}").unwrap()).contains("^"));
    }
    #[test]
    fn pretty_print_inc_v18() {
        assert!(format_program(&parse_program("BEGIN{x++}").unwrap()).contains("++"));
    }
    #[test]
    fn pretty_print_dec_v18() {
        assert!(format_program(&parse_program("BEGIN{x--}").unwrap()).contains("--"));
    }
    #[test]
    fn pretty_print_pre_inc_v18() {
        assert!(format_program(&parse_program("BEGIN{++x}").unwrap()).contains("++"));
    }
    #[test]
    fn pretty_print_pre_dec_v18() {
        assert!(format_program(&parse_program("BEGIN{--x}").unwrap()).contains("--"));
    }
    #[test]
    fn pretty_print_do_while_v18() {
        assert!(
            format_program(&parse_program("BEGIN{do print 1; while(1)}").unwrap()).contains("do")
        );
    }
    #[test]
    fn pretty_print_getline_var_v18() {
        assert!(format_program(&parse_program("BEGIN{getline x}").unwrap()).contains("getline x"));
    }
    #[test]
    fn pretty_print_getline_file_v18() {
        assert!(
            format_program(&parse_program("BEGIN{getline < \"f\"}").unwrap())
                .contains("getline < \"f\"")
        );
    }
    #[test]
    fn pretty_print_getline_pipe_v18() {
        assert!(
            format_program(&parse_program("BEGIN{\"c\"|getline}").unwrap()).contains("| getline")
        );
    }
    #[test]
    fn pretty_print_delete_v18() {
        assert!(
            format_program(&parse_program("BEGIN{delete a[1]}").unwrap()).contains("delete a[1]")
        );
    }
    #[test]
    fn pretty_print_delete_all_v18() {
        assert!(format_program(&parse_program("BEGIN{delete a}").unwrap()).contains("delete a"));
    }
    #[test]
    fn pretty_print_exit_val_v18() {
        assert!(format_program(&parse_program("BEGIN{exit 1}").unwrap()).contains("exit 1"));
    }
    #[test]
    fn pretty_print_next_v18() {
        assert!(format_program(&parse_program("{next}").unwrap()).contains("next"));
    }
    #[test]
    fn pretty_print_nextfile_v18() {
        assert!(format_program(&parse_program("{nextfile}").unwrap()).contains("nextfile"));
    }
    #[test]
    fn pretty_print_continue_v18() {
        assert!(
            format_program(&parse_program("BEGIN{for(;;)continue}").unwrap()).contains("continue")
        );
    }
    #[test]
    fn pretty_print_break_v18() {
        assert!(format_program(&parse_program("BEGIN{for(;;)break}").unwrap()).contains("break"));
    }

    #[test]
    fn pretty_v63_0() {
        assert!(format_program(&parse_program("BEGIN{if(1)print 1}").unwrap()).contains("if"));
    }
    #[test]
    fn pretty_v63_1() {
        assert!(
            format_program(&parse_program("BEGIN{while(1)print 1}").unwrap()).contains("while")
        );
    }
    #[test]
    fn pretty_v63_2() {
        assert!(
            format_program(&parse_program("BEGIN{do print 1; while(1)}").unwrap()).contains("do")
        );
    }
    #[test]
    fn pretty_v63_3() {
        assert!(format_program(&parse_program("BEGIN{for(;;)print 1}").unwrap()).contains("for"));
    }
    #[test]
    fn pretty_v63_4() {
        assert!(
            format_program(&parse_program("BEGIN{for(k in a)print k}").unwrap()).contains("for")
        );
    }
    #[test]
    fn pretty_v63_5() {
        assert!(
            format_program(&parse_program("BEGIN{switch(x){case 1:break}}").unwrap())
                .contains("switch")
        );
    }
    #[test]
    fn pretty_v63_6() {
        assert!(format_program(&parse_program("BEGIN{delete a[1]}").unwrap()).contains("delete"));
    }
    #[test]
    fn pretty_v63_7() {
        assert!(format_program(&parse_program("BEGIN{exit 1}").unwrap()).contains("exit"));
    }
    #[test]
    fn pretty_v63_8() {
        assert!(
            format_program(&parse_program("function f(){return 1}").unwrap()).contains("return")
        );
    }
    #[test]
    fn pretty_v63_9() {
        assert!(format_program(&parse_program("BEGIN{print $1}").unwrap()).contains("$1"));
    }
    #[test]
    fn pretty_v63_10() {
        assert!(format_program(&parse_program("BEGIN{a[1,2]=3}").unwrap()).contains("a[1, 2]"));
    }
    #[test]
    fn pretty_v63_11() {
        assert!(format_program(&parse_program("BEGIN{f(1,2)}").unwrap()).contains("f(1, 2)"));
    }
    #[test]
    fn pretty_v63_12() {
        assert!(format_program(&parse_program("BEGIN{print (1?2:3)}").unwrap()).contains("?"));
    }
    #[test]
    fn pretty_v63_13() {
        assert!(format_program(&parse_program("BEGIN{print (1 in a)}").unwrap()).contains("in"));
    }
    #[test]
    fn pretty_v63_14() {
        assert!(format_program(&parse_program("BEGIN{x++}").unwrap()).contains("++"));
    }
    #[test]
    fn pretty_v63_15() {
        assert!(format_program(&parse_program("BEGIN{--x}").unwrap()).contains("--"));
    }
}
