use std::path::PathBuf;
use thiserror::Error;
/// `Error` — see variants for the choices.
#[derive(Debug, Error)]
pub enum Error {
    /// `Io` variant.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Parse-time error with source-line context.
    #[error("parse error at line {line}: {msg}")]
    Parse {
        /// 1-based source line number where the parse fault was hit.
        line: usize,
        /// Human-readable diagnostic ("expected `}`, found `;`" etc.).
        msg: String,
    },
    /// `Runtime` variant.
    #[error("runtime error: {0}")]
    Runtime(String),
    /// `ProgramFile` variant.
    #[error("cannot read program file {0:?}: {1}")]
    ProgramFile(PathBuf, std::io::Error),
    /// Failure opening an input data file (positional arg after the program).
    /// Phrased like gawk's "cannot open file ... for reading" to keep error
    /// messages consistent across implementations.
    #[error("cannot open file {0:?} for reading: {1}")]
    InputFile(PathBuf, std::io::Error),
    /// Rejected by `validate_program` before any rule ran: a builtin called with
    /// the wrong number of arguments, `break`/`continue` outside a loop, a
    /// parenthesized comma list where one is not allowed. gawk reports all of
    /// these while parsing, so they exit 1 like a syntax error rather than 2
    /// like a fault. Carries no line number because the AST does not record one.
    #[error("{0}")]
    Validate(String),
    /// `exit` was evaluated (propagated from functions / expressions).
    #[error("exit {0}")]
    Exit(i32),
}
impl Error {
    /// Re-tag a `validate_program` rejection as the parse-time diagnostic it
    /// is, so it exits 1 rather than 2.
    ///
    /// The validator builds its messages as [`Error::Runtime`] because that is
    /// the only free-text variant; nothing it reports can actually reach the
    /// runtime, since it runs before compilation. Other variants pass through.
    #[must_use]
    pub fn into_validate(self) -> Self {
        match self {
            Error::Runtime(msg) => Error::Validate(msg),
            other => other,
        }
    }

    /// Process exit status this error should produce, matching the reference awks.
    ///
    /// * **2** — every *fatal* condition: runtime faults (`1/0`, calling an
    ///   undefined function), an unreadable `-f` program file, an input file that
    ///   cannot be opened, and I/O failures on output redirection. gawk, mawk and
    ///   one-true-awk all exit 2 for these.
    /// * **1** — parse diagnostics, including [`Error::Validate`] rejections,
    ///   which gawk also reports before running the program. Here the references
    ///   disagree (gawk 1, mawk and one-true-awk 2); awkrs follows gawk.
    ///
    /// [`Error::Exit`] carries the program's own status and never reaches this.
    pub fn exit_status(&self) -> i32 {
        match self {
            Error::Parse { .. } | Error::Validate(_) => 1,
            Error::Exit(code) => *code,
            Error::Io(_) | Error::Runtime(_) | Error::ProgramFile(..) | Error::InputFile(..) => 2,
        }
    }
}

