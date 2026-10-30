//! Error type shared by the client and the manager.

use std::fmt;
use std::time::Duration;

/// JSON-RPC error code: the method is not known to the receiver.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC error code: invalid method parameters.
pub const INVALID_PARAMS: i64 = -32602;
/// LSP error code: the request was cancelled by the client.
pub const REQUEST_CANCELLED: i64 = -32800;
/// LSP error code: the document changed while the request was processed.
pub const CONTENT_MODIFIED: i64 = -32801;

/// Everything that can go wrong while talking to a language server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspError {
    /// Spawning the server process or reading/writing its pipes failed.
    Io(String),
    /// The server binary could not be found.
    NotFound(String),
    /// A request did not get a response in time.
    Timeout(Duration),
    /// The server is not running (it exited, crashed or was shut down).
    ServerStopped(String),
    /// The server answered with a JSON-RPC error.
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// Human-readable message from the server.
        message: String,
    },
    /// A message could not be serialized or deserialized.
    Protocol(String),
    /// No running server for the document supports the request.
    NoServer,
}

impl LspError {
    /// True when the server reported that the request was cancelled or the
    /// content changed; the UI usually ignores these silently.
    pub fn is_cancellation(&self) -> bool {
        matches!(
            self,
            LspError::Rpc { code, .. } if *code == REQUEST_CANCELLED || *code == CONTENT_MODIFIED
        )
    }
}

impl fmt::Display for LspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LspError::Io(msg) => write!(f, "I/O error: {msg}"),
            LspError::NotFound(msg) => write!(f, "{msg}"),
            LspError::Timeout(d) => write!(f, "request timed out after {:.1}s", d.as_secs_f64()),
            LspError::ServerStopped(msg) => write!(f, "language server stopped: {msg}"),
            LspError::Rpc { code, message } => write!(f, "server error {code}: {message}"),
            LspError::Protocol(msg) => write!(f, "protocol error: {msg}"),
            LspError::NoServer => write!(f, "no running language server supports this request"),
        }
    }
}

impl std::error::Error for LspError {}

impl From<std::io::Error> for LspError {
    fn from(err: std::io::Error) -> Self {
        LspError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for LspError {
    fn from(err: serde_json::Error) -> Self {
        LspError::Protocol(err.to_string())
    }
}

/// Result alias used throughout the crate.
pub type Result<T, E = LspError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_codes_are_detected() {
        let e = LspError::Rpc {
            code: CONTENT_MODIFIED,
            message: "x".into(),
        };
        assert!(e.is_cancellation());
        assert!(!LspError::NoServer.is_cancellation());
    }

    #[test]
    fn display_is_readable() {
        let e = LspError::Rpc {
            code: METHOD_NOT_FOUND,
            message: "nope".into(),
        };
        assert_eq!(e.to_string(), "server error -32601: nope");
    }
}
