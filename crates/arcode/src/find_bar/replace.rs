//! Replacing in the find bar: the current match or all of them, as
//! undoable editor edits.

use arcode_buffer::Point;
use arcode_search::{SearchQuery, search_text};
use gpui::{Context, Focusable, Window};

use super::{FindBar, FocusNextField, logic, replace_edits};

impl FindBar {
    /// Replaces the current match (or moves to the first one) and goes on
    /// to the next match.
    pub fn replace_next(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        if editor.read(cx).is_read_only() {
            return;
        }
        let range = self.current.and_then(|ix| self.matches.get(ix).cloned());
        let (Some(range), true) = (range, self.is_fresh(cx)) else {
            self.go(true, cx);
            return;
        };
        let query = self.build_query(cx);
        let replacement = self.replacement.read(cx).text().to_string();
        let line = editor.read(cx).buffer().line_text(range.start.line);
        let Some((r, text)) = logic::replace_one_edit(&range, &line, &query, &replacement) else {
            return;
        };
        let after = Point::new(r.start.line, r.start.column + text.chars().count());
        editor.update(cx, |e, cx| e.apply_edits(vec![(r, text)], cx));
        self.research(Some(after), None, cx);
    }

    /// Replaces every match in the file as one undo step. Returns how many
    /// matches were replaced.
    pub fn replace_all(&mut self, cx: &mut Context<Self>) -> usize {
        let Some(editor) = self.editor.clone() else {
            return 0;
        };
        let query = SearchQuery {
            max_results: 0,
            ..self.build_query(cx)
        };
        if editor.read(cx).is_read_only() || query.validate().is_err() {
            return 0;
        }
        let replacement = self.replacement.read(cx).text().to_string();
        let edits = {
            let e = editor.read(cx);
            let text = e.buffer().text().to_string();
            let Ok(lines) = search_text(&text, &query) else {
                return 0;
            };
            let buffer = e.buffer();
            replace_edits(
                lines.iter().map(|l| l.line),
                |line| buffer.line_text(line),
                &query,
                &replacement,
            )
        };
        let count = edits.len();
        if count > 0 {
            editor.update(cx, |e, cx| e.apply_edits(edits, cx));
        }
        self.notice = Some(match count {
            0 => "Nothing to replace".into(),
            1 => "Replaced 1 occurrence".into(),
            n => format!("Replaced {n} occurrences").into(),
        });
        self.research(None, None, cx);
        cx.notify();
        count
    }

    pub(super) fn focus_next_field(
        &mut self,
        _: &FocusNextField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query_focused = self.query.focus_handle(cx).is_focused(window);
        if query_focused && self.show_replace {
            window.focus(&self.replacement.focus_handle(cx));
        } else {
            window.focus(&self.query.focus_handle(cx));
        }
    }
}
