//! The find bar of an editor pane (Ctrl+F, Ctrl+H for replace): a query
//! with match case / whole word / regex toggles, next and previous, and an
//! optional replace row. Matches are found with `arcode_search` on a
//! snapshot of the buffer on the background executor and shown through the
//! editor's background-highlight extension point (sets `"find"` and
//! `"find-current"`). Replacements are editor edits, so they are undoable.

pub mod bulk;
mod highlight;
mod logic;
mod render;
mod replace;
#[cfg(test)]
mod review_find_bar;
mod run;
#[cfg(test)]
mod tests;

use std::ops::Range;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::{Editor, EditorEvent};
use arcode_search::SearchQuery;
use gpui::{
    App, AppContext, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, KeyBinding,
    SharedString, Subscription, Task, Window, actions,
};

use crate::search_widgets::{SearchOption, SearchOptions};
use crate::text_field::{TextChanged, TextField};

pub use logic::{first_at_or_after, match_ranges, status_text, step};

actions!(
    find_bar,
    [
        /// Closes the find bar.
        CloseFindBar,
        /// Replaces the current match and moves to the next one.
        ReplaceNext,
        /// Replaces every match in the file.
        ReplaceAllInFile,
        /// Moves focus between the query and replacement fields.
        FocusNextField,
        /// Toggles match case.
        ToggleCase,
        /// Toggles whole word.
        ToggleWord,
        /// Toggles regular expressions.
        ToggleRegex,
    ]
);

/// Key context of the find bar.
pub const CONTEXT: &str = "FindBar";
/// Key context of the replacement field.
pub const REPLACE_CONTEXT: &str = "FindReplace";
/// Background highlight set of all matches.
pub const MATCHES_SET: &str = "find";
/// Background highlight set of the current match.
pub const CURRENT_SET: &str = "find-current";
/// Most matches found (and highlighted) in one file.
const MAX_MATCHES: usize = 20_000;
/// Quiet time after an edit before the matches are refreshed.
const EDIT_DEBOUNCE: Duration = Duration::from_millis(150);

/// Registers the find bar key bindings.
pub fn bind_keys(cx: &mut App) {
    use crate::actions::{FindNext, FindPrevious};
    let ctx = Some(CONTEXT);
    let replace = Some(REPLACE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", FindNext, ctx),
        KeyBinding::new("shift-enter", FindPrevious, ctx),
        KeyBinding::new("f3", FindNext, ctx),
        KeyBinding::new("shift-f3", FindPrevious, ctx),
        KeyBinding::new("escape", CloseFindBar, ctx),
        KeyBinding::new("tab", FocusNextField, ctx),
        KeyBinding::new("shift-tab", FocusNextField, ctx),
        KeyBinding::new("alt-c", ToggleCase, ctx),
        KeyBinding::new("alt-w", ToggleWord, ctx),
        KeyBinding::new("alt-r", ToggleRegex, ctx),
        KeyBinding::new("enter", ReplaceNext, replace),
        KeyBinding::new("secondary-alt-enter", ReplaceAllInFile, replace),
    ]);
}

/// Events emitted by the find bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindBarEvent {
    /// The bar was closed (Escape or its close button).
    Closed,
}

/// What the current matches were computed for.
#[derive(Clone, PartialEq)]
struct Searched {
    editor: EntityId,
    version: u64,
    query: SearchQuery,
}

/// The find bar view.
pub struct FindBar {
    /// Shown: a hidden bar keeps its query but neither searches nor
    /// highlights.
    open: bool,
    editor: Option<Entity<Editor>>,
    query: Entity<TextField>,
    replacement: Entity<TextField>,
    options: SearchOptions,
    show_replace: bool,
    matches: Vec<Range<Point>>,
    capped: bool,
    current: Option<usize>,
    error: Option<SharedString>,
    notice: Option<SharedString>,
    searched: Option<Searched>,
    /// Where to select the first match once the running search ends.
    pending_select: Option<Point>,
    /// A next (`true`) or previous step asked for while the matches were
    /// stale; taken once the running search ends.
    pending_step: Option<bool>,
    task: Option<Task<()>>,
    editor_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<FindBarEvent> for FindBar {}

impl Focusable for FindBar {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl FindBar {
    /// Creates a closed-state bar without a target editor.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextField::new("Find", cx));
        let replacement = cx.new(|cx| TextField::new("Replace", cx));
        let subs = vec![cx.subscribe(&query, |bar, _, _: &TextChanged, cx| {
            let from = bar.selection_start(cx);
            bar.notice = None;
            bar.research(from, None, cx);
        })];
        Self {
            open: false,
            editor: None,
            query,
            replacement,
            options: SearchOptions::default(),
            show_replace: false,
            matches: Vec::new(),
            capped: false,
            current: None,
            error: None,
            notice: None,
            searched: None,
            pending_select: None,
            pending_step: None,
            task: None,
            editor_sub: None,
            _subs: subs,
        }
    }

