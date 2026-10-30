//! Inlay hints: asked from the provider for the visible lines plus a page
//! above and below, drawn as inline virtual text (set
//! [`INLAY_HINT_SET`](super::INLAY_HINT_SET)).
//!
//! Hints follow edits (the extension store maps their positions) until
//! the answer for the new text arrives, so typing does not flicker. They
//! are asked again 400 ms after the last edit, when scrolling leaves the
//! fetched lines, and when the provider says they are stale
//! ([`Editor::refresh_inlay_hints`], e.g. `workspace/inlayHint/refresh`).
//! Nothing is asked while they are turned off (`editor.inlay_hints`, or
//! the session-wide [`ToggleInlayHints`](crate::ToggleInlayHints)).

use std::ops::Range;
use std::time::Duration;

use arcode_ui_kit::{ColorKey, settings};
use gpui::{App, Context, Global, Task};

use super::{INLAY_HINT_SET, InlayHint};
use crate::Editor;
use crate::ext::VirtualText;

/// Wait after the last edit before asking again.
const EDIT_DEBOUNCE: Duration = Duration::from_millis(400);
/// Wait after scrolling out of the fetched lines.
const SCROLL_DEBOUNCE: Duration = Duration::from_millis(60);

/// Session-wide override of the `editor.inlay_hints` setting, flipped by
/// [`ToggleInlayHints`](crate::ToggleInlayHints).
#[derive(Debug, Default, Clone, Copy)]
pub struct InlayHintsToggle(pub Option<bool>);

impl Global for InlayHintsToggle {}

/// Whether inlay hints are shown (the toggle, else the setting).
pub fn inlay_hints_enabled(cx: &App) -> bool {
    cx.try_global::<InlayHintsToggle>()
        .and_then(|t| t.0)
        .unwrap_or_else(|| settings(cx).editor.inlay_hints)
}

pub(super) fn init(cx: &mut App) {
    if !cx.has_global::<InlayHintsToggle>() {
        cx.set_global(InlayHintsToggle::default());
    }
}

/// Inlay hint requests of one editor.
#[derive(Default)]
pub(crate) struct InlayState {
    /// Whether hints were on at the last render.
    enabled: bool,
    seq: u64,
    task: Option<Task<()>>,
    /// Buffer version and lines of the last answered (or impossible)
    /// request.
    fetched: Option<(u64, Range<usize>)>,
}

/// The lines asked for when `visible` is shown: one page above and below.
pub(crate) fn wanted_lines(visible: Range<usize>, line_count: usize) -> Range<usize> {
    let page = visible.len().max(1);
    let start = visible.start.saturating_sub(page);
    let end = (visible.end + page).min(line_count);
    start..end.max(start)
}

/// The virtual text for `hint`.
pub(crate) fn hint_text(hint: &InlayHint) -> VirtualText {
    let mut label = hint.label.replace(['\n', '\r'], " ");
    if hint.padding_left {
        label.insert(0, ' ');
    }
    if hint.padding_right {
        label.push(' ');
    }
    let mut text = VirtualText::inline(hint.position, label);
    text.color = ColorKey::new("text_muted");
    text
}

impl Editor {
    fn inlay_active(&self) -> bool {
        self.lang.inlay.enabled && self.lang.provider.is_some()
    }

    /// Asks the provider for hints again (they are stale, e.g. the server
    /// finished indexing). Keeps the shown hints until the answer.
    pub fn refresh_inlay_hints(&mut self, cx: &mut Context<Self>) {
        self.lang.inlay.fetched = None;
        if self.inlay_active() {
            self.inlay_schedule(SCROLL_DEBOUNCE, cx);
        }
    }

    /// The hints shown, as `(position, text)`.
    pub fn inlay_hint_labels(&self) -> Vec<(arcode_buffer::Point, String)> {
        self.virtual_text(INLAY_HINT_SET)
            .iter()
            .filter_map(|v| match v.position {
                crate::VirtualTextPosition::Inline(p) => Some((p, v.text.to_string())),
                crate::VirtualTextPosition::EndOfLine(_) => None,
            })
            .collect()
    }

    /// Flips the session-wide inlay hint toggle (all editors follow).
    pub fn toggle_inlay_hints(&mut self, cx: &mut Context<Self>) {
        let enabled = !inlay_hints_enabled(cx);
        cx.set_global(InlayHintsToggle(Some(enabled)));
        self.inlay_sync(cx);
        cx.refresh_windows();
    }

