//! Embedding entry point: run an awk program from a host and capture its output.

use crate::compiler::Compiler;
use crate::error::{Error, Result};
use crate::parser::parse_program;
use crate::runtime::Runtime;
use crate::vm::{vm_run_begin, vm_run_beginfile, vm_run_end, EMBEDDED};
use std::io::{BufReader, Cursor, Read};
use std::sync::{Arc, Mutex};

/// Run `src` as a complete program with `input` as its stdin and return
/// everything it wrote to stdout.
///
/// `input` is the primary input stream: the record loop and plain `getline`
/// read it, and `RS`/`FS` set in `BEGIN` apply to it as on the command line.
/// Each call runs on its own thread (with the interpreter's 64 MiB stack) and
/// a fresh `Runtime`; stdout is held in that runtime's print buffer, so
/// concurrent calls never share output and nothing reaches the process stdout.
///
/// Parse, compile and runtime errors, a nonzero `exit` status, and a panic in
/// the interpreter all return `Err`. `exit` / `exit 0` returns `Ok` with the
/// output printed so far (`END` still runs, as in awk).
///
/// The stdout of `system()` and `print | "cmd"` children is captured too,
/// placed where it would appear on the CLI (awk flushes before `system()`, on
/// opening an output pipe, and before `close`), and a `system()` child gets
/// a null stdin. `getline < "/dev/stdin"` (or `"-"`) reads `input` through the same
/// cursor as the record loop. `print > "/dev/stderr"` and child stderr go to the
/// process stderr.
pub fn run_program(src: &str, input: &str) -> std::result::Result<String, String> {
    let src = src.to_owned();
    let input = input.as_bytes().to_vec();
    std::thread::Builder::new()
        .name("awkrs-embed".into())
        .stack_size(crate::INTERP_STACK_BYTES)
        .spawn(move || {
            EMBEDDED.with(|c| c.set(true));
            run_captured(&src, input).map_err(|e| e.to_string())
        })
        .map_err(|e| format!("spawn interpreter thread: {e}"))?
        .join()
        .unwrap_or_else(|panic| Err(panic_message(panic.as_ref())))
}

fn run_captured(src: &str, input: Vec<u8>) -> Result<String> {
    let prog = parse_program(src)?;
    let cp = Compiler::compile_program(&prog)?;

    let mut rt = Runtime::new();
    rt.slots = cp.init_slots(&rt.vars);
    rt.symtab_slot_map = cp.slot_map.clone();
    rt.refresh_special_arrays(&cp, "awkrs");
    let reader = Box::new(Cursor::new(input)) as Box<dyn Read + Send>;
    rt.attach_input_reader(Arc::new(Mutex::new(BufReader::new(reader))));
    rt.filename = "-".into();

    vm_run_begin(&cp, &mut rt)?;
    // Same rule as the CLI: a program of only `BEGIN` actions reads no input.
    let reads_input = !cp.record_rules.is_empty()
        || !cp.end_chunks.is_empty()
        || !cp.beginfile_chunks.is_empty()
        || !cp.endfile_chunks.is_empty();
    if reads_input && !rt.exit_pending {
        // As on the CLI, what `BEGIN` printed counts as flushed before input is
        // read; child output from here on lands after it.
        crate::vm::flush_print_buf(&mut rt.print_buf)?;
        let skip = !std::mem::take(&mut rt.beginfile_ran) && vm_run_beginfile(&cp, &mut rt)?;
        // `input_reader` is gone once a `getline` in `BEGIN` drained `input`;
        // the record loop would then fall back to the process stdin.
        if !skip && !rt.exit_pending && rt.input_reader.is_some() {
            let mut range_state = vec![false; prog.rules.len()];
            crate::process_file_records(None, &cp, &mut range_state, &mut rt)?;
        }
        crate::run_endfile_once(&cp, &mut rt)?;
    }
    rt.detach_input_reader();
    vm_run_end(&cp, &mut rt)?;
    rt.close_output_redirects();

    if rt.exit_code != 0 {
        return Err(Error::Exit(rt.exit_code));
    }
    Ok(String::from_utf8_lossy(&rt.print_buf).into_owned())
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    let msg = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic");
    format!("interpreter panicked: {msg}")
}
