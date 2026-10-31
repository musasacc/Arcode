//! Completion popup, snippet choices and signature help.
//!
//! Items come from a [`CompletionProvider`] (the app's language server
//! bridge) merged with local sources: words of the open buffers (only when
//! the provider has nothing or there is none), file paths inside string
//! literals and snippets (built-in plus the user's `snippets.toml`). They
//! are ranked with `arcode_completion::Ranker`.
//!
//! Requests never block typing. They run as tasks after
//! `editor.completion.delay_ms`; the popup keeps showing (re-filtered) items
//! while a newer list loads, and each response carries the number of its
//! request: anything but the latest is dropped.

mod accept;
mod choices;
pub(crate) mod code_highlight;
pub(crate) mod docs;
mod docs_view;
mod keys;
mod kind;
mod menu;
mod request;
mod session;
mod signature;
mod signature_view;
pub(crate) mod snippets;
mod view;
mod words;

use std::any::Any;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

use arcode_buffer::Point;
use arcode_completion::Ranker;
use gpui::{App, Context, Entity, Task};

use crate::Editor;

pub use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate, InsertAction};
pub use docs::{Block, Inline, InlineStyle, parse_markdown};
pub(crate) use docs_view::DocsView;
pub(crate) use session::{Pending, Session};
pub(crate) use signature::SignatureState;
pub use snippets::{
    SnippetStore, load_user_snippets, reload_user_snippets, take_user_snippets_error,
    user_snippets_error,
};
pub(crate) use view::MenuView;
pub(crate) use words::{BufferWords, register_editor};

/// Popover key of the completion list.
pub const COMPLETION_POPOVER: &str = "completion";
/// Popover key of the signature help.
pub const SIGNATURE_POPOVER: &str = "signature-help";

/// Documentation text of an item or signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Documentation {
    /// Shown as is.
    PlainText(String),
    /// Rendered as styled text (headings, emphasis, code).
    Markdown(String),
}

impl Documentation {
    /// The raw text.
    pub fn text(&self) -> &str {
        match self {
            Documentation::PlainText(t) | Documentation::Markdown(t) => t,
        }
    }
}

/// Where accepting an item edits, in buffer coordinates at the time the
/// list arrived. Text typed at the cursor since then is included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemEdit {
    /// Replaced when inserting (Enter, Tab): usually the typed word.
    pub insert: Range<Point>,
    /// Replaced in replace mode (Shift+Enter): usually the whole word.
    pub replace: Range<Point>,
}

impl ItemEdit {
    /// The same range for both modes.
    pub fn new(range: Range<Point>) -> Self {
        ItemEdit {
            insert: range.clone(),
            replace: range,
        }
    }
}

/// One completion item: a ranked candidate plus how to apply it.
#[derive(Clone)]
pub struct CompletionItem {
    /// Label, filter and sort text, kind, detail and the text to insert.
    pub candidate: CompletionCandidate,
    /// Explicit range to replace; `None` replaces the word before the
    /// cursor.
    pub edit: Option<ItemEdit>,
    /// More edits applied with the item (e.g. an auto-import). They must
    /// not overlap the main edit.
    pub additional_edits: Vec<(Range<Point>, String)>,
    /// Typing one of these while the item is selected accepts it first.
    pub commit_characters: Vec<String>,
    /// Shown in the side panel.
    pub documentation: Option<Documentation>,
    /// Drawn struck through.
    pub deprecated: bool,
    /// A command to run after accepting (e.g.
    /// `editor.action.triggerParameterHints`).
    pub command: Option<String>,
    /// The provider can fill in documentation or additional edits later
    /// ([`CompletionProvider::resolve`]).
    pub needs_resolve: bool,
    /// Provider-private data (e.g. the raw LSP item for resolving).
    pub provider_data: Option<Rc<dyn Any>>,
}

impl CompletionItem {
    /// An item that replaces the typed word with the candidate's text.
    pub fn new(candidate: CompletionCandidate) -> Self {
        CompletionItem {
            candidate,
            edit: None,
            additional_edits: Vec::new(),
            commit_characters: Vec::new(),
            documentation: None,
            deprecated: false,
            command: None,
            needs_resolve: false,
            provider_data: None,
        }
    }
}

impl fmt::Debug for CompletionItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletionItem")
            .field("label", &self.candidate.label)
            .field("kind", &self.candidate.kind)
            .field("edit", &self.edit)
            .field("additional_edits", &self.additional_edits)
            .field("needs_resolve", &self.needs_resolve)
            .finish_non_exhaustive()
    }
}

