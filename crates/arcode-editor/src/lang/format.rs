//! Formatting: the document (Shift+Alt+F), the selection (Ctrl+K Ctrl+F)
//! and format on save (`editor.format_on_save`). The provider's edits are
//! trimmed to what really changes and applied as one undo step; a cursor
//! inside a rewritten range keeps its line and column. Saving waits for
//! the formatter at most `editor.format_on_save_timeout_ms`, then saves
//! whatever the buffer holds, so saving never hangs.

use std::ops::Range;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_ui_kit::settings;
use gpui::{Context, Task};

use super::{TextEdits, with_timeout};
use crate::Editor;

/// Formatting of one editor.
#[derive(Default)]
pub(crate) struct FormatState {
    task: Option<Task<()>>,
    /// A formatted save is running; another save request waits for it.
    saving: bool,
}

/// Common prefix and suffix lengths (in chars) of `a` and `b`, not
/// overlapping.
fn common_ends(a: &str, b: &str) -> (usize, usize) {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    (prefix, suffix)
}

impl Editor {
    /// Formats the whole document.
    pub fn format_document(&mut self, cx: &mut Context<Self>) {
        self.format_range(None, cx);
    }

    /// Formats the selection (the cursor line without one).
    pub fn format_selection(&mut self, cx: &mut Context<Self>) {
        let sel = self.selections.primary();
        let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        let range = if a == b {
            let line = self.buffer.char_to_line(a);
            Point::new(line, 0)..Point::new(line, self.buffer.line_len(line))
        } else {
            self.buffer.char_to_point(a)..self.buffer.char_to_point(b)
        };
        self.format_range(Some(range), cx);
    }

    fn format_range(&mut self, range: Option<Range<Point>>, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let request = self
            .lang
            .provider
            .clone()
            .and_then(|p| p.format(self, range, cx));
        let Some(request) = request else {
            self.show_notice("No formatter for this file", cx);
            return;
        };
        let version = self.buffer.version();
        self.lang.format.task = Some(cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |e, cx| match answer {
                Ok(edits) if e.buffer.version() == version => {
                    e.apply_format(edits, cx);
                }
                Ok(_) => log::debug!("formatting dropped: the buffer changed"),
                Err(err) => e.show_notice(format!("Formatting failed: {err}"), cx),
            })
            .ok();
        }));
    }

    /// Applies formatter edits (trimmed to the changed text) as one undo
    /// step. Returns whether the buffer changed.
    pub fn apply_format(&mut self, edits: TextEdits, cx: &mut Context<Self>) -> bool {
        let edits = self.minimize_edits(edits);
        if edits.is_empty() {
            return false;
        }
        let heads: Vec<Point> = self
            .selections
            .iter()
            .map(|s| self.buffer.char_to_point(s.head))
            .collect();
        let rewritten = heads
            .iter()
            .any(|p| edits.iter().any(|(r, _)| r.start < *p && *p < r.end));
        if !self.apply_edits(edits, cx) {
            return false;
        }
        if rewritten {
            let cursors: Vec<Range<Point>> = heads.into_iter().map(|p| p..p).collect();
            self.select_ranges(cursors, cx);
        }
        true
    }

    /// Shrinks each edit to the part that differs from the buffer and drops
    /// edits that change nothing.
    fn minimize_edits(&self, edits: TextEdits) -> TextEdits {
        edits
            .into_iter()
            .filter_map(|(range, text)| {
                let text = text.replace("\r\n", "\n");
                let old = self.text_in_range(range.clone());
                if old == text {
                    return None;
                }
                let (prefix, suffix) = common_ends(&old, &text);
                let start = self.point_to_offset(range.start) + prefix;
                let end = self.point_to_offset(range.end) - suffix;
                let middle: String = text
                    .chars()
                    .skip(prefix)
                    .take(text.chars().count() - prefix - suffix)
                    .collect();
                Some((self.offset_to_point(start)..self.offset_to_point(end), middle))
            })
            .collect()
    }

    /// Saves, formatting first when `editor.format_on_save` is on (waiting
    /// for the formatter at most `editor.format_on_save_timeout_ms`).
    pub fn save_with_format(&mut self, cx: &mut Context<Self>) {
        if self.lang.format.saving {
            return;
        }
        let config = &settings(cx).editor;
        let timeout = Duration::from_millis(config.format_on_save_timeout_ms);
        let request = (config.format_on_save && !self.read_only)
            .then(|| self.lang.provider.clone())
            .flatten()
            .and_then(|p| p.format(self, None, cx));
        let Some(request) = request else {
            self.save_logged(cx);
            return;
        };
        let version = self.buffer.version();
        self.lang.format.saving = true;
        self.lang.format.task = Some(cx.spawn(async move |this, cx| {
            let timer = cx.background_executor().timer(timeout);
            let answer = with_timeout(request, timer).await;
            this.update(cx, |e, cx| {
                e.lang.format.saving = false;
                match answer {
                    Some(Ok(edits)) if e.buffer.version() == version => {
                        e.apply_format(edits, cx);
                    }
                    Some(Ok(_)) => log::debug!("format on save dropped: the buffer changed"),
                    Some(Err(err)) => log::warn!("format on save failed: {err}"),
                    None => log::warn!("format on save timed out after {timeout:?}"),
                }
                e.save_logged(cx);
            })
            .ok();
        }));
    }

    /// Whether a formatted save is waiting for the formatter.
    pub fn is_saving(&self) -> bool {
        self.lang.format.saving
    }

    fn save_logged(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.save(cx) {
            log::error!("failed to save {}: {err}", self.title());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::common_ends;

    #[test]
    fn common_ends_do_not_overlap() {
        assert_eq!(common_ends("fn a(){}", "fn a() {}"), (6, 2));
        assert_eq!(common_ends("aaa", "aa"), (2, 0));
        assert_eq!(common_ends("same", "same"), (4, 0));
        assert_eq!(common_ends("", "x"), (0, 0));
        assert_eq!(common_ends("äbc", "äxc"), (1, 1));
    }
}
