//! Events sent from language servers to the UI.

use std::path::PathBuf;

use lsp_types::{Diagnostic, MessageType, WorkspaceEdit};

/// Lifecycle state of one server instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// The process was spawned and `initialize` is in flight.
    Starting,
    /// Initialized and idle.
    Running,
    /// Initialized and reporting work-done progress (e.g. "indexing").
    Busy(String),
    /// Shut down on request.
    Stopped,
    /// Could not start, or exited unexpectedly.
    Failed(String),
}

impl ServerStatus {
    /// True for [`ServerStatus::Running`] and [`ServerStatus::Busy`].
    pub fn is_running(&self) -> bool {
        matches!(self, ServerStatus::Running | ServerStatus::Busy(_))
    }
}

/// Something the UI may want to react to.
#[derive(Debug, Clone, PartialEq)]
pub enum LspEvent {
    /// `textDocument/publishDiagnostics`; replaces the server's set for `path`.
    Diagnostics {
        /// Server name.
        server: String,
        /// File the diagnostics belong to.
        path: PathBuf,
        /// Document version the diagnostics were computed for, if known.
        version: Option<i32>,
        /// The full set for this file from this server (may be empty).
        diagnostics: Vec<Diagnostic>,
    },
    /// The server changed state.
    ServerStatus {
        /// Server name.
        server: String,
        /// Project root the instance serves.
        root: PathBuf,
        /// New state.
        status: ServerStatus,
    },
    /// `$/progress` work-done progress.
    Progress {
        /// Server name.
        server: String,
        /// Progress token rendered as a string.
        token: String,
        /// Title from the `begin` message (repeated on later reports).
        title: Option<String>,
        /// Latest message, if any.
        message: Option<String>,
        /// Latest percentage (0-100), if the server reports one.
        percentage: Option<u32>,
        /// True on the `end` message.
        done: bool,
    },
    /// `window/logMessage` and lines the server wrote to stderr.
    Log {
        /// Server name.
        server: String,
        /// Severity.
        level: MessageType,
        /// Text.
        message: String,
    },
    /// `window/showMessage`: something the user should see.
    ShowMessage {
        /// Server name.
        server: String,
        /// Severity.
        typ: MessageType,
        /// Text.
        message: String,
    },
    /// `workspace/applyEdit`: the server wants the client to edit files.
    ApplyEdit {
        /// Server name.
        server: String,
        /// The requested edit.
        edit: WorkspaceEdit,
    },
}

/// Runtime-agnostic receiving end of the event channel (poll it from any
/// executor, e.g. a GPUI task).
pub type EventReceiver = async_channel::Receiver<LspEvent>;

/// Sending end of the event channel; cheap to clone, never blocks.
#[derive(Debug, Clone)]
pub struct EventSink(async_channel::Sender<LspEvent>);

impl EventSink {
    /// Creates an unbounded event channel.
    pub fn channel() -> (EventSink, EventReceiver) {
        let (tx, rx) = async_channel::unbounded();
        (EventSink(tx), rx)
    }

    /// Sends an event; silently drops it when the receiver is gone.
    pub fn send(&self, event: LspEvent) {
        let _ = self.0.try_send(event);
    }
}

/// How LSP `Position::character` counts columns, as negotiated with a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PositionEncoding {
    /// Bytes of UTF-8.
    Utf8,
    /// UTF-16 code units (the LSP default).
    #[default]
    Utf16,
    /// Unicode scalar values.
    Utf32,
}

impl PositionEncoding {
    /// Maps the server's `positionEncoding` capability; unknown or missing
    /// values mean UTF-16 per the specification.
    pub fn from_kind(kind: Option<&lsp_types::PositionEncodingKind>) -> Self {
        match kind.map(|k| k.as_str()) {
            Some("utf-8") => PositionEncoding::Utf8,
            Some("utf-32") => PositionEncoding::Utf32,
            _ => PositionEncoding::Utf16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::PositionEncodingKind;

    #[test]
    fn encoding_from_kind() {
        assert_eq!(PositionEncoding::from_kind(None), PositionEncoding::Utf16);
        let k = PositionEncodingKind::UTF8;
        assert_eq!(
            PositionEncoding::from_kind(Some(&k)),
            PositionEncoding::Utf8
        );
        let k = PositionEncodingKind::new("weird");
        assert_eq!(
            PositionEncoding::from_kind(Some(&k)),
            PositionEncoding::Utf16
        );
    }

    #[test]
    fn sink_ignores_closed_receiver() {
        let (tx, rx) = EventSink::channel();
        drop(rx);
        tx.send(LspEvent::Log {
            server: "s".into(),
            level: MessageType::LOG,
            message: "m".into(),
        });
    }

    #[test]
    fn running_states() {
        assert!(ServerStatus::Busy("x".into()).is_running());
        assert!(!ServerStatus::Failed("x".into()).is_running());
    }
}
