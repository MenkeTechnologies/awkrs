//! Behaviors for gawk-style CLI flags (dump, pretty-print, gen-pot, lint, debug listing, profile timing).

use crate::ast::{
    Expr, GetlineRedir, IncDecTarget, Pattern, PrintRedir, Program, Stmt, SwitchArm, SwitchLabel,
};
use crate::ast_fmt;
use crate::bytecode::CompiledProgram;
use crate::error::{Error, Result};
use crate::format::awk_sprintf;
use crate::namespace::{BUILTIN_NAMES, SPECIAL_GLOBAL_NAMES};
use crate::runtime::{Runtime, Value};
use rustc_hash::FxHashSet;
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
        Stmt::GetLine {
            pipe_cmd, redir, ..
        } => {
            use crate::ast::GetlineRedir;
            if let Some(cmd) = pipe_cmd {
                collect_expr_strings(cmd, out);
            }
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
        Expr::RegexpLiteral(_) => {}
        Expr::Number(_) | Expr::IntegerLiteral(_) | Expr::Var(_) => {}
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
        Expr::Tuple(parts) => {
            for p in parts {
                collect_expr_strings(p, out);
            }
        }
        Expr::IncDec { target, .. } => match target {
            crate::ast::IncDecTarget::Field(inner) => collect_expr_strings(inner, out),
            crate::ast::IncDecTarget::Index { indices, .. } => {
                for x in indices {
                    collect_expr_strings(x, out);
                }
            }
            crate::ast::IncDecTarget::Var(_) => {}
        },
        Expr::GetLine {
            pipe_cmd, redir, ..
        } => {
            use crate::ast::GetlineRedir;
            if let Some(cmd) = pipe_cmd {
                collect_expr_strings(cmd, out);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => collect_expr_strings(e, out),
            }
        }
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
        Value::Str(s) | Value::StrLit(s) => format!("{s:?}"),
        Value::Regexp(s) => format!("@/{s}/ (regexp)"),
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
///
/// Prepends `#` comment lines so stdout/file output is obviously awkrs-specific (diffs against gawk are
/// not meaningful for format parity).
pub fn pretty_print_ast(bin_name: &str, prog: &Program) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {bin_name} — awkrs --pretty-print: AST-derived awk-like listing (NOT gawk’s --pretty-print output).\n"
    ));
    out.push_str(
        "# Do not expect byte-for-byte or structural parity with GNU awk when diffing this output.\n",
    );
    out.push('\n');
    out.push_str(&ast_fmt::format_program(prog));
    out
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
    emit_lint_diagnostics(&w, prog, lint_old);
}

/// Static lint passes (used by [`emit_lint_warnings`]; exposed for tests).
pub(crate) fn emit_lint_diagnostics(w: &impl Fn(&str), prog: &Program, lint_old: bool) {
    w("awkrs --lint: static checks (best-effort; not full gawk --lint parity)");

    let mut global_def = collect_program_defines(prog);
    for n in SPECIAL_GLOBAL_NAMES {
        global_def.insert((*n).to_string());
    }

    let mut begin_rules = 0usize;
    let mut end_rules = 0usize;
    for r in &prog.rules {
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

    let mut warned = FxHashSet::default();
    let empty_params = FxHashSet::default();

    for r in &prog.rules {
        lint_pattern_reads(&r.pattern, &global_def, &empty_params, &mut warned, w);
        for s in &r.stmts {
            stmt_lint_reads(s, &global_def, &empty_params, &mut warned, w);
        }
    }
    for fd in prog.funcs.values() {
        let params: FxHashSet<String> = fd.params.iter().cloned().collect();
        for s in &fd.body {
            stmt_lint_reads(s, &global_def, &params, &mut warned, w);
        }
    }

    for r in &prog.rules {
        lint_pattern_regex(w, &r.pattern);
        for s in &r.stmts {
            lint_stmt_regex(w, s);
        }
    }
    for fd in prog.funcs.values() {
        for s in &fd.body {
            lint_stmt_regex(w, s);
        }
    }

    for r in &prog.rules {
        for s in &r.stmts {
            lint_stmt_printf_args(w, s);
        }
    }
    for fd in prog.funcs.values() {
        for s in &fd.body {
            lint_stmt_printf_args(w, s);
        }
    }
}

fn collect_program_defines(prog: &Program) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    for r in &prog.rules {
        collect_pattern_defines(&r.pattern, &mut out);
        for s in &r.stmts {
            stmt_collect_defines(s, &mut out);
        }
    }
    for fd in prog.funcs.values() {
        for s in &fd.body {
            stmt_collect_defines(s, &mut out);
        }
    }
    out
}

