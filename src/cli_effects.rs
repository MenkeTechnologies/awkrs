//! Behaviors for gawk-style CLI flags (dump, pretty-print, gen-pot, lint, debug listing, profile timing).

use crate::ast::{Expr, Program, Stmt, SwitchArm};
use crate::ast_fmt;
use crate::bytecode::CompiledProgram;
use crate::error::{Error, Result};
use crate::format::awk_sprintf;
use crate::runtime::{Runtime, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// Write a GNU gettext–style POT skeleton with string literals collected from the AST.
pub fn gen_pot(prog: &Program) -> String {
    let mut msgids = BTreeMap::new();
    for r in &prog.rules {
        for s in &r.stmts {
            collect_stmt_strings(s, &mut msgids);
        }
    }
    for fd in prog.funcs.values() {
        for s in &fd.body {
            collect_stmt_strings(s, &mut msgids);
        }
    }
    let mut out = String::from(
        "# SOME DESCRIPTIVE TITLE.\n\
         # Copyright (C) YEAR THE PACKAGE'S COPYRIGHT HOLDER\n\
         # This file is distributed under the same license as the PACKAGE package.\n\
         # FIRST AUTHOR <EMAIL@ADDRESS>, YEAR.\n\
         #\n\
         #, fuzzy\n\
         msgid \"\"\n\
         msgstr \"\"\n\
         \"Project-Id-Version: awkrs\\n\"\n\
         \"Report-Msgid-Bugs-To: \\n\"\n\
         \"POT-Creation-Date: \\n\"\n\
         \"PO-Revision-Date: YEAR-MO-DA HO:MI+ZONE\\n\"\n\
         \"Last-Translator: FULL NAME <EMAIL@ADDRESS>\\n\"\n\
         \"Language-Team: LANGUAGE <LL@li.org>\\n\"\n\
         \"Language: \\n\"\n\
         \"MIME-Version: 1.0\\n\"\n\
         \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
         \"Content-Transfer-Encoding: 8bit\\n\"\n\n",
    );
    for (s, _count) in msgids {
        out.push_str("msgid \"");
        out.push_str(&escape_pot_str(&s));
        out.push_str("\"\nmsgstr \"\"\n\n");
    }
    out
}

fn escape_pot_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
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
    o
}

