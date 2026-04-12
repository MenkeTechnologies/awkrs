//! gawk-style `@namespace "name"` — default namespace prefixes unqualified identifiers in the AST.

use crate::ast::*;
use rustc_hash::FxHashSet;
use std::collections::HashMap;

/// Built-in function names (must not be prefixed with `ns::`).
pub const BUILTIN_NAMES: &[&str] = &[
    "and",
    "asort",
    "asorti",
    "atan2",
    "bindtextdomain",
    "chdir",
    "chr",
    "close",
    "compl",
    "cos",
    "dcgettext",
    "dcngettext",
    "exp",
    "fflush",
    "fts",
    "gensub",
    "getlocaltime",
    "gettimeofday",
    "gsub",
    "index",
    "int",
    "intdiv",
    "intdiv0",
    "inplace_commit",
    "inplace_tmpfile",
    "isarray",
    "length",
    "log",
    "lshift",
    "match",
    "mkbool",
    "mktime",
    "or",
    "ord",
    "patsplit",
    "printf",
    "rand",
    "reada",
    "readdir",
    "readfile",
    "rename",
    "revoutput",
    "revtwoway",
    "rshift",
    "sin",
    "sleep",
    "split",
    "sprintf",
    "sqrt",
    "srand",
    "stat",
    "statvfs",
    "strftime",
    "strtonum",
    "sub",
    "substr",
    "system",
    "systime",
    "tolower",
    "toupper",
    "typeof",
    "writea",
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
        Expr::Tuple(parts) => {
            for p in parts {
                qualify_expr(p, ns, locals);
            }
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
        Expr::GetLine {
            pipe_cmd,
            var,
            redir,
        } => {
            if let Some(v) = var {
                *v = qualify_name(v, ns, locals);
            }
            if let Some(cmd) = pipe_cmd {
                qualify_expr(cmd.as_mut(), ns, locals);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => qualify_expr(e, ns, locals),
            }
        }
        Expr::Number(_) | Expr::IntegerLiteral(_) | Expr::Str(_) | Expr::RegexpLiteral(_) => {}
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
        Stmt::GetLine {
            pipe_cmd,
            var,
            redir,
        } => {
            if let Some(v) = var {
                *v = qualify_name(v, ns, locals);
            }
            if let Some(cmd) = pipe_cmd {
                qualify_expr(cmd.as_mut(), ns, locals);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        BinOp, Expr, FunctionDef, GetlineRedir, Pattern, PrintRedir, Program, Rule, Stmt,
    };
    use std::collections::HashMap;

    fn prog_one_rule(rule: Rule) -> Program {
        Program {
            rules: vec![rule],
            funcs: HashMap::new(),
        }
    }

    #[test]
    fn apply_default_namespace_none_is_noop() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Var("x".into()))],
        });
        let before = prog.clone();
        apply_default_namespace(&mut prog, None);
        assert_eq!(prog, before);
    }

    #[test]
    fn apply_default_namespace_empty_string_is_noop() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Var("x".into()))],
        });
        let before = prog.clone();
        apply_default_namespace(&mut prog, Some(""));
        assert_eq!(prog, before);
    }

    #[test]
    fn unqualified_var_gets_prefix() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Var("x".into()))],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Var(n)) => assert_eq!(n, "ns::x"),
            _ => panic!("expected Var"),
        }
    }

    #[test]
    fn special_global_fs_not_prefixed() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Var("FS".into()))],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Var(n)) => assert_eq!(n, "FS"),
            _ => panic!("expected Var"),
        }
    }

    #[test]
    fn special_global_ignorecase_not_prefixed() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Var("IGNORECASE".into()))],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Var(n)) => assert_eq!(n, "IGNORECASE"),
            _ => panic!("expected Var"),
        }
    }

    #[test]
    fn already_qualified_name_unchanged() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Var("other::x".into()))],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Var(n)) => assert_eq!(n, "other::x"),
            _ => panic!("expected Var"),
        }
    }

    #[test]
    fn builtin_call_name_not_prefixed_but_args_are() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Call {
                name: "split".into(),
                args: vec![Expr::Var("line".into()), Expr::Var("arr".into())],
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Call { name, args }) => {
                assert_eq!(name, "split");
                assert!(matches!(&args[0], Expr::Var(s) if s == "ns::line"));
                assert!(matches!(&args[1], Expr::Var(s) if s == "ns::arr"));
            }
            _ => panic!("expected Call"),
        }
    }

    #[test]
    fn user_function_call_gets_prefix() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Call {
                name: "helper".into(),
                args: vec![],
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Call { name, .. }) => assert_eq!(name, "ns::helper"),
            _ => panic!("expected Call"),
        }
    }

    #[test]
    fn for_in_var_and_array_prefixed() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::ForIn {
                var: "k".into(),
                arr: "data".into(),
                body: vec![],
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::ForIn { var, arr, .. } => {
                assert_eq!(var, "ns::k");
                assert_eq!(arr, "ns::data");
            }
            _ => panic!("expected ForIn"),
        }
    }

    #[test]
    fn function_params_not_prefixed_other_locals_are() {
        let mut prog = Program {
            rules: vec![],
            funcs: HashMap::from([(
                "f".into(),
                FunctionDef {
                    name: "f".into(),
                    params: vec!["a".into()],
                    body: vec![
                        Stmt::Expr(Expr::Var("a".into())),
                        Stmt::Expr(Expr::Var("b".into())),
                    ],
                },
            )]),
        };
        apply_default_namespace(&mut prog, Some("ns"));
        let fd = prog.funcs.get("ns::f").expect("qualified key");
        assert_eq!(fd.name, "ns::f");
        assert_eq!(fd.params, vec!["a"]);
        match (&fd.body[0], &fd.body[1]) {
            (Stmt::Expr(Expr::Var(x)), Stmt::Expr(Expr::Var(y))) => {
                assert_eq!(x, "a");
                assert_eq!(y, "ns::b");
            }
            _ => panic!("expected two Var stmts"),
        }
    }

    #[test]
    fn pattern_expr_gets_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Expr(Expr::Var("ok".into())),
            stmts: vec![],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].pattern {
            Pattern::Expr(Expr::Var(n)) => assert_eq!(n, "ns::ok"),
            _ => panic!("expected Expr pattern"),
        }
    }

    #[test]
    fn in_expr_array_name_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::In {
                key: Box::new(Expr::Var("k".into())),
                arr: "tbl".into(),
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::In { key, arr }) => {
                assert!(matches!(**key, Expr::Var(ref s) if s == "ns::k"));
                assert_eq!(arr, "ns::tbl");
            }
            _ => panic!("expected In"),
        }
    }

    #[test]
    fn assign_op_rhs_walked() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Assign {
                name: "x".into(),
                op: Some(BinOp::Add),
                rhs: Box::new(Expr::Var("y".into())),
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Assign { name, rhs, .. }) => {
                assert_eq!(name, "ns::x");
                assert!(matches!(**rhs, Expr::Var(ref s) if s == "ns::y"));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn delete_array_name_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Delete {
                name: "a".into(),
                indices: None,
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Delete { name, indices } => {
                assert_eq!(name, "ns::a");
                assert!(indices.is_none());
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn delete_subscript_indices_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Delete {
                name: "a".into(),
                indices: Some(vec![Expr::Var("i".into())]),
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Delete {
                name,
                indices: Some(idxs),
            } => {
                assert_eq!(name, "ns::a");
                assert!(matches!(&idxs[0], Expr::Var(s) if s == "ns::i"));
            }
            _ => panic!("expected Delete with indices"),
        }
    }

    #[test]
    fn print_redir_expr_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Print {
                args: vec![Expr::Str("hi".into())],
                redir: Some(PrintRedir::Overwrite(Box::new(Expr::Var("path".into())))),
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Print {
                redir: Some(PrintRedir::Overwrite(e)),
                ..
            } => assert!(matches!(**e, Expr::Var(ref s) if s == "ns::path")),
            _ => panic!("expected Print with Overwrite"),
        }
    }

    #[test]
    fn printf_redir_append_expr_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Printf {
                args: vec![Expr::Str("%s".into()), Expr::Var("msg".into())],
                redir: Some(PrintRedir::Append(Box::new(Expr::Var("logpath".into())))),
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Printf {
                args,
                redir: Some(PrintRedir::Append(e)),
            } => {
                assert!(matches!(&args[0], Expr::Str(s) if s == "%s"));
                assert!(matches!(&args[1], Expr::Var(s) if s == "ns::msg"));
                assert!(matches!(**e, Expr::Var(ref s) if s == "ns::logpath"));
            }
            _ => panic!("expected Printf with Append"),
        }
    }

    #[test]
    fn stmt_getline_var_and_file_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::GetLine {
                pipe_cmd: None,
                var: Some("line".into()),
                redir: GetlineRedir::File(Box::new(Expr::Var("path".into()))),
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::GetLine {
                var: Some(v),
                redir: GetlineRedir::File(e),
                ..
            } => {
                assert_eq!(v, "ns::line");
                assert!(matches!(**e, Expr::Var(ref s) if s == "ns::path"));
            }
            _ => panic!("expected GetLine"),
        }
    }

    #[test]
    fn print_pipe_and_printf_coproc_targets_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![
                Stmt::Print {
                    args: vec![Expr::Str("x".into())],
                    redir: Some(PrintRedir::Pipe(Box::new(Expr::Var("shcmd".into())))),
                },
                Stmt::Printf {
                    args: vec![Expr::Str("%d".into()), Expr::Number(1.0)],
                    redir: Some(PrintRedir::Coproc(Box::new(Expr::Var("twoway".into())))),
                },
            ],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Print {
                redir: Some(PrintRedir::Pipe(e)),
                ..
            } => assert!(matches!(**e, Expr::Var(ref s) if s == "ns::shcmd")),
            _ => panic!("expected Print pipe"),
        }
        match &prog.rules[0].stmts[1] {
            Stmt::Printf {
                redir: Some(PrintRedir::Coproc(e)),
                ..
            } => assert!(matches!(**e, Expr::Var(ref s) if s == "ns::twoway")),
            _ => panic!("expected Printf coproc"),
        }
    }

    #[test]
    fn getline_pipe_cmd_and_coproc_redir_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::GetLine {
                pipe_cmd: Some(Box::new(Expr::Var("producer".into()))),
                var: Some("rec".into()),
                redir: GetlineRedir::Coproc(Box::new(Expr::Var("coprocfd".into()))),
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::GetLine {
                pipe_cmd: Some(cmd),
                var: Some(v),
                redir: GetlineRedir::Coproc(e),
            } => {
                assert_eq!(v, "ns::rec");
                assert!(matches!(**cmd, Expr::Var(ref s) if s == "ns::producer"));
                assert!(matches!(**e, Expr::Var(ref s) if s == "ns::coprocfd"));
            }
            _ => panic!("expected GetLine coproc"),
        }
    }

    #[test]
    fn expr_getline_form_qualifies_var_pipe_and_file() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::GetLine {
                pipe_cmd: Some(Box::new(Expr::Var("cmdstr".into()))),
                var: Some("data".into()),
                redir: GetlineRedir::File(Box::new(Expr::Var("infile".into()))),
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::GetLine {
                pipe_cmd: Some(cmd),
                var: Some(v),
                redir: GetlineRedir::File(e),
            }) => {
                assert_eq!(v, "ns::data");
                assert!(matches!(**cmd, Expr::Var(ref s) if s == "ns::cmdstr"));
                assert!(matches!(**e, Expr::Var(ref s) if s == "ns::infile"));
            }
            _ => panic!("expected expr GetLine"),
        }
    }

    #[test]
    fn indirect_call_callee_and_args_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::IndirectCall {
                callee: Box::new(Expr::Var("fnref".into())),
                args: vec![Expr::Var("arg".into())],
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::IndirectCall { callee, args }) => {
                assert!(matches!(**callee, Expr::Var(ref s) if s == "ns::fnref"));
                assert!(matches!(&args[0], Expr::Var(s) if s == "ns::arg"));
            }
            _ => panic!("expected IndirectCall"),
        }
    }

    #[test]
    fn range_pattern_endpoints_recursively_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Range(
                Box::new(Pattern::Expr(Expr::Var("lo".into()))),
                Box::new(Pattern::Expr(Expr::Var("hi".into()))),
            ),
            stmts: vec![],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].pattern {
            Pattern::Range(a, b) => {
                assert!(matches!(**a, Pattern::Expr(Expr::Var(ref s)) if s == "ns::lo"));
                assert!(matches!(**b, Pattern::Expr(Expr::Var(ref s)) if s == "ns::hi"));
            }
            _ => panic!("expected range"),
        }
    }

    #[test]
    fn if_and_while_cond_and_body_vars_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![
                Stmt::If {
                    cond: Expr::Var("ready".into()),
                    then_: vec![Stmt::Expr(Expr::Var("a".into()))],
                    else_: vec![Stmt::Expr(Expr::Var("b".into()))],
                },
                Stmt::While {
                    cond: Expr::Var("go".into()),
                    body: vec![Stmt::Expr(Expr::Var("c".into()))],
                },
            ],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::If { cond, then_, else_ } => {
                assert!(matches!(cond, Expr::Var(s) if s == "ns::ready"));
                assert!(matches!(&then_[0], Stmt::Expr(Expr::Var(s)) if s == "ns::a"));
                assert!(matches!(&else_[0], Stmt::Expr(Expr::Var(s)) if s == "ns::b"));
            }
            _ => panic!("expected If"),
        }
        match &prog.rules[0].stmts[1] {
            Stmt::While { cond, body } => {
                assert!(matches!(cond, Expr::Var(s) if s == "ns::go"));
                assert!(matches!(&body[0], Stmt::Expr(Expr::Var(s)) if s == "ns::c"));
            }
            _ => panic!("expected While"),
        }
    }

    #[test]
    fn do_while_for_c_exit_return_expressions_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![
                Stmt::DoWhile {
                    body: vec![Stmt::Expr(Expr::Var("step".into()))],
                    cond: Expr::Var("more".into()),
                },
                Stmt::ForC {
                    init: Some(Expr::Assign {
                        name: "i".into(),
                        op: None,
                        rhs: Box::new(Expr::Number(0.0)),
                    }),
                    cond: Some(Expr::Var("cond".into())),
                    iter: Some(Expr::Assign {
                        name: "i".into(),
                        op: Some(BinOp::Add),
                        rhs: Box::new(Expr::Number(1.0)),
                    }),
                    body: vec![Stmt::Expr(Expr::Var("bodyv".into()))],
                },
                Stmt::Exit(Some(Expr::Var("ecode".into()))),
                Stmt::Return(Some(Expr::Var("retval".into()))),
            ],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        match &prog.rules[0].stmts[0] {
            Stmt::DoWhile { body, cond } => {
                assert!(matches!(cond, Expr::Var(s) if s == "ns::more"));
                assert!(matches!(&body[0], Stmt::Expr(Expr::Var(s)) if s == "ns::step"));
            }
            _ => panic!("expected DoWhile"),
        }
        match &prog.rules[0].stmts[1] {
            Stmt::ForC {
                init,
                cond,
                iter,
                body,
            } => {
                let Expr::Assign { name, rhs, .. } = init.as_ref().unwrap() else {
                    panic!("expected init assign");
                };
                assert_eq!(name, "ns::i");
                assert!(matches!(**rhs, Expr::Number(n) if n == 0.0));
                assert!(matches!(cond.as_ref().unwrap(), Expr::Var(s) if s == "ns::cond"));
                let Expr::Assign {
                    name: iname,
                    rhs: irhs,
                    ..
                } = iter.as_ref().unwrap()
                else {
                    panic!("expected iter assign");
                };
                assert_eq!(iname, "ns::i");
                assert!(matches!(**irhs, Expr::Number(n) if n == 1.0));
                assert!(matches!(&body[0], Stmt::Expr(Expr::Var(s)) if s == "ns::bodyv"));
            }
            _ => panic!("expected ForC"),
        }
        assert!(matches!(
            &prog.rules[0].stmts[2],
            Stmt::Exit(Some(Expr::Var(s))) if s == "ns::ecode"
        ));
        assert!(matches!(
            &prog.rules[0].stmts[3],
            Stmt::Return(Some(Expr::Var(s))) if s == "ns::retval"
        ));
    }

    #[test]
    fn switch_expr_and_case_labels_qualified() {
        use crate::ast::{SwitchArm, SwitchLabel};
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Switch {
                expr: Expr::Var("v".into()),
                arms: vec![
                    SwitchArm::Case {
                        label: SwitchLabel::Expr(Expr::Var("k".into())),
                        stmts: vec![Stmt::Expr(Expr::Var("hit".into()))],
                    },
                    SwitchArm::Default {
                        stmts: vec![Stmt::Expr(Expr::Var("miss".into()))],
                    },
                ],
            }],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        let Stmt::Switch { expr, arms } = &prog.rules[0].stmts[0] else {
            panic!("expected switch");
        };
        assert!(matches!(expr, Expr::Var(s) if s == "ns::v"));
        match &arms[0] {
            SwitchArm::Case { label, stmts } => {
                assert!(matches!(label, SwitchLabel::Expr(Expr::Var(s)) if s == "ns::k"));
                assert!(matches!(&stmts[0], Stmt::Expr(Expr::Var(s)) if s == "ns::hit"));
            }
            _ => panic!("expected case"),
        }
        match &arms[1] {
            SwitchArm::Default { stmts } => {
                assert!(matches!(&stmts[0], Stmt::Expr(Expr::Var(s)) if s == "ns::miss"));
            }
            _ => panic!("expected default"),
        }
    }

    #[test]
    fn block_statement_bodies_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Block(vec![
                Stmt::Expr(Expr::Var("inner".into())),
                Stmt::Expr(Expr::Var("FS".into())),
            ])],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        let Stmt::Block(ss) = &prog.rules[0].stmts[0] else {
            panic!("expected block");
        };
        assert!(matches!(&ss[0], Stmt::Expr(Expr::Var(s)) if s == "ns::inner"));
        assert!(matches!(&ss[1], Stmt::Expr(Expr::Var(s)) if s == "FS"));
    }

    #[test]
    fn function_body_keeps_parameter_names_unprefixed() {
        let mut prog = Program {
            rules: vec![],
            funcs: HashMap::from([(
                "g".into(),
                FunctionDef {
                    name: "g".into(),
                    params: vec!["p".into()],
                    body: vec![Stmt::Expr(Expr::Assign {
                        name: "q".into(),
                        op: None,
                        rhs: Box::new(Expr::Var("p".into())),
                    })],
                },
            )]),
        };
        apply_default_namespace(&mut prog, Some("ns"));
        let fd = prog.funcs.get("ns::g").expect("qualified function");
        assert_eq!(fd.params, vec!["p".to_string()]);
        let Stmt::Expr(Expr::Assign { name, rhs, .. }) = &fd.body[0] else {
            panic!("expected assign in body");
        };
        assert_eq!(name, "ns::q");
        assert!(matches!(**rhs, Expr::Var(ref s) if s == "p"));
    }

    #[test]
    fn ternary_expression_subexprs_qualified() {
        let mut prog = prog_one_rule(Rule {
            pattern: Pattern::Begin,
            stmts: vec![Stmt::Expr(Expr::Ternary {
                cond: Box::new(Expr::Var("flag".into())),
                then_: Box::new(Expr::Var("a".into())),
                else_: Box::new(Expr::Var("b".into())),
            })],
        });
        apply_default_namespace(&mut prog, Some("ns"));
        let Stmt::Expr(Expr::Ternary { cond, then_, else_ }) = &prog.rules[0].stmts[0] else {
            panic!("expected ternary");
        };
        assert!(matches!(**cond, Expr::Var(ref s) if s == "ns::flag"));
        assert!(matches!(**then_, Expr::Var(ref s) if s == "ns::a"));
        assert!(matches!(**else_, Expr::Var(ref s) if s == "ns::b"));
    }
}