fn collect_pattern_defines(p: &Pattern, out: &mut FxHashSet<String>) {
    match p {
        Pattern::Begin
        | Pattern::End
        | Pattern::BeginFile
        | Pattern::EndFile
        | Pattern::Empty
        | Pattern::Regexp(_) => {}
        Pattern::Expr(e) => expr_collect_defines(e, out),
        Pattern::Range(a, b) => {
            collect_pattern_defines(a, out);
            collect_pattern_defines(b, out);
        }
    }
}

fn stmt_collect_defines(s: &Stmt, out: &mut FxHashSet<String>) {
    match s {
        Stmt::If { cond, then_, else_ } => {
            expr_collect_defines(cond, out);
            for t in then_ {
                stmt_collect_defines(t, out);
            }
            for t in else_ {
                stmt_collect_defines(t, out);
            }
        }
        Stmt::While { cond, body } => {
            expr_collect_defines(cond, out);
            for t in body {
                stmt_collect_defines(t, out);
            }
        }
        Stmt::DoWhile { cond, body } => {
            expr_collect_defines(cond, out);
            for t in body {
                stmt_collect_defines(t, out);
            }
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                expr_collect_defines(e, out);
            }
            if let Some(e) = cond {
                expr_collect_defines(e, out);
            }
            if let Some(e) = iter {
                expr_collect_defines(e, out);
            }
            for t in body {
                stmt_collect_defines(t, out);
            }
        }
        Stmt::ForIn { var, body, .. } => {
            out.insert(var.clone());
            for t in body {
                stmt_collect_defines(t, out);
            }
        }
        Stmt::Block(ss) => {
            for t in ss {
                stmt_collect_defines(t, out);
            }
        }
        Stmt::Expr(e) => expr_collect_defines(e, out),
        Stmt::Print { args, redir } => {
            for e in args {
                expr_collect_defines(e, out);
            }
            if let Some(r) = redir {
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                expr_collect_defines(e, out);
            }
        }
        Stmt::Printf { args, redir } => {
            for e in args {
                expr_collect_defines(e, out);
            }
            if let Some(r) = redir {
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                expr_collect_defines(e, out);
            }
        }
        Stmt::GetLine {
            pipe_cmd,
            var,
            redir,
        } => {
            if let Some(v) = var {
                out.insert(v.clone());
            }
            if let Some(cmd) = pipe_cmd {
                expr_collect_defines(cmd, out);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => expr_collect_defines(e, out),
            }
        }
        Stmt::Delete { indices, .. } => {
            if let Some(ix) = indices {
                for e in ix {
                    expr_collect_defines(e, out);
                }
            }
        }
        Stmt::Switch { expr, arms } => {
            expr_collect_defines(expr, out);
            for a in arms {
                match a {
                    SwitchArm::Case { label, stmts } => {
                        if let SwitchLabel::Expr(e) = label {
                            expr_collect_defines(e, out);
                        }
                        for st in stmts {
                            stmt_collect_defines(st, out);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for st in stmts {
                            stmt_collect_defines(st, out);
                        }
                    }
                }
            }
        }
        Stmt::Exit(e) | Stmt::Return(e) => {
            if let Some(ex) = e {
                expr_collect_defines(ex, out);
            }
        }
        Stmt::Next | Stmt::NextFile | Stmt::Break | Stmt::Continue => {}
    }
}