fn collect_stmt_strings(s: &Stmt, out: &mut BTreeMap<String, usize>) {
    match s {
        Stmt::If { cond, then_, else_ } => {
            collect_expr_strings(cond, out);
            for t in then_ {
                collect_stmt_strings(t, out);
            }
            for t in else_ {
                collect_stmt_strings(t, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_expr_strings(cond, out);
            for t in body {
                collect_stmt_strings(t, out);
            }
        }
        Stmt::DoWhile { cond, body } => {
            collect_expr_strings(cond, out);
            for t in body {
                collect_stmt_strings(t, out);
            }
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                collect_expr_strings(e, out);
            }
            if let Some(e) = cond {
                collect_expr_strings(e, out);
            }
            if let Some(e) = iter {
                collect_expr_strings(e, out);
            }
            for t in body {
                collect_stmt_strings(t, out);
            }
        }
        Stmt::ForIn { body, .. } => {
            for t in body {
                collect_stmt_strings(t, out);
            }
        }
        Stmt::Block(ss) => {
            for t in ss {
                collect_stmt_strings(t, out);
            }
        }
        Stmt::Expr(e) => collect_expr_strings(e, out),
        Stmt::Print { args, redir } => {
            for e in args {
                collect_expr_strings(e, out);
            }
            if let Some(r) = redir {
                use crate::ast::PrintRedir;
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                collect_expr_strings(e, out);
            }
        }
        Stmt::Printf { args, redir } => {
            for e in args {
                collect_expr_strings(e, out);
            }
            if let Some(r) = redir {
                use crate::ast::PrintRedir;
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                collect_expr_strings(e, out);
            }
        }
        Stmt::GetLine { redir, .. } => {
            use crate::ast::GetlineRedir;
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => collect_expr_strings(e, out),
            }
        }
        Stmt::Delete { indices, .. } => {
            if let Some(ix) = indices {
                for e in ix {
                    collect_expr_strings(e, out);
                }
            }
        }
        Stmt::Switch { expr, arms } => {
            collect_expr_strings(expr, out);
            for a in arms {
                use crate::ast::SwitchArm;
                match a {
                    SwitchArm::Case { label, stmts } => {
                        if let crate::ast::SwitchLabel::Expr(e) = label {
                            collect_expr_strings(e, out);
                        }
                        for st in stmts {
                            collect_stmt_strings(st, out);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for st in stmts {
                            collect_stmt_strings(st, out);
                        }
                    }
                }
            }
        }
        Stmt::Exit(e) | Stmt::Return(e) => {
            if let Some(ex) = e {
                collect_expr_strings(ex, out);
            }
        }
        Stmt::Next | Stmt::NextFile | Stmt::Break | Stmt::Continue => {}
    }
}

fn collect_expr_strings(e: &Expr, out: &mut BTreeMap<String, usize>) {
    match e {
        Expr::Str(s) if !s.is_empty() => {
            *out.entry(s.clone()).or_insert(0) += 1;
        }
        Expr::Str(_) => {}
        Expr::Number(_) | Expr::Var(_) => {}
        Expr::Field(inner) => collect_expr_strings(inner, out),
        Expr::Index { indices, .. } => {
            for x in indices {
                collect_expr_strings(x, out);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_strings(left, out);
            collect_expr_strings(right, out);
        }
        Expr::Unary { expr, .. } => collect_expr_strings(expr, out),
        Expr::Assign { rhs, .. } => {
            collect_expr_strings(rhs, out);
        }
        Expr::AssignField { rhs, field, .. } => {
            collect_expr_strings(field, out);
            collect_expr_strings(rhs, out);
        }
        Expr::AssignIndex { rhs, indices, .. } => {
            for x in indices {
                collect_expr_strings(x, out);
            }
            collect_expr_strings(rhs, out);
        }
        Expr::Call { args, .. } => {
            for a in args {
                collect_expr_strings(a, out);
            }
        }
        Expr::IndirectCall { args, callee } => {
            collect_expr_strings(callee, out);
            for a in args {
                collect_expr_strings(a, out);
            }
        }
        Expr::Ternary { cond, then_, else_ } => {
            collect_expr_strings(cond, out);
            collect_expr_strings(then_, out);
            collect_expr_strings(else_, out);
        }
        Expr::In { key, .. } => collect_expr_strings(key, out),
        Expr::IncDec { target, .. } => match target {
            crate::ast::IncDecTarget::Field(inner) => collect_expr_strings(inner, out),
            crate::ast::IncDecTarget::Index { indices, .. } => {
                for x in indices {
                    collect_expr_strings(x, out);
                }
            }
            crate::ast::IncDecTarget::Var(_) => {}
        },
    }
}

/// Dump globals (and slots) in a readable form. Arrays use `name[k] = value` lines.
pub fn dump_variables(rt: &Runtime, cp: &CompiledProgram, out: &mut dyn Write) -> Result<()> {
    let mut names: Vec<String> = rt.vars.keys().cloned().collect();
    names.sort();
    for name in names {
        if let Some(v) = rt.vars.get(&name) {
            dump_value(out, &name, v, "")?;
        }
    }
    for (slot, name) in cp.slot_names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        if rt.vars.contains_key(name) {
            continue;
        }
        let v = rt.slots.get(slot).cloned().unwrap_or(Value::Uninit);
        dump_value(out, name, &v, "slot:")?;
    }
    Ok(())
}

fn dump_value(out: &mut dyn Write, name: &str, v: &Value, tag: &str) -> Result<()> {
    match v {
        Value::Array(a) => {
            let mut keys: Vec<_> = a.keys().cloned().collect();
            keys.sort();
            for k in keys {
                if let Some(elem) = a.get(&k) {
                    let fq = format!("{name}[{k}]");
                    dump_value(out, &fq, elem, tag)?;
                }
            }
        }
        _ => {
            if !tag.is_empty() {
                writeln!(out, "{tag} {name} = {}", value_dump_scalar(v)).map_err(Error::Io)?;
            } else {
                writeln!(out, "{name} = {}", value_dump_scalar(v)).map_err(Error::Io)?;
            }
        }
    }
    Ok(())
}

fn value_dump_scalar(v: &Value) -> String {
    match v {
        Value::Uninit => "(uninitialized)".to_string(),
        Value::Str(s) => format!("{s:?}"),
        Value::Num(n) => format!("{n}"),
        Value::Mpfr(f) => f.to_string(),
        Value::Array(_) => "(array)".to_string(),
    }
}

/// Rule/function listing for `-D` / `--debug` (static inspection only — **not** GNU awk’s interactive debugger).
pub fn write_debug_listing(prog: &Program, out: &mut dyn Write, bin_name: &str) -> Result<()> {
    writeln!(
        out,
        "# {bin_name} — awkrs --debug: static program listing (NOT gawk’s interactive debugger)."
    )
    .map_err(Error::Io)?;
    writeln!(
        out,
        "# gawk’s -D debugger (break, step, next, print, watch, backtrace, stack, etc.) is not implemented; this output is for inspection only."
    )
    .map_err(Error::Io)?;
    writeln!(out, "rules: {}", prog.rules.len()).map_err(Error::Io)?;
    for (i, r) in prog.rules.iter().enumerate() {
        writeln!(out, "  [{i}] pattern: {:?}", r.pattern).map_err(Error::Io)?;
        writeln!(out, "      stmts: {}", r.stmts.len()).map_err(Error::Io)?;
        writeln!(out, "      --- pretty ---").map_err(Error::Io)?;
        let one = crate::ast::Program {
            rules: vec![r.clone()],
            funcs: Default::default(),
        };
        for line in ast_fmt::format_program(&one).lines() {
            writeln!(out, "      {line}").map_err(Error::Io)?;
        }
    }
    writeln!(out, "functions: {}", prog.funcs.len()).map_err(Error::Io)?;
    let mut fnames: Vec<&String> = prog.funcs.keys().collect();
    fnames.sort();
    for name in fnames {
        let fd = &prog.funcs[name];
        writeln!(
            out,
            "  function {name}({}) body_stmts={}",
            fd.params.join(", "),
            fd.body.len()
        )
        .map_err(Error::Io)?;
    }
    Ok(())
}

/// Pretty-print program as awk-like source from the AST ([`ast_fmt::format_program`]), not `Debug` output
/// and not gawk’s canonical `--pretty-print` source reformatter.
pub fn pretty_print_ast(prog: &Program) -> String {
    ast_fmt::format_program(prog)
}

/// Emit lint warnings for gawk extensions when `-L`, `-t`, or a truthy **`LINT`** variable is set.
pub fn emit_lint_warnings(
    bin_name: &str,
    prog: &Program,
    lint: Option<&str>,
    lint_old: bool,
    runtime_lint_var: bool,
) {
    let fatal = matches!(lint, Some(s) if s.eq_ignore_ascii_case("fatal"));
    let warn = lint.is_some() || lint_old || runtime_lint_var;
    if !warn {
        return;
    }
    let w = |msg: &str| {
        eprintln!("{bin_name}: lint: {msg}");
        if fatal {
            std::process::exit(2);
        }
    };
    if warn {
        w(
            "awkrs implements only a subset of gawk’s lint (missing: uninitialized variables, regex issues, full printf-format validation, …)",
        );
    }
    let mut begin_rules = 0usize;
    let mut end_rules = 0usize;
    for r in &prog.rules {
        use crate::ast::Pattern;
        if matches!(r.pattern, Pattern::BeginFile | Pattern::EndFile) {
            w("BEGINFILE/ENDFILE is a gawk extension");
        }
        if matches!(r.pattern, Pattern::Begin) {
            begin_rules += 1;
        }
        if matches!(r.pattern, Pattern::End) {
            end_rules += 1;
        }
    }
    if begin_rules > 1 {
        w("multiple BEGIN rules (gawk merges them; order is significant)");
    }
    if end_rules > 1 {
        w("multiple END rules (gawk merges them; order is significant)");
    }
    for r in &prog.rules {
        for s in &r.stmts {
            if matches!(s, Stmt::Switch { .. }) {
                w("switch is a gawk extension");
                break;
            }
        }
    }
    for fd in prog.funcs.values() {
        for s in &fd.body {
            if matches!(s, Stmt::NextFile) {
                w("nextfile in a user function may be invalid depending on context");
                break;
            }
        }
    }
    if lint_old {
        w("-t/--lint-old: deprecated extension checks are not fully implemented");
    }
    if warn {
        for r in &prog.rules {
            for s in &r.stmts {
                lint_stmt_printf_args(&w, s);
            }
        }
        for fd in prog.funcs.values() {
            for s in &fd.body {
                lint_stmt_printf_args(&w, s);
            }
        }
    }
}

/// Best-effort: if the format is a string literal, compare minimum `sprintf` value count to `printf` args.
fn printf_min_args_for_format(fmt: &str) -> Option<usize> {
    const MAX: usize = 256;
    let dummies: Vec<Value> = (0..MAX).map(|_| Value::Num(0.0)).collect();
    for n in 0..MAX {
        if awk_sprintf(fmt, &dummies[..n]).is_ok() {
            return Some(n);
        }
    }
    None
}

fn lint_stmt_printf_args(w: &impl Fn(&str), stmt: &Stmt) {
    match stmt {
        Stmt::Printf { args, .. } => {
            if let Some(Expr::Str(fmt)) = args.first() {
                if let Some(min) = printf_min_args_for_format(fmt) {
                    let have = args.len().saturating_sub(1);
                    if have < min {
                        w(&format!(
                            "printf: format may need at least {min} value argument(s) (have {have})"
                        ));
                    }
                }
            }
        }
        Stmt::If { then_, else_, .. } => {
            for s in then_ {
                lint_stmt_printf_args(w, s);
            }
            for s in else_ {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::ForC { body, .. } | Stmt::ForIn { body, .. } => {
            for s in body {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::Block(ss) => {
            for s in ss {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::Switch { arms, .. } => {
            for arm in arms {
                let stmts = match arm {
                    SwitchArm::Case { stmts, .. } => stmts,
                    SwitchArm::Default { stmts } => stmts,
                };
                for s in stmts {
                    lint_stmt_printf_args(w, s);
                }
            }
        }
        _ => {}
    }
}

/// Write profile summary: wall-clock plus per-record-rule execution counts when **sequential**.
/// Empty path or `-` writes to stderr (matches gawk-style default when no file is given).
pub fn write_profile_summary(
    path: &str,
    elapsed: std::time::Duration,
    records_hint: Option<usize>,
    record_rule_hits: &[u64],
    parallel_mode: bool,
) -> Result<()> {
    let mut w: Box<dyn Write> = if path.is_empty() || path == "-" {
        Box::new(std::io::stderr())
    } else {
        Box::new(
            std::fs::File::create(Path::new(path))
                .map_err(|e| Error::Runtime(format!("profile {path}: {e}")))?,
        )
    };
    writeln!(
        w,
        "# awkrs profile: wall time + per-record-rule invocation counts (sequential runs only)."
    )
    .map_err(Error::Io)?;
    writeln!(
        w,
        "# This is not gawk’s full profiler (no per-function or per-line counts). Use a single thread (-j1) for rule-hit counts."
    )
    .map_err(Error::Io)?;
    writeln!(
        w,
        "# This is not gawk’s annotated profile output (no per-statement execution counts in the canonical profile format)."
    )
    .map_err(Error::Io)?;
    writeln!(w, "wall_seconds: {:.6}", elapsed.as_secs_f64()).map_err(Error::Io)?;
    if let Some(n) = records_hint {
        writeln!(w, "records_processed: {n}").map_err(Error::Io)?;
    }
    if parallel_mode {
        writeln!(
            w,
            "record_rule_hits: (skipped: parallel mode — use -j1 for per-rule counts)"
        )
        .map_err(Error::Io)?;
    } else {
        writeln!(w, "record_rule_hits:").map_err(Error::Io)?;
        for (i, &n) in record_rule_hits.iter().enumerate() {
            writeln!(w, "  rule[{i}]: {n}").map_err(Error::Io)?;
        }
    }
    Ok(())
}
