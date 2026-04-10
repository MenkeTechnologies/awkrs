//! gawk-style `@namespace "name"` — default namespace prefixes unqualified identifiers in the AST.

use crate::ast::*;
use rustc_hash::FxHashSet;
use std::collections::HashMap;

/// Built-in function names (must not be prefixed with `ns::`).
const BUILTIN_NAMES: &[&str] = &[
    "and",
    "asort",
    "asorti",
    "atan2",
    "bindtextdomain",
    "close",
    "compl",
    "cos",
    "dcgettext",
    "dcngettext",
    "exp",
    "fflush",
    "gensub",
    "gsub",
    "index",
    "int",
    "isarray",
    "length",
    "log",
    "lshift",
    "match",
    "mktime",
    "or",
    "patsplit",
    "printf",
    "rand",
    "rshift",
    "sin",
    "split",
    "sprintf",
    "sqrt",
    "srand",
    "strftime",
    "strtonum",
    "sub",
    "substr",
    "system",
    "systime",
    "tolower",
    "toupper",
    "typeof",
    "xor",
];

/// Same set as [`crate::compiler::SPECIAL_VARS`] — globals that live in the global namespace.
pub const SPECIAL_GLOBAL_NAMES: &[&str] = &[
    "NR",
    "FNR",
    "NF",
    "FILENAME",
    "FS",
    "OFS",
    "ORS",
    "RS",
    "RT",
    "SUBSEP",
    "OFMT",
    "CONVFMT",
    "FPAT",
    "RSTART",
    "RLENGTH",
    "ENVIRON",
    "ARGC",
    "ARGV",
    "ARGIND",
    "ERRNO",
    "PROCINFO",
    "SYMTAB",
    "FUNCTAB",
    "FIELDWIDTHS",
    "IGNORECASE",
    "BINMODE",
    "LINT",
    "TEXTDOMAIN",
];

fn qualify_name(name: &str, ns: &str, locals: &FxHashSet<String>) -> String {
    if name.contains("::") {
        return name.to_string();
    }
    if locals.contains(name) {
        return name.to_string();
    }
    if SPECIAL_GLOBAL_NAMES.contains(&name) {
        return name.to_string();
    }
    if BUILTIN_NAMES.contains(&name) {
        return name.to_string();
    }
    format!("{ns}::{name}")
}

fn qualify_expr(e: &mut Expr, ns: &str, locals: &FxHashSet<String>) {
    match e {
        Expr::Var(name) => *name = qualify_name(name, ns, locals),
        Expr::Index { name, indices } => {
            *name = qualify_name(name, ns, locals);
            for x in indices {
                qualify_expr(x, ns, locals);
            }
        }
        Expr::Assign { name, rhs, .. } => {
            *name = qualify_name(name, ns, locals);
            qualify_expr(rhs, ns, locals);
        }
        Expr::AssignIndex {
            name, indices, rhs, ..
        } => {
            *name = qualify_name(name, ns, locals);
            for x in indices {
                qualify_expr(x, ns, locals);
            }
            qualify_expr(rhs, ns, locals);
        }
        Expr::Call { name, args } => {
            if !BUILTIN_NAMES.contains(&name.as_str()) {
                *name = qualify_name(name, ns, locals);
            }
            for a in args {
                qualify_expr(a, ns, locals);
            }
        }
        Expr::IndirectCall { callee, args } => {
            qualify_expr(callee, ns, locals);
            for a in args {
                qualify_expr(a, ns, locals);
            }
        }
        Expr::In { key, arr } => {
            qualify_expr(key, ns, locals);
            *arr = qualify_name(arr, ns, locals);
        }
        Expr::Field(inner) => qualify_expr(inner, ns, locals),
        Expr::Binary { left, right, .. } => {
            qualify_expr(left, ns, locals);
            qualify_expr(right, ns, locals);
        }
        Expr::Unary { expr, .. } => qualify_expr(expr, ns, locals),
        Expr::Ternary { cond, then_, else_ } => {
            qualify_expr(cond, ns, locals);
            qualify_expr(then_, ns, locals);
            qualify_expr(else_, ns, locals);
        }
        Expr::IncDec { target, .. } => match target {
            IncDecTarget::Var(name) => *name = qualify_name(name, ns, locals),
            IncDecTarget::Field(inner) => qualify_expr(inner, ns, locals),
            IncDecTarget::Index { name, indices } => {
                *name = qualify_name(name, ns, locals);
                for x in indices {
                    qualify_expr(x, ns, locals);
                }
            }
        },
        Expr::AssignField { field, rhs, .. } => {
            qualify_expr(field.as_mut(), ns, locals);
            qualify_expr(rhs.as_mut(), ns, locals);
        }
        Expr::Number(_) | Expr::Str(_) => {}
    }
}

