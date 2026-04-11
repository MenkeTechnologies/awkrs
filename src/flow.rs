//! Control flow from executing a rule action (returned by the VM).

/// Result of running a rule body until a control-flow effect visible to the record loop.
#[derive(Debug)]
pub enum Flow {
    Normal,
    Next,
    /// Skip to the next input file (invalid in `BEGIN`/`END`/`BEGINFILE`/`ENDFILE`).
    NextFile,
    /// POSIX: run `END`, then exit with `Runtime.exit_code`.
    ExitPending,
}
