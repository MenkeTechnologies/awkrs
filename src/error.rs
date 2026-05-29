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
    /// `exit` was evaluated (propagated from functions / expressions).
    #[error("exit {0}")]
    Exit(i32),
}
/// `Result` type alias.

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::Error;
    use std::path::PathBuf;

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
