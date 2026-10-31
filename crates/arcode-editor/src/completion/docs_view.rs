//! The documentation panel next to the completion list. It is the list
//! popover's side view: placed right of the list (left when there is no
//! room), top-aligned with it below the cursor and bottom-aligned above.

use arcode_completion::CandidateSource;
use arcode_ui_kit::{popover, settings, theme, type_scale, ui_font_family};
use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use super::docs::{Block, blocks_for, render_blocks};
use super::{CompletionItem, Documentation};

/// Width of the panel.
const DOCS_WIDTH: f32 = 340.;
/// The panel is at least this tall (when its text needs it).
const MIN_MAX_HEIGHT: f32 = 120.;

/// The documentation of the selected item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocPanel {
    pub detail: Option<SharedString>,
    pub blocks: Vec<Block>,
}

impl DocPanel {
    /// The panel for `item`; `None` when it has no documentation.
    pub fn for_item(item: &CompletionItem) -> Option<Self> {
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
        (!blocks.is_empty()).then(|| DocPanel {
            detail: item.candidate.detail.clone().map(SharedString::from),
            blocks,
        })
    }
}

/// The panel view; draws nothing without documentation.
pub(crate) struct DocsView {
    docs: Option<DocPanel>,
    /// Height of the list next to it.
    list_height: Pixels,
}

impl DocsView {
    pub fn new() -> Self {
        DocsView {
            docs: None,
            list_height: px(0.),
        }
    }

    /// Replaces the shown docs; redraws only when they changed.
    pub fn set_docs(
        &mut self,
        docs: Option<DocPanel>,
        list_height: Pixels,
        cx: &mut Context<Self>,
    ) {
        if docs == self.docs && list_height == self.list_height {
            return;
        }
        self.docs = docs;
        self.list_height = list_height;
        cx.notify();
    }
}

impl Render for DocsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(docs) = &self.docs else {
            return div().into_any_element();
        };
        let colors = &theme(cx).colors;
        let mono = SharedString::from(settings(cx).editor.font_family.clone());
        // The padding sits outside the scrolling part, so text scrolled
        // past the bottom ends 8 px above the edge instead of being cut
        // right at the border.
        popover(cx)
            .id("completion-docs")
            .occlude()
            .w(px(DOCS_WIDTH))
            .max_h(self.list_height.max(px(MIN_MAX_HEIGHT)))
            .px(px(0.))
            .py(px(8.))
            .font_family(ui_font_family(cx))
            .text_size(type_scale(cx).body)
            .text_color(colors.text)
            .child(
                div()
                    .id("completion-docs-scroll")
                    .flex()
                    .flex_col()
                    .flex_initial()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(10.))
                    .gap(px(4.))
                    .children(docs.detail.clone().map(|d| {
                        div()
                            .font_family(mono)
                            .text_color(colors.text_muted)
                            .child(d)
                    }))
                    .children(render_blocks(&docs.blocks, cx)),
            )
            .into_any_element()
    }
}
