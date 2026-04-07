//! Static checks for whether record processing can run in parallel (rayon).

use super::{Expr, GetlineRedir, Pattern, Program, Stmt};

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
        Stmt::ForC { body, .. } => body.iter().any(stmt_blocks_parallel),
        Stmt::ForIn { body, .. } => body.iter().any(stmt_blocks_parallel),
        Stmt::Block(ss) => ss.iter().any(stmt_blocks_parallel),
        Stmt::Expr(e) => expr_blocks_parallel(e),
        Stmt::Print { args, redir } => redir.is_some() || args.iter().any(expr_blocks_parallel),
        Stmt::Printf { args, redir } => redir.is_some() || args.iter().any(expr_blocks_parallel),
        Stmt::Break | Stmt::Continue | Stmt::Next | Stmt::Return(_) => false,
        Stmt::Delete { .. } => true,
    }
}

fn expr_blocks_parallel(e: &Expr) -> bool {
    match e {
        Expr::Assign { .. } | Expr::AssignField { .. } | Expr::AssignIndex { .. } => true,
        Expr::Binary { left, right, .. } => {
            expr_blocks_parallel(left) || expr_blocks_parallel(right)
        }
        Expr::Unary { expr, .. } => expr_blocks_parallel(expr),
        Expr::Call { args, .. } => args.iter().any(expr_blocks_parallel),
        Expr::Index { indices, .. } => indices.iter().any(expr_blocks_parallel),
        Expr::Field(inner) => expr_blocks_parallel(inner),
        Expr::Ternary { cond, then_, else_ } => {
            expr_blocks_parallel(cond) || expr_blocks_parallel(then_) || expr_blocks_parallel(else_)
        }
        Expr::Number(_) | Expr::Str(_) | Expr::Var(_) => false,
    }
}
