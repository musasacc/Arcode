//! The [`Buffer`] type: text storage, metadata and line accessors.

use std::fmt;
use std::path::{Path, PathBuf};

use ropey::{Rope, RopeSlice};

use crate::history::History;
use crate::line_ending::{LineEnding, normalize};

/// A rope-backed text buffer.
///
/// Text is stored with `\n` line endings; the file's original
/// [`LineEnding`] (and UTF-8 BOM, if any) is restored on save.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub(crate) rope: Rope,
    pub(crate) path: Option<PathBuf>,
    pub(crate) line_ending: LineEnding,
    pub(crate) bom: bool,
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
        let rope = Rope::from_str(&normalize(text));
        Buffer {
            rope,
            path: None,
            line_ending,
            bom: false,
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

    /// Changes the line ending used when saving. Marks the buffer dirty
    /// unless it already used `line_ending`.
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        if self.line_ending != line_ending {
            self.line_ending = line_ending;
            // The on-disk form differs now; no history state matches it.
            self.saved_state = u64::MAX;
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

    /// True if the text differs from the last save/load. Undoing back to the
    /// saved state makes the buffer clean again.
    pub fn is_dirty(&self) -> bool {
        self.history.current_state() != self.saved_state
    }

    /// Declares the current text as saved (e.g. after an external write).
    pub fn mark_saved(&mut self) {
        self.saved_state = self.history.current_state();
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
    /// `"a\n"` has two lines and `""` has one.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Clamps a line index to the last line.
    pub(crate) fn clamp_line(&self, line: usize) -> usize {
        line.min(self.line_count().saturating_sub(1))
    }

    /// Line `line` including its line break. Out-of-range indices are
    /// clamped to the last line.
    pub fn line(&self, line: usize) -> RopeSlice<'_> {
        self.rope.line(self.clamp_line(line))
    }

    /// Line `line` without its line break (clamped like [`Buffer::line`]).
    pub fn line_text(&self, line: usize) -> String {
        let line = self.clamp_line(line);
        let start = self.rope.line_to_char(line);
        self.rope
            .slice(start..start + self.line_len(line))
            .to_string()
    }

    /// Length of line `line` in chars, excluding its line break.
    pub fn line_len(&self, line: usize) -> usize {
        let line = self.clamp_line(line);
        let len = self.rope.line(line).len_chars();
        // Every line but the last ends with exactly one break char (text is
        // normalized, so `\r\n` never occurs).
        if line + 1 < self.line_count() {
            len.saturating_sub(1)
        } else {
            len
        }
    }

    /// Char index of the start of line `line` (clamped; a line past the end
    /// yields the buffer length).
    pub fn line_start(&self, line: usize) -> usize {
        if line >= self.line_count() {
            return self.len_chars();
        }
        self.rope.line_to_char(line)
    }

    /// Char index of the end of line `line`, before its line break.
    pub fn line_end(&self, line: usize) -> usize {
        let line = self.clamp_line(line);
        self.rope.line_to_char(line) + self.line_len(line)
    }

    /// Line containing `char_idx` (clamped).
    pub fn char_to_line(&self, char_idx: usize) -> usize {
        self.rope.char_to_line(self.clamp_char(char_idx))
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
    fn unicode_line_separator_is_stripped() {
        // ropey may treat U+2028 as a line break; line_len must agree.
        let b = Buffer::new("a\u{2028}b");
        let total: usize = (0..b.line_count()).map(|l| b.line_len(l)).sum();
        assert_eq!(total + b.line_count() - 1, b.len_chars());
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
    }
}
