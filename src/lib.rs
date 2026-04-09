//! Awk-style record processor: library crate shared by the `awkrs` and `ars` binaries.

mod ast;
mod builtins;
mod bytecode;
mod cli;
mod compiler;
mod cyber_help;
mod error;
mod format;
pub mod jit;
pub use jit::{
    is_jit_eligible, is_numeric_stack_eligible, try_compile, try_compile_numeric_expr,
    try_jit_dispatch_numeric_chunk, try_jit_execute, JitChunk, JitNumericChunk, JitRuntimeState,
};
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
use crate::bytecode::{CompiledPattern, CompiledProgram, Op, RedirKind, SubTarget};
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
use memchr::memmem;
use memmap2::Mmap;
use rayon::prelude::*;
use rayon::ThreadPool;
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

    // Compile once; `Arc` is shared with parallel workers (cheap refcount) instead of cloning [`CompiledProgram`].
    let cp: Arc<CompiledProgram> = Arc::new(Compiler::compile_program(&prog));

    let mut rt = Runtime::new();
    if args.use_lc_numeric {
        locale_numeric::set_locale_numeric_from_env();
        rt.numeric_decimal = locale_numeric::decimal_point_from_locale();
    }
    rt.init_argv(&files);
    apply_assigns(&args, &mut rt)?;
    if let Some(fs) = &args.field_sep {
        rt.vars
            .insert("FS".into(), Value::Str(String::from(fs.as_str())));
    }
    if args.csv {
        rt.csv_mode = true;
        rt.vars.insert("FS".into(), Value::Str(",".into()));
        // gawk reports this FPAT in CSV mode even though splitting is handled internally.
        rt.vars
            .insert("FPAT".into(), Value::Str("[^[:space:]]+".into()));
    }

    rt.slots = cp.init_slots(&rt.vars);

    vm_run_begin(cp.as_ref(), &mut rt)?;
    flush_print_buf(&mut rt.print_buf)?;
    if rt.exit_pending {
        vm_run_end(cp.as_ref(), &mut rt)?;
        flush_print_buf(&mut rt.print_buf)?;
        std::process::exit(rt.exit_code);
    }

    let mut range_state: Vec<bool> = vec![false; prog.rules.len()];

    // Parallel record mode: whole regular files in memory, or stdin in chunks of `--read-ahead` lines.
    let use_parallel_files = threads > 1 && parallel_ok && !files.is_empty();
    let stdin_parallel =
        files.is_empty() && threads > 1 && parallel_ok && !uses_primary_getline(cp.as_ref());
    if threads > 1 && !parallel_ok {
        eprintln!("{bin_name}: warning: program is not parallel-safe (range patterns, exit, getline without file, getline coprocess, cross-record assignments, …); running sequentially (use a single thread to silence this warning)");
    }

    let mut nr_global = 0.0f64;
    let chunk_lines = args.read_ahead.max(1);

    if files.is_empty() {
        rt.filename = "-".into();
        vm_run_beginfile(cp.as_ref(), &mut rt)?;
        if rt.exit_pending {
            vm_run_endfile(cp.as_ref(), &mut rt)?;
            vm_run_end(cp.as_ref(), &mut rt)?;
            std::process::exit(rt.exit_code);
        }
        if stdin_parallel {
            process_stdin_parallel(&cp, &mut rt, threads, chunk_lines)?;
        } else {
            process_file(None, &prog, cp.as_ref(), &mut range_state, &mut rt)?;
        }
        vm_run_endfile(cp.as_ref(), &mut rt)?;
    } else {
        for p in &files {
            rt.filename = p.to_string_lossy().into_owned();
            rt.fnr = 0.0;
            vm_run_beginfile(cp.as_ref(), &mut rt)?;
            if rt.exit_pending {
                vm_run_endfile(cp.as_ref(), &mut rt)?;
                vm_run_end(cp.as_ref(), &mut rt)?;
                std::process::exit(rt.exit_code);
            }
            let n = if use_parallel_files {
                process_file_parallel(Some(p.as_path()), &prog, &cp, &mut rt, threads, nr_global)?
            } else {
                process_file(
                    Some(p.as_path()),
                    &prog,
                    cp.as_ref(),
                    &mut range_state,
                    &mut rt,
                )?
            };
            nr_global += n as f64;
            vm_run_endfile(cp.as_ref(), &mut rt)?;
            if rt.exit_pending {
                break;
            }
        }
    }

    flush_print_buf(&mut rt.print_buf)?;
    vm_run_end(cp.as_ref(), &mut rt)?;
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

