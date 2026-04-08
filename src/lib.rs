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
    vm_pattern_matches, vm_run_begin, vm_run_beginfile, vm_run_end, vm_run_endfile, vm_run_rule,
};
use clap::Parser;
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
    let threads = args.threads.unwrap_or_else(num_cpus::get).max(1);

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

    vm_run_begin(&cp, &mut rt)?;
    if rt.exit_pending {
        vm_run_end(&cp, &mut rt)?;
        std::process::exit(rt.exit_code);
    }

    let mut range_state: Vec<bool> = vec![false; prog.rules.len()];

    // Parallel record mode only reads regular files fully; stdin is always streamed line-by-line.
    let use_parallel = threads > 1 && parallel_ok && !files.is_empty();
    if threads > 1 && !parallel_ok {
        eprintln!("{bin_name}: warning: program is not parallel-safe (range patterns, exit, getline without file, getline coprocess, cross-record assignments, …); running sequentially (use -j 1 to silence)");
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
                process_file_parallel(
                    Some(p.as_path()),
                    &prog,
                    &cp,
                    &mut rt,
                    threads,
                    nr_global,
                )?
            } else {
                process_file(
                    Some(p.as_path()),
                    &prog,
                    &cp,
                    &mut range_state,
                    &mut rt,
                )?
            };
            nr_global += n as f64;
            vm_run_endfile(&cp, &mut rt)?;
            if rt.exit_pending {
                break;
            }
        }
    }

    vm_run_end(&cp, &mut rt)?;
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

fn process_file(
    path: Option<&Path>,
    prog: &Program,
    cp: &CompiledProgram,
    range_state: &mut [bool],
    rt: &mut Runtime,
) -> Result<usize> {
    let reader: Box<dyn Read + Send> = if let Some(p) = path {
        Box::new(File::open(p).map_err(|e| Error::ProgramFile(p.to_path_buf(), e))?)
    } else {
        Box::new(std::io::stdin())
    };
    let br = Arc::new(std::sync::Mutex::new(BufReader::new(reader)));
    rt.attach_input_reader(Arc::clone(&br));

    let mut count = 0usize;
    loop {
        let mut line = String::new();
        let n = br
            .lock()
            .map_err(|_| Error::Runtime("input reader lock poisoned".into()))?
            .read_line(&mut line)
            .map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        count += 1;
        rt.nr += 1.0;
        rt.fnr += 1.0;
        rt.set_record_from_line(&line);

        for rule in &cp.record_rules {
            let run = match &rule.pattern {
                CompiledPattern::Range => {
                    let orig = &prog.rules[rule.original_index];
                    if let Pattern::Range(p1, p2) = &orig.pattern {
                        range_step(
                            &mut range_state[rule.original_index],
                            p1,
                            p2,
                            rt,
                            prog,
                        )?
                    } else {
                        false
                    }
                }
                _ => vm_pattern_matches(rule, cp, rt)?,
            };
            if run {
                match vm_run_rule(rule, cp, rt, None) {
                    Ok(Flow::Next) => break,
                    Ok(Flow::ExitPending) => {
                        rt.detach_input_reader();
                        return Ok(count);
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
        if rt.exit_pending {
            break;
        }
    }
    rt.detach_input_reader();
    Ok(count)
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