/// A provider's answer.
#[derive(Debug, Clone, Default)]
pub struct CompletionList {
    /// The items, unranked.
    pub items: Vec<CompletionItem>,
    /// More typing should ask again instead of only filtering.
    pub is_incomplete: bool,
}

/// Why completions are requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionTrigger {
    /// Typing a word or the manual shortcut.
    Invoked,
    /// One of the provider's trigger characters (e.g. `.`).
    Character(String),
    /// Typing while an incomplete list is shown.
    Incomplete,
}

/// Characters that open or update signature help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureTriggers {
    /// Open it (e.g. `(` and `,`).
    pub trigger: Vec<String>,
    /// Update it while open (e.g. `)`).
    pub retrigger: Vec<String>,
}

impl Default for SignatureTriggers {
    fn default() -> Self {
        SignatureTriggers {
            trigger: vec!["(".into(), ",".into()],
            retrigger: vec![")".into()],
        }
    }
}

/// One parameter of a signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInfo {
    /// Char range of the parameter inside the signature label.
    pub label: Range<usize>,
    /// Its documentation.
    pub documentation: Option<Documentation>,
}

/// One callable signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureInfo {
    /// The full signature, e.g. `fn add(a: i32, b: i32) -> i32`.
    pub label: String,
    /// Its documentation.
    pub documentation: Option<Documentation>,
    /// The parameters.
    pub parameters: Vec<ParameterInfo>,
    /// Highlighted parameter (overrides [`SignatureHelp::active_parameter`]).
    pub active_parameter: Option<usize>,
}

/// Signature help for the call around the cursor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignatureHelp {
    /// Overloads; at least one.
    pub signatures: Vec<SignatureInfo>,
    /// The one shown first.
    pub active_signature: usize,
    /// Highlighted parameter of every signature without its own.
    pub active_parameter: Option<usize>,
}

/// A signature help request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignatureRequest {
    /// The typed trigger character, if any.
    pub trigger_character: Option<String>,
    /// Whether help was already shown (typing inside the call).
    pub is_retrigger: bool,
    /// The help shown now.
    pub active: Option<SignatureHelp>,
}

/// Something that supplies completions for an editor, such as a language
/// server. Calls happen on the main thread; long work belongs in the
/// returned task. `None` means the provider cannot answer now (e.g. no
/// server runs), so local sources are used right away.
pub trait CompletionProvider {
    /// Characters after which completion opens (e.g. `.` and `::`).
    fn trigger_characters(&self, _editor: &Editor, _cx: &App) -> Vec<String> {
        Vec::new()
    }

    /// Characters that open or update signature help.
    fn signature_triggers(&self, _editor: &Editor, _cx: &App) -> SignatureTriggers {
        SignatureTriggers::default()
    }

    /// Completions at the primary cursor.
    fn completions(
        &self,
        editor: &Editor,
        trigger: &CompletionTrigger,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<CompletionList>>>;

    /// Fills in the documentation, detail or additional edits of an item
    /// with [`CompletionItem::needs_resolve`].
    fn resolve(
        &self,
        _editor: &Editor,
        _item: &CompletionItem,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<CompletionItem>>> {
        None
    }

    /// Signature help at the primary cursor; `Ok(None)` closes it.
    fn signature_help(
        &self,
        _editor: &Editor,
        _request: &SignatureRequest,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<SignatureHelp>>>> {
        None
    }
}

/// Completion state of one editor.
#[derive(Default)]
pub(crate) struct CompletionState {
    pub provider: Option<Rc<dyn CompletionProvider>>,
    /// The list being shown.
    pub session: Option<Session>,
    /// The request in flight (its answer replaces `session`).
    pub pending: Option<Pending>,
    /// Number of the latest request; older answers are dropped.
    pub seq: u64,
    pub request_task: Option<Task<()>>,
    pub resolve_task: Option<Task<()>>,
    /// Session id and item index of the resolve in flight.
    pub resolving: Option<(u64, usize)>,
    pub view: Option<Entity<MenuView>>,
    pub docs_view: Option<Entity<DocsView>>,
    pub ranker: Ranker,
    pub words: BufferWords,
    /// Word start and word whose complete answer had nothing; typing on
    /// in that word does not ask again.
    pub no_results: Option<(usize, String)>,
    pub signature: SignatureState,
}

/// Whether `c` continues an identifier.
pub(crate) fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// The completion settings.
pub(crate) fn settings(cx: &App) -> arcode_config::CompletionSettings {
    arcode_ui_kit::settings(cx).editor.completion.clone()
}

/// Sets up the snippet store (call once at startup).
pub(crate) fn init(cx: &mut App) {
    snippets::init(cx);
}
