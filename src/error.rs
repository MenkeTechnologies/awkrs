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
