//! Language features that edit or annotate code: inlay hints, code actions
//! (light bulb and menu), rename and formatting, plus the editor's context
//! menu.
//!
//! The editor knows nothing about language servers. A [`LanguageServices`]
//! provider (the app's LSP bridge) answers with plain editor types in
//! buffer [`Point`]s, and applies what may touch other files (code actions
//! and renames, which carry workspace edits) itself.

mod action_rows;
mod actions;
mod context_menu;
mod format;
mod inlay;
mod notice;
mod rename;
mod rename_field;

use std::any::Any;
use std::future::Future;
use std::ops::Range;
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;

use arcode_buffer::Point;
use gpui::{App, Context, Task, Window};

use crate::Editor;

pub use action_rows::{ActionGroup, action_group};
pub use inlay::{InlayHintsToggle, inlay_hints_enabled};

/// Popover key of the code action menu.
pub const CODE_ACTIONS_POPOVER: &str = "code-actions";
/// Popover key of the context menu.
pub const CONTEXT_MENU_POPOVER: &str = "context-menu";
/// Popover key of the inline rename field.
pub const RENAME_POPOVER: &str = "rename";
/// Popover key of short notices ("No code actions available").
pub const NOTICE_POPOVER: &str = "notice";
/// Virtual text set of the inlay hints.
pub const INLAY_HINT_SET: &str = "inlay-hints";

/// What an inlay hint describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InlayHintKind {
    /// A type (`: i32` after a binding).
    Type,
    /// A parameter name (`count:` before an argument).
    Parameter,
    /// Anything else (chaining hints, closing-brace labels).
    #[default]
    Other,
}

/// A hint drawn inside the code (not part of the buffer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    /// Drawn before the char at this position.
    pub position: Point,
    /// The text.
    pub label: String,
    /// Type, parameter or other.
    pub kind: InlayHintKind,
    /// A space before the label.
    pub padding_left: bool,
    /// A space after the label.
    pub padding_right: bool,
}

/// A code action offered for a range (a quick fix, a refactoring, a source
/// action).
#[derive(Clone)]
pub struct CodeAction {
    /// Menu label.
    pub title: String,
    /// Hierarchical kind such as `quickfix`, `refactor.extract` or
    /// `source.organizeImports` (`None` for plain commands).
    pub kind: Option<String>,
    /// The server marked it as the preferred fix.
    pub preferred: bool,
    /// Why it cannot be applied now (shown greyed out).
    pub disabled: Option<String>,
    /// Provider data needed to apply it (the raw LSP action).
    pub data: Option<Rc<dyn Any>>,
}

impl std::fmt::Debug for CodeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeAction")
            .field("title", &self.title)
            .field("kind", &self.kind)
            .field("preferred", &self.preferred)
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl CodeAction {
    /// An enabled action with `title` and `kind`.
    pub fn new(title: impl Into<String>, kind: Option<&str>) -> Self {
        CodeAction {
            title: title.into(),
            kind: kind.map(str::to_string),
            preferred: false,
            disabled: None,
            data: None,
        }
    }
}

/// Where a rename edits and the text the field starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    /// The symbol's range.
    pub range: Range<Point>,
    /// The current name.
    pub placeholder: String,
}

/// Text edits in buffer points (applied as one undo step).
pub type TextEdits = Vec<(Range<Point>, String)>;

