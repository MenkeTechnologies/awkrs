//! Native AOT for awkrs via `fusevm::aot` (`--aot OUT`).
//!
//! Compiles a `BEGIN`-only awk program to native machine code: the BEGIN actions
//! lower to a single fusevm chunk (functions appended as sub_chunks), which
//! `fusevm::aot` lowers to a relocatable object linked against the awkrs runtime
//! staticlib (`libawkrs.a`) into a standalone executable.
//!
//! Scope: BEGIN-only programs (the `awk 'BEGIN{…}'` calculator case — awk's
//! compute-bound sweet spot). Programs with per-record rules or `END` need the
//! Rust-driven record loop (`run_compiled_files`), which is multi-chunk and
//! outside the single-entry AOT model; those are rejected with a clear message.
#![allow(improper_ctypes, improper_ctypes_definitions)]

use crate::ast::{Pattern, Program};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, Ordering};

/// Leaked `'static` Runtime installed for the native run, flushed at exit.
static AOT_RT: AtomicPtr<crate::runtime::Runtime> = AtomicPtr::new(std::ptr::null_mut());

/// `atexit` handler: awk buffers output in `rt.print_buf` and flushes at the end
/// of the record loop; the single-entry AOT run has no post-run step, so we
/// flush here when the process exits.
extern "C" fn flush_print_buf() {
    let p = AOT_RT.load(Ordering::Acquire);
    if p.is_null() {
        return;
    }
    // SAFETY: `p` is the leaked 'static Runtime stored by the register hook.
    let rt = unsafe { &mut *p };
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(&rt.print_buf);
    let _ = out.flush();
}

/// Runtime hook invoked by `fusevm::aot::fusevm_aot_run_embedded` at startup:
/// stand up a leaked-`'static` awk `Runtime`, install it as the thread's
/// `CURRENT_RT`, install awkrs's AwkHost/regex host + builtins on the run VM, and
/// arrange the output flush at exit.
///
/// # Safety
/// `vm` is the live run VM passed by the fusevm runtime; borrowed only here.
#[no_mangle]
pub extern "C" fn fusevm_aot_register_builtins(vm: *mut fusevm::VM) {
    // SAFETY: the fusevm runtime hands us the live run VM for this call.
    let vm = unsafe { &mut *vm };
    let rt: &'static mut crate::runtime::Runtime =
        Box::leak(Box::new(crate::runtime::Runtime::new()));
    // BEGIN can read ARGV/ARGC; seed from argv (BEGIN-only reads no records).
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    rt.init_argv(&files);
    AOT_RT.store(rt as *mut crate::runtime::Runtime, Ordering::Release);
    // Install permanently (one-shot process): forget the guard so CURRENT_RT
    // stays set for the whole native run.
    std::mem::forget(crate::fusevm_host::RuntimeGuard::enter(rt));
    crate::fusevm_host::install_awk_host(vm);
    // SAFETY: registering a plain extern "C" fn with libc atexit.
    unsafe {
        libc::atexit(flush_print_buf);
    }
}

fn is_begin_only(prog: &Program) -> bool {
    prog.rules
        .iter()
        .all(|r| matches!(r.pattern, Pattern::Begin))
}

/// Locate the awkrs runtime staticlib. `AWKRS_AOT_RUNTIME_LIB` overrides;
/// otherwise look for `libawkrs.a` beside the running executable.
fn runtime_staticlib() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("AWKRS_AOT_RUNTIME_LIB") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(dir) = exe.parent() {
        let cand = dir.join("libawkrs.a");
        if cand.exists() {
            return Ok(cand);
        }
    }
    Err("could not locate libawkrs.a (set AWKRS_AOT_RUNTIME_LIB)".to_string())
}

/// `awkrs --aot OUT <program>`: AOT-compile a BEGIN-only awk program to native
/// machine code and link a standalone executable.
pub fn build_native(program_text: &str, out_path: &Path) -> Result<PathBuf, String> {
    let prog = crate::parser::parse_program(program_text)
        .map_err(|e| format!("awkrs --aot: parse: {e}"))?;
    if !is_begin_only(&prog) {
        return Err("awkrs --aot: only BEGIN-only programs are supported \
                    (per-record rules / END need the record-loop driver)"
            .to_string());
    }
    let chunk = crate::fusevm_compile::compile_begin_only(&prog)
        .map_err(|e| format!("awkrs --aot: compile: {e}"))?;
    if chunk.ops.is_empty() {
        return Err("awkrs --aot: program compiled to an empty chunk".to_string());
    }

    let runtime_lib = runtime_staticlib()?;
    if !runtime_lib.exists() {
        return Err(format!(
            "awkrs --aot: runtime staticlib not found at {}",
            runtime_lib.display()
        ));
    }

    let obj = out_path.with_extension("o");
    fusevm::aot::compile_object(&chunk, &obj).map_err(|e| format!("awkrs --aot: {e}"))?;

    let stub = out_path.with_extension("aot_main.c");
    std::fs::write(
        &stub,
        b"extern long fusevm_aot_run_embedded(void);\nint main(void){return (int)fusevm_aot_run_embedded();}\n" as &[u8],
    )
    .map_err(|e| format!("awkrs --aot: write entry stub: {e}"))?;

    let mut cmd = std::process::Command::new("cc");
    cmd.arg(&stub).arg(&obj).arg(&runtime_lib);
    if cfg!(target_os = "macos") {
        cmd.arg("-framework").arg("CoreFoundation");
    }
    cmd.arg("-o").arg(out_path);
    let status = cmd
        .status()
        .map_err(|e| format!("awkrs --aot: invoking cc: {e}"))?;
    let _ = std::fs::remove_file(&stub);
    let _ = std::fs::remove_file(&obj);
    if !status.success() {
        return Err(format!(
            "awkrs --aot: link failed (cc exit {:?})",
            status.code()
        ));
    }
    Ok(out_path.to_path_buf())
}
