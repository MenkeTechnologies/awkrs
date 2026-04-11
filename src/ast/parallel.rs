//! Static checks for whether record processing can run in parallel (rayon).

use super::{Expr, GetlineRedir, Pattern, Program, Stmt, SwitchArm, SwitchLabel};

/// True when record rules can run in parallel: no range patterns, no `exit`, no primary `getline`,
/// no `getline <&` coprocess, no cross-record mutations (assignments / `delete`), and no constructs
/// that require sequential input across records.
pub fn record_rules_parallel_safe(prog: &Program) -> bool {
    for rule in &prog.rules {
        if matches!(rule.pattern, Pattern::Range(_, _)) {
            return false;
        }
        if matches!(
            rule.pattern,
            Pattern::Begin | Pattern::End | Pattern::BeginFile | Pattern::EndFile
        ) {
            continue;
        }
        for s in &rule.stmts {
            if stmt_blocks_parallel(s) {
                return false;
            }
        }
    }
    for f in prog.funcs.values() {
        for s in &f.body {
            if stmt_blocks_parallel(s) {
                return false;
            }
        }
    }
    true
}

fn stmt_blocks_parallel(s: &Stmt) -> bool {
    match s {
        Stmt::Exit(_) => true,
        Stmt::GetLine {
            pipe_cmd: Some(_), ..
        } => true,
        Stmt::GetLine {
            pipe_cmd: None,
            redir: GetlineRedir::Primary,
            ..
        } => true,
        Stmt::GetLine {
            redir: GetlineRedir::Coproc(_),
            ..
        } => true,
        Stmt::GetLine { .. } => false,
        Stmt::If { then_, else_, .. } => {
            then_.iter().any(stmt_blocks_parallel) || else_.iter().any(stmt_blocks_parallel)
        }
        Stmt::While { body, .. } => body.iter().any(stmt_blocks_parallel),
        Stmt::DoWhile { body, .. } => body.iter().any(stmt_blocks_parallel),
        Stmt::ForC { body, .. } => body.iter().any(stmt_blocks_parallel),
        Stmt::ForIn { body, .. } => body.iter().any(stmt_blocks_parallel),
        Stmt::Block(ss) => ss.iter().any(stmt_blocks_parallel),
        Stmt::Expr(e) => expr_blocks_parallel(e),
        Stmt::Print { args, redir } => redir.is_some() || args.iter().any(expr_blocks_parallel),
        Stmt::Printf { args, redir } => redir.is_some() || args.iter().any(expr_blocks_parallel),
        Stmt::NextFile => true,
        Stmt::Break | Stmt::Continue | Stmt::Next | Stmt::Return(_) => false,
        Stmt::Delete { .. } => true,
        Stmt::Switch { expr, arms } => {
            expr_blocks_parallel(expr)
                || arms.iter().any(|a| match a {
                    SwitchArm::Case { label, stmts } => {
                        let label_bad = match label {
                            SwitchLabel::Expr(e) => expr_blocks_parallel(e),
                            SwitchLabel::Regexp(_) => false,
                        };
                        label_bad || stmts.iter().any(stmt_blocks_parallel)
                    }
                    SwitchArm::Default { stmts } => stmts.iter().any(stmt_blocks_parallel),
                })
        }
    }
}

