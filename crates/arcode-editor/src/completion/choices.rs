//! A list of a snippet placeholder's choices (`${1|one,two|}`).

use arcode_completion::{CandidateKind, CandidateSource, CompletionCandidate};
use gpui::Context;

use super::menu::MenuState;
use super::session::Session;
use super::{CompletionItem, ItemEdit};
use crate::Editor;

impl Editor {
    /// Opens the choices of the active snippet tabstop, if it has any.
    pub(crate) fn snippet_show_choices(&mut self, cx: &mut Context<Self>) {
        let Some(snippet) = &self.snippet else {
            return;
        };
        let Some(choices) = snippet.current_choices().map(<[String]>::to_vec) else {
            return;
        };
        let Some(range) = snippet.active_ranges().into_iter().next() else {
            return;
        };
        let points = self.offset_to_point(range.start)..self.offset_to_point(range.end);
        // Re-entering the placeholder selects the value it holds now.
        let current = self.buffer.slice_to_string(range.clone());
        let items: Vec<CompletionItem> = choices
            .into_iter()
            .map(|choice| {
                let candidate = CompletionCandidate::new(
                    choice,
                    CandidateKind::Value,
                    CandidateSource::Snippet,
                );
                let mut item = CompletionItem::new(candidate);
                item.edit = Some(ItemEdit::new(points.clone()));
                item
            })
            .collect();
        let candidates: Vec<CompletionCandidate> =
            items.iter().map(|i| i.candidate.clone()).collect();
        let mut menu = MenuState::rank(&mut self.completion.ranker, "", &candidates);
        if let Some(row) = menu
            .ranked
            .iter()
            .position(|r| candidates[r.index].label == current)
        {
            menu.selected = row;
            menu.reveal();
        }
        self.completion.seq += 1;
        let session = Session {
            id: self.completion.seq,
            anchor: self.choice_anchor(range),
            after_trigger: false,
            items,
            candidates,
            incomplete: false,
            menu,
            query: String::new(),
        };
        self.completion.pending = None;
        self.completion.request_task = None;
        self.completion_open(session, cx);
    }
}
