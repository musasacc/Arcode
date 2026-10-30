//! Host implementations of the `arcode:plugin` imports (§6). Each function
//! first checks the capability and the gesture, then does its work.
//!
//! Status per plan step:
//! - `host`: complete (step 3/4: quick pick, settings, cancel, tasks,
//!   notify rate limit).
//! - `kv`: validation and quota, kept in memory per plugin; persistence to
//!   `plugin-data/<id>/*.kv` is step 6.
//! - `workspace`: grant check, glob validation and limit; the
//!   `.gitignore`-aware index behind [`crate::bridge::ProjectIndex`] is
//!   the app's (step 6 tests it against a temp project).
//! - `editor`: snapshot reads, `context`/`all-open` rules, write consent.
//! - `http`: gesture, master switch, manifest hosts and per-host consent;
//!   the transfer itself is [`crate::bridge::HttpTransport`] (ureq: step 7).
//! - `ui`: panel bookkeeping and size caps; `PanelTree` validation and
//!   snapshots are step 8.

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
