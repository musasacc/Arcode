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
        let menu = MenuState::rank(&mut self.completion.ranker, "", &candidates);
        self.completion.seq += 1;
        let session = Session {
            id: self.completion.seq,
            anchor: self.choice_anchor(range),
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
