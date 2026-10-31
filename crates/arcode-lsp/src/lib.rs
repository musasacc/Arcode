//! Async Language Server Protocol client for Arcode.
//!
//! - [`transport`]: `Content-Length` framing.
//! - [`uri`]: path <-> `file://` URI conversion.
//! - [`LspClient`]: one server process, JSON-RPC over stdio, one tokio task
//!   set per server.
//! - [`DocumentSync`]: turns edits into the `didChange` form a server wants.
//! - [`LspManager`]: the UI-facing registry. It owns its own tokio runtime,
//!   so the UI (GPUI, its own executor) never has to be inside tokio; events
//!   arrive on a runtime-agnostic [`EventReceiver`].
//! - [`DiagnosticStore`]: merged diagnostics from several servers.
//!
//! The crate does not depend on GPUI and performs no network access.

#![warn(missing_docs)]

pub mod capabilities;
mod client;
pub mod diagnostics;
mod error;
mod event;
mod features;
mod manager;
mod memory;
mod rpc;
pub mod sync;
pub mod transport;
pub mod uri;

pub use client::{
    DEFAULT_REQUEST_TIMEOUT, INITIALIZE_TIMEOUT, LspClient, SHUTDOWN_TIMEOUT, ServerSpec,
    kill_all_servers, kill_servers_on_signals,
};
pub use diagnostics::{DiagnosticCounts, DiagnosticStore, source_label};
pub use error::{
    CONTENT_MODIFIED, INVALID_PARAMS, LspError, METHOD_NOT_FOUND, REQUEST_CANCELLED, Result,
};
pub use event::{
    EditOutcome, EditReply, EventReceiver, EventSink, LspEvent, PositionEncoding, ServerStatus,
};
pub use features::Features;
pub use lsp_types;
pub use manager::{Answer, LspManager, ServerConfig, find_root};
pub use sync::{DocumentSync, TextChange};
pub use uri::{path_to_uri, uri_to_path};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_thread_safe() {
        fn check<T: Send + Sync>() {}
        check::<LspManager>();
        check::<LspClient>();
        check::<EventSink>();
        check::<EventReceiver>();
        check::<DiagnosticStore>();
        check::<LspError>();
    }
}
