//! The shown list (session) and the request in flight: where they start,
//! the query typed since, and closing them when the cursor leaves.

use std::ops::Range;

use arcode_completion::{CandidateKind, CompletionCandidate, detect_path_prefix, path_query};
use gpui::{AppContext, Context};

use super::docs_view::{DocPanel, DocsView};
use super::menu::MenuState;
use super::view::{MenuView, Row, list_height};
use super::{COMPLETION_POPOVER, CompletionItem, is_word_char};
use crate::Editor;
use crate::ext::EditorPopover;

/// What kind of text is completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// An identifier (the word before the cursor).
    Word,
    /// A file name inside a string literal path.
    Path,
    /// A snippet placeholder with choices; it stays selected.
    Choice,
}

/// Where a list applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Anchor {
    pub mode: Mode,
    /// Char offset where the completed text starts.
    pub start: usize,
    /// Line of `start`.
    pub line: usize,
    /// Cursor offset when the list was requested (the end of the
    /// placeholder for [`Mode::Choice`]).
    pub request_cursor: usize,
}

/// A request in flight.
pub(crate) struct Pending {
    pub id: u64,
    pub anchor: Anchor,
    /// The list opened after a trigger character (only server items).
    pub after_trigger: bool,
}

/// The list being shown.
pub(crate) struct Session {
    pub id: u64,
    pub anchor: Anchor,
    /// Opened after a trigger character; re-requests keep it.
    pub after_trigger: bool,
    pub items: Vec<CompletionItem>,
    pub candidates: Vec<CompletionCandidate>,
    pub incomplete: bool,
    pub menu: MenuState,
    pub query: String,
}

/// What identifies an item across two answers of the same list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemKey {
    label: String,
    kind: CandidateKind,
    detail: Option<String>,
}

impl ItemKey {
    pub fn of(item: &CompletionItem) -> Self {
        ItemKey {
            label: item.candidate.label.clone(),
            kind: item.candidate.kind,
            detail: item.candidate.detail.clone(),
        }
    }

    pub fn matches(&self, item: &CompletionItem) -> bool {
        self.label == item.candidate.label
            && self.kind == item.candidate.kind
            && self.detail == item.candidate.detail
    }
}

impl Session {
    /// The selected item.
    pub fn selected_item(&self) -> Option<&CompletionItem> {
        let ranked = self.menu.ranked.get(self.menu.selected)?;
        self.items.get(ranked.index)
    }
}

/// Whether `c` may appear in a file name being completed.
fn is_path_char(c: char) -> bool {
    !matches!(c, '/' | '\\' | '"' | '\'' | '`' | '\n')
}

impl Editor {
    /// The single empty cursor, if there is exactly one.
    pub(crate) fn single_cursor(&self) -> Option<usize> {
        let sel = self.selections.primary();
        (self.selections.len() == 1 && sel.is_empty()).then_some(sel.head)
    }

    /// Where a list requested now would apply: a path inside a string, or
    /// the word before the cursor. `None` with several cursors or a
    /// selection.
    pub(crate) fn completion_anchor(&self) -> Option<Anchor> {
        let cursor = self.single_cursor()?;
        let line = self.buffer.char_to_line(cursor);
        let line_start = self.buffer.line_start(line);
        let col = cursor - line_start;
        let text = self.buffer.line_text(line);
        if let Some((start_col, typed)) = detect_path_prefix(&text, col) {
            let name_len = path_query(&typed).chars().count();
            let typed_len = typed.chars().count();
            return Some(Anchor {
                mode: Mode::Path,
                start: line_start + start_col + typed_len - name_len,
                line,
                request_cursor: cursor,
            });
        }
        let chars: Vec<char> = text.chars().take(col).collect();
        let word = chars.iter().rev().take_while(|c| is_word_char(**c)).count();
        Some(Anchor {
            mode: Mode::Word,
            start: cursor - word,
            line,
            request_cursor: cursor,
        })
    }

