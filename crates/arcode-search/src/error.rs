//! Error type for search, replace and scanning.

use std::fmt;
use std::path::PathBuf;

/// Result alias used throughout the crate.
pub type Result<T, E = SearchError> = std::result::Result<T, E>;

/// Everything that can go wrong before or during a search.
#[derive(Debug)]
pub enum SearchError {
    /// The search pattern is empty.
    EmptyPattern,
    /// The pattern is not a valid regular expression (message from the
    /// regex engine, suitable for showing to the user).
    InvalidPattern(String),
    /// An include/exclude glob is malformed.
    InvalidGlob { glob: String, message: String },
    /// The search root does not exist or is not readable.
    InvalidRoot(PathBuf),
    /// The file is not valid UTF-8 and cannot be rewritten safely.
    NotUtf8(PathBuf),
    /// The file looks binary (contains NUL bytes).
    Binary(PathBuf),
    /// I/O failure on a specific file.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl SearchError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => f.write_str("search pattern is empty"),
            Self::InvalidPattern(msg) => write!(f, "invalid regular expression: {msg}"),
            Self::InvalidGlob { glob, message } => write!(f, "invalid glob `{glob}`: {message}"),
            Self::InvalidRoot(p) => write!(f, "cannot search {}: not a readable path", p.display()),
            Self::NotUtf8(p) => write!(f, "{} is not valid UTF-8", p.display()),
            Self::Binary(p) => write!(f, "{} is a binary file", p.display()),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for SearchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
