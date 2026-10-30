//! Showing the find bar's matches in its editor: highlights, selecting the
//! current match and forgetting it when the cursor moves away.

use arcode_editor::{BackgroundHighlight, ColorKey, ScrollAnchor};
use gpui::Context;

use super::{CURRENT_SET, FindBar, MATCHES_SET};

impl FindBar {
    pub(super) fn apply_highlights(&self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone().filter(|_| self.open) else {
            return;
        };
        let all: Vec<_> = self
            .matches
            .iter()
            .map(|m| BackgroundHighlight::text(m.clone(), ColorKey::new("warning").opacity(0.28)))
            .collect();
        let current: Vec<_> = self
            .current
            .and_then(|ix| self.matches.get(ix))
            .map(|m| BackgroundHighlight::text(m.clone(), ColorKey::new("accent").opacity(0.5)))
            .into_iter()
            .collect();
        editor.update(cx, |e, cx| {
            e.set_background_highlights(MATCHES_SET, all, cx);
            e.set_background_highlights(CURRENT_SET, current, cx);
        });
    }

    pub(super) fn clear_highlights(&self, cx: &mut Context<Self>) {
        if let Some(editor) = &self.editor {
            editor.update(cx, |e, cx| {
                e.clear_decorations(MATCHES_SET, cx);
                e.clear_decorations(CURRENT_SET, cx);
            });
        }
    }

    /// Selects the current match in the editor and scrolls to it.
    pub(super) fn select_current(&self, cx: &mut Context<Self>) {
        let (Some(editor), Some(range)) = (
            self.editor.clone(),
            self.current.and_then(|ix| self.matches.get(ix).cloned()),
        ) else {
            return;
        };
        editor.update(cx, |e, cx| {
            e.select_ranges(vec![range.clone()], cx);
            if !e.visible_line_range().contains(&range.start.line) {
                e.scroll_to_line(range.start.line, ScrollAnchor::Center, cx);
            }
        });
    }

    /// The cursor left the current match: next/previous start from it.
    pub(super) fn forget_current_if_moved(&mut self, cx: &mut Context<Self>) {
        let (Some(editor), Some(ix)) = (&self.editor, self.current) else {
            return;
        };
        let e = editor.read(cx);
        let sel = e.selections().primary();
        let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        let selected = e.offset_to_point(a)..e.offset_to_point(b);
        if self.matches.get(ix) != Some(&selected) {
            self.current = None;
            self.apply_highlights(cx);
            cx.notify();
        }
    }
}