    /// The text typed since `anchor.start`, or `None` when the cursor left
    /// the completed word (another line, before the start, a non-word
    /// char in between, several cursors).
    pub(crate) fn completion_query(&self, anchor: &Anchor) -> Option<String> {
        if anchor.mode == Mode::Choice {
            let sel = self.selections.primary();
            let range = anchor.start..anchor.request_cursor;
            return (self.selections.len() == 1 && sel.range() == range).then(String::new);
        }
        let cursor = self.single_cursor()?;
        if cursor < anchor.start
            || anchor.start > self.buffer.len_chars()
            || self.buffer.char_to_line(cursor) != anchor.line
            || self.buffer.char_to_line(anchor.start) != anchor.line
        {
            return None;
        }
        let query = self.buffer.slice_to_string(anchor.start..cursor);
        let ok = match anchor.mode {
            Mode::Word => query.chars().all(is_word_char),
            Mode::Path => query.chars().all(is_path_char),
            Mode::Choice => false,
        };
        ok.then_some(query)
    }

    /// Re-filters the shown list for the text typed since it opened, or
    /// closes it (and drops a request in flight) once the cursor left.
    pub(crate) fn completion_sync(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = &self.completion.pending
            && self.completion_query(&p.anchor).is_none()
        {
            self.completion.pending = None;
            self.completion.request_task = None;
        }
        let Some(session) = &self.completion.session else {
            return;
        };
        let Some(query) = self.completion_query(&session.anchor) else {
            self.hide_completions(cx);
            return;
        };
        if query == session.query {
            return;
        }
        let session = self.completion.session.as_mut().expect("checked above");
        session.menu = MenuState::rank(&mut self.completion.ranker, &query, &session.candidates);
        session.query = query.clone();
        if session.menu.ranked.is_empty() {
            if session.incomplete {
                // The re-request may still find something: keep the list,
                // but draw nothing and leave the keys to the editor.
                self.completion_hide_box(cx);
                return;
            }
            // A complete list has nothing for this word: typing on in it
            // does not ask again.
            self.completion.no_results = Some((session.anchor.start, query));
            self.hide_completions(cx);
            return;
        }
        self.completion_push_rows(cx);
        self.completion_show_box(cx);
        self.completion_resolve_selected(cx);
    }

    /// The item the user selected with the arrows in the list shown at
    /// `anchor`, to select it again in a newer answer.
    pub(crate) fn completion_user_choice(&self, anchor: &Anchor) -> Option<ItemKey> {
        let session = self.completion.session.as_ref()?;
        let same = session.anchor.start == anchor.start && session.anchor.mode == anchor.mode;
        if !(same && session.menu.user_moved) {
            return None;
        }
        session.selected_item().map(ItemKey::of)
    }

    /// Hides the popup box but keeps the (empty) list.
    pub(crate) fn completion_hide_box(&mut self, cx: &mut Context<Self>) {
        self.completion.resolve_task = None;
        self.completion.resolving = None;
        self.hide_popover(COMPLETION_POPOVER, cx);
    }

    /// Whether the completion list is shown.
    pub fn completion_visible(&self) -> bool {
        self.completion
            .session
            .as_ref()
            .is_some_and(|s| !s.menu.ranked.is_empty())
    }

    /// Labels of the shown completion items, best first.
    pub fn completion_labels(&self) -> Vec<String> {
        self.completion.session.as_ref().map_or_else(Vec::new, |s| {
            s.menu
                .ranked
                .iter()
                .map(|r| s.items[r.index].candidate.label.clone())
                .collect()
        })
    }

    /// The selected completion item.
    pub fn selected_completion(&self) -> Option<&CompletionItem> {
        self.completion.session.as_ref()?.selected_item()
    }

    /// Closes the completion list and cancels its requests.
    pub fn hide_completions(&mut self, cx: &mut Context<Self>) {
        self.completion.session = None;
        self.completion.pending = None;
        self.completion.request_task = None;
        self.completion.resolve_task = None;
        self.completion.resolving = None;
        self.hide_popover(COMPLETION_POPOVER, cx);
    }