fn expr_blocks_parallel(e: &Expr) -> bool {
    match e {
        Expr::Assign { .. }
        | Expr::AssignField { .. }
        | Expr::AssignIndex { .. }
        | Expr::IncDec { .. } => true,
        Expr::Binary { left, right, .. } => {
            expr_blocks_parallel(left) || expr_blocks_parallel(right)
        }
        Expr::Unary { expr, .. } => expr_blocks_parallel(expr),
        Expr::Call { name, args } => {
            if matches!(name.as_str(), "asort" | "asorti") {
                return true;
            }
            args.iter().any(expr_blocks_parallel)
        }
        // Dynamic callee — cannot prove parallel-safety.
        Expr::IndirectCall { .. } => true,
        Expr::Index { indices, .. } => indices.iter().any(expr_blocks_parallel),
        Expr::Field(inner) => expr_blocks_parallel(inner),
        Expr::Ternary { cond, then_, else_ } => {
            expr_blocks_parallel(cond) || expr_blocks_parallel(then_) || expr_blocks_parallel(else_)
        }
        Expr::In { key, .. } => expr_blocks_parallel(key),
        Expr::Tuple(parts) => parts.iter().any(expr_blocks_parallel),
        Expr::GetLine {
            pipe_cmd: Some(_), ..
        } => true,
        Expr::GetLine {
            pipe_cmd: None,
            redir: GetlineRedir::Primary,
            ..
        } => true,
        Expr::GetLine {
            redir: GetlineRedir::Coproc(_),
            ..
        } => true,
        Expr::GetLine { .. } => false,
        Expr::Number(_)
        | Expr::IntegerLiteral(_)
        | Expr::Str(_)
        | Expr::RegexpLiteral(_)
        | Expr::Var(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::record_rules_parallel_safe;
    use crate::ast::{Expr, FunctionDef, GetlineRedir, Pattern, Program, Rule, Stmt};
    use crate::parser::parse_program;
    use std::collections::HashMap;

    #[test]
    fn parallel_safe_simple_print() {
        let p = parse_program("{ print $1 }").unwrap();
        assert!(record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_range_pattern() {
        let p = parse_program("/a/,/b/ { print }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_exit_in_rule() {
        let p = parse_program("{ exit 0 }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_nextfile_in_rule() {
        let p = parse_program("{ nextfile }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_primary_getline() {
        let p = parse_program("{ getline x }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_coproc_getline() {
        let p = parse_program(r#"{ getline x <& "cat" }"#).unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_safe_getline_from_file() {
        let p = parse_program(r#"{ getline x < "f.txt" }"#).unwrap();
        assert!(record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_assignment_in_rule() {
        let p = parse_program("{ x = 1 }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_delete() {
        let p = parse_program("{ delete a[1] }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn begin_only_still_checked_for_functions() {
        let p = parse_program("function f() { exit 1 } BEGIN { }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_print_redirect() {
        let p = parse_program(r#"{ print $1 > "out.txt" }"#).unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_asort_in_record_rule() {
        let p = parse_program("{ asort(a) }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_asorti_in_record_rule() {
        let p = parse_program("{ asorti(a) }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_safe_next_in_rule() {
        let p = parse_program("{ next }").unwrap();
        assert!(record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_safe_print_two_fields_implicit_concat_ok() {
        let p = parse_program("{ print $1, $2 }").unwrap();
        assert!(record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_switch_case_with_assignment() {
        let p = parse_program(r#"BEGIN { x = 1 } { switch (x) { case 1: y = 2 } }"#).unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_getline_pipe_from_command_string() {
        let p = parse_program(r#"{ "true" | getline x }"#).unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_expr_stmt_primary_getline() {
        let prog = Program {
            rules: vec![Rule {
                pattern: Pattern::Empty,
                stmts: vec![Stmt::Expr(Expr::GetLine {
                    pipe_cmd: None,
                    var: Some("x".into()),
                    redir: GetlineRedir::Primary,
                })],
            }],
            funcs: HashMap::new(),
        };
        assert!(!record_rules_parallel_safe(&prog));
    }

    #[test]
    fn parallel_unsafe_indirect_call_in_record_rule() {
        let prog = Program {
            rules: vec![Rule {
                pattern: Pattern::Empty,
                stmts: vec![Stmt::Expr(Expr::IndirectCall {
                    callee: Box::new(Expr::Var("callee".into())),
                    args: vec![Expr::Number(0.0)],
                })],
            }],
            funcs: HashMap::from([(
                "dummy".into(),
                FunctionDef {
                    name: "dummy".into(),
                    params: vec![],
                    body: vec![],
                },
            )]),
        };
        assert!(!record_rules_parallel_safe(&prog));
    }
}
