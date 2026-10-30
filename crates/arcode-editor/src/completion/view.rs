//! The completion popup: up to ten rows (kind badge, label with the
//! matched characters highlighted, detail on the right) and a side panel
//! with the selected item's documentation.
//!
//! The editor owns the list and pushes the visible state here after every
//! change; the view only keeps its own scroll position for the mouse wheel.

use arcode_completion::{CandidateKind, CandidateSource};
use arcode_ui_kit::{settings, theme, truncated};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, ScrollDelta, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement, Styled, StyledText, WeakEntity, Window, div, prelude::*, px,
};

use super::docs::{Block, blocks_for, render_blocks};
use super::kind::kind_badge;
use super::menu::{VISIBLE_ROWS, clamp_top};
use super::{CompletionItem, Documentation};
use crate::Editor;

/// Height of one row.
const ROW_HEIGHT: f32 = 22.;
/// Width of the list.
const LIST_WIDTH: f32 = 420.;
/// Width of the documentation panel.
const DOCS_WIDTH: f32 = 340.;

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

/// The documentation side panel of the selected item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocPanel {
    pub detail: Option<SharedString>,
    pub blocks: Vec<Block>,
}

impl DocPanel {
    /// The panel for `item` (empty when it has neither detail nor docs).
    pub fn for_item(item: &CompletionItem) -> Self {
        let blocks = item
            .documentation
            .as_ref()
            .map(blocks_for)
            .unwrap_or_default();
        // Snippets show their body when they have no documentation.
        let is_snippet = item.candidate.source == CandidateSource::Snippet;
        let blocks = if blocks.is_empty() && is_snippet && item.candidate.insert.is_snippet() {
            blocks_for(&Documentation::Markdown(format!(
                "```\n{}\n```",
                item.candidate.insert.text()
            )))
        } else {
            blocks
        };
        DocPanel {
            detail: item.candidate.detail.clone().map(SharedString::from),
            blocks,
        }
    }

    fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// The popup view.
pub(crate) struct MenuView {
    editor: WeakEntity<Editor>,
    rows: Vec<Row>,
    selected: usize,
    scroll_top: usize,
    docs: Option<DocPanel>,
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
            docs: None,
            wheel: 0.,
        }
    }

    /// Replaces the shown state; redraws only when it changed.
    pub fn set_state(
        &mut self,
        rows: Vec<Row>,
        selected: usize,
        scroll_top: usize,
        docs: Option<DocPanel>,
        cx: &mut Context<Self>,
    ) {
        let docs = docs.filter(|d| !d.is_empty());
        if rows == self.rows
            && selected == self.selected
            && scroll_top == self.scroll_top
            && docs == self.docs
        {
            return;
        }
        self.rows = rows;
        self.selected = selected;
        self.scroll_top = scroll_top;
        self.docs = docs;
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
            font_weight: Some(FontWeight::BOLD),
            ..base
        };
        let highlights: Vec<_> = if row.deprecated {
            vec![(0..row.label.len(), base)]
        } else {
            row.matched.iter().map(|r| (r.clone(), matched)).collect()
        };
        let label = StyledText::new(row.label.clone()).with_highlights(highlights);
        let editor = self.editor.clone();
        div()
            .id(("completion-row", index))
            .h(px(ROW_HEIGHT))
            .px_1p5()
            .mx_1()
            .rounded_sm()
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .when(selected, |d| d.bg(colors.selection))
            .when(!selected, |d| d.hover(|d| d.bg(colors.hover)))
            .cursor_pointer()
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
                    .text_size(px(settings(cx).ui.font_size - 1.))
                    .child(truncated(detail))
            }))
    }

    fn render_docs(&self, docs: &DocPanel, height: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let colors = &theme(cx).colors;
        let mono = SharedString::from(settings(cx).editor.font_family.clone());
        div()
            .id("completion-docs")
            .occlude()
            .w(px(DOCS_WIDTH))
            .max_h(height.max(px(120.)))
            .overflow_y_scroll()
            .px_2()
            .py_1p5()
            .flex()
            .flex_col()
            .gap_1()
            .bg(colors.surface_raised)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_md()
            .children(docs.detail.clone().map(|d| {
                div()
                    .font_family(mono)
                    .text_color(colors.text_muted)
                    .child(d)
            }))
            .children(render_blocks(&docs.blocks, cx))
            .into_any_element()
    }
}

impl Render for MenuView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors.clone();
        let ui = &settings(cx).ui;
        let font_size = px(ui.font_size);
        let font_family = ui.font_family.clone();
        let top = clamp_top(self.scroll_top, self.rows.len());
        let visible = self.rows.len().min(VISIBLE_ROWS);
        let list_height = px(ROW_HEIGHT * visible as f32 + 8.);
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
            let y = 4. + (track - h) * top as f32 / (total - VISIBLE_ROWS as f32);
            div()
                .absolute()
                .right(px(2.))
                .top(px(y))
                .w(px(3.))
                .h(px(h))
                .rounded_full()
                .bg(colors.scrollbar_thumb)
        });
        let list = div()
            .id("completion-list")
            .occlude()
            .relative()
            .w(px(LIST_WIDTH))
            .h(list_height)
            .py_1()
            .flex()
            .flex_col()
            .bg(colors.surface_raised)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_md()
            .on_scroll_wheel(cx.listener(|v, e, _, cx| v.scroll(e, cx)))
            .children(rows)
            .children(thumb);
        let docs = self
            .docs
            .clone()
            .map(|d| self.render_docs(&d, list_height, cx));
        let mut root = div()
            .flex()
            .flex_row()
            .items_start()
            .gap_1()
            .text_size(font_size)
            .text_color(colors.text)
            .child(list)
            .children(docs);
        if let Some(family) = font_family {
            root = root.font_family(SharedString::from(family));
        }
        root
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