/// Supplies language features (usually a language server). Calls happen on
/// the main thread; long work belongs in the returned task. `None` means
/// the provider cannot do it for this document right now.
pub trait LanguageServices {
    /// Inlay hints for `lines` (end exclusive).
    fn inlay_hints(
        &self,
        _editor: &Editor,
        _lines: Range<usize>,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<InlayHint>>>> {
        None
    }

    /// Code actions for `range` (the selection, or the cursor).
    fn code_actions(
        &self,
        _editor: &Editor,
        _range: Range<Point>,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<CodeAction>>>> {
        None
    }

    /// Applies `action`: its edit (possibly across files) and/or command.
    fn apply_code_action(
        &self,
        _editor: &Editor,
        _action: CodeAction,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<()>>> {
        None
    }

    /// The symbol a rename at `at` would change. `Ok(None)`: it cannot be
    /// renamed. Returning `None` (no prepare step) renames the word at `at`.
    fn prepare_rename(
        &self,
        _editor: &Editor,
        _at: Point,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<RenameTarget>>>> {
        None
    }

    /// Renames the symbol at `at` to `new_name` everywhere.
    fn rename(
        &self,
        _editor: &Editor,
        _at: Point,
        _new_name: String,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<()>>> {
        None
    }

    /// Formatting edits for the document (`range` = `None`) or a range.
    fn format(
        &self,
        _editor: &Editor,
        _range: Option<Range<Point>>,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<TextEdits>>> {
        None
    }
}

/// Per-editor state of the language features.
#[derive(Default)]
pub(crate) struct LangState {
    pub provider: Option<Rc<dyn LanguageServices>>,
    pub inlay: inlay::InlayState,
    pub actions: actions::ActionsState,
    pub rename: rename::RenameState,
    pub format: format::FormatState,
}

impl Editor {
    /// Sets (or removes) the provider of inlay hints, code actions, rename
    /// and formatting.
    pub fn set_language_services(
        &mut self,
        provider: Option<Rc<dyn LanguageServices>>,
        cx: &mut Context<Self>,
    ) {
        self.lang.provider = provider;
        self.inlay_reset(cx);
        self.bulb_schedule(cx);
    }

    /// Whether a language services provider is set.
    pub fn has_language_services(&self) -> bool {
        self.lang.provider.is_some()
    }

    /// Bookkeeping after an edit: hints and the light bulb are asked again,
    /// menus, the rename field and notices close.
    pub(crate) fn lang_after_edits(&mut self, cx: &mut Context<Self>) {
        // The listed actions were computed for the old text.
        self.hide_popover(CODE_ACTIONS_POPOVER, cx);
        self.hide_popover(CONTEXT_MENU_POPOVER, cx);
        self.close_transient_popovers(cx);
        self.inlay_after_edit(cx);
        self.bulb_schedule(cx);
    }

    /// Bookkeeping after the cursor moved without an edit.
    pub(crate) fn lang_after_cursor_move(&mut self, cx: &mut Context<Self>) {
        self.close_transient_popovers(cx);
        self.bulb_schedule(cx);
    }

    /// Called on render: menus, the rename field and notices close when
    /// the editor loses focus.
    pub(crate) fn lang_check_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.focus_handle.is_focused(window) {
            for key in [
                CODE_ACTIONS_POPOVER,
                CONTEXT_MENU_POPOVER,
                RENAME_POPOVER,
                NOTICE_POPOVER,
            ] {
                if self.has_popover(key) {
                    self.hide_popover(key, cx);
                }
            }
            self.lang.rename.field = None;
            self.lang.rename.target = None;
        }
    }

    /// A click in the editor closes menus and the rename field.
    pub(crate) fn lang_mouse_down(&mut self, cx: &mut Context<Self>) {
        self.hide_popover(CODE_ACTIONS_POPOVER, cx);
        self.hide_popover(CONTEXT_MENU_POPOVER, cx);
        self.close_transient_popovers(cx);
    }

    /// Closes the rename field and notices.
    fn close_transient_popovers(&mut self, cx: &mut Context<Self>) {
        if self.lang.rename.field.is_some() {
            self.close_rename(cx);
        }
        self.lang.rename.task = None;
        self.hide_popover(NOTICE_POPOVER, cx);
    }
}

/// Resolves to `Some(output)` of `fut`, or `None` once `timeout` finished
/// first.
pub(crate) async fn with_timeout<T>(
    fut: impl Future<Output = T>,
    timeout: impl Future<Output = ()>,
) -> Option<T> {
    let mut fut = pin!(fut);
    let mut timeout = pin!(timeout);
    std::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

/// Registers the global state the features need.
pub(crate) fn init(cx: &mut App) {
    inlay::init(cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: Future>(fut: F) -> F::Output {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut fut = pin!(fut);
        loop {
            if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
                return out;
            }
        }
    }

    #[test]
    fn timeout_wins_only_when_the_work_is_pending() {
        assert_eq!(
            block_on(with_timeout(async { 5 }, std::future::ready(()))),
            Some(5)
        );
        assert_eq!(
            block_on(with_timeout(
                std::future::pending::<i32>(),
                std::future::ready(())
            )),
            None
        );
    }
}
