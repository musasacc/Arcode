//! Error type of the crate.

use std::fmt;
use std::time::Duration;

/// Everything that can go wrong while talking to a debug adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DapError {
    /// The adapter executable could not be found; the text says what to
    /// install (e.g. "codelldb not found — install the Rust language pack").
    NotFound(String),
    /// Spawning, connecting, reading or writing failed.
    Io(String),
    /// A request did not get a response in time.
    Timeout {
        /// Command of the request.
        command: String,
        /// How long we waited.
        after: Duration,
    },
    /// The adapter answered `success: false`.
    Adapter {
        /// Command of the request.
        command: String,
        /// Error text from the adapter.
        message: String,
    },
    /// The session or adapter is gone (exited, crashed, stopped).
    SessionEnded(String),
    /// A message could not be serialized or deserialized.
    Protocol(String),
    /// The adapter or the session does not support the operation.
    Unsupported(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, DapError>;

impl fmt::Display for DapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DapError::NotFound(msg) => write!(f, "{msg}"),
            DapError::Io(msg) => write!(f, "I/O error: {msg}"),
            DapError::Timeout { command, after } => {
                write!(f, "{command} timed out after {:.1}s", after.as_secs_f64())
            }
            DapError::Adapter { command, message } => write!(f, "{command} failed: {message}"),
            DapError::SessionEnded(msg) => write!(f, "debug session ended: {msg}"),
            DapError::Protocol(msg) => write!(f, "protocol error: {msg}"),
            DapError::Unsupported(msg) => write!(f, "not supported: {msg}"),
        }
    }
}

impl std::error::Error for DapError {}

impl From<serde_json::Error> for DapError {
    fn from(e: serde_json::Error) -> Self {
        DapError::Protocol(e.to_string())
    }
}

impl From<std::io::Error> for DapError {
    fn from(e: std::io::Error) -> Self {
        DapError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        let e = DapError::Timeout {
            command: "threads".into(),
            after: Duration::from_millis(1500),
        };
        assert_eq!(e.to_string(), "threads timed out after 1.5s");
        let e = DapError::NotFound("codelldb not found — install the Rust language pack".into());
        assert_eq!(
            e.to_string(),
            "codelldb not found — install the Rust language pack"
        );
        let e = DapError::Adapter {
            command: "evaluate".into(),
            message: "no symbol".into(),
        };
        assert_eq!(e.to_string(), "evaluate failed: no symbol");
    }
}