    /// Closes the list and signature help when the editor lost focus.
    pub(crate) fn completion_blurred(&mut self, cx: &mut Context<Self>) {
        if self.completion.session.is_some() || self.completion.pending.is_some() {
            self.hide_completions(cx);
        }
        if self.signature_help_visible() {
            self.hide_signature_help(cx);
        }
    }

    /// Replaces the shown list with `session` and shows the popover.
    pub(crate) fn completion_open(&mut self, session: Session, cx: &mut Context<Self>) {
        let same_place = self.completion.session.as_ref().is_some_and(|s| {
            s.anchor.start == session.anchor.start && s.anchor.mode == session.anchor.mode
        });
        self.completion.session = Some(session);
        if self.completion.view.is_none() {
            let editor = cx.entity().downgrade();
            self.completion.view = Some(cx.new(|_| MenuView::new(editor)));
            self.completion.docs_view = Some(cx.new(|_| DocsView::new()));
        }
        self.completion_push_rows(cx);
        if !same_place {
            self.hide_popover(COMPLETION_POPOVER, cx);
        }
        self.completion_show_box(cx);
        self.completion_resolve_selected(cx);
    }

    /// Shows the popup box of the current list if it is not shown.
    pub(crate) fn completion_show_box(&mut self, cx: &mut Context<Self>) {
        if self.has_popover(COMPLETION_POPOVER) {
            return;
        }
        let (Some(session), Some(view), Some(docs)) = (
            &self.completion.session,
            &self.completion.view,
            &self.completion.docs_view,
        ) else {
            return;
        };
        let start = self.offset_to_point(session.anchor.start);
        let popover = EditorPopover::new(start, view.clone().into())
            .side(docs.clone().into())
            .on_key(|editor, ks, _, cx| editor.update(cx, |e, cx| e.completion_key(ks, cx)));
        self.show_popover(COMPLETION_POPOVER, popover, cx);
    }

    /// Sends the visible state to the popup view.
    pub(crate) fn completion_push_rows(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(view), Some(docs_view)) = (
            &self.completion.session,
            &self.completion.view,
            &self.completion.docs_view,
        ) else {
            return;
        };
        let rows: Vec<Row> = session
            .menu
            .ranked
            .iter()
            .map(|r| Row::new(&session.items[r.index], &r.match_indices))
            .collect();
        let selected = session.menu.selected;
        let scroll_top = session.menu.scroll_top;
        let height = list_height(rows.len());
        let docs = session.selected_item().and_then(DocPanel::for_item);
        let docs_view = docs_view.clone();
        view.update(cx, |v, cx| v.set_state(rows, selected, scroll_top, cx));
        docs_view.update(cx, |v, cx| v.set_docs(docs, height, cx));
    }

    /// Replaces the item at `index` (in `items`) of session `id`, e.g. after
    /// resolving it; ignored when that list is gone.
    pub(crate) fn completion_replace_item(
        &mut self,
        id: u64,
        index: usize,
        item: CompletionItem,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.completion.session.as_mut().filter(|s| s.id == id) else {
            return;
        };
        if let Some(slot) = session.items.get_mut(index) {
            *slot = item;
            self.completion_push_rows(cx);
        }
    }

    /// The char range of the word around `cursor` that starts at `start`
    /// (for replace mode).
    pub(crate) fn word_end_from(&self, cursor: usize) -> usize {
        let len = self.buffer.len_chars();
        let mut end = cursor;
        while end < len && self.buffer.char_at(end).is_some_and(is_word_char) {
            end += 1;
        }
        end
    }

    /// The range of a choice placeholder as an anchor.
    pub(crate) fn choice_anchor(&self, range: Range<usize>) -> Anchor {
        Anchor {
            mode: Mode::Choice,
            start: range.start,
            line: self.buffer.char_to_line(range.start),
            request_cursor: range.end,
        }
    }
}
