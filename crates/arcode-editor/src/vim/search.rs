//! Highlighting the matches of Vim's last search (`/`, `?`, `*`, `#`)
//! until `:noh`.
//!
//! Only the lines around the viewport are searched (a window of
//! [`MARGIN`] lines on each side, at most [`MAX_WINDOW_CHARS`] chars), so a
//! search or an edit in a huge file stays cheap. The window is searched
//! again when the view scrolls out of it, the search changes, or Vim is
//! back in Normal mode after edits.

use std::ops::Range;

use arcode_buffer::Buffer;
use arcode_vim::SearchQuery;
use gpui::Context;

use super::SharedVim;
use crate::Editor;
use crate::ext::BackgroundHighlight;

/// The decoration set of the search highlights.
pub(crate) const SEARCH_SET: &str = "vim-search";

/// Lines searched above and below the viewport.
const MARGIN: usize = 200;

/// Most chars searched at once (a huge single-line file).
const MAX_WINDOW_CHARS: usize = 1_000_000;

/// More matches than this in the window are not highlighted.
const MAX_MATCHES: usize = 10_000;

/// What was highlighted last.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Highlighted {
    query: SearchQuery,
    version: u64,
    /// Lines searched.
    lines: Range<usize>,
}

/// Simple lowercase mapping of one char (as in `arcode_buffer::search`).
fn fold(c: char, case_sensitive: bool) -> char {
    if case_sensitive {
        return c;
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Non-overlapping literal matches of `query` (whole words for `*` / `#`)
/// that start in `range`, a char range of `buffer`.
pub(crate) fn matches_in(
    buffer: &Buffer,
    query: &SearchQuery,
    range: Range<usize>,
) -> Vec<Range<usize>> {
    let fold = |c| fold(c, query.case_sensitive);
    let pattern: Vec<char> = query.pattern.chars().map(fold).collect();
    let mut out = Vec::new();
    if pattern.is_empty() || range.is_empty() {
        return out;
    }
    // Matches may run past the end of the range.
    let end = (range.end + pattern.len() - 1).min(buffer.len_chars());
    let text: Vec<char> = buffer
        .text()
        .slice(range.start..end)
        .chars()
        .map(fold)
        .collect();
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut i = 0;
    while i + pattern.len() <= text.len() && i < range.len() && out.len() < MAX_MATCHES {
        if text[i..i + pattern.len()] != pattern[..] {
            i += 1;
            continue;
        }
        let r = range.start + i..range.start + i + pattern.len();
        let whole = !query.whole_word
            || (!is_word(r.start.checked_sub(1).and_then(|p| buffer.char_at(p)))
                && !is_word(buffer.char_at(r.end)));
        if whole {
            out.push(r);
            i += pattern.len();
        } else {
            i += 1;
        }
    }
    out
}

/// The lines to search for a viewport showing `visible`.
fn window(buffer: &Buffer, visible: Range<usize>) -> Range<usize> {
    let count = buffer.line_count();
    let start = visible.start.saturating_sub(MARGIN).min(count);
    let end = (visible.end + MARGIN).min(count).max(start);
    start..end
}

impl Editor {
    /// The lines in view (the first lines before the first layout).
    fn vim_visible_lines(&self) -> Range<usize> {
        match self.layout.as_ref().map(|l| l.visible_range()) {
            Some(r) if !r.is_empty() => r,
            _ => 0..self.buffer.line_count().min(100),
        }
    }

    /// Re-highlights the matches when the search changed, or when Vim is
    /// back in Normal mode after edits (highlights follow edits in
    /// between, so typing never rescans). `query` is Vim's highlighted
    /// search (read while the shared state was swapped in).
    pub(super) fn vim_update_search(&mut self, query: Option<SearchQuery>, cx: &mut Context<Self>) {
        let Some(state) = self.vim.as_ref() else {
            return;
        };
        let version = self.buffer.version();
        let idle = matches!(state.vim.mode(), arcode_vim::Mode::Normal);
        let changed = match (&query, &state.highlighted) {
            (None, None) => false,
            (Some(q), Some(old)) => {
                *q != old.query || (idle && old.version != version) || {
                    let visible = self.vim_visible_lines();
                    visible.start < old.lines.start || visible.end > old.lines.end
                }
            }
            _ => true,
        };
        if changed {
            let search_changed = query.as_ref() != state.highlighted.as_ref().map(|h| &h.query);
            self.vim_highlight(query, cx);
            if search_changed {
                // Other editors show the same search (it is shared).
                cx.refresh_windows();
            }
        }
    }

    /// After each layout: follows the shared search (it may have changed
    /// in another editor) and the viewport.
    pub(crate) fn vim_after_layout(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.vim.as_ref().filter(|s| !s.handling) else {
            return;
        };
        let query = cx
            .try_global::<SharedVim>()
            .and_then(|s| s.0.search_query())
            .cloned();
        let visible = self.vim_visible_lines();
        let stale = match (&query, &state.highlighted) {
            (None, None) => false,
            (Some(q), Some(old)) => {
                *q != old.query || visible.start < old.lines.start || visible.end > old.lines.end
            }
            _ => true,
        };
        if stale {
            self.vim_highlight(query, cx);
        }
    }

    /// Highlights the matches of `query` around the viewport.
    fn vim_highlight(&mut self, query: Option<SearchQuery>, cx: &mut Context<Self>) {
        let lines = window(&self.buffer, self.vim_visible_lines());
        let buffer = &self.buffer;
        let start = buffer.line_start(lines.start);
        let end = buffer.line_start(lines.end).min(start + MAX_WINDOW_CHARS);
        let highlights = match &query {
            Some(q) => matches_in(buffer, q, start..end)
                .into_iter()
                .map(|r| {
                    let range = buffer.char_to_point(r.start)..buffer.char_to_point(r.end);
                    BackgroundHighlight::text(
                        range,
                        arcode_ui_kit::ColorKey::new("warning").opacity(0.28),
                    )
                })
                .collect(),
            None => Vec::new(),
        };
        let version = buffer.version();
        if let Some(state) = self.vim.as_mut() {
            state.highlighted = query.map(|query| Highlighted {
                query,
                version,
                lines,
            });
        }
        self.set_background_highlights(SEARCH_SET, highlights, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(pattern: &str, whole_word: bool, case_sensitive: bool) -> SearchQuery {
        SearchQuery {
            pattern: pattern.into(),
            whole_word,
            case_sensitive,
            forward: true,
        }
    }

    fn all(b: &Buffer, q: &SearchQuery) -> Vec<Range<usize>> {
        matches_in(b, q, 0..b.len_chars())
    }

    #[test]
    fn whole_word_and_case() {
        let b = Buffer::new("foo food Foo foo_bar foo");
        assert_eq!(all(&b, &query("foo", false, true)).len(), 4);
        assert_eq!(all(&b, &query("foo", true, true)), vec![0..3, 21..24]);
        assert_eq!(all(&b, &query("foo", true, false)).len(), 3);
        assert_eq!(
            all(&b, &query("aa", false, true)),
            Vec::<Range<usize>>::new()
        );
        assert_eq!(
            all(&Buffer::new("aaaa"), &query("aa", false, true)),
            vec![0..2, 2..4]
        );
    }

    #[test]
    fn only_matches_starting_in_the_range() {
        let b = Buffer::new("ab ab ab ab");
        // A match may end past the range.
        assert_eq!(
            matches_in(&b, &query("ab", false, true), 3..7),
            vec![3..5, 6..8]
        );
        assert!(matches_in(&b, &query("ab", false, true), 4..5).is_empty());
        // Whole-word checks look outside the range.
        assert!(matches_in(&b, &query("b", true, true), 1..2).is_empty());
    }

    #[test]
    fn the_window_surrounds_the_viewport() {
        let b = Buffer::new(&"x\n".repeat(1000));
        assert_eq!(window(&b, 500..540), 300..740);
        assert_eq!(window(&b, 0..40), 0..240);
        assert_eq!(window(&b, 990..1001), 790..1001);
    }
}