fn qualify_pattern(p: &mut Pattern, ns: &str, locals: &FxHashSet<String>) {
    match p {
        Pattern::Begin
        | Pattern::End
        | Pattern::BeginFile
        | Pattern::EndFile
        | Pattern::Empty
        | Pattern::Regexp(_) => {}
        Pattern::Expr(e) => qualify_expr(e, ns, locals),
        Pattern::Range(a, b) => {
            qualify_pattern(a, ns, locals);
            qualify_pattern(b, ns, locals);
        }
    }
}

fn qualify_stmt(s: &mut Stmt, ns: &str, locals: &FxHashSet<String>) {
    match s {
        Stmt::If { cond, then_, else_ } => {
            qualify_expr(cond, ns, locals);
            for x in then_ {
                qualify_stmt(x, ns, locals);
            }
            for x in else_ {
                qualify_stmt(x, ns, locals);
            }
        }
        Stmt::While { cond, body } => {
            qualify_expr(cond, ns, locals);
            for x in body {
                qualify_stmt(x, ns, locals);
            }
        }
        Stmt::DoWhile { body, cond } => {
            for x in body {
                qualify_stmt(x, ns, locals);
            }
            qualify_expr(cond, ns, locals);
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                qualify_expr(e, ns, locals);
            }
            if let Some(e) = cond {
                qualify_expr(e, ns, locals);
            }
            if let Some(e) = iter {
                qualify_expr(e, ns, locals);
            }
            for x in body {
                qualify_stmt(x, ns, locals);
            }
        }
        Stmt::ForIn { var, arr, body } => {
            *var = qualify_name(var, ns, locals);
            *arr = qualify_name(arr, ns, locals);
            for x in body {
                qualify_stmt(x, ns, locals);
            }
        }
        Stmt::Block(v) => {
            for x in v {
                qualify_stmt(x, ns, locals);
            }
        }
        Stmt::Expr(e) => qualify_expr(e, ns, locals),
        Stmt::Print { args, redir } => {
            for a in args {
                qualify_expr(a, ns, locals);
            }
            match redir {
                Some(PrintRedir::Overwrite(e))
                | Some(PrintRedir::Append(e))
                | Some(PrintRedir::Pipe(e))
                | Some(PrintRedir::Coproc(e)) => qualify_expr(e, ns, locals),
                None => {}
            }
        }
        Stmt::Printf { args, redir } => {
            for a in args {
                qualify_expr(a, ns, locals);
            }
            match redir {
                Some(PrintRedir::Overwrite(e))
                | Some(PrintRedir::Append(e))
                | Some(PrintRedir::Pipe(e))
                | Some(PrintRedir::Coproc(e)) => qualify_expr(e, ns, locals),
                None => {}
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::Next | Stmt::NextFile => {}
        Stmt::Exit(e) => {
            if let Some(x) = e {
                qualify_expr(x, ns, locals);
            }
        }
        Stmt::Delete { name, indices } => {
            *name = qualify_name(name, ns, locals);
            if let Some(idxs) = indices {
                for x in idxs {
                    qualify_expr(x, ns, locals);
                }
            }
        }
        Stmt::Return(e) => {
            if let Some(x) = e {
                qualify_expr(x, ns, locals);
            }
        }
        Stmt::GetLine { var, redir } => {
            if let Some(v) = var {
                *v = qualify_name(v, ns, locals);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => qualify_expr(e, ns, locals),
            }
        }
        Stmt::Switch { expr, arms } => {
            qualify_expr(expr, ns, locals);
            for arm in arms {
                match arm {
                    SwitchArm::Case { label, stmts } => {
                        match label {
                            SwitchLabel::Expr(e) => qualify_expr(e, ns, locals),
                            SwitchLabel::Regexp(_) => {}
                        }
                        for st in stmts {
                            qualify_stmt(st, ns, locals);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for st in stmts {
                            qualify_stmt(st, ns, locals);
                        }
                    }
                }
            }
        }
    }
}

/// Apply gawk default namespace to all rules and functions (after `@namespace "…"` preprocessing).
pub fn apply_default_namespace(prog: &mut Program, ns: Option<&str>) {
    let Some(ns) = ns else { return };
    if ns.is_empty() {
        return;
    }
    let locals_empty = FxHashSet::default();
    for rule in &mut prog.rules {
        qualify_pattern(&mut rule.pattern, ns, &locals_empty);
        for s in &mut rule.stmts {
            qualify_stmt(s, ns, &locals_empty);
        }
    }
    let mut new_funcs: HashMap<String, FunctionDef> = HashMap::default();
    for (name, mut fd) in std::mem::take(&mut prog.funcs) {
        let qname = qualify_name(&name, ns, &locals_empty);
        let mut locals: FxHashSet<String> = FxHashSet::default();
        for p in &fd.params {
            locals.insert(p.clone());
        }
        fd.name = qname.clone();
        for s in &mut fd.body {
            qualify_stmt(s, ns, &locals);
        }
        new_funcs.insert(qname, fd);
    }
    prog.funcs = new_funcs;
}