fn expr_collect_defines(e: &Expr, out: &mut FxHashSet<String>) {
    match e {
        Expr::Number(_)
        | Expr::IntegerLiteral(_)
        | Expr::Str(_)
        | Expr::RegexpLiteral(_)
        | Expr::Var(_) => {}
        Expr::Field(inner) => expr_collect_defines(inner, out),
        Expr::Index { name, indices } => {
            out.insert(name.clone());
            for x in indices {
                expr_collect_defines(x, out);
            }
        }
        Expr::Binary { left, right, .. } => {
            expr_collect_defines(left, out);
            expr_collect_defines(right, out);
        }
        Expr::Unary { expr, .. } => expr_collect_defines(expr, out),
        Expr::Assign { name, rhs, .. } => {
            out.insert(name.clone());
            expr_collect_defines(rhs, out);
        }
        Expr::AssignField { field, rhs, .. } => {
            expr_collect_defines(field, out);
            expr_collect_defines(rhs, out);
        }
        Expr::AssignIndex {
            name, indices, rhs, ..
        } => {
            out.insert(name.clone());
            for x in indices {
                expr_collect_defines(x, out);
            }
            expr_collect_defines(rhs, out);
        }
        Expr::Call { args, .. } => {
            for a in args {
                expr_collect_defines(a, out);
            }
        }
        Expr::IndirectCall { callee, args } => {
            expr_collect_defines(callee, out);
            for a in args {
                expr_collect_defines(a, out);
            }
        }
        Expr::Ternary { cond, then_, else_ } => {
            expr_collect_defines(cond, out);
            expr_collect_defines(then_, out);
            expr_collect_defines(else_, out);
        }
        Expr::In { key, .. } => expr_collect_defines(key, out),
        Expr::Tuple(parts) => {
            for p in parts {
                expr_collect_defines(p, out);
            }
        }
        Expr::IncDec { target, .. } => match target {
            IncDecTarget::Var(n) => {
                out.insert(n.clone());
            }
            IncDecTarget::Field(inner) => expr_collect_defines(inner, out),
            IncDecTarget::Index { name, indices } => {
                out.insert(name.clone());
                for x in indices {
                    expr_collect_defines(x, out);
                }
            }
        },
        Expr::GetLine {
            pipe_cmd,
            var,
            redir,
        } => {
            if let Some(v) = var {
                out.insert(v.clone());
            }
            if let Some(cmd) = pipe_cmd {
                expr_collect_defines(cmd, out);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => expr_collect_defines(e, out),
            }
        }
    }
}

fn warn_uninit_var(
    name: &str,
    global_def: &FxHashSet<String>,
    params: &FxHashSet<String>,
    warned: &mut FxHashSet<String>,
    w: &impl Fn(&str),
) {
    if name.starts_with('_') {
        return;
    }
    if SPECIAL_GLOBAL_NAMES.contains(&name) {
        return;
    }
    if BUILTIN_NAMES.contains(&name) {
        return;
    }
    if params.contains(name) || global_def.contains(name) {
        return;
    }
    if !warned.insert(name.to_string()) {
        return;
    }
    w(&format!(
        "possible use of uninitialized variable `{name}` (best-effort static check)"
    ));
}

fn lint_pattern_reads(
    p: &Pattern,
    global_def: &FxHashSet<String>,
    params: &FxHashSet<String>,
    warned: &mut FxHashSet<String>,
    w: &impl Fn(&str),
) {
    match p {
        Pattern::Expr(e) => expr_lint_reads(e, global_def, params, warned, w),
        Pattern::Range(a, b) => {
            lint_pattern_reads(a, global_def, params, warned, w);
            lint_pattern_reads(b, global_def, params, warned, w);
        }
        _ => {}
    }
}

