//! One error type for the crate (`N-9`).
//!
//! Every fallible path returns this instead of panicking. A harness that panics
//! mid-batch cannot reconcile on restart (`L-7`), so a panic is a defect here,
//! not a shortcut.

use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    /// An IO failure, carrying the path it happened to.
    Io { path: PathBuf, source: std::io::Error },
    /// The binding is missing, malformed, or names something that is not there.
    Unbound { key: String, reason: String },
    /// The journal is not readable as JSON.
    Json { at: usize, reason: String },
    /// A journal line parsed as JSON but is not a record.
    Record { line: usize, reason: String },
    /// A step id is not `c<cycle>/<stage>/s<seq>`.
    Step { text: String, reason: String },
}

impl Error {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Error::Io { path: path.as_ref().to_path_buf(), source }
    }

    pub fn unbound(key: impl Into<String>, reason: impl Into<String>) -> Self {
        Error::Unbound { key: key.into(), reason: reason.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Unbound { key, reason } => write!(f, "binding key `{key}`: {reason}"),
            Error::Json { at, reason } => write!(f, "invalid JSON at byte {at}: {reason}"),
            Error::Record { line, reason } => write!(f, "journal line {line}: {reason}"),
            Error::Step { text, reason } => write!(f, "step id `{text}`: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
