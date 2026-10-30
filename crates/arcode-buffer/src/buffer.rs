//! The [`Buffer`] type: text storage, metadata and line accessors.

use std::fmt;
use std::path::{Path, PathBuf};

use ropey::{Rope, RopeSlice};

use crate::history::History;
use crate::line_ending::{LineEnding, normalize};
use crate::line_index::ExtraBreaks;

/// A rope-backed text buffer.
///
/// Text is stored with `\n` line endings; the file's original
/// [`LineEnding`] (and UTF-8 BOM, if any) is restored on save.
///
/// Lines are delimited by `\n` only (like tree-sitter and LSP), even if
/// ropey is built with its `unicode_lines` feature: use the buffer's line
/// methods ([`Buffer::char_to_line`], [`Buffer::line_start`], ...) rather
/// than ropey's line methods on [`Buffer::text`].
#[derive(Debug, Clone)]
pub struct Buffer {
    pub(crate) rope: Rope,
    pub(crate) path: Option<PathBuf>,
    pub(crate) line_ending: LineEnding,
    /// Line ending at the last save/load.
    pub(crate) saved_line_ending: LineEnding,
    pub(crate) bom: bool,
    /// Positions of chars ropey counts as line breaks besides `\n`.
    pub(crate) extras: ExtraBreaks,
    pub(crate) version: u64,
    pub(crate) history: History,
    /// History state id at the last save/load.
    pub(crate) saved_state: u64,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer::new("")
    }
}

impl fmt::Display for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.rope.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl Buffer {
    /// Creates a buffer from `text`, normalizing `\r\n`/`\r` to `\n` and
    /// remembering the detected line ending.
    pub fn new(text: &str) -> Self {
        let line_ending = LineEnding::detect(text);
        let normalized = normalize(text);
        let rope = Rope::from_str(&normalized);
        let extras = ExtraBreaks::scan(&normalized);
        Buffer {
            rope,
            path: None,
            line_ending,
            saved_line_ending: line_ending,
            bom: false,
            extras,
            version: 0,
            history: History::default(),
            saved_state: 0,
        }
    }

    /// The underlying rope (always `\n` line endings).
    pub fn text(&self) -> &Rope {
        &self.rope
    }

