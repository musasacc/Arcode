//! Hover: documentation and diagnostics for the symbol under the mouse
//! (after `editor.hover_delay_ms`) or the cursor ([`crate::ShowHover`],
//! Ctrl+K Ctrl+I), and the definition link drawn under a symbol while the
//! secondary modifier (Ctrl, Cmd on macOS) is held.
//!
//! The editor knows nothing about language servers: a [`HoverProvider`]
//! (the app's LSP bridge) answers with plain [`HoverInfo`]. Diagnostics
//! under the mouse show at once; the provider's documentation follows them
//! in the same popover. The popover stays while the mouse is inside it, so
//! long documentation can be scrolled.
//!
//! The popover is placed during prepaint from the frame's own layout, so it
//! follows the text when the view moves. A mouse hover closes when the view
//! scrolls; any hover closes when its line leaves the view or when the
//! window's focus moves elsewhere (a picker, the terminal, another editor),
//! so it never floats over the workspace's overlays.

mod mouse;
mod view;

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use arcode_buffer::{Point, ops};
use gpui::{Bounds, Context, Pixels, Task, WeakFocusHandle, Window, point};

use crate::Editor;
use crate::completion::docs::{Block, blocks_for};
use crate::completion::{Documentation, is_word_char};
use crate::diagnostics::EditorDiagnostic;
use crate::layout::LayoutSnapshot;

pub(crate) use view::defer_hover_popover;

/// What a [`HoverProvider`] knows about a symbol.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HoverInfo {
    /// Documentation sections, shown in order with a rule between them.
    pub contents: Vec<Documentation>,
    /// The symbol's range; the hover stays while the mouse is inside it.
    pub range: Option<Range<Point>>,
}

/// Supplies hover documentation and definition links, such as a language
/// server. Calls happen on the main thread; long work belongs in the
/// returned task. `None` means the provider cannot answer now.
pub trait HoverProvider {
    /// Documentation for the symbol at `at`; `Ok(None)` shows nothing.
    fn hover(
        &self,
        editor: &Editor,
        at: Point,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<HoverInfo>>>>;

    /// While the secondary modifier is held over the symbol at `at`: the
    /// range to underline as a link when it has a definition.
    fn definition_link(
        &self,
        _editor: &Editor,
        _at: Point,
        _cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<Range<Point>>>>> {
        None
    }
}

/// The hover popover being shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HoverState {
    /// The buffer point it was computed for.
    pub at: Point,
    /// Diagnostics at `at`, shown first.
    pub diagnostics: Vec<EditorDiagnostic>,
    /// The provider's documentation, parsed.
    pub docs: Vec<Block>,
    /// The symbol the docs describe (the popover stays while the mouse is
    /// inside it).
    pub range: Option<Range<Point>>,
    /// The char the popover is attached to (placed below or above it).
    pub anchor: Point,
    /// Opened from the keyboard: stays until the cursor moves.
    pub keyboard: bool,
}

/// Hover requests and the definition link of one editor.
#[derive(Default)]
pub(crate) struct HoverTracker {
    pub provider: Option<Rc<dyn HoverProvider>>,
    /// Word (char range) a hover request is scheduled or running for.
    pub pending: Option<Range<usize>>,
    /// The pending request came from the keyboard: mouse motion keeps it.
    pub pending_keyboard: bool,
    pub task: Option<Task<()>>,
    pub seq: u64,
    /// Where the popover was drawn (window coordinates), from prepaint.
    pub popover_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Underlined definition link while the secondary modifier is held.
    pub link: Option<Range<Point>>,
    /// Word the link was requested for.
    pub link_word: Option<Range<usize>>,
    pub link_task: Option<Task<()>>,
    /// The window's focus at the last render (to notice it moving).
    pub last_focus: Option<WeakFocusHandle>,
}

impl Editor {
    /// Sets (or removes) the hover and definition-link provider.
    pub fn set_hover_provider(&mut self, provider: Option<Rc<dyn HoverProvider>>) {
        self.hovering.provider = provider;
    }

    /// Shows the hover for the symbol at the cursor (diagnostics at once,
    /// documentation when the provider answers).
    pub fn show_hover(&mut self, cx: &mut Context<Self>) {
        let head = self.selections.primary().head;
        let word = ops::word_range_at(&self.buffer, head)
            .or_else(|| ops::word_range_at(&self.buffer, head.checked_sub(1)?));
        let at = self
            .buffer
            .char_to_point(word.as_ref().map_or(head, |w| w.start));
        let diagnostics = self.diagnostics_at(at);
        if !diagnostics.is_empty() {
            self.hover = self.hover_state(at, diagnostics, Vec::new(), None, true);
            cx.notify();
        }
        if let Some(word) = word {
            self.request_hover(word, at, Duration::ZERO, true, cx);
        }
    }

