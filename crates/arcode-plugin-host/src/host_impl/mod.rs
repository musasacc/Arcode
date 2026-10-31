//! Host implementations of the `arcode:plugin` imports (§6). Each function
//! first checks the capability and the gesture, then does its work.
//!
//! - `host`: log, notify (5 per minute), status item, quick pick, live
//!   settings, cancellation, tasks.
//! - `kv`: key rules and quota; scopes read lazily from
//!   `plugin-data/<id>/*.kv` and written once per call.
//! - `workspace`: grant check, glob validation and limit; the paths come
//!   from [`crate::bridge::ProjectIndex`] ([`crate::index::WalkIndex`] by
//!   default).
//! - `editor`: snapshot reads, `context`/`all-open` rules, version and range
//!   checks before write consent.
//! - `http`: gesture, master switch, manifest hosts and per-host consent;
//!   the transfer is [`crate::bridge::HttpTransport`] ([`crate::net`]).
//! - `ui`: panel bookkeeping and [`crate::ui_tree::PanelTree`] validation.

mod editor;
mod host;
pub mod http;
pub mod kv;
mod ui;
mod workspace;

use crate::bindings::arcode::plugin::types;
use crate::state::HostState;

impl types::Host for HostState {}

/// Truncates `text` to at most `max` bytes on a char boundary.
pub(crate) fn cap(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    text
}