/// `Result` type alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::Error;
    use std::path::PathBuf;

    /// gawk reports a `validate_program` rejection while parsing and exits 1;
    /// a fault that reaches the runtime exits 2. Before `Error::Validate`
    /// existed the validator borrowed `Error::Runtime` and every rejection
    /// exited 2, so `awk 'BEGIN { substr() }'` disagreed with gawk on status
    /// even once it stopped running the program.
    #[test]
    fn validate_exits_one_and_runtime_exits_two() {
        assert_eq!(Error::Validate("0 is invalid".into()).exit_status(), 1);
        assert_eq!(Error::Runtime("division by zero".into()).exit_status(), 2);
        assert_eq!(
            Error::Parse {
                line: 1,
                msg: "x".into()
            }
            .exit_status(),
            1
        );
    }

    /// `into_validate` re-tags only the free-text variant the validator uses;
    /// anything else must survive unchanged, or a genuine fault would start
    /// exiting 1.
    #[test]
    fn into_validate_retags_runtime_and_leaves_others_alone() {
        assert!(matches!(
            Error::Runtime("m".into()).into_validate(),
            Error::Validate(m) if m == "m"
        ));
        assert!(matches!(
            Error::InputFile(PathBuf::from("f"), std::io::Error::other("x")).into_validate(),
            Error::InputFile(..)
        ));
        assert!(matches!(Error::Exit(3).into_validate(), Error::Exit(3)));
    }

    /// The message carries no "runtime error:" prefix, because it is not one.
    #[test]
    fn validate_displays_the_bare_message() {
        assert_eq!(
            Error::Validate("0 is invalid as number of arguments for sin".into()).to_string(),
            "0 is invalid as number of arguments for sin"
        );
    }

    #[test]
    fn parse_error_display_includes_line_and_message() {
        let e = Error::Parse {
            line: 3,
            msg: "expected token".into(),
        };
        let s = e.to_string();
        assert!(s.contains('3') && s.contains("expected token"), "{s}");
    }

    #[test]
    fn runtime_error_display() {
        let e = Error::Runtime("bad op".into());
        assert_eq!(e.to_string(), "runtime error: bad op");
    }

    #[test]
    fn exit_error_display() {
        let e = Error::Exit(7);
        assert_eq!(e.to_string(), "exit 7");
    }

    #[test]
    fn program_file_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        let e = Error::ProgramFile(PathBuf::from("/no/such/file"), io_err);
        let s = e.to_string();
        assert!(s.contains("no/such") && s.contains("cannot read"), "{s}");
    }

    #[test]
    fn io_error_from_std_io_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "eacces");
        let e: Error = io_err.into();
        let s = e.to_string();
        assert!(s.contains("I/O") && s.contains("eacces"), "{s}");
    }

    #[test]
    fn exit_error_negative_code_display() {
        let e = Error::Exit(-1);
        assert_eq!(e.to_string(), "exit -1");
    }

    #[test]
    fn exit_error_zero_display() {
        assert_eq!(Error::Exit(0).to_string(), "exit 0");
    }

    #[test]
    fn io_error_wrapped_keeps_source_chain() {
        use std::error::Error as _;
        let inner = std::io::Error::other("inner");
        let e: Error = inner.into();
        assert!(e.source().is_some());
    }

    #[test]
    fn exit_error_large_positive_code_display() {
        let e = Error::Exit(i32::MAX);
        let s = e.to_string();
        assert!(s.contains(&i32::MAX.to_string()), "{s}");
    }

    #[test]
    fn parse_error_no_line_number_format() {
        // If line is 0, does it display correctly?
        let e = Error::Parse {
            line: 0,
            msg: "err".into(),
        };
        assert!(e.to_string().contains("line 0"));
    }

    #[test]
    fn runtime_error_empty_msg() {
        let e = Error::Runtime("".into());
        assert_eq!(e.to_string(), "runtime error: ");
    }

    #[test]
    fn vm_error_format_v2() {
        let e = Error::Runtime("stack overflow".into());
        assert!(e.to_string().contains("runtime error: stack overflow"));
    }

    #[test]
    fn parse_error_with_long_msg_v2() {
        let msg = "a".repeat(100);
        let e = Error::Parse {
            line: 1,
            msg: msg.clone(),
        };
        assert!(e.to_string().contains(&msg));
    }

    #[test]
    fn io_error_format_v2() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let e = Error::Io(inner);
        assert!(e.to_string().contains("I/O error: not found"));
    }

    #[test]
    fn program_file_error_format_v2() {
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e = Error::ProgramFile(PathBuf::from("script.awk"), inner);
        assert!(e
            .to_string()
            .contains("cannot read program file \"script.awk\": denied"));
    }

    #[test]
    fn exit_error_format_v2() {
        let e = Error::Exit(1);
        assert_eq!(e.to_string(), "exit 1");
    }

    #[test]
    fn result_alias_usage_v2() {
        let r: crate::error::Result<i32> = Ok(1);
        assert!(matches!(r, Ok(1)));
    }

    #[test]
    fn result_alias_error_v2() {
        let r: crate::error::Result<i32> = Err(Error::Runtime("err".into()));
        assert!(r.is_err());
    }

    #[test]
    fn error_debug_format_v2() {
        let e = Error::Runtime("err".into());
        let s = format!("{:?}", e);
        assert!(s.contains("Runtime"));
    }

    #[test]
    fn error_display_io_v3() {
        let e = Error::Io(std::io::Error::other("ioerr"));
        assert_eq!(format!("{e}"), "I/O error: ioerr");
    }

    #[test]
    fn error_display_parse_v3() {
        let e = Error::Parse {
            line: 10,
            msg: "msg".into(),
        };
        assert_eq!(format!("{e}"), "parse error at line 10: msg");
    }

    #[test]
    fn error_display_runtime_v3() {
        let e = Error::Runtime("runerr".into());
        assert_eq!(format!("{e}"), "runtime error: runerr");
    }

    #[test]
    fn error_display_program_file_v3() {
        let e = Error::ProgramFile(PathBuf::from("f.awk"), std::io::Error::other("f-err"));
        assert!(format!("{e}").contains("cannot read program file \"f.awk\""));
    }

    #[test]
    fn error_display_exit_v3() {
        let e = Error::Exit(42);
        assert_eq!(format!("{e}"), "exit 42");
    }

    #[test]
    fn error_from_io_v2() {
        let io = std::io::Error::other("raw");
        let e: Error = io.into();
        assert!(matches!(e, Error::Io(_)));
    }

    #[test]
    fn error_is_std_error_v2() {
        let e = Error::Runtime("err".into());
        let _s: &dyn std::error::Error = &e;
    }

    #[test]
    fn error_display_io_inner_v2() {
        let e = Error::Io(std::io::Error::other("inner_err"));
        assert!(e.to_string().contains("inner_err"));
    }

    #[test]
    fn error_display_runtime_v21() {
        assert!(format!("{}", Error::Runtime("a".into())).contains("runtime error: a"));
    }
    #[test]
    fn error_display_parse_v21() {
        assert!(format!(
            "{}",
            Error::Parse {
                line: 1,
                msg: "b".into()
            }
        )
        .contains("parse error at line 1: b"));
    }
    #[test]
    fn error_display_programfile_v21() {
        assert!(format!(
            "{}",
            Error::ProgramFile("f".into(), std::io::Error::other("c"))
        )
        .contains("cannot read program file \"f\""));
    }
    #[test]
    fn error_display_exit_v21() {
        assert_eq!(format!("{}", Error::Exit(1)), "exit 1");
    }
}