    /// The file path this buffer is associated with, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Associates the buffer with `path` (does not touch the disk).
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = Some(path.into());
    }

    /// The line ending used when saving.
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Changes the line ending used when saving (bumps the version). The
    /// buffer is dirty while the line ending differs from the one at the
    /// last save/load.
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        if self.line_ending != line_ending {
            self.line_ending = line_ending;
            self.version += 1;
        }
    }

    /// Whether the file started with a UTF-8 byte order mark (kept on save).
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// Monotonic version, bumped on every change (edit, undo, redo).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// True if the text or line ending differs from the last save/load.
    /// Undoing back to the saved state (and restoring the line ending)
    /// makes the buffer clean again.
    pub fn is_dirty(&self) -> bool {
        self.history.current_state() != self.saved_state
            || self.line_ending != self.saved_line_ending
    }

    /// Declares the current text as saved (e.g. after an external write).
    pub fn mark_saved(&mut self) {
        self.saved_state = self.history.current_state();
        self.saved_line_ending = self.line_ending;
        self.history.break_merge();
    }

    /// Number of chars.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Number of UTF-8 bytes (in memory, i.e. with `\n` line endings).
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// True if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Number of lines. A trailing newline starts a final empty line, so
    /// `"a\n"` has two lines and `""` has one. Only `\n` breaks lines.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines() - self.extras.len()
    }

    /// Clamps a line index to the last line.
    pub(crate) fn clamp_line(&self, line: usize) -> usize {
        line.min(self.line_count().saturating_sub(1))
    }

    /// Line `line` including its line break. Out-of-range indices are
    /// clamped to the last line.
    pub fn line(&self, line: usize) -> RopeSlice<'_> {
        let line = self.clamp_line(line);
        self.rope
            .slice(self.line_start(line)..self.line_start(line + 1))
    }

    /// Line `line` without its line break (clamped like [`Buffer::line`]).
    pub fn line_text(&self, line: usize) -> String {
        let line = self.clamp_line(line);
        self.rope
            .slice(self.line_start(line)..self.line_end(line))
            .to_string()
    }

    /// Length of line `line` in chars, excluding its line break (clamped
    /// like [`Buffer::line`]).
    pub fn line_len(&self, line: usize) -> usize {
        let line = self.clamp_line(line);
        self.line_end(line) - self.line_start(line)
    }

    /// Char index of the start of line `line` (clamped; a line past the end
    /// yields the buffer length).
    pub fn line_start(&self, line: usize) -> usize {
        if line >= self.line_count() {
            return self.len_chars();
        }
        if self.extras.len() == 0 {
            return self.rope.line_to_char(line);
        }
        // `j` = number of extra breaks before the start of `line`: the
        // smallest index whose break lies on `line` or later.
        let (mut lo, mut hi) = (0, self.extras.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let newlines_before = self.rope.char_to_line(self.extras.get(mid)) - mid;
            if newlines_before < line {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        self.rope.line_to_char(line + lo)
    }

    /// Char index of the end of line `line`, before its line break
    /// (clamped like [`Buffer::line`]).
    pub fn line_end(&self, line: usize) -> usize {
        let line = self.clamp_line(line);
        if line + 1 < self.line_count() {
            self.line_start(line + 1) - 1
        } else {
            self.len_chars()
        }
    }

    /// Line containing `char_idx` (clamped).
    pub fn char_to_line(&self, char_idx: usize) -> usize {
        let char_idx = self.clamp_char(char_idx);
        self.rope.char_to_line(char_idx) - self.extras.count_before(char_idx)
    }

    /// The char at `char_idx`, or `None` past the end.
    pub fn char_at(&self, char_idx: usize) -> Option<char> {
        self.rope.get_char(char_idx)
    }

    /// Text in the char range `range` (clamped).
    pub fn slice_to_string(&self, range: std::ops::Range<usize>) -> String {
        let end = self.clamp_char(range.end);
        let start = self.clamp_char(range.start).min(end);
        self.rope.slice(start..end).to_string()
    }

    /// Clamps a char index to `0..=len_chars()`.
    pub fn clamp_char(&self, char_idx: usize) -> usize {
        char_idx.min(self.len_chars())
    }

    /// Whether an undo step is available.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether a redo step is available.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Ends the current mergeable undo step, so the next typing starts a
    /// new one (call on cursor jumps, focus changes, etc.).
    pub fn break_undo_group(&mut self) {
        self.history.break_merge();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_normalizes_and_detects() {
        let b = Buffer::new("a\r\nb\r\n");
        assert_eq!(b.to_string(), "a\nb\n");
        assert_eq!(b.line_ending(), LineEnding::CrLf);
        assert_eq!(b.line_count(), 3);
        let b = Buffer::new("x\ry");
        assert_eq!(b.to_string(), "x\ny");
        assert!(!b.is_dirty());
        assert_eq!(b.version(), 0);
    }

    #[test]
    fn default_is_empty() {
        let b = Buffer::default();
        assert!(b.is_empty());
        assert_eq!(b.line_count(), 1);
        assert_eq!(b.line_len(0), 0);
        assert_eq!(b.line_text(0), "");
        assert_eq!(b.path(), None);
    }

    #[test]
    fn line_accessors() {
        let b = Buffer::new("héllo\n😀 x\n\nlast");
        assert_eq!(b.len_chars(), 15);
        assert_eq!(b.len_bytes(), "héllo\n😀 x\n\nlast".len());
        assert_eq!(b.line_count(), 4);
        assert_eq!(b.line(0).to_string(), "héllo\n");
        assert_eq!(b.line_text(0), "héllo");
        assert_eq!(b.line_text(1), "😀 x");
        assert_eq!(b.line_len(1), 3);
        assert_eq!(b.line_text(2), "");
        assert_eq!(b.line_len(2), 0);
        assert_eq!(b.line_text(3), "last");
        assert_eq!(b.line_text(99), "last");
        assert_eq!(b.line_start(1), 6);
        assert_eq!(b.line_end(1), 9);
        assert_eq!(b.line_start(99), b.len_chars());
        assert_eq!(b.char_to_line(7), 1);
        assert_eq!(b.char_to_line(999), 3);
        assert_eq!(b.char_at(6), Some('😀'));
        assert_eq!(b.char_at(99), None);
        assert_eq!(b.slice_to_string(6..9), "😀 x");
        assert_eq!(b.slice_to_string(10..999), "\nlast");
    }

    #[test]
    fn trailing_newline_is_a_line() {
        let b = Buffer::new("a\n");
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.line_text(1), "");
        assert_eq!(b.line_len(0), 1);
    }

    #[test]
    fn unicode_line_separators_do_not_break_lines() {
        // ropey may treat these as line breaks; the buffer must not.
        let b = Buffer::new("a\u{2028}b\u{c}c\u{b}d\u{85}e\u{2029}f\ng\u{c}");
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.line_text(0), "a\u{2028}b\u{c}c\u{b}d\u{85}e\u{2029}f");
        assert_eq!(b.line_len(0), 11);
        assert_eq!(b.line_start(1), 12);
        assert_eq!(b.line_end(0), 11);
        assert_eq!(
            b.line(0).to_string(),
            "a\u{2028}b\u{c}c\u{b}d\u{85}e\u{2029}f\n"
        );
        assert_eq!(b.line(1).to_string(), "g\u{c}");
        assert_eq!(b.line_text(1), "g\u{c}");
        for i in 0..=11 {
            assert_eq!(b.char_to_line(i), 0, "{i}");
        }
        for i in 12..=b.len_chars() {
            assert_eq!(b.char_to_line(i), 1, "{i}");
        }
    }

    #[test]
    fn line_index_matches_naive_split_after_edits() {
        let mut b = Buffer::new("x\u{c}y\nz\u{2028}\n\u{c}\n");
        let edits: [(std::ops::Range<usize>, &str); 5] = [
            (1..2, "\n\u{c}\u{c}"),
            (0..0, "\u{2029}a\n"),
            (5..9, ""),
            (2..2, "\u{b}\u{85}\nq"),
            (0..3, "\u{c}"),
        ];
        for (range, text) in edits {
            b.edit(vec![(range, text.to_string())]);
            check_lines(&b);
        }
        while b.undo().is_some() {
            check_lines(&b);
        }
    }

    fn check_lines(b: &Buffer) {
        let text = b.to_string();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(b.line_count(), lines.len(), "{text:?}");
        let mut start = 0;
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(b.line_start(i), start, "{text:?} line {i}");
            assert_eq!(b.line_text(i), *l);
            let n = l.chars().count();
            for c in start..=start + n {
                assert_eq!(b.char_to_line(c), i, "{text:?} char {c}");
            }
            start += n + 1;
        }
    }

    #[test]
    fn set_line_ending_marks_dirty() {
        let mut b = Buffer::new("a\n");
        b.set_line_ending(LineEnding::Lf);
        assert!(!b.is_dirty());
        b.set_line_ending(LineEnding::CrLf);
        assert!(b.is_dirty());
        assert_eq!(b.version(), 1);
        b.mark_saved();
        assert!(!b.is_dirty());
        b.set_line_ending(LineEnding::Lf);
        assert!(b.is_dirty());
        b.set_line_ending(LineEnding::CrLf);
        assert!(!b.is_dirty());
        assert_eq!(b.version(), 3);
    }
}