    /// The query text.
    pub fn query_text(&self, cx: &App) -> String {
        self.query.read(cx).text().to_string()
    }

    /// Sets the query text (searching again).
    pub fn set_query(&mut self, text: &str, cx: &mut Context<Self>) {
        self.query.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// Sets the replacement text.
    pub fn set_replacement(&mut self, text: &str, cx: &mut Context<Self>) {
        self.replacement.update(cx, |f, cx| f.set_text(text, cx));
    }

    /// The current options.
    pub fn options(&self) -> SearchOptions {
        self.options
    }

    /// The matches (buffer ranges) of the last search.
    pub fn matches(&self) -> &[Range<Point>] {
        &self.matches
    }

    /// Index of the current match.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Whether the bar is shown (searching and highlighting).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the replace row is shown.
    pub fn shows_replace(&self) -> bool {
        self.show_replace
    }

    /// The editor searched.
    pub fn editor(&self) -> Option<&Entity<Editor>> {
        self.editor.as_ref()
    }

    /// Prepares the bar for showing on `editor`: seeds the query with a
    /// one-line selection, shows or hides the replace row and searches.
    pub fn open(&mut self, editor: Option<Entity<Editor>>, replace: bool, cx: &mut Context<Self>) {
        self.open = true;
        self.set_editor(editor, cx);
        self.show_replace = replace;
        self.notice = None;
        let seed = self.editor.as_ref().and_then(|e| {
            let e = e.read(cx);
            let sel = e.selections().primary();
            let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
            let text =
                (a != b).then(|| e.text_in_range(e.offset_to_point(a)..e.offset_to_point(b)));
            text.filter(|t| !t.contains('\n') && t.chars().count() <= 200)
        });
        match seed {
            Some(text) if text != self.query_text(cx) => self.set_query(&text, cx),
            _ => {
                let from = self.selection_start(cx);
                self.research(from, None, cx);
            }
        }
        self.query.update(cx, |f, cx| f.select_all_text(cx));
        cx.notify();
    }

    /// Focuses the query field.
    pub fn focus_query(&self, window: &mut Window, cx: &App) {
        window.focus(&self.query.focus_handle(cx));
    }

    /// Targets `editor` (the pane's active editor); highlights move with it.
    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        if self.editor == editor {
            return;
        }
        self.clear_highlights(cx);
        self.editor = editor;
        self.notice = None;
        self.matches.clear();
        self.current = None;
        self.searched = None;
        self.editor_sub = self.editor.as_ref().map(|e| {
            cx.subscribe(e, |bar, _, event: &EditorEvent, cx| match event {
                EditorEvent::Edited { .. } => bar.research(None, Some(EDIT_DEBOUNCE), cx),
                EditorEvent::SelectionsChanged => bar.forget_current_if_moved(cx),
                _ => {}
            })
        });
        self.research(None, None, cx);
    }

    /// Hides the bar's effects (highlights, pending search) and tells the
    /// pane to hide it.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.task = None;
        self.pending_select = None;
        self.pending_step = None;
        self.clear_highlights(cx);
        self.searched = None;
        cx.emit(FindBarEvent::Closed);
    }

    /// Re-applies the highlights after the bar is shown again.
    pub fn reshow(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        self.notice = None;
        self.research(None, None, cx);
    }

    /// Flips `option` and searches again.
    pub fn toggle_option(&mut self, option: SearchOption, cx: &mut Context<Self>) {
        option.toggle(&mut self.options);
        let from = self.selection_start(cx);
        self.research(from, None, cx);
        cx.notify();
    }

    /// Shows or hides the replace row.
    pub fn toggle_replace(&mut self, cx: &mut Context<Self>) {
        self.show_replace = !self.show_replace;
        cx.notify();
    }

    fn selection_start(&self, cx: &App) -> Option<Point> {
        let e = self.editor.as_ref()?.read(cx);
        let sel = e.selections().primary();
        Some(e.offset_to_point(sel.anchor.min(sel.head)))
    }

    fn build_query(&self, cx: &App) -> SearchQuery {
        SearchQuery {
            max_results: MAX_MATCHES,
            ..self.options.query(&self.query_text(cx))
        }
    }

    /// Moves to the next (or previous) match and selects it. When the
    /// matches are stale (edited buffer, other editor, changed query, a
    /// search still running), searches now and steps once it is done, so
    /// the selection is always a match of the current text.
    pub fn go(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.is_fresh(cx) {
            if self.open && self.editor.is_some() {
                self.pending_step = Some(forward);
                self.research(None, None, cx);
            }
            return;
        }
        let Some(cursor) = self.selection_start(cx) else {
            return;
        };
        let cursor = if forward && self.current.is_none() {
            // Start after a selection that is not a match (e.g. typed text).
            self.editor
                .as_ref()
                .map(|e| e.read(cx).cursor_position())
                .unwrap_or(cursor)
        } else {
            cursor
        };
        self.current = step(&self.matches, self.current, cursor, forward);
        self.select_current(cx);
        self.apply_highlights(cx);
        cx.notify();
    }
}
