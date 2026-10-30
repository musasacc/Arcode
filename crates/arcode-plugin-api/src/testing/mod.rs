//! Native fakes for every host import, so plugin logic is tested with a
//! plain `cargo test` (docs/PLUGINS.md §8).
//!
//! On non-wasm targets the SDK routes every import to a thread-local
//! [`FakeHost`]. Tests arrange state with [`with`], call plugin code, then
//! inspect what the plugin did through [`with`] again. Each test thread has
//! its own fake, so tests stay independent; call [`reset`] to start over.
//!
//! ```
//! use arcode_plugin::{host, testing};
//! testing::reset();
//! host::log(host::Level::Info, "hi");
//! assert_eq!(testing::with(|h| h.log.len()), 1);
//! ```

pub(crate) mod imports;
mod text;

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::bindings::arcode::plugin::editor::DocInfo;
use crate::bindings::arcode::plugin::http::{HttpError, Request, Response};
use crate::bindings::arcode::plugin::kv::Scope;
use crate::bindings::arcode::plugin::types::{DocId, Level, PanelId, Position, Range};
use crate::bindings::arcode::plugin::workspace::FsError;
use crate::ui::FlatView;

/// Answers `http.send` in tests.
pub type HttpResponder = Box<dyn FnMut(&Request) -> Result<Response, HttpError>>;

/// A document known to the fake editor.
#[derive(Debug, Clone, PartialEq)]
pub struct FakeDocument {
    /// What `document-info` returns; `version` and `line-count` are kept in
    /// sync by `apply-edits`.
    pub info: DocInfo,
    /// Full text.
    pub text: String,
    /// What `selections` returns.
    pub selections: Vec<Range>,
}

/// A panel opened through the fake `ui` imports.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FakePanel {
    /// Manifest-local contribution id.
    pub contribution: String,
    /// Current title.
    pub title: String,
    /// The view as the host would hold it after every render and patch.
    pub view: FlatView,
    /// Number of full `render` calls.
    pub renders: usize,
    /// Every `apply-patches` call, in order.
    pub patch_calls: Vec<Vec<crate::ui::Patch>>,
    /// True after `close-panel`.
    pub closed: bool,
}

/// State behind every faked import. All fields are public for arranging and
/// asserting.
#[derive(Default)]
pub struct FakeHost {
    /// `host.log` calls.
    pub log: Vec<(Level, String)>,
    /// `host.notify` calls.
    pub notifications: Vec<(Level, String)>,
    /// Last `host.set-status`: (text, command).
    pub status: Option<(Option<String>, Option<String>)>,
    /// Answers for `host.quick-pick`, consumed front to back; empty = escape.
    pub quick_pick_answers: Vec<Option<u32>>,
    /// Titles of every `host.quick-pick`.
    pub quick_picks: Vec<String>,
    /// JSON returned by `host.settings` (empty means `{}`).
    pub settings: String,
    /// Returned by `host.cancelled`.
    pub cancelled: bool,
    /// Whether the current call is a user gesture (`begin-task` succeeds).
    pub gesture: bool,
    /// Titles passed to a successful `host.begin-task`.
    pub tasks: Vec<String>,
    /// kv store per scope.
    pub kv: BTreeMap<(Scope, String), Vec<u8>>,
    /// Returned by `workspace.root-name`.
    pub root_name: Option<String>,
    /// Returned by `workspace.find-files` (not filtered by the glob, capped
    /// by `limit`) unless `files_error` is set.
    pub files: Vec<String>,
    /// Makes `workspace.find-files` fail.
    pub files_error: Option<FsError>,
    /// Documents by id.
    pub documents: BTreeMap<DocId, FakeDocument>,
    /// Returned by `editor.current-document`.
    pub current_document: Option<DocId>,
    /// `editor.open-file` calls.
    pub opened_files: Vec<(String, Option<Position>)>,
    /// `editor.open-scratch` calls: (title, language, text).
    pub scratches: Vec<(String, String, String)>,
    /// Every `http.send` request.
    pub requests: Vec<Request>,
    /// Answers `http.send`; `None` answers `denied`.
    pub http: Option<HttpResponder>,
    /// Panels by id.
    pub panels: BTreeMap<PanelId, FakePanel>,
}

impl FakeHost {
    /// Adds a document and returns its id.
    pub fn add_document(&mut self, path: Option<&str>, language: &str, text: &str) -> DocId {
        let id = self.documents.keys().next_back().map_or(1, |last| last + 1);
        let info = DocInfo {
            id,
            path: path.map(str::to_owned),
            language: language.to_owned(),
            version: 1,
            line_count: text::line_count(text),
        };
        self.documents.insert(
            id,
            FakeDocument {
                info,
                text: text.to_owned(),
                selections: Vec::new(),
            },
        );
        id
    }

    /// The single open panel of `contribution`, if any.
    pub fn panel(&self, contribution: &str) -> Option<&FakePanel> {
        self.panels
            .values()
            .find(|p| p.contribution == contribution && !p.closed)
    }
}

thread_local! {
    static FAKE: RefCell<FakeHost> = RefCell::new(FakeHost::default());
}

/// Runs `f` with this thread's fake host.
///
/// # Panics
/// If called re-entrantly from inside `f` (a test bug, not runtime data).
pub fn with<R>(f: impl FnOnce(&mut FakeHost) -> R) -> R {
    FAKE.with(|cell| f(&mut cell.borrow_mut()))
}

/// Resets this thread's fake host to its empty state.
pub fn reset() {
    with(|h| *h = FakeHost::default());
}

#[cfg(test)]
mod tests;
