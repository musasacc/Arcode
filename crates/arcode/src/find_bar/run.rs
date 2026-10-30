//! Running the find bar's search on a buffer snapshot in the background.

use std::time::Duration;

use arcode_buffer::Point;
use arcode_search::search_text;
use gpui::{App, AppContext, Context};

use crate::search_widgets::short_error;

use super::{FindBar, MAX_MATCHES, Searched, first_at_or_after, match_ranges};

impl FindBar {
    /// Searches the editor again after `delay`; with `select_from` the
    /// first match at or after it becomes current and is selected.
    pub(super) fn research(
        &mut self,
        select_from: Option<Point>,
        delay: Option<Duration>,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editor.clone().filter(|_| self.open) else {
            return;
        };
        let query = self.build_query(cx);
        // An edit right after "replace" must not drop the pending selection.
        let select_from = select_from.or(self.pending_select);
        self.pending_select = select_from;
        self.error = None;
        if query.pattern.is_empty() {
            self.pending_select = None;
            self.pending_step = None;
            self.task = None;
            self.matches.clear();
            self.current = None;
            self.searched = None;
            self.clear_highlights(cx);
            cx.notify();
            return;
        }
        if let Err(err) = query.validate() {
            self.pending_select = None;
            self.pending_step = None;
            self.task = None;
            self.error = Some(short_error(&err.to_string()).into());
            self.matches.clear();
            self.current = None;
            self.searched = None;
            self.clear_highlights(cx);
            cx.notify();
            return;
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            if let Some(delay) = delay {
                cx.background_executor().timer(delay).await;
            }
            let Ok((rope, version)) =
                editor.read_with(cx, |e, _| (e.buffer().text().clone(), e.buffer().version()))
            else {
                return;
            };
            let q = query.clone();
            let found = cx
                .background_spawn(async move { search_text(&rope.to_string(), &q) })
                .await;
            this.update(cx, |bar, cx| {
                let searched = Searched {
                    editor: editor.entity_id(),
                    version,
                    query,
                };
                bar.apply_results(found, searched, select_from, cx);
            })
            .ok();
        }));
    }

    pub(super) fn apply_results(
        &mut self,
        found: arcode_search::Result<Vec<arcode_search::LineMatch>>,
        searched: Searched,
        select_from: Option<Point>,
        cx: &mut Context<Self>,
    ) {
        let lines = match found {
            Ok(lines) => lines,
            Err(err) => {
                self.error = Some(short_error(&err.to_string()).into());
                Vec::new()
            }
        };
        self.pending_select = None;
        let previous = self.current.and_then(|ix| self.matches.get(ix).cloned());
        self.matches = match_ranges(&lines);
        self.capped = self.matches.len() >= MAX_MATCHES;
        self.searched = Some(searched);
        self.current = match (select_from, previous) {
            (Some(from), _) => first_at_or_after(&self.matches, from),
            (None, Some(prev)) => self.matches.iter().position(|m| *m == prev),
            (None, None) => None,
        };
        if select_from.is_some() {
            self.select_current(cx);
        }
        self.apply_highlights(cx);
        cx.notify();
        if let Some(forward) = self.pending_step.take() {
            self.go(forward, cx);
        }
    }

    pub(super) fn is_fresh(&self, cx: &App) -> bool {
        let (Some(editor), Some(s)) = (&self.editor, &self.searched) else {
            return false;
        };
        s.editor == editor.entity_id()
            && s.version == editor.read(cx).buffer().version()
            && s.query == self.build_query(cx)
    }
}
