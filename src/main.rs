mod ast;
mod builtins;
mod cli;
mod error;
mod interp;
mod lexer;
mod parser;
mod runtime;

use crate::ast::{Pattern, Program};
use crate::cli::{Args, MawkWAction};
use crate::error::{Error, Result};
use crate::interp::{pattern_matches, range_step, run_begin, run_end, run_rule_on_record, Flow};
use crate::parser::parse_program;
use crate::runtime::{Runtime, Value};
use clap::{CommandFactory, Parser};
use std::cell::RefCell;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::rc::Rc;

fn main() {
    match run() {
        Ok(()) => {}
        Err(Error::Exit(code)) => std::process::exit(code),
        Err(e) => {
            eprintln!("awkrs: {e}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<()> {
    let mut args = Args::parse();
    args.normalize();
    match args.apply_mawk_w() {
        Ok(()) => {}
        Err(MawkWAction::Help) => {
            let mut cmd = Args::command();
            let _ = cmd.print_help();
            return Ok(());
        }
        Err(MawkWAction::Version) => {
            println!("awkrs {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    }
    if args.copyright {
        println!(
            "awkrs {} — Copyright (c) MenkeTechnologies; MIT license.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.dump_variables.is_some() {
        eprintln!("awkrs: warning: --dump-variables is not fully implemented");
    }
    if args.debug.is_some() {
        eprintln!("awkrs: warning: --debug is not fully implemented");
    }
    let threads = args.threads.unwrap_or_else(num_cpus::get);
    let _ = threads;

    let (program_text, files) = resolve_program_and_files(&args)?;
    let prog = parse_program(&program_text)?;

    let mut rt = Runtime::new();
    apply_assigns(&args, &mut rt)?;
    if let Some(fs) = &args.field_sep {
        rt.vars.insert("FS".into(), Value::Str(String::from(fs.as_str())));
    }

    run_begin(&prog, &mut rt)?;
    if rt.exit_pending {
        run_end(&prog, &mut rt)?;
        std::process::exit(rt.exit_code);
    }

    let record_rule_indices: Vec<usize> = prog
        .rules
        .iter()
        .enumerate()
        .filter(|(_, r)| !matches!(r.pattern, Pattern::Begin | Pattern::End))
        .map(|(i, _)| i)
        .collect();

    let mut range_state: Vec<bool> = vec![false; prog.rules.len()];

    if files.is_empty() {
        process_file(
            None,
            &prog,
            &record_rule_indices,
            &mut range_state,
            &mut rt,
        )?;
    } else {
        for p in &files {
            rt.filename = p.to_string_lossy().into_owned();
            rt.fnr = 0.0;
            process_file(
                Some(p.as_path()),
                &prog,
                &record_rule_indices,
                &mut range_state,
                &mut rt,
            )?;
            if rt.exit_pending {
                break;
            }
        }
    }

    run_end(&prog, &mut rt)?;
    if rt.exit_pending {
        std::process::exit(rt.exit_code);
    }
    Ok(())
}

fn process_file(
    path: Option<&std::path::Path>,
    prog: &Program,
    record_rule_indices: &[usize],
    range_state: &mut [bool],
    rt: &mut Runtime,
) -> Result<()> {
    let reader: Box<dyn Read> = if let Some(p) = path {
        Box::new(
            File::open(p).map_err(|e| Error::ProgramFile(p.to_path_buf(), e))?,
        )
    } else {
        Box::new(std::io::stdin())
    };
    let br = Rc::new(RefCell::new(BufReader::new(reader)));
    rt.attach_input_reader(br.clone());

    loop {
        let mut line = String::new();
        let n = br.borrow_mut().read_line(&mut line).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        rt.nr += 1.0;
        rt.fnr += 1.0;
        rt.set_record_from_line(&line);

        for &idx in record_rule_indices {
            let rule = &prog.rules[idx];
            let run = match &rule.pattern {
                Pattern::Range(p1, p2) => {
                    range_step(&mut range_state[idx], p1, p2, rt, prog)?
                }
                pat => pattern_matches(pat, rt, prog)?,
            };
            if run {
                match run_rule_on_record(prog, rt, idx) {
                    Ok(Flow::Next) => break,
                    Ok(Flow::ExitPending) => {
                        rt.detach_input_reader();
                        return Ok(());
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
    Ok(())
}

fn resolve_program_and_files(args: &Args) -> Result<(String, Vec<PathBuf>)> {
    let mut prog = String::new();
    for p in &args.include {
        prog.push_str(
            &std::fs::read_to_string(p).map_err(|e| Error::ProgramFile(p.clone(), e))?,
        );
    }
    for p in &args.progfiles {
        prog.push_str(
            &std::fs::read_to_string(p).map_err(|e| Error::ProgramFile(p.clone(), e))?,
        );
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
        let (name, val) = a
            .split_once('=')
            .ok_or_else(|| Error::Parse {
                line: 1,
                msg: format!("invalid -v `{a}`, expected name=value"),
            })?;
        rt.vars
            .insert(name.to_string(), Value::Str(val.to_string()));
    }
    Ok(())
}