    /// Called on render: follows the setting and the toggle.
    pub(crate) fn inlay_sync(&mut self, cx: &mut Context<Self>) {
        let enabled = inlay_hints_enabled(cx);
        if enabled != self.lang.inlay.enabled {
            self.lang.inlay.enabled = enabled;
            self.inlay_reset(cx);
        }
    }

    /// Drops the hints and any request; the next layout asks again.
    pub(crate) fn inlay_reset(&mut self, cx: &mut Context<Self>) {
        let inlay = &mut self.lang.inlay;
        inlay.task = None;
        inlay.fetched = None;
        inlay.seq += 1;
        self.set_virtual_text(INLAY_HINT_SET, Vec::new(), cx);
    }

    pub(crate) fn inlay_after_edit(&mut self, cx: &mut Context<Self>) {
        if self.inlay_active() {
            self.inlay_schedule(EDIT_DEBOUNCE, cx);
        }
    }

    /// Called after each layout: asks for the lines in view when the last
    /// answer does not cover them.
    pub(crate) fn inlay_after_layout(&mut self, cx: &mut Context<Self>) {
        if !self.inlay_active() || self.lang.inlay.task.is_some() {
            return;
        }
        let wanted = self.inlay_wanted();
        let version = self.buffer.version();
        let delay = match &self.lang.inlay.fetched {
            Some((v, lines))
                if *v == version && lines.start <= wanted.start && wanted.end <= lines.end =>
            {
                return;
            }
            Some((v, _)) if *v == version => SCROLL_DEBOUNCE,
            None => Duration::ZERO,
            Some(_) => EDIT_DEBOUNCE,
        };
        self.inlay_schedule(delay, cx);
    }

    fn inlay_wanted(&self) -> Range<usize> {
        let count = self.buffer.line_count();
        match self.layout.as_ref().map(|l| l.visible_range()) {
            Some(visible) if !visible.is_empty() => wanted_lines(visible, count),
            _ => 0..count.min(100),
        }
    }

    fn inlay_schedule(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.lang.inlay.seq += 1;
        let seq = self.lang.inlay.seq;
        self.lang.inlay.task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            this.update(cx, |e, cx| e.inlay_request(seq, cx)).ok();
        }));
    }

    fn inlay_request(&mut self, seq: u64, cx: &mut Context<Self>) {
        if seq != self.lang.inlay.seq || !self.inlay_active() {
            return;
        }
        let lines = self.inlay_wanted();
        let version = self.buffer.version();
        let Some(provider) = self.lang.provider.clone() else {
            return;
        };
        let Some(request) = provider.inlay_hints(self, lines.clone(), cx) else {
            self.lang.inlay.task = None;
            self.lang.inlay.fetched = Some((version, lines));
            return;
        };
        self.lang.inlay.task = Some(cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |e, cx| {
                if seq != e.lang.inlay.seq {
                    return;
                }
                e.lang.inlay.task = None;
                e.lang.inlay.fetched = Some((version, lines));
                match answer {
                    Ok(hints) if e.buffer.version() == version => {
                        let texts = hints.iter().map(hint_text).collect();
                        e.set_virtual_text(INLAY_HINT_SET, texts, cx);
                    }
                    Ok(_) => {}
                    Err(err) => log::debug!("inlay hints failed: {err}"),
                }
            })
            .ok();
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::InlayHintKind;
    use arcode_buffer::Point;

    #[test]
    fn wanted_lines_add_a_page_each_way() {
        assert_eq!(wanted_lines(100..140, 1000), 60..180);
        assert_eq!(wanted_lines(0..40, 50), 0..50);
        assert_eq!(wanted_lines(0..0, 0), 0..0);
    }

    #[test]
    fn hint_padding_becomes_spaces() {
        let hint = InlayHint {
            position: Point::new(1, 4),
            label: "count:".into(),
            kind: InlayHintKind::Parameter,
            padding_left: false,
            padding_right: true,
        };
        let text = hint_text(&hint);
        assert_eq!(text.text.as_ref(), "count: ");
        let hint = InlayHint {
            label: ": Vec<\n i32>".into(),
            padding_left: true,
            padding_right: false,
            ..hint
        };
        assert_eq!(hint_text(&hint).text.as_ref(), " : Vec<  i32>");
    }
}