    /// Text of the shown hover documentation (blocks joined by newlines).
    pub fn hover_docs_text(&self) -> Option<String> {
        let hover = self.hover.as_ref().filter(|h| !h.docs.is_empty())?;
        Some(
            hover
                .docs
                .iter()
                .map(block_text)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Messages of the diagnostics in the shown hover.
    pub fn hover_diagnostics(&self) -> Vec<String> {
        self.hover
            .as_ref()
            .map(|h| h.diagnostics.iter().map(|d| d.message.clone()).collect())
            .unwrap_or_default()
    }

    /// The underlined definition link, if any.
    pub fn hover_link(&self) -> Option<Range<Point>> {
        self.hovering.link.clone()
    }

    /// Hides the hover and drops requests in flight. Returns whether a
    /// popover or link was shown.
    pub(crate) fn hide_hover(&mut self) -> bool {
        self.cancel_hover_request();
        self.hovering.link_word = None;
        self.hovering.link_task = None;
        let link = self.hovering.link.take().is_some();
        self.hover.take().is_some() || link
    }

    pub(crate) fn cancel_hover_request(&mut self) {
        self.hovering.seq += 1;
        self.hovering.pending = None;
        self.hovering.pending_keyboard = false;
        self.hovering.task = None;
    }

    /// Keyboard hovers (shown or still asked for) close when the cursor
    /// moves.
    pub(crate) fn hover_after_cursor_move(&mut self) {
        if self.hover.as_ref().is_some_and(|h| h.keyboard) {
            self.hover = None;
            self.cancel_hover_request();
        } else if self.hovering.pending_keyboard {
            self.cancel_hover_request();
        }
    }

    /// Called on render: when the window's focus moved and this editor does
    /// not have it, the hover closes (it is drawn above everything, so it
    /// would cover a picker or the finder that took the focus).
    pub(crate) fn hover_check_focus(&mut self, window: &Window, cx: &gpui::App) {
        let now = window.focused(cx).map(|f| f.downgrade());
        if now == self.hovering.last_focus {
            return;
        }
        self.hovering.last_focus = now;
        if !self.focus_handle.is_focused(window) {
            self.hide_hover();
        }
    }

    /// Called after each layout: a mouse hover closes when the view
    /// scrolled (the mouse is no longer over its symbol), any hover when
    /// its line left the view.
    pub(crate) fn hover_after_layout(&mut self, previous: Option<(Pixels, Pixels)>) {
        let Some(hover) = &self.hover else {
            return;
        };
        let Some(layout) = &self.layout else {
            return;
        };
        let scrolled = previous.is_some_and(|p| p != (layout.scroll_x, layout.scroll_y));
        if (scrolled && !hover.keyboard) || layout.visible(hover.anchor.line).is_none() {
            self.hover = None;
            self.cancel_hover_request();
        }
    }

    /// Diagnostics changed: refresh those in the hover, keep its docs.
    pub(crate) fn hover_after_diagnostics(&mut self) {
        let Some(mut hover) = self.hover.take() else {
            return;
        };
        hover.diagnostics = self.diagnostics_at(hover.at);
        if !hover.diagnostics.is_empty() || !hover.docs.is_empty() {
            self.hover = Some(hover);
        }
    }

    /// Diagnostics whose range covers `at` (an empty range matches its
    /// own column).
    pub(crate) fn diagnostics_at(&self, at: Point) -> Vec<EditorDiagnostic> {
        let line_len = self.buffer.line_len(at.line);
        let key = (at.line, at.column);
        self.diagnostics
            .iter()
            .filter(|d| {
                let (s, e) = (d.range.start, d.range.end);
                if s == e {
                    s.line == at.line && s.column.min(line_len) == at.column
                } else {
                    key >= (s.line, s.column) && key < (e.line, e.column)
                }
            })
            .cloned()
            .collect()
    }

    /// The identifier (char range) containing the char at `at`.
    pub(crate) fn word_under(&self, at: Point) -> Option<Range<usize>> {
        let pos = self.buffer.point_to_char(at);
        let c = self.buffer.char_at(pos)?;
        if !is_word_char(c) {
            return None;
        }
        ops::word_range_at(&self.buffer, pos)
    }

    /// A popover state anchored at `range.start` (else `at`); `None` when
    /// that line is not laid out.
    pub(crate) fn hover_state(
        &self,
        at: Point,
        diagnostics: Vec<EditorDiagnostic>,
        docs: Vec<Block>,
        range: Option<Range<Point>>,
        keyboard: bool,
    ) -> Option<HoverState> {
        let layout = self.layout.as_ref()?;
        let anchor = match (&range, diagnostics.first()) {
            (Some(r), _) => r.start,
            (None, Some(d)) if d.range.start.line == at.line => d.range.start,
            _ => Point::new(at.line, 0),
        };
        layout.visible(anchor.line)?;
        Some(HoverState {
            at,
            diagnostics,
            docs,
            range,
            anchor,
            keyboard,
        })
    }

    /// Asks the provider for the hover of `word` at `at` after `delay`.
    pub(crate) fn request_hover(
        &mut self,
        word: Range<usize>,
        at: Point,
        delay: Duration,
        keyboard: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.hovering.provider.clone() else {
            return;
        };
        self.cancel_hover_request();
        self.hovering.pending = Some(word.clone());
        self.hovering.pending_keyboard = keyboard;
        let seq = self.hovering.seq;
        self.hovering.task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some(request)) = this.update(cx, |e, cx| provider.hover(e, at, cx)) else {
                return;
            };
            let answer = request.await;
            this.update(cx, |e, cx| {
                e.hover_arrived(seq, word, at, keyboard, answer, cx)
            })
            .ok();
        }));
    }

    fn hover_arrived(
        &mut self,
        seq: u64,
        word: Range<usize>,
        at: Point,
        keyboard: bool,
        answer: anyhow::Result<Option<HoverInfo>>,
        cx: &mut Context<Self>,
    ) {
        if seq != self.hovering.seq {
            return;
        }
        self.hovering.pending_keyboard = false;
        let info = match answer {
            Ok(Some(info)) => info,
            Ok(None) => return,
            Err(err) => {
                log::debug!("hover failed: {err:#}");
                return;
            }
        };
        let docs = hover_blocks(&info.contents, self.language.id);
        if docs.is_empty() {
            return;
        }
        let range = info.range.unwrap_or_else(|| {
            self.buffer.char_to_point(word.start)..self.buffer.char_to_point(word.end)
        });
        let diagnostics = self.diagnostics_at(at);
        if let Some(state) = self.hover_state(at, diagnostics, docs, Some(range), keyboard) {
            self.hover = Some(state);
            cx.notify();
        }
    }
}

