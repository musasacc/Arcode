//! The completion popup: up to ten rows (kind badge, label with the
//! matched characters highlighted, detail on the right). The selected
//! item's documentation is a separate side view ([`super::docs_view`]).
//!
//! The editor owns the list and pushes the visible state here after every
//! change; the view only keeps its own scroll position for the mouse wheel.

use arcode_completion::CandidateKind;
use arcode_ui_kit::metrics::WEIGHT_SEMIBOLD;
use arcode_ui_kit::{menu_row, popover, theme, truncated, type_scale, ui_font_family};
use gpui::{
    Context, HighlightStyle, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    Render, ScrollDelta, ScrollWheelEvent, SharedString, Styled, StyledText, WeakEntity, Window,
    div, px,
};

use super::CompletionItem;
use super::kind::kind_badge;
use super::menu::{VISIBLE_ROWS, clamp_top};
use crate::Editor;

/// Height of one row.
const ROW_HEIGHT: f32 = 24.;
/// Width of the list.
const LIST_WIDTH: f32 = 420.;

/// The popover's padding plus its 1 px edge, above and below the rows.
const LIST_PAD: f32 = 5.;

/// Height of the list box showing `rows` rows.
pub(crate) fn list_height(rows: usize) -> Pixels {
    px(ROW_HEIGHT * rows.min(VISIBLE_ROWS) as f32 + 2. * LIST_PAD)
}

/// One visible row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub label: SharedString,
    /// Byte ranges of `label` that matched the query.
    pub matched: Vec<std::ops::Range<usize>>,
    pub detail: Option<SharedString>,
    pub kind: CandidateKind,
    pub deprecated: bool,
}

impl Row {
    /// A row for `item` with `match_indices` (char indices into its label).
    pub fn new(item: &CompletionItem, match_indices: &[u32]) -> Self {
        let label = &item.candidate.label;
        Row {
            label: SharedString::from(label.clone()),
            matched: char_ranges(label, match_indices),
            detail: item.candidate.detail.clone().map(SharedString::from),
            kind: item.candidate.kind,
            deprecated: item.deprecated,
        }
    }
}

/// Byte ranges of the chars at sorted `indices`, adjacent ones merged.
fn char_ranges(text: &str, indices: &[u32]) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut want = indices.iter().peekable();
    for (i, (byte, c)) in text.char_indices().enumerate() {
        if want.peek().is_none() {
            break;
        }
        if want.peek().is_some_and(|w| **w as usize == i) {
            want.next();
            let end = byte + c.len_utf8();
            match out.last_mut() {
                Some(last) if last.end == byte => last.end = end,
                _ => out.push(byte..end),
            }
        }
    }
    out
}

/// The popup view.
pub(crate) struct MenuView {
    editor: WeakEntity<Editor>,
    rows: Vec<Row>,
    selected: usize,
    scroll_top: usize,
    /// Fractional wheel scrolling not yet turned into rows.
    wheel: f32,
}

impl MenuView {
    pub fn new(editor: WeakEntity<Editor>) -> Self {
        MenuView {
            editor,
            rows: Vec::new(),
            selected: 0,
            scroll_top: 0,
            wheel: 0.,
        }
    }

    /// Replaces the shown state; redraws only when it changed.
    pub fn set_state(
        &mut self,
        rows: Vec<Row>,
        selected: usize,
        scroll_top: usize,
        cx: &mut Context<Self>,
    ) {
        if rows == self.rows && selected == self.selected && scroll_top == self.scroll_top {
            return;
        }
        self.rows = rows;
        self.selected = selected;
        self.scroll_top = scroll_top;
        cx.notify();
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let rows = match event.delta {
            ScrollDelta::Lines(d) => -d.y,
            ScrollDelta::Pixels(d) => -f32::from(d.y) / ROW_HEIGHT,
        };
        self.wheel += rows;
        let whole = self.wheel.trunc();
        self.wheel -= whole;
        if whole != 0. {
            let top = (self.scroll_top as isize + whole as isize).max(0) as usize;
            let top = clamp_top(top, self.rows.len());
            if top != self.scroll_top {
                self.scroll_top = top;
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    fn render_row(&self, index: usize, row: &Row, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme(cx).colors;
        let selected = index == self.selected;
        let mut base = HighlightStyle::default();
        if row.deprecated {
            base.strikethrough = Some(gpui::StrikethroughStyle {
                thickness: px(1.),
                color: None,
            });
        }
        let matched = HighlightStyle {
            color: Some(colors.accent),
            font_weight: Some(WEIGHT_SEMIBOLD),
            ..base
        };
        let highlights: Vec<_> = if row.deprecated {
            vec![(0..row.label.len(), base)]
        } else {
            row.matched.iter().map(|r| (r.clone(), matched)).collect()
        };
        let label = StyledText::new(row.label.clone()).with_highlights(highlights);
        let editor = self.editor.clone();
        menu_row(("completion-row", index), selected, cx)
            .h(px(ROW_HEIGHT))
            .px(px(6.))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                editor
                    .update(cx, |e, cx| e.completion_click(index, cx))
                    .ok();
            })
            .child(kind_badge(row.kind, cx))
            .child(
                div()
                    .flex_none()
                    .max_w(px(LIST_WIDTH * 0.7))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(if row.deprecated {
                        colors.text_muted
                    } else {
                        colors.text
                    })
                    .child(label),
            )
            .child(div().flex_1().min_w(px(12.)))
            .children(row.detail.clone().map(|detail| {
                div()
                    .flex_shrink()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(colors.text_muted)
                    .text_size(type_scale(cx).meta)
                    .child(truncated(detail))
            }))
    }
}

impl Render for MenuView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors.clone();
        let top = clamp_top(self.scroll_top, self.rows.len());
        let visible = self.rows.len().min(VISIBLE_ROWS);
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .skip(top)
            .take(VISIBLE_ROWS)
            .map(|(i, row)| self.render_row(i, row, cx).into_any_element())
            .collect();
        let thumb = (self.rows.len() > VISIBLE_ROWS).then(|| {
            let total = self.rows.len() as f32;
            let track = ROW_HEIGHT * visible as f32;
            let h = (track * VISIBLE_ROWS as f32 / total).max(12.);
            let y = LIST_PAD + (track - h) * top as f32 / (total - VISIBLE_ROWS as f32);
            div()
                .absolute()
                .right(px(1.))
                .top(px(y))
                .w(px(3.))
                .h(px(h))
                .rounded_full()
                .bg(colors.scrollbar_thumb)
        });
        popover(cx)
            .id("completion-list")
            .occlude()
            .relative()
            .w(px(LIST_WIDTH))
            .h(list_height(self.rows.len()))
            .on_scroll_wheel(cx.listener(|v, e, _, cx| v.scroll(e, cx)))
            .font_family(ui_font_family(cx))
            .text_size(type_scale(cx).body)
            .text_color(colors.text)
            .children(rows)
            .children(thumb)
    }
}

#[cfg(test)]
mod tests {
    use super::char_ranges;

    #[test]
    fn match_indices_become_merged_byte_ranges() {
        assert_eq!(char_ranges("from_str", &[0, 1, 5]), vec![0..2, 5..6]);
        assert_eq!(char_ranges("äbc", &[0, 1]), vec![0..3]);
        assert!(char_ranges("abc", &[]).is_empty());
    }
}
