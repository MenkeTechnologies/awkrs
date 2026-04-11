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

#[cfg(test)]
mod tests {
    use super::Flow;

    #[test]
    fn flow_variants_distinct() {
        assert!(matches!(Flow::Normal, Flow::Normal));
        assert!(!matches!(Flow::Next, Flow::NextFile));
        assert!(matches!(Flow::ExitPending, Flow::ExitPending));
    }

    #[test]
    fn flow_debug_includes_variant_name() {
        let s = format!("{:?}", Flow::NextFile);
        assert!(s.contains("NextFile"), "{s}");
    }

    #[test]
    fn flow_exhaustive_variant_names_in_debug() {
        for (f, needle) in [
            (Flow::Normal, "Normal"),
            (Flow::Next, "Next"),
            (Flow::NextFile, "NextFile"),
            (Flow::ExitPending, "ExitPending"),
        ] {
            let s = format!("{f:?}");
            assert!(s.contains(needle), "{s}");
        }
    }
}