fn parallel_pool(threads: usize) -> Result<ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| Error::Runtime(format!("rayon pool: {e}")))
}

/// Run record rules for `lines` in parallel; `line_base` is the 0-based index of `lines[0]` within
/// the current input file, and `nr_offset` is global `NR` before the first line of that file.
#[allow(clippy::too_many_arguments)]
fn process_lines_parallel_chunk(
    pool: &ThreadPool,
    lines: Vec<String>,
    line_base: usize,
    nr_offset: f64,
    cp: &Arc<CompiledProgram>,
    fname: String,
    shared_globals: Arc<crate::runtime::AwkMap<String, Value>>,
    shared_slots: Arc<Vec<Value>>,
    seed_base: u64,
    numeric_dec: char,
    csv_mode: bool,
) -> Result<Vec<(usize, ParallelRecordOut)>> {
    let shared_cp = Arc::clone(cp);
    let results: Vec<std::result::Result<(usize, ParallelRecordOut), Error>> = pool.install(|| {
        lines
            .into_par_iter()
            .enumerate()
            .map(|(j, line)| {
                let i = line_base + j;
                let cp = Arc::clone(&shared_cp);
                let mut local = Runtime::for_parallel_worker(
                    Arc::clone(&shared_globals),
                    fname.clone(),
                    seed_base ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15),
                    numeric_dec,
                    csv_mode,
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
                    let run = vm_pattern_matches(rule, cp.as_ref(), &mut local)?;
                    if run {
                        match vm_run_rule(rule, cp.as_ref(), &mut local, Some(&mut buf)) {
                            Ok(Flow::Next) => break,
                            Ok(Flow::NextFile) => {
                                return Err(Error::Runtime(
                                    "`nextfile` cannot be used in parallel record mode".into(),
                                ));
                            }
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
    Ok(outs)
}

fn write_parallel_chunk_output(
    outs: &mut [(usize, ParallelRecordOut)],
    stdout: &mut impl Write,
    rt: &mut Runtime,
) -> Result<()> {
    outs.sort_by_key(|(i, _)| *i);
    for (_, out) in outs.iter() {
        for chunk in &out.prints {
            stdout.write_all(chunk.as_bytes()).map_err(Error::Io)?;
        }
    }
    for (_, out) in outs.iter() {
        if out.exit_pending {
            rt.exit_pending = true;
            rt.exit_code = out.exit_code;
            break;
        }
    }
    Ok(())
}

/// Chunked parallel stdin: buffer up to `chunk_lines` records per batch, process with rayon, emit in order.
fn process_stdin_parallel(
    cp: &Arc<CompiledProgram>,
    rt: &mut Runtime,
    threads: usize,
    chunk_lines: usize,
) -> Result<()> {
    let pool = parallel_pool(threads)?;
    let shared_globals = Arc::new(rt.vars.clone());
    let shared_slots = Arc::new(rt.slots.clone());
    let fname = rt.filename.clone();
    let seed_base = rt.rand_seed;
    let numeric_dec = rt.numeric_decimal;
    let csv_mode = rt.csv_mode;
    let stdin_nr_offset = rt.nr;

    let mut stdin = BufReader::new(std::io::stdin());
    let mut line_base = 0usize;
    let mut stdout = io::stdout().lock();

    loop {
        let mut chunk = Vec::with_capacity(chunk_lines);
        for _ in 0..chunk_lines {
            let mut s = String::new();
            let n = stdin.read_line(&mut s).map_err(Error::Io)?;
            if n == 0 {
                break;
            }
            chunk.push(s);
        }
        if chunk.is_empty() {
            break;
        }

        let mut outs = process_lines_parallel_chunk(
            &pool,
            chunk,
            line_base,
            stdin_nr_offset,
            cp,
            fname.clone(),
            Arc::clone(&shared_globals),
            Arc::clone(&shared_slots),
            seed_base,
            numeric_dec,
            csv_mode,
        )?;

        let n = outs.len();
        write_parallel_chunk_output(&mut outs, &mut stdout, rt)?;

        line_base += n;
        rt.nr = stdin_nr_offset + line_base as f64;
        rt.fnr = line_base as f64;

        if rt.exit_pending {
            break;
        }
    }
    Ok(())
}

fn mmap_file_readonly(path: &Path) -> Result<Mmap> {
    let file = File::open(path).map_err(|e| Error::ProgramFile(path.to_path_buf(), e))?;
    // SAFETY: read-only map of a file we opened; no concurrent writes assumed (same as `fs::read`).
    unsafe {
        memmap2::MmapOptions::new()
            .map(&file)
            .map_err(|e| Error::ProgramFile(path.to_path_buf(), e))
    }
}

/// Newline-split `data` into owned lines (same boundaries as the slurp loop; `\r` before `\n` trimmed).
fn split_bytes_into_owned_lines(data: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pos = 0usize;
    let len = data.len();
    while pos < len {
        let eol = memchr(b'\n', &data[pos..len])
            .map(|i| pos + i)
            .unwrap_or(len);
        let end = if eol > pos && data[eol - 1] == b'\r' {
            eol - 1
        } else {
            eol
        };
        lines.push(String::from_utf8_lossy(&data[pos..end]).into_owned());
        pos = eol + 1;
    }
    lines
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

/// Per-record workers run the bytecode VM (`vm_pattern_matches` / `vm_run_rule`), same as sequential mode.
/// Each worker gets `Arc::clone` of the shared program (O(1)) and a fresh `Runtime` (slots, VM stack, fields, print capture).
fn process_file_parallel(
    path: Option<&Path>,
    _prog: &Program,
    cp: &Arc<CompiledProgram>,
    rt: &mut Runtime,
    threads: usize,
    nr_offset: f64,
) -> Result<usize> {
    let lines = if let Some(p) = path {
        let mmap = mmap_file_readonly(p)?;
        split_bytes_into_owned_lines(mmap.as_ref())
    } else {
        read_all_lines(std::io::stdin())?
    };
    let nlines = lines.len();
    if nlines == 0 {
        return Ok(0);
    }

    let shared_globals = Arc::new(rt.vars.clone());
    let shared_slots = Arc::new(rt.slots.clone());
    let fname = rt.filename.clone();
    let seed_base = rt.rand_seed;
    let numeric_dec = rt.numeric_decimal;
    let csv_mode = rt.csv_mode;

    let pool = parallel_pool(threads)?;

    let mut outs = process_lines_parallel_chunk(
        &pool,
        lines,
        0,
        nr_offset,
        cp,
        fname,
        shared_globals,
        shared_slots,
        seed_base,
        numeric_dec,
        csv_mode,
    )?;

    let mut stdout = io::stdout().lock();
    write_parallel_chunk_output(&mut outs, &mut stdout, rt)?;

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
    AddMulFieldsToSlot {
        f1: u16,
        f2: u16,
        slot: u16,
    },
    ArrayFieldAddConst {
        arr: u32,
        field: u16,
        delta: f64,
    },
    PrintFieldSepField {
        f1: u16,
        sep: u32,
        f2: u16,
    },
    PrintThreeFieldsStdout {
        f1: u16,
        f2: u16,
        f3: u16,
    },
    /// `{ gsub("pat", "repl"); print }` on `$0` — literal pattern + simple replacement (pool indices).
    GsubLiteralPrint {
        pat: u32,
        repl: u32,
    },
}

/// Pattern for inline fast path.
#[derive(Clone)]
enum InlinePattern {
    Always,
    LiteralContains(String),
    /// `NR % modulus` compared to `eq_val` (numeric `==`), e.g. `NR % 2 == 0`.
    NrModEq {
        modulus: f64,
        eq_val: f64,
    },
}

fn match_nr_mod_eq_pattern(ops: &[Op]) -> Option<(f64, f64)> {
    if ops.len() != 5 {
        return None;
    }
    match (&ops[0], &ops[1], &ops[2], &ops[3], &ops[4]) {
        (Op::GetNR, Op::PushNum(m), Op::Mod, Op::PushNum(eq), Op::CmpEq) => Some((*m, *eq)),
        _ => None,
    }
}

#[inline]
fn awk_float_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= f64::EPSILON * 128.0 * a.abs().max(b.abs()).max(1.0)
}

fn set_record_from_line_bytes(rt: &mut Runtime, fs: &str, line_bytes: &[u8]) {
    match std::str::from_utf8(line_bytes) {
        Ok(line) => rt.set_field_sep_split(fs, line),
        Err(_) => {
            let lossy = String::from_utf8_lossy(line_bytes);
            rt.set_field_sep_split(fs, &lossy);
        }
    }
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
        CompiledPattern::Expr(chunk) => {
            let (m, eq) = match_nr_mod_eq_pattern(&chunk.ops)?;
            InlinePattern::NrModEq {
                modulus: m,
                eq_val: eq,
            }
        }
        _ => return None,
    };
    let ops = &rule.body.ops;
    let action = if ops.len() == 1 {
        match ops[0] {
            Op::PrintFieldStdout(f) => InlineAction::PrintFieldStdout(f),
            Op::AddFieldToSlot { field, slot } => InlineAction::AddFieldToSlot { field, slot },
            Op::AddMulFieldsToSlot { f1, f2, slot } => {
                InlineAction::AddMulFieldsToSlot { f1, f2, slot }
            }
            Op::ArrayFieldAddConst { arr, field, delta } => {
                InlineAction::ArrayFieldAddConst { arr, field, delta }
            }
            Op::PrintFieldSepField { f1, sep, f2 } => {
                InlineAction::PrintFieldSepField { f1, sep, f2 }
            }
            Op::PrintThreeFieldsStdout { f1, f2, f3 } => {
                InlineAction::PrintThreeFieldsStdout { f1, f2, f3 }
            }
            _ => return None,
        }
    } else if ops.len() == 3 {
        // PushNum(N) + CompoundAssignSlot(slot, Add) + Pop → AddConstToSlot
        if let (Op::PushNum(n), Op::CompoundAssignSlot(slot, crate::ast::BinOp::Add), Op::Pop) =
            (ops[0], ops[1], ops[2])
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
    } else if ops.len() == 5 {
        match (&ops[0], &ops[1], &ops[2], &ops[3], &ops[4]) {
            (
                Op::PushStr(pat_idx),
                Op::PushStr(repl_idx),
                Op::GsubFn(SubTarget::Record),
                Op::Pop,
                Op::Print {
                    argc: 0,
                    redir: RedirKind::Stdout,
                },
            ) => {
                let pat = cp.strings.get(*pat_idx);
                let repl = cp.strings.get(*repl_idx);
                if !pat.is_empty() && crate::builtins::gsub_literal_eligible(pat, repl) {
                    InlineAction::GsubLiteralPrint {
                        pat: *pat_idx,
                        repl: *repl_idx,
                    }
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    } else {
        return None;
    };

    if let CompiledPattern::Expr(_) = &rule.pattern {
        if !matches!(action, InlineAction::PrintFieldStdout(_)) {
            return None;
        }
    }

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
    let mmap = mmap_file_readonly(path)?;
    let data = mmap.as_ref();
    // Cache FS once (only changes if program assigns FS mid-execution, rare).
    let fs = rt
        .vars
        .get("FS")
        .map(|v| v.as_str())
        .unwrap_or_else(|| " ".into());

    // Try the inlined fast path for trivial single-rule programs.
    if let Some((pattern, action)) = detect_inline_program(cp) {
        return process_file_slurp_inline(data, &fs, pattern, action, cp, rt);
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
    data: &[u8],
    fs: &str,
    pattern: InlinePattern,
    action: InlineAction,
    cp: &CompiledProgram,
    rt: &mut Runtime,
) -> Result<usize> {
    // Ultra-fast path: for PrintFieldStdout with default FS=" " and field > 0,
    // skip field splitting + record copy entirely and scan bytes directly.
    // Only when the pattern matches every record (`Always`); `LiteralContains` must filter per line.
    if matches!(pattern, InlinePattern::Always) {
        if let InlineAction::GsubLiteralPrint { pat, repl } = action {
            return process_file_gsub_literal_print(data, fs, pat, repl, cp, rt);
        }
        if let InlineAction::PrintFieldStdout(field) = action {
            if field > 0 && fs == " " {
                return process_file_print_field_raw(data, field as usize, rt);
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

    let mut ofs_local = [0u8; 64];
    let ofs_len = rt.ofs_bytes.len().min(64);
    ofs_local[..ofs_len].copy_from_slice(&rt.ofs_bytes[..ofs_len]);

    let literal_finder = match &pattern {
        InlinePattern::LiteralContains(needle) if !needle.is_empty() => {
            Some(memmem::Finder::new(needle.as_bytes()))
        }
        _ => None,
    };

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

        match &pattern {
            InlinePattern::LiteralContains(_) => {
                if let Some(ref finder) = literal_finder {
                    if finder.find(line_bytes).is_none() {
                        pos = eol + 1;
                        continue;
                    }
                }
            }
            InlinePattern::NrModEq { modulus, eq_val } => {
                let rem = rt.nr % modulus;
                if !awk_float_eq(rem, *eq_val) {
                    pos = eol + 1;
                    continue;
                }
            }
            InlinePattern::Always => {}
        }

        match action {
            InlineAction::AddConstToSlot { val, slot } => {
                let sv = rt.slots[slot as usize].as_number();
                rt.slots[slot as usize] = Value::Num(sv + val as f64);
            }
            InlineAction::AddMulFieldsToSlot { f1, f2, slot } => {
                set_record_from_line_bytes(rt, fs, line_bytes);
                let p = rt.field_as_number(f1 as i32) * rt.field_as_number(f2 as i32);
                let old = rt.slots[slot as usize].as_number();
                rt.slots[slot as usize] = Value::Num(old + p);
            }
            InlineAction::ArrayFieldAddConst { arr, field, delta } => {
                set_record_from_line_bytes(rt, fs, line_bytes);
                let name = cp.strings.get(arr);
                let key = rt.field(field as i32).as_str();
                let old = rt.array_get(name, &key).as_number();
                rt.array_set(name, key, Value::Num(old + delta));
            }
            InlineAction::PrintFieldSepField { f1, sep, f2 } => {
                set_record_from_line_bytes(rt, fs, line_bytes);
                let sep_s = cp.strings.get(sep);
                rt.print_field_to_buf(f1 as usize);
                rt.print_buf.extend_from_slice(sep_s.as_bytes());
                rt.print_field_to_buf(f2 as usize);
                rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
            }
            InlineAction::PrintThreeFieldsStdout { f1, f2, f3 } => {
                set_record_from_line_bytes(rt, fs, line_bytes);
                rt.print_field_to_buf(f1 as usize);
                rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                rt.print_field_to_buf(f2 as usize);
                rt.print_buf.extend_from_slice(&ofs_local[..ofs_len]);
                rt.print_field_to_buf(f3 as usize);
                rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
            }
            InlineAction::PrintFieldStdout(field) => {
                set_record_from_line_bytes(rt, fs, line_bytes);
                rt.print_field_to_buf(field as usize);
                rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
            }
            InlineAction::AddFieldToSlot { field, slot } => {
                set_record_from_line_bytes(rt, fs, line_bytes);
                let fv = rt.field_as_number(field as i32);
                let sv = rt.slots[slot as usize].as_number();
                rt.slots[slot as usize] = Value::Num(sv + fv);
            }
            InlineAction::GsubLiteralPrint { .. } => {
                unreachable!("GsubLiteralPrint is handled in process_file_gsub_literal_print")
            }
        }

        pos = eol + 1;
    }
    Ok(count)
}

/// Slurp path for `{ gsub("needle", "repl"); print }` on `$0`: no VM, no record copy when the needle is absent.
fn process_file_gsub_literal_print(
    data: &[u8],
    fs: &str,
    pat: u32,
    repl: u32,
    cp: &CompiledProgram,
    rt: &mut Runtime,
) -> Result<usize> {
    let needle = cp.strings.get(pat);
    let repl_s = cp.strings.get(repl);
    debug_assert!(!needle.is_empty() && crate::builtins::gsub_literal_eligible(needle, repl_s));

    let finder = memmem::Finder::new(needle.as_bytes());
    let mut ors_local = [0u8; 64];
    let ors_len = rt.ors_bytes.len().min(64);
    ors_local[..ors_len].copy_from_slice(&rt.ors_bytes[..ors_len]);

    let mut count = 0usize;
    let mut pos = 0usize;
    let len = data.len();

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

        let no_match = needle.len() > line_bytes.len() || finder.find(line_bytes).is_none();

        if no_match {
            rt.print_buf.extend_from_slice(line_bytes);
            rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
        } else {
            set_record_from_line_bytes(rt, fs, line_bytes);
            crate::builtins::gsub(rt, needle, repl_s, None)?;
            rt.print_buf.extend_from_slice(rt.record.as_bytes());
            rt.print_buf.extend_from_slice(&ors_local[..ors_len]);
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
                Ok(Flow::NextFile) => return Ok(true),
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

#[cfg(test)]
mod lib_internal_tests {
    use super::*;
    use crate::bytecode::{GetlineSource, Op};
    use crate::compiler::Compiler;
    use crate::parser::parse_program;
    use std::io::Cursor;

    #[test]
    fn read_all_lines_splits_on_newline() {
        let lines = read_all_lines(Cursor::new(b"a\nb\n")).unwrap();
        assert_eq!(lines, vec!["a\n".to_string(), "b\n".to_string()]);
    }

    #[test]
    fn read_all_lines_empty_input() {
        let lines = read_all_lines(Cursor::new(b"")).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn split_bytes_into_owned_lines_matches_slurp_boundaries() {
        assert_eq!(
            split_bytes_into_owned_lines(b"a\nb\r\nc"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(split_bytes_into_owned_lines(b"").is_empty());
    }

    #[test]
    fn split_bytes_into_owned_lines_trailing_line_without_newline() {
        assert_eq!(
            split_bytes_into_owned_lines(b"only"),
            vec!["only".to_string()]
        );
        assert_eq!(
            split_bytes_into_owned_lines(b"a\nb"),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn split_bytes_into_owned_lines_lone_carriage_return_not_trimmed_as_crlf() {
        // `\r` alone is not the "CR before LF" case; line is preserved (lossy UTF-8).
        assert_eq!(split_bytes_into_owned_lines(b"a\rb"), vec!["a\rb".to_string()]);
    }

    #[test]
    fn read_all_lines_last_line_without_newline() {
        let lines = read_all_lines(Cursor::new(b"hello")).unwrap();
        assert_eq!(lines, vec!["hello".to_string()]);
    }

    #[test]
    fn read_all_lines_single_newline_only() {
        let lines = read_all_lines(Cursor::new(b"\n")).unwrap();
        assert_eq!(lines, vec!["\n".to_string()]);
    }

    #[test]
    fn awk_float_eq_close_values() {
        assert!(awk_float_eq(1.0, 1.0));
        assert!(awk_float_eq(1e-20, 1e-20));
        assert!(!awk_float_eq(1.0, 2.0));
    }

    #[test]
    fn awk_float_eq_negative_and_symmetric() {
        assert!(awk_float_eq(-3.0, -3.0));
        assert!(!awk_float_eq(-1.0, 1.0));
    }

    #[test]
    fn split_bytes_into_owned_lines_consecutive_newlines_yield_empty_records() {
        assert_eq!(
            split_bytes_into_owned_lines(b"\n\n"),
            vec!["".to_string(), "".to_string()]
        );
    }

    #[test]
    fn match_nr_mod_eq_pattern_matches_five_op_form() {
        let ops = vec![
            Op::GetNR,
            Op::PushNum(3.0),
            Op::Mod,
            Op::PushNum(1.0),
            Op::CmpEq,
        ];
        assert_eq!(match_nr_mod_eq_pattern(&ops), Some((3.0, 1.0)));
    }

    #[test]
    fn match_nr_mod_eq_pattern_wrong_len() {
        let ops = vec![Op::GetNR, Op::PushNum(2.0), Op::Mod];
        assert!(match_nr_mod_eq_pattern(&ops).is_none());
    }

    #[test]
    fn match_nr_mod_eq_pattern_wrong_ops() {
        let ops = vec![
            Op::PushNum(3.0),
            Op::GetNR,
            Op::Mod,
            Op::PushNum(1.0),
            Op::CmpEq,
        ];
        assert!(match_nr_mod_eq_pattern(&ops).is_none());
    }

    #[test]
    fn uses_primary_getline_detects_bare_getline() {
        let prog = parse_program("{ getline }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(uses_primary_getline(&cp));
    }

    #[test]
    fn uses_primary_getline_false_without_primary_getline() {
        let prog = parse_program("BEGIN { x = 1 }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!uses_primary_getline(&cp));
    }

    #[test]
    fn uses_primary_getline_file_redirect_not_primary() {
        let prog = parse_program("{ getline < \"/dev/null\" }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(!uses_primary_getline(&cp));
    }

    #[test]
    fn uses_primary_getline_scans_functions() {
        let prog = parse_program("function f(){ getline } BEGIN { f() }").unwrap();
        let cp = Compiler::compile_program(&prog);
        assert!(uses_primary_getline(&cp));
    }

    #[test]
    fn getline_source_tagged_in_bytecode() {
        let prog = parse_program("{ getline }").unwrap();
        let cp = Compiler::compile_program(&prog);
        let has_primary = cp.record_rules.iter().any(|r| {
            r.body.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::GetLine {
                        source: GetlineSource::Primary,
                        ..
                    }
                )
            })
        });
        assert!(has_primary);
    }
}