fn stmt_lint_reads(
    s: &Stmt,
    global_def: &FxHashSet<String>,
    params: &FxHashSet<String>,
    warned: &mut FxHashSet<String>,
    w: &impl Fn(&str),
) {
    match s {
        Stmt::If { cond, then_, else_ } => {
            expr_lint_reads(cond, global_def, params, warned, w);
            for t in then_ {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
            for t in else_ {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
        }
        Stmt::While { cond, body } => {
            expr_lint_reads(cond, global_def, params, warned, w);
            for t in body {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
        }
        Stmt::DoWhile { cond, body } => {
            for t in body {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
            expr_lint_reads(cond, global_def, params, warned, w);
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                expr_lint_reads(e, global_def, params, warned, w);
            }
            if let Some(e) = cond {
                expr_lint_reads(e, global_def, params, warned, w);
            }
            if let Some(e) = iter {
                expr_lint_reads(e, global_def, params, warned, w);
            }
            for t in body {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
        }
        Stmt::ForIn { body, .. } => {
            for t in body {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
        }
        Stmt::Block(ss) => {
            for t in ss {
                stmt_lint_reads(t, global_def, params, warned, w);
            }
        }
        Stmt::Expr(e) => expr_lint_reads(e, global_def, params, warned, w),
        Stmt::Print { args, redir } => {
            for e in args {
                expr_lint_reads(e, global_def, params, warned, w);
            }
            if let Some(r) = redir {
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                expr_lint_reads(e, global_def, params, warned, w);
            }
        }
        Stmt::Printf { args, redir } => {
            for e in args {
                expr_lint_reads(e, global_def, params, warned, w);
            }
            if let Some(r) = redir {
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                expr_lint_reads(e, global_def, params, warned, w);
            }
        }
        Stmt::GetLine {
            pipe_cmd, redir, ..
        } => {
            if let Some(cmd) = pipe_cmd {
                expr_lint_reads(cmd, global_def, params, warned, w);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => {
                    expr_lint_reads(e, global_def, params, warned, w);
                }
            }
        }
        Stmt::Delete { indices, .. } => {
            if let Some(ix) = indices {
                for e in ix {
                    expr_lint_reads(e, global_def, params, warned, w);
                }
            }
        }
        Stmt::Switch { expr, arms } => {
            expr_lint_reads(expr, global_def, params, warned, w);
            for a in arms {
                match a {
                    SwitchArm::Case { label, stmts } => {
                        if let SwitchLabel::Expr(e) = label {
                            expr_lint_reads(e, global_def, params, warned, w);
                        }
                        for st in stmts {
                            stmt_lint_reads(st, global_def, params, warned, w);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for st in stmts {
                            stmt_lint_reads(st, global_def, params, warned, w);
                        }
                    }
                }
            }
        }
        Stmt::Exit(e) | Stmt::Return(e) => {
            if let Some(ex) = e {
                expr_lint_reads(ex, global_def, params, warned, w);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::Next | Stmt::NextFile => {}
    }
}

fn expr_lint_reads(
    e: &Expr,
    global_def: &FxHashSet<String>,
    params: &FxHashSet<String>,
    warned: &mut FxHashSet<String>,
    w: &impl Fn(&str),
) {
    match e {
        Expr::Number(_) | Expr::IntegerLiteral(_) | Expr::Str(_) | Expr::RegexpLiteral(_) => {}
        Expr::Var(name) => warn_uninit_var(name, global_def, params, warned, w),
        Expr::Field(inner) => expr_lint_reads(inner, global_def, params, warned, w),
        Expr::Index { indices, .. } => {
            for x in indices {
                expr_lint_reads(x, global_def, params, warned, w);
            }
        }
        Expr::Binary { left, right, .. } => {
            expr_lint_reads(left, global_def, params, warned, w);
            expr_lint_reads(right, global_def, params, warned, w);
        }
        Expr::Unary { expr, .. } => expr_lint_reads(expr, global_def, params, warned, w),
        Expr::Assign { name, op, rhs } => {
            expr_lint_reads(rhs, global_def, params, warned, w);
            if op.is_some() {
                warn_uninit_var(name, global_def, params, warned, w);
            }
        }
        Expr::AssignField { field, rhs, .. } => {
            expr_lint_reads(field, global_def, params, warned, w);
            expr_lint_reads(rhs, global_def, params, warned, w);
        }
        Expr::AssignIndex { indices, rhs, .. } => {
            for x in indices {
                expr_lint_reads(x, global_def, params, warned, w);
            }
            expr_lint_reads(rhs, global_def, params, warned, w);
        }
        Expr::Call { args, .. } => {
            for a in args {
                expr_lint_reads(a, global_def, params, warned, w);
            }
        }
        Expr::IndirectCall { callee, args } => {
            expr_lint_reads(callee, global_def, params, warned, w);
            for a in args {
                expr_lint_reads(a, global_def, params, warned, w);
            }
        }
        Expr::Ternary { cond, then_, else_ } => {
            expr_lint_reads(cond, global_def, params, warned, w);
            expr_lint_reads(then_, global_def, params, warned, w);
            expr_lint_reads(else_, global_def, params, warned, w);
        }
        Expr::In { key, .. } => expr_lint_reads(key, global_def, params, warned, w),
        Expr::Tuple(parts) => {
            for p in parts {
                expr_lint_reads(p, global_def, params, warned, w);
            }
        }
        Expr::IncDec { target, .. } => match target {
            IncDecTarget::Var(n) => warn_uninit_var(n, global_def, params, warned, w),
            IncDecTarget::Field(inner) => expr_lint_reads(inner, global_def, params, warned, w),
            IncDecTarget::Index { indices, .. } => {
                for x in indices {
                    expr_lint_reads(x, global_def, params, warned, w);
                }
            }
        },
        Expr::GetLine {
            pipe_cmd, redir, ..
        } => {
            if let Some(cmd) = pipe_cmd {
                expr_lint_reads(cmd, global_def, params, warned, w);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => {
                    expr_lint_reads(e, global_def, params, warned, w);
                }
            }
        }
    }
}

fn lint_pattern_regex(w: &impl Fn(&str), p: &Pattern) {
    match p {
        Pattern::Begin
        | Pattern::End
        | Pattern::BeginFile
        | Pattern::EndFile
        | Pattern::Empty
        | Pattern::Expr(_) => {}
        Pattern::Regexp(s) => lint_regex_literal(w, s),
        Pattern::Range(a, b) => {
            lint_pattern_regex(w, a);
            lint_pattern_regex(w, b);
        }
    }
}

fn lint_regex_literal(w: &impl Fn(&str), s: &str) {
    if s.is_empty() {
        w("empty regex pattern matches every position");
    }
    if s.ends_with('\\') {
        w("regex pattern ends with a trailing backslash (possible mistake)");
    }
}

fn lint_stmt_regex(w: &impl Fn(&str), s: &Stmt) {
    match s {
        Stmt::Switch { expr: _, arms } => {
            for a in arms {
                match a {
                    SwitchArm::Case { label, stmts } => {
                        if let SwitchLabel::Regexp(re) = label {
                            lint_regex_literal(w, re);
                        }
                        for st in stmts {
                            lint_stmt_regex(w, st);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for st in stmts {
                            lint_stmt_regex(w, st);
                        }
                    }
                }
            }
        }
        Stmt::If { then_, else_, .. } => {
            for t in then_ {
                lint_stmt_regex(w, t);
            }
            for t in else_ {
                lint_stmt_regex(w, t);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for t in body {
                lint_stmt_regex(w, t);
            }
        }
        Stmt::ForC { body, .. } | Stmt::ForIn { body, .. } => {
            for t in body {
                lint_stmt_regex(w, t);
            }
        }
        Stmt::Block(ss) => {
            for t in ss {
                lint_stmt_regex(w, t);
            }
        }
        Stmt::Expr(_)
        | Stmt::Print { .. }
        | Stmt::Printf { .. }
        | Stmt::GetLine { .. }
        | Stmt::Delete { .. }
        | Stmt::Exit(_)
        | Stmt::Return(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::Next
        | Stmt::NextFile => {}
    }
}

fn lint_expr_printf_deep(w: &impl Fn(&str), e: &Expr) {
    match e {
        Expr::Call { name, args } => {
            if (name == "printf" || name == "sprintf") && !args.is_empty() {
                if let Some(Expr::Str(fmt)) = args.first() {
                    if let Some(min) = printf_min_args_for_format(fmt) {
                        let have = args.len().saturating_sub(1);
                        if have < min {
                            w(&format!(
                                "printf/sprintf: format may need at least {min} value argument(s) (have {have})"
                            ));
                        }
                    }
                }
            }
            for a in args {
                lint_expr_printf_deep(w, a);
            }
        }
        Expr::IndirectCall { callee, args } => {
            lint_expr_printf_deep(w, callee);
            for a in args {
                lint_expr_printf_deep(w, a);
            }
        }
        Expr::Number(_)
        | Expr::IntegerLiteral(_)
        | Expr::Str(_)
        | Expr::RegexpLiteral(_)
        | Expr::Var(_) => {}
        Expr::Field(inner) => lint_expr_printf_deep(w, inner),
        Expr::Index { indices, .. } => {
            for x in indices {
                lint_expr_printf_deep(w, x);
            }
        }
        Expr::Binary { left, right, .. } => {
            lint_expr_printf_deep(w, left);
            lint_expr_printf_deep(w, right);
        }
        Expr::Unary { expr, .. } => lint_expr_printf_deep(w, expr),
        Expr::Assign { rhs, .. } => lint_expr_printf_deep(w, rhs),
        Expr::AssignField { field, rhs, .. } => {
            lint_expr_printf_deep(w, field);
            lint_expr_printf_deep(w, rhs);
        }
        Expr::AssignIndex { indices, rhs, .. } => {
            for x in indices {
                lint_expr_printf_deep(w, x);
            }
            lint_expr_printf_deep(w, rhs);
        }
        Expr::Ternary { cond, then_, else_ } => {
            lint_expr_printf_deep(w, cond);
            lint_expr_printf_deep(w, then_);
            lint_expr_printf_deep(w, else_);
        }
        Expr::In { key, .. } => lint_expr_printf_deep(w, key),
        Expr::Tuple(parts) => {
            for p in parts {
                lint_expr_printf_deep(w, p);
            }
        }
        Expr::IncDec { target, .. } => match target {
            IncDecTarget::Field(inner) => lint_expr_printf_deep(w, inner),
            IncDecTarget::Index { indices, .. } => {
                for x in indices {
                    lint_expr_printf_deep(w, x);
                }
            }
            IncDecTarget::Var(_) => {}
        },
        Expr::GetLine {
            pipe_cmd, redir, ..
        } => {
            if let Some(cmd) = pipe_cmd {
                lint_expr_printf_deep(w, cmd);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => lint_expr_printf_deep(w, e),
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
        Stmt::Printf { args, redir } => {
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
            for a in args {
                lint_expr_printf_deep(w, a);
            }
            if let Some(r) = redir {
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                lint_expr_printf_deep(w, e);
            }
        }
        Stmt::Expr(e) => lint_expr_printf_deep(w, e),
        Stmt::Print { args, redir } => {
            for a in args {
                lint_expr_printf_deep(w, a);
            }
            if let Some(r) = redir {
                let e = match r {
                    PrintRedir::Overwrite(e)
                    | PrintRedir::Append(e)
                    | PrintRedir::Pipe(e)
                    | PrintRedir::Coproc(e) => e,
                };
                lint_expr_printf_deep(w, e);
            }
        }
        Stmt::If { cond, then_, else_ } => {
            lint_expr_printf_deep(w, cond);
            for s in then_ {
                lint_stmt_printf_args(w, s);
            }
            for s in else_ {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::While { cond, body } => {
            lint_expr_printf_deep(w, cond);
            for s in body {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::DoWhile { cond, body } => {
            for s in body {
                lint_stmt_printf_args(w, s);
            }
            lint_expr_printf_deep(w, cond);
        }
        Stmt::ForC {
            init,
            cond,
            iter,
            body,
        } => {
            if let Some(e) = init {
                lint_expr_printf_deep(w, e);
            }
            if let Some(e) = cond {
                lint_expr_printf_deep(w, e);
            }
            if let Some(e) = iter {
                lint_expr_printf_deep(w, e);
            }
            for s in body {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::ForIn { body, .. } => {
            for s in body {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::Block(ss) => {
            for s in ss {
                lint_stmt_printf_args(w, s);
            }
        }
        Stmt::GetLine {
            pipe_cmd, redir, ..
        } => {
            if let Some(cmd) = pipe_cmd {
                lint_expr_printf_deep(w, cmd);
            }
            match redir {
                GetlineRedir::Primary => {}
                GetlineRedir::File(e) | GetlineRedir::Coproc(e) => lint_expr_printf_deep(w, e),
            }
        }
        Stmt::Delete { indices, .. } => {
            if let Some(ix) = indices {
                for e in ix {
                    lint_expr_printf_deep(w, e);
                }
            }
        }
        Stmt::Switch { expr, arms } => {
            lint_expr_printf_deep(w, expr);
            for arm in arms {
                match arm {
                    SwitchArm::Case { label, stmts } => {
                        if let SwitchLabel::Expr(e) = label {
                            lint_expr_printf_deep(w, e);
                        }
                        for s in stmts {
                            lint_stmt_printf_args(w, s);
                        }
                    }
                    SwitchArm::Default { stmts } => {
                        for s in stmts {
                            lint_stmt_printf_args(w, s);
                        }
                    }
                }
            }
        }
        Stmt::Exit(e) | Stmt::Return(e) => {
            if let Some(ex) = e {
                lint_expr_printf_deep(w, ex);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::Next | Stmt::NextFile => {}
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
        "# Output layout is awkrs-specific (key/value style); diffing against gawk --profile will not match."
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

#[cfg(test)]
mod lint_tests {
    use super::emit_lint_diagnostics;
    use crate::parser::parse_program;
    use std::cell::RefCell;

    fn lint_msgs(src: &str) -> Vec<String> {
        let prog = parse_program(src).unwrap();
        let msgs = RefCell::new(Vec::new());
        emit_lint_diagnostics(&|m| msgs.borrow_mut().push(m.to_string()), &prog, false);
        msgs.into_inner()
    }

    #[test]
    fn lint_warns_uninitialized_scalar() {
        let msgs = lint_msgs("BEGIN { print u }");
        assert!(
            msgs.iter()
                .any(|m| m.contains("uninitialized") && m.contains('u')),
            "{msgs:?}"
        );
    }

    #[test]
    fn lint_no_uninit_after_assign() {
        let msgs = lint_msgs("BEGIN { u = 1; print u }");
        assert!(
            !msgs
                .iter()
                .any(|m| m.contains("uninitialized") && m.contains('u')),
            "{msgs:?}"
        );
    }

    #[test]
    fn lint_printf_sprintf_in_expr() {
        let msgs = lint_msgs("BEGIN { s = sprintf(\"%d\") }");
        assert!(
            msgs.iter()
                .any(|m| m.contains("printf") && m.contains("format")),
            "{msgs:?}"
        );
    }

    #[test]
    fn lint_empty_regex_pattern() {
        let msgs = lint_msgs("// { print }");
        assert!(msgs.iter().any(|m| m.contains("empty regex")), "{msgs:?}");
    }

    #[test]
    fn lint_for_c_body_warns_uninitialized() {
        let msgs = lint_msgs("BEGIN { for (i = 0; i < 1; i++) { print u } }");
        assert!(
            msgs.iter()
                .any(|m| m.contains("uninitialized") && m.contains('u')),
            "{msgs:?}"
        );
    }
}

#[cfg(test)]
mod pretty_print_tests {
    use super::pretty_print_ast;
    use crate::parser::parse_program;

    #[test]
    fn pretty_print_starts_with_gawk_disclaimer() {
        let prog = parse_program("BEGIN { print 1 }").unwrap();
        let s = pretty_print_ast("awkrs", &prog);
        assert!(
            s.starts_with("# awkrs — awkrs --pretty-print:"),
            "unexpected prefix: {:?}",
            s.lines().take(3).collect::<Vec<_>>()
        );
        assert!(
            s.contains("NOT gawk"),
            "expected NOT gawk disclaimer in {s:?}"
        );
    }
}

#[cfg(test)]
mod gen_pot_tests {
    use super::gen_pot;
    use crate::parser::parse_program;

    #[test]
    fn gen_pot_collects_nonempty_string_literals_from_print() {
        let p = parse_program(r#"BEGIN { print "Hello", "World" }"#).unwrap();
        let s = gen_pot(&p);
        assert!(s.contains("msgid \"Hello\""), "{s}");
        assert!(s.contains("msgid \"World\""), "{s}");
    }

    #[test]
    fn gen_pot_dedupes_duplicate_msgids_single_entry() {
        let p = parse_program(r#"BEGIN { print "x"; print "x" }"#).unwrap();
        let s = gen_pot(&p);
        assert_eq!(s.matches("msgid \"x\"").count(), 1, "{s}");
    }

    #[test]
    fn gen_pot_escapes_quotes_backslash_newline_in_msgid() {
        let p = parse_program(r#"BEGIN { s = "a\"b\nc" }"#).unwrap();
        let s = gen_pot(&p);
        assert!(s.contains("msgid \"a\\\"b\\nc\""), "{s}");
    }
}
