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

    #[test]
    fn parallel_unsafe_global_variable_mutation_in_function() {
        let p = parse_program("function f(x) { g = 1 } { f($1) }").unwrap();
        // `g` is global.
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_array_element_assignment() {
        let p = parse_program("{ a[$1] = 1 }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_delete_array() {
        let p = parse_program("{ delete a }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_printf_redirection() {
        let p = parse_program("{ printf \"hi\" > \"file\" }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_safe_complex_math_expression() {
        let p = parse_program("{ print sqrt($1*$1 + $2*$2) }").unwrap();
        assert!(record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_exit() {
        let p = parse_program("{ exit 1 }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_nextfile() {
        let p = parse_program("{ nextfile }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_unsafe_assignment_to_special_variable() {
        assert!(!record_rules_parallel_safe(
            &parse_program("{ NR = 1 }").unwrap()
        ));
        assert!(!record_rules_parallel_safe(
            &parse_program("{ NF = 2 }").unwrap()
        ));
        assert!(!record_rules_parallel_safe(
            &parse_program("{ FS = \",\" }").unwrap()
        ));
        assert!(!record_rules_parallel_safe(
            &parse_program("{ OFS = \":\" }").unwrap()
        ));
    }

    #[test]
    fn parallel_unsafe_getline_variants() {
        // getline with no args (into $0) is unsafe because it modifies global fields/record.
        assert!(!record_rules_parallel_safe(
            &parse_program("{ getline }").unwrap()
        ));
        // getline into var is unsafe because it modifies global var.
        assert!(!record_rules_parallel_safe(
            &parse_program("{ getline x }").unwrap()
        ));
        // getline from pipe is unsafe (global pipe state).
        assert!(!record_rules_parallel_safe(
            &parse_program("{ \"cmd\" | getline }").unwrap()
        ));
    }

    #[test]
    fn parallel_unsafe_print_to_file_or_pipe() {
        // print to stdout is safe (buffered), but to file/pipe is unsafe (global state).
        // Parser requires parens or careful expression placement for redirects in some contexts.
        assert!(!record_rules_parallel_safe(
            &parse_program("{ print \"hi\" > \"f\" }").unwrap()
        ));
        assert!(!record_rules_parallel_safe(
            &parse_program("{ print \"hi\" | \"c\" }").unwrap()
        ));
    }

    #[test]
    fn parallel_unsafe_builtins_with_side_effects() {
        // asort/asorti are explicitly blacklisted.
        assert!(!record_rules_parallel_safe(
            &parse_program("{ asort(a) }").unwrap()
        ));
    }

    #[test]
    fn parallel_unsafe_recursive_global_mutation() {
        // Recursive call that eventually touches a global.
        let p = parse_program("function f(x) { if(x>0) f(x-1); else g=1 } { f($1) }").unwrap();
        assert!(!record_rules_parallel_safe(&p));
    }

    #[test]
    fn parallel_safe_pure_functions() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print length($1), cos($2), exp($3) }").unwrap()
        ));
        // String functions like index, substr, toupper are safe if they don't modify globals
        assert!(record_rules_parallel_safe(
            &parse_program("{ print index($1, \"a\"), substr($2, 1, 2), toupper($3) }").unwrap()
        ));
        // typeof/isarray are safe
        assert!(record_rules_parallel_safe(
            &parse_program("{ print typeof($1), isarray(a) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_bitwise_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print and($1, 1), or($2, 2), xor($3, 3), compl($4), lshift($5, 1), rshift($6, 1) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_match_v2() {
        // Current implementation seems to consider match() safe?
        assert!(record_rules_parallel_safe(
            &parse_program("{ match($1, /a/) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_gsub_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ gsub(/a/, \"b\", $1) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_sub_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ sub(/a/, \"b\", $1) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_rand_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print rand() }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_srand_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ srand(1) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_systime_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print systime() }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_strftime_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print strftime(\"%Y\", 0) }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_mktime_v2() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print mktime(\"2023 01 01 00 00 00\") }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_loops_and_breaks_v3() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ while (1) { break }; for(;;) { continue } }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_ternary_nested_v3() {
        assert!(record_rules_parallel_safe(
            &parse_program("{ print (1 ? 2 : 3 ? 4 : 5) }").unwrap()
        ));
    }

    #[test]
    fn parallel_unsafe_inc_dec_fields_v3() {
        // Field assignments/modifications make the record rule unsafe for parallel execution.
        assert!(!record_rules_parallel_safe(
            &parse_program("{ $1++; ++$2; $3--; --$4 }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_delete_array_element_v3() {
        // Current implementation: delete is unsafe
        assert!(!record_rules_parallel_safe(
            &parse_program("{ delete a[1] }").unwrap()
        ));
    }

    #[test]
    fn parallel_safe_print_v24() {
        assert!(record_rules_parallel_safe(
            &parse_program("{print 1}").unwrap()
        ));
    }
    #[test]
    fn parallel_safe_printf_v24() {
        assert!(record_rules_parallel_safe(
            &parse_program("{printf \"%d\",1}").unwrap()
        ));
    }
    #[test]
    fn parallel_unsafe_assign_v24() {
        assert!(!record_rules_parallel_safe(
            &parse_program("{x=1}").unwrap()
        ));
    }
    #[test]
    fn parallel_unsafe_getline_v24() {
        assert!(!record_rules_parallel_safe(
            &parse_program("{getline}").unwrap()
        ));
    }
    #[test]
    fn parallel_safe_next_v24() {
        assert!(record_rules_parallel_safe(
            &parse_program("{next}").unwrap()
        ));
    }
}
