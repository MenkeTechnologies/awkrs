use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error at line {line}: {msg}")]
    Parse { line: usize, msg: String },
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("cannot read program file {0:?}: {1}")]
    ProgramFile(PathBuf, std::io::Error),
    /// `exit` was evaluated (propagated from functions / expressions).
    #[error("exit {0}")]
    Exit(i32),
}

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
}