/// Window position below `p` and the top of its line, if laid out.
pub(crate) fn anchor_position(
    layout: &LayoutSnapshot,
    p: Point,
) -> Option<(gpui::Point<Pixels>, Pixels)> {
    layout.visible(p.line)?;
    let top = layout.line_top(p.line);
    let x = layout.text_left() + layout.x_for(p.line, p.column);
    Some((point(x, top + layout.line_height), top))
}

/// Blocks of every section with a rule between sections. Code blocks
/// without a language tag are colored as the document's language.
pub(crate) fn hover_blocks(contents: &[Documentation], language: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for doc in contents {
        let blocks = blocks_for(doc);
        if blocks.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(Block::Rule);
        }
        out.extend(blocks.into_iter().map(|b| match b {
            Block::Code {
                language: None,
                text,
            } => Block::Code {
                language: Some(language.to_string()),
                text,
            },
            other => other,
        }));
    }
    out
}

/// Plain text of a block (for tests and accessibility).
fn block_text(block: &Block) -> String {
    match block {
        Block::Heading(i) | Block::Paragraph(i) | Block::Bullet(i) => i.text.clone(),
        Block::Code { text, .. } => text.clone(),
        Block::Rule => "---".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_are_separated_and_untagged_code_gets_the_document_language() {
        let blocks = hover_blocks(
            &[
                Documentation::Markdown("```\nfn a()\n```".into()),
                Documentation::PlainText(String::new()),
                Documentation::Markdown("Adds *two* numbers.".into()),
            ],
            "rust",
        );
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[0],
            Block::Code {
                language: Some("rust".into()),
                text: "fn a()".into()
            }
        );
        assert_eq!(blocks[1], Block::Rule);
        assert_eq!(block_text(&blocks[2]), "Adds two numbers.");
    }
}
