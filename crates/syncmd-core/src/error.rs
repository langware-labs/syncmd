//! Error types and the syncmd exit-code contract.

use std::path::PathBuf;

/// The library error. Each variant maps to a CLI exit code via [`Error::exit_code`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `<path>` is not inside a git work tree. Exit 2.
    #[error("not a git repository: {0} (run `git init` if you want tracking)")]
    NotARepo(PathBuf),

    /// `<path>` does not exist or does not resolve inside the repo. Exit 2.
    #[error("path not found or outside repository: {0}")]
    BadPath(PathBuf),

    /// `syncmd.toml` could not be parsed. Exit 2.
    #[error("invalid configuration: {0}")]
    BadConfig(String),

    /// At least one group could not be resolved (strategy = error / interactive abort). Exit 1.
    #[error("unresolved conflict in {count} group(s)")]
    Conflict { count: usize },

    /// Filesystem error. Exit 3.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// Git invocation / object read error. Exit 3.
    #[error("git error: {0}")]
    Git(String),
}

impl Error {
    /// The process exit code this error should produce.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::NotARepo(_) | Error::BadPath(_) | Error::BadConfig(_) => 2,
            Error::Conflict { .. } => 1,
            Error::Io(_) | Error::Git(_) => 3,
        }
    }
}

/// Convenience result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
