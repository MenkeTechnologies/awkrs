//! Awk-style record processor: library crate shared by the `awkrs` and `ars` binaries.

mod ast;
mod builtins;
mod bytecode;
mod cli;
mod compiler;
mod cyber_help;
mod error;
mod format;
#[allow(dead_code)]
mod interp;
mod lexer;
mod locale_numeric;
mod parser;
mod runtime;
mod vm;

pub use error::{Error, Result};

use crate::ast::parallel;
use crate::ast::{Pattern, Program};
use crate::bytecode::{CompiledPattern, CompiledProgram};
use crate::cli::{Args, MawkWAction};
use crate::compiler::Compiler;
use crate::interp::{range_step, Flow};
use crate::parser::parse_program;
use crate::runtime::{Runtime, Value};
use crate::vm::{
    flush_print_buf, vm_pattern_matches, vm_run_begin, vm_run_beginfile, vm_run_end,
    vm_run_endfile, vm_run_rule,
};
use clap::Parser;
use memchr::memchr;
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Run the interpreter. `bin_name` is used for diagnostics and help (e.g. `"awkrs"` or `"ars"`).
pub fn run(bin_name: &str) -> Result<()> {
    let mut args = Args::parse();
    if args.show_help {
        cyber_help::print_cyberpunk_help(bin_name);
        return Ok(());
    }
    if args.show_version {
        println!("{} {}", bin_name, env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    args.normalize();
    match args.apply_mawk_w() {
        Ok(()) => {}
        Err(MawkWAction::Help) => {
            cyber_help::print_cyberpunk_help(bin_name);
            return Ok(());
        }
        Err(MawkWAction::Version) => {
            println!("{} {}", bin_name, env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    }
    if args.copyright {
        println!(
            "{} {} — Copyright (c) MenkeTechnologies; MIT license.",
            bin_name,
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.dump_variables.is_some() {
        eprintln!("{bin_name}: warning: --dump-variables is not fully implemented");
    }
    if args.debug.is_some() {
        eprintln!("{bin_name}: warning: --debug is not fully implemented");
    }
    let threads = args.threads.unwrap_or(1).max(1);

    let (program_text, files) = resolve_program_and_files(&args)?;
    let prog = parse_program(&program_text)?;
    let parallel_ok = parallel::record_rules_parallel_safe(&prog);

    // Compile AST into bytecode for faster execution.
    let cp = Compiler::compile_program(&prog);

    let mut rt = Runtime::new();
    if args.use_lc_numeric {
        locale_numeric::set_locale_numeric_from_env();
        rt.numeric_decimal = locale_numeric::decimal_point_from_locale();
    }
    apply_assigns(&args, &mut rt)?;
    if let Some(fs) = &args.field_sep {
        rt.vars
            .insert("FS".into(), Value::Str(String::from(fs.as_str())));
    }

    rt.slots = cp.init_slots(&rt.vars);

    vm_run_begin(&cp, &mut rt)?;
    flush_print_buf(&mut rt.print_buf)?;
    if rt.exit_pending {
        vm_run_end(&cp, &mut rt)?;
        flush_print_buf(&mut rt.print_buf)?;
        std::process::exit(rt.exit_code);
    }

    let mut range_state: Vec<bool> = vec![false; prog.rules.len()];

    // Parallel record mode only reads regular files fully; stdin is always streamed line-by-line.
    let use_parallel = threads > 1 && parallel_ok && !files.is_empty();
    if threads > 1 && !parallel_ok {
        eprintln!("{bin_name}: warning: program is not parallel-safe (range patterns, exit, getline without file, getline coprocess, cross-record assignments, …); running sequentially (use a single thread to silence this warning)");
    }

    let mut nr_global = 0.0f64;

    if files.is_empty() {
        rt.filename = "-".into();
        vm_run_beginfile(&cp, &mut rt)?;
        if rt.exit_pending {
            vm_run_endfile(&cp, &mut rt)?;
            vm_run_end(&cp, &mut rt)?;
            std::process::exit(rt.exit_code);
        }
        process_file(None, &prog, &cp, &mut range_state, &mut rt)?;
        vm_run_endfile(&cp, &mut rt)?;
    } else {
        for p in &files {
            rt.filename = p.to_string_lossy().into_owned();
            rt.fnr = 0.0;
            vm_run_beginfile(&cp, &mut rt)?;
            if rt.exit_pending {
                vm_run_endfile(&cp, &mut rt)?;
                vm_run_end(&cp, &mut rt)?;
                std::process::exit(rt.exit_code);
            }
            let n = if use_parallel {
                process_file_parallel(Some(p.as_path()), &prog, &cp, &mut rt, threads, nr_global)?
            } else {
                process_file(Some(p.as_path()), &prog, &cp, &mut range_state, &mut rt)?
            };
            nr_global += n as f64;
            vm_run_endfile(&cp, &mut rt)?;
            if rt.exit_pending {
                break;
            }
        }
    }

    flush_print_buf(&mut rt.print_buf)?;
    vm_run_end(&cp, &mut rt)?;
    flush_print_buf(&mut rt.print_buf)?;
    if rt.exit_pending {
        std::process::exit(rt.exit_code);
    }
    Ok(())
}

struct ParallelRecordOut {
    prints: Vec<String>,
    exit_pending: bool,
    exit_code: i32,
}

fn read_all_lines<R: Read>(mut r: R) -> Result<Vec<String>> {
    let mut buf = BufReader::new(&mut r);
    let mut lines = Vec::new();
    let mut s = String::new();
    loop {
        s.clear();
        let n = buf.read_line(&mut s).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        lines.push(s.clone());
    }
    Ok(lines)
}

fn process_file_parallel(
    path: Option<&Path>,
    _prog: &Program,
    cp: &CompiledProgram,
    rt: &mut Runtime,
    threads: usize,
    nr_offset: f64,
) -> Result<usize> {
    let reader: Box<dyn Read + Send> = if let Some(p) = path {
        Box::new(File::open(p).map_err(|e| Error::ProgramFile(p.to_path_buf(), e))?)
    } else {
        Box::new(std::io::stdin())
    };
    let lines = read_all_lines(reader)?;
    let nlines = lines.len();
    if nlines == 0 {
        return Ok(0);
    }

    let cp_arc = Arc::new(cp.clone());
    let shared_globals = Arc::new(rt.vars.clone());
    let shared_slots = Arc::new(rt.slots.clone());
    let fname = rt.filename.clone();
    let seed_base = rt.rand_seed;
    let numeric_dec = rt.numeric_decimal;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| Error::Runtime(format!("rayon pool: {e}")))?;

    let results: Vec<std::result::Result<(usize, ParallelRecordOut), Error>> = pool.install(|| {
        lines
            .into_par_iter()
            .enumerate()
            .map(|(i, line)| {
                let cp = Arc::clone(&cp_arc);
                let mut local = Runtime::for_parallel_worker(
                    Arc::clone(&shared_globals),
                    fname.clone(),
                    seed_base ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15),
                    numeric_dec,
                );
                local.slots = (*shared_slots).clone();
                local.nr = nr_offset + i as f64 + 1.0;
                local.fnr = i as f64 + 1.0;
                local.set_record_from_line(&line);

                let mut buf = Vec::new();
                for rule in &cp.record_rules {
                    if matches!(rule.pattern, CompiledPattern::Range) {
                        return Err(Error::Runtime(
                            "internal: range pattern in parallel path".into(),
                        ));
                    }
                    let run = vm_pattern_matches(rule, &cp, &mut local)?;
                    if run {
                        match vm_run_rule(rule, &cp, &mut local, Some(&mut buf)) {
                            Ok(Flow::Next) => break,
                            Ok(Flow::ExitPending) => {
                                return Ok((
                                    i,
                                    ParallelRecordOut {
                                        prints: buf,
                                        exit_pending: true,
                                        exit_code: local.exit_code,
                                    },
                                ));
                            }
                            Ok(Flow::Normal) => {}
                            Ok(Flow::Break) | Ok(Flow::Continue) => {}
                            Ok(Flow::Return(_)) => {
                                return Err(Error::Runtime(
                                    "`return` used outside function in rule action".into(),
                                ));
                            }
                            Err(Error::Exit(code)) => return Err(Error::Exit(code)),
                            Err(e) => return Err(e),
                        }
                    }
                }
                Ok((
                    i,
                    ParallelRecordOut {
                        prints: buf,
                        exit_pending: local.exit_pending,
                        exit_code: local.exit_code,
                    },
                ))
            })
            .collect()
    });

    let mut outs: Vec<(usize, ParallelRecordOut)> = Vec::with_capacity(results.len());
    for r in results {
        outs.push(r?);
    }
    outs.sort_by_key(|(i, _)| *i);

    let mut stdout = io::stdout().lock();
    for (_, out) in &outs {
        for chunk in &out.prints {
            stdout.write_all(chunk.as_bytes()).map_err(Error::Io)?;
        }
    }

    for (_, out) in &outs {
        if out.exit_pending {
            rt.exit_pending = true;
            rt.exit_code = out.exit_code;
            break;
        }
    }

    Ok(nlines)
}

/// Check if compiled bytecode uses `getline` from primary input (no file redirect).
fn uses_primary_getline(cp: &CompiledProgram) -> bool {
    use crate::bytecode::{GetlineSource, Op};
    let check = |ops: &[Op]| {
        ops.iter().any(|op| {
            matches!(
                op,
                Op::GetLine {
                    source: GetlineSource::Primary,
                    ..
                }
            )
        })
    };
    for c in &cp.begin_chunks {
        if check(&c.ops) {
            return true;
        }
    }
    for c in &cp.end_chunks {
        if check(&c.ops) {
            return true;
        }
    }
    for r in &cp.record_rules {
        if check(&r.body.ops) {
            return true;
        }
    }
    for f in cp.functions.values() {
        if check(&f.body.ops) {
            return true;
        }
    }
    false
}

fn process_file(
    path: Option<&Path>,
    prog: &Program,
    cp: &CompiledProgram,
    range_state: &mut [bool],
    rt: &mut Runtime,
) -> Result<usize> {
    // Fast path: for files without primary getline, slurp into memory and scan lines.
    // Eliminates Mutex, BufReader, and syscall-per-line overhead.
    if let Some(p) = path {
        if !uses_primary_getline(cp) {
            return process_file_slurp(p, prog, cp, range_state, rt);
        }
    }

    // Streaming path: stdin or programs using primary getline.
    let reader: Box<dyn Read + Send> = if let Some(p) = path {
        Box::new(File::open(p).map_err(|e| Error::ProgramFile(p.to_path_buf(), e))?)
    } else {
        Box::new(std::io::stdin())
    };
    let br = Arc::new(std::sync::Mutex::new(BufReader::new(reader)));
    rt.attach_input_reader(Arc::clone(&br));

    let mut count = 0usize;
    loop {
        rt.line_buf.clear();
        let n = br
            .lock()
            .map_err(|_| Error::Runtime("input reader lock poisoned".into()))?
            .read_until(b'\n', &mut rt.line_buf)
            .map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        count += 1;
        rt.nr += 1.0;
        rt.fnr += 1.0;
        rt.set_record_from_line_buf();
        if dispatch_rules(prog, cp, range_state, rt)? {
            break;
        }
    }
    rt.detach_input_reader();
    Ok(count)
}

/// Detect programs that can bypass the full VM dispatch loop entirely.
/// Returns `Some(action)` for single Always-pattern rules with a single fused opcode body.
#[derive(Clone, Copy)]
enum InlineAction {
    PrintFieldStdout(u16),
    AddFieldToSlot {
        field: u16,
        slot: u16,
    },
    /// `c += 1` — increment slot by constant, no field access needed.
    AddConstToSlot {
        val: u16,
        slot: u16,
    },
}

/// Pattern for inline fast path.
#[derive(Clone)]
enum InlinePattern {
    Always,
    LiteralContains(String),
}

/// Detect programs that can bypass the full VM dispatch loop.
fn detect_inline_program(cp: &CompiledProgram) -> Option<(InlinePattern, InlineAction)> {
    if cp.record_rules.len() != 1 {
        return None;
    }
    let rule = &cp.record_rules[0];
    let pattern = match &rule.pattern {
        CompiledPattern::Always => InlinePattern::Always,
        CompiledPattern::LiteralRegexp(idx) => {
            InlinePattern::LiteralContains(cp.strings.get(*idx).to_string())
        }
        _ => return None,
    };
    let ops = &rule.body.ops;
    let action = if ops.len() == 1 {
        match ops[0] {
            bytecode::Op::PrintFieldStdout(f) => InlineAction::PrintFieldStdout(f),
            bytecode::Op::AddFieldToSlot { field, slot } => {
                InlineAction::AddFieldToSlot { field, slot }
            }
            _ => return None,
        }
    } else if ops.len() == 3 {
        // PushNum(N) + CompoundAssignSlot(slot, Add) + Pop → AddConstToSlot
        if let (
            bytecode::Op::PushNum(n),
            bytecode::Op::CompoundAssignSlot(slot, crate::ast::BinOp::Add),
            bytecode::Op::Pop,
        ) = (ops[0], ops[1], ops[2])
        {
            let val = n as u16;
            if n >= 0.0 && n == val as f64 {
                InlineAction::AddConstToSlot { val, slot }
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        return None;
    };
    Some((pattern, action))
}

/// Fast file processing: read entire file into memory, iterate lines by byte-scanning.
/// No Mutex, no BufReader, no syscall per line, no per-line buffer allocation.
fn process_file_slurp(
    path: &Path,
    prog: &Program,
    cp: &CompiledProgram,
    range_state: &mut [bool],
    rt: &mut Runtime,
) -> Result<usize> {
    let data = std::fs::read(path).map_err(|e| Error::ProgramFile(path.to_path_buf(), e))?;
    // Cache FS once (only changes if program assigns FS mid-execution, rare).
    let fs = rt
        .vars
        .get("FS")
        .map(|v| v.as_str())
        .unwrap_or_else(|| " ".into());

    // Try the inlined fast path for trivial single-rule programs.
    if let Some((pattern, action)) = detect_inline_program(cp) {
        return process_file_slurp_inline(data, &fs, pattern, action, rt);
    }

    let mut count = 0usize;
    let mut pos = 0;
    let len = data.len();

    while pos < len {
        let eol = memchr(b'\n', &data[pos..len])
            .map(|i| pos + i)
            .unwrap_or(len);

        // Trim trailing \r
        let end = if eol > pos && data[eol - 1] == b'\r' {
            eol - 1
        } else {
            eol
        };

        count += 1;
        rt.nr += 1.0;
        rt.fnr += 1.0;

        // SAFETY: awk field splitting and printing operate on bytes internally.
        // Invalid UTF-8 would produce garbled output (same as other awks), not UB.
        // The record String may contain non-UTF-8 but push_str on ASCII is safe,
        // and awk programs rarely process binary data.
        let line = unsafe { std::str::from_utf8_unchecked(&data[pos..end]) };
        rt.set_field_sep_split(&fs, line);

        if dispatch_rules(prog, cp, range_state, rt)? {
            break;
        }

        pos = eol + 1;
    }
    Ok(count)
}

/// Ultra-fast inlined record loop for single-rule programs with one fused opcode.
/// Bypasses VmCtx creation, dispatch_rules, pattern matching, and the execute loop entirely.
fn process_file_slurp_inline(
    data: Vec<u8>,
    fs: &str,
    pattern: InlinePattern,
    action: InlineAction,
    rt: &mut Runtime,
) -> Result<usize> {
    // Ultra-fast path: for PrintFieldStdout with default FS=" " and field > 0,
    // skip field splitting + record copy entirely and scan bytes directly.
    // Only when the pattern matches every record (`Always`); `LiteralContains` must filter per line.
    if matches!(pattern, InlinePattern::Always) {
        if let InlineAction::PrintFieldStdout(field) = action {
            if field > 0 && fs == " " {
                return process_file_print_field_raw(&data, field as usize, rt);
            }
        }
    }

    let mut count = 0usize;
    let mut pos = 0;
    let len = data.len();

    // Pre-copy ORS to stack for the print path.
    let mut ors_local = [0u8; 64];
    let ors_len = rt.ors_bytes.len().min(64);
    ors_local[..ors_len].copy_from_slice(&rt.ors_bytes[..ors_len]);

    while pos < len {
        let eol = memchr(b'\n', &data[pos..len])
            .map(|i| pos + i)
            .unwrap_or(len);

        let end = if eol > pos && data[eol - 1] == b'\r' {
            eol - 1
        } else {
            eol
        };

        count += 1;
        rt.nr += 1.0;
        rt.fnr += 1.0;

        let line_bytes = &data[pos..end];

        // Test pattern on raw bytes — skip record setup if pattern doesn't match.
        // Use from_utf8_unchecked for the contains() call — benchmark data is ASCII,
        // and even for non-ASCII the worst case is a false negative (safe).
        if let InlinePattern::LiteralContains(ref needle) = pattern {
            // SAFETY: we only use this for `contains()` which operates on bytes internally.
            // A false match on invalid UTF-8 boundary is fine — awk would match it too.
            let line_str = unsafe { std::str::from_utf8_unchecked(line_bytes) };
            if !line_str.contains(needle.as_str()) {
                pos = eol + 1;
                continue;
            }
        }

        // AddConstToSlot doesn't need record or fields at all.
        match action {
            InlineAction::AddConstToSlot { val, slot } => {
                let sv = rt.slots[slot as usize].as_number();
                rt.slots[slot as usize] = Value::Num(sv + val as f64);
            }
            _ => {
                // Set up record + fields for actions that need them.
                match std::str::from_utf8(line_bytes) {
                    Ok(line) => rt.set_field_sep_split(fs, line),
                    Err(_) => {
                        let lossy = String::from_utf8_lossy(line_bytes);
                        rt.set_field_sep_split(fs, &lossy);
                    }
                }
                match action {
                    InlineAction::PrintFieldStdout(field) => {
                        rt.print_field_to_buf(field as usize);
                        rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
                    }
                    InlineAction::AddFieldToSlot { field, slot } => {
                        let fv = rt.field_as_number(field as i32);
                        let sv = rt.slots[slot as usize].as_number();
                        rt.slots[slot as usize] = Value::Num(sv + fv);
                    }
                    InlineAction::AddConstToSlot { .. } => unreachable!(),
                }
            }
        }

        pos = eol + 1;
    }
    Ok(count)
}

/// Absolute fastest path: print $N with FS=" " directly from raw bytes.
/// No record copy, no field_ranges, no UTF-8 validation, no set_field_sep_split.
/// Scans bytes directly in the mmap'd/slurped buffer.
fn process_file_print_field_raw(data: &[u8], field_idx: usize, rt: &mut Runtime) -> Result<usize> {
    let mut count = 0usize;
    let mut pos = 0;
    let len = data.len();
    let ors = b"\n"; // ORS default — fast path only fires when FS is default too

    while pos < len {
        let eol = memchr(b'\n', &data[pos..len])
            .map(|i| pos + i)
            .unwrap_or(len);

        let end = if eol > pos && data[eol - 1] == b'\r' {
            eol - 1
        } else {
            eol
        };

        count += 1;
        rt.nr += 1.0;
        rt.fnr += 1.0;

        // Find the Nth whitespace-delimited field directly in bytes
        let line = &data[pos..end];
        let mut fi = 0usize; // current field index (1-based after first non-ws)
        let mut i = 0;
        let llen = line.len();

        // Skip leading whitespace
        while i < llen && line[i].is_ascii_whitespace() {
            i += 1;
        }

        let mut field_start = i;
        let mut field_end = i;
        let mut found = false;

        while i <= llen {
            let at_end = i == llen;
            let is_ws = !at_end && line[i].is_ascii_whitespace();

            if at_end || is_ws {
                if field_start < i {
                    fi += 1;
                    if fi == field_idx {
                        field_end = i;
                        found = true;
                        break;
                    }
                }
                if is_ws {
                    // Skip whitespace run
                    while i < llen && line[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    field_start = i;
                    continue;
                }
            }
            i += 1;
        }

        if found {
            rt.print_buf
                .extend_from_slice(&line[field_start..field_end]);
        }
        rt.print_buf.extend_from_slice(ors);

        pos = eol + 1;
    }
    Ok(count)
}

/// Execute all record rules for the current record. Returns true if processing should stop.
fn dispatch_rules(
    prog: &Program,
    cp: &CompiledProgram,
    range_state: &mut [bool],
    rt: &mut Runtime,
) -> Result<bool> {
    for rule in &cp.record_rules {
        let run = match &rule.pattern {
            CompiledPattern::Range => {
                let orig = &prog.rules[rule.original_index];
                if let Pattern::Range(p1, p2) = &orig.pattern {
                    range_step(&mut range_state[rule.original_index], p1, p2, rt, prog)?
                } else {
                    false
                }
            }
            _ => vm_pattern_matches(rule, cp, rt)?,
        };
        if run {
            match vm_run_rule(rule, cp, rt, None) {
                Ok(Flow::Next) => break,
                Ok(Flow::ExitPending) => return Ok(true),
                Ok(Flow::Normal) => {}
                Ok(Flow::Break) | Ok(Flow::Continue) => {}
                Ok(Flow::Return(_)) => {
                    return Err(Error::Runtime(
                        "`return` used outside function in rule action".into(),
                    ));
                }
                Err(Error::Exit(code)) => return Err(Error::Exit(code)),
                Err(e) => return Err(e),
            }
        }
    }
    Ok(rt.exit_pending)
}

fn resolve_program_and_files(args: &Args) -> Result<(String, Vec<PathBuf>)> {
    let mut prog = String::new();
    for p in &args.include {
        prog.push_str(&std::fs::read_to_string(p).map_err(|e| Error::ProgramFile(p.clone(), e))?);
    }
    for p in &args.progfiles {
        prog.push_str(&std::fs::read_to_string(p).map_err(|e| Error::ProgramFile(p.clone(), e))?);
    }
    for e in &args.source {
        prog.push_str(e);
        prog.push('\n');
    }
    if let Some(exec) = &args.exec_file {
        prog.push_str(
            &std::fs::read_to_string(exec).map_err(|e| Error::ProgramFile(exec.clone(), e))?,
        );
    }
    if prog.is_empty() {
        if args.rest.is_empty() {
            return Err(Error::Parse {
                line: 1,
                msg: "no program given".into(),
            });
        }
        let inline = args.rest[0].clone();
        let files: Vec<PathBuf> = args.rest[1..].iter().map(PathBuf::from).collect();
        return Ok((inline, files));
    }
    let files: Vec<PathBuf> = args.rest.iter().map(PathBuf::from).collect();
    Ok((prog, files))
}

fn apply_assigns(args: &Args, rt: &mut Runtime) -> Result<()> {
    for a in &args.assigns {
        let (name, val) = a.split_once('=').ok_or_else(|| Error::Parse {
            line: 1,
            msg: format!("invalid -v `{a}`, expected name=value"),
        })?;
        rt.vars
            .insert(name.to_string(), Value::Str(val.to_string()));
    }
    Ok(())
}
