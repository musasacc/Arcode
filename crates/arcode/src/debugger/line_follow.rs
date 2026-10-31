//! Where a breakpoint's line goes when the buffer is edited.
//!
//! Breakpoints follow their code by themselves instead of reading back the
//! editor's gutter marks: the editor drops marks on lines inside a
//! multi-line replacement (a line move, a whole-document format, a paste
//! over a selection) and hides the dot under the stopped arrow. Here a
//! line inside a replacement keeps its breakpoint when its text is still
//! there (the closest copy wins); it is dropped only when its text really
//! disappeared.

use std::collections::HashMap;
use std::path::Path;

use arcode_buffer::Edit;
use arcode_editor::Editor;
use gpui::{Context, Entity};

use super::Debugger;

/// Key of the stopped line in a tracked editor's line texts.
const STOPPED_KEY: u64 = u64::MAX;

/// A line's text for matching: whitespace does not count (a formatter may
/// re-indent it).
pub fn line_key(text: &str) -> String {
    text.split_whitespace().collect()
}

/// Where the line `line` (whose text has the [`line_key`] `key`) is after
/// `edit`, or `None` when it was deleted.
pub fn follow_line(line: usize, key: &str, edit: &Edit) -> Option<usize> {
    let (start, old_end, new_end) = (edit.start_point, edit.old_end_point, edit.new_end_point);
    if line < start.line {
        return Some(line);
    }
    if line > old_end.line {
        return Some(line - old_end.line + new_end.line);
    }
    if start.line == old_end.line {
        // Text inserted at column 0 pushes the line's content down.
        if start.column == 0 && start == old_end {
            return Some(new_end.line);
        }
        return Some(line);
    }
    // A multi-line replacement. A start line whose beginning is kept stays
    // where it is; an end line that is only touched at column 0 keeps its
    // text after the edit.
    if line == start.line && start.column > 0 {
        return Some(line);
    }
    if line == old_end.line && old_end.column == 0 {
        return Some(new_end.line);
    }
    // Otherwise look for the line's text among the inserted lines.
    let pieces: Vec<&str> = edit.new_text.split('\n').collect();
    let last = pieces.len() - 1;
    let old_offset = line - start.line;
    pieces
        .iter()
        .enumerate()
        .filter(|&(i, piece)| {
            // A piece is a whole line when it starts a line (not glued to
            // the start line's kept text) and, for the last piece, holds
            // text (an empty last piece is just the start of the next
            // line).
            let starts_line = i > 0 || start.column == 0;
            let whole = i < last || !piece.trim().is_empty();
            starts_line && whole && line_key(piece) == key
        })
        .min_by_key(|&(i, _)| i.abs_diff(old_offset))
        .map(|(i, _)| start.line + i)
}

impl Debugger {
    /// Moves this file's breakpoints (and the stopped line) with `edits`;
    /// a breakpoint whose line was deleted is removed.
    pub(super) fn follow_edits(
        &mut self,
        editor: &Entity<Editor>,
        edits: &[Edit],
        cx: &mut Context<Self>,
    ) {
        let Some(path) = editor.read(cx).path().map(Path::to_path_buf) else {
            return;
        };
        let keys = self
            .tracked(editor)
            .map(|t| t.keys.clone())
            .unwrap_or_default();
        let follow = |id: u64, line: usize| {
            let key = keys.get(&id).map_or("", String::as_str);
            edits
                .iter()
                .try_fold(line, |line, edit| follow_line(line, key, edit))
        };
        if let Some((p, line)) = &mut self.stopped_at
            && *p == path
            && let Some(moved) = follow(STOPPED_KEY, *line)
        {
            *line = moved;
        }
        let moved: Vec<(u64, usize)> = self
            .book
            .in_file(&path)
            .filter_map(|b| Some((b.id, follow(b.id, b.line)?)))
            .collect();
        if self.book.relocate(&path, &moved) {
            self.breakpoints_changed(&path, cx);
        } else {
            // The editor may have dropped marks inside a replacement.
            self.refresh_editor(editor, cx);
        }
    }

    /// Remembers the text of `editor`'s breakpoint lines.
    pub(super) fn remember_lines(&mut self, editor: &Entity<Editor>, path: &Path, cx: &gpui::App) {
        let buffer = editor.read(cx).buffer();
        let mut keys: HashMap<u64, String> = self
            .book
            .in_file(path)
            .map(|b| (b.id, line_key(&buffer.line_text(b.line))))
            .collect();
        if let Some((_, line)) = self.stopped_at.as_ref().filter(|(p, _)| p == path) {
            keys.insert(STOPPED_KEY, line_key(&buffer.line_text(*line)));
        }
        if let Some(t) = self.tracked(editor) {
            t.keys = keys;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_buffer::{Buffer, Point};

    /// Applies `edit` (a range and text) to `text` and returns the edit.
    fn edit(text: &str, range: std::ops::Range<Point>, new: &str) -> Edit {
        let mut buffer = Buffer::new(text);
        let start = buffer.point_to_char(range.start);
        let end = buffer.point_to_char(range.end);
        let mut edits = buffer.edit(vec![(start..end, new.to_string())]);
        assert_eq!(edits.len(), 1);
        edits.remove(0)
    }

    fn key(text: &str, line: usize) -> String {
        line_key(text.lines().nth(line).unwrap_or(""))
    }

    const TEXT: &str = "fn main() {\n    let a = 1;\n    let b = 2;\n    use_it(a, b);\n}\n";

    #[test]
    fn lines_before_and_after_an_edit_shift() {
        let e = edit(TEXT, Point::new(1, 0)..Point::new(1, 0), "    // note\n");
        assert_eq!(follow_line(0, &key(TEXT, 0), &e), Some(0));
        assert_eq!(follow_line(1, &key(TEXT, 1), &e), Some(2));
        assert_eq!(follow_line(3, &key(TEXT, 3), &e), Some(4));
        let e = edit(TEXT, Point::new(1, 4)..Point::new(1, 4), " ");
        assert_eq!(follow_line(1, &key(TEXT, 1), &e), Some(1));
    }

    #[test]
    fn a_moved_line_keeps_its_breakpoint() {
        // Alt+Down on line 1: lines 1..=2 are replaced by "b\na".
        let e = edit(
            TEXT,
            Point::new(1, 0)..Point::new(2, 14),
            "    let b = 2;\n    let a = 1;",
        );
        assert_eq!(follow_line(1, &key(TEXT, 1), &e), Some(2));
        assert_eq!(follow_line(2, &key(TEXT, 2), &e), Some(1));
        assert_eq!(follow_line(3, &key(TEXT, 3), &e), Some(3));
    }

    #[test]
    fn a_reformatted_document_keeps_breakpoints_on_unchanged_code() {
        let formatted = "fn main() {\n  let a = 1;\n\n  let b = 2;\n  use_it(a, b);\n}\n";
        let end = Point::new(5, 0);
        let e = edit(TEXT, Point::new(0, 0)..end, formatted);
        assert_eq!(follow_line(1, &key(TEXT, 1), &e), Some(1));
        assert_eq!(follow_line(2, &key(TEXT, 2), &e), Some(3));
        assert_eq!(follow_line(3, &key(TEXT, 3), &e), Some(4));
    }

    #[test]
    fn deleted_lines_lose_their_breakpoint() {
        let e = edit(TEXT, Point::new(1, 0)..Point::new(3, 0), "");
        assert_eq!(follow_line(1, &key(TEXT, 1), &e), None);
        assert_eq!(follow_line(2, &key(TEXT, 2), &e), None);
        assert_eq!(follow_line(3, &key(TEXT, 3), &e), Some(1));
        // Joining a line onto the previous one keeps the previous line's
        // breakpoint and drops the joined one.
        let e = edit(TEXT, Point::new(1, 14)..Point::new(2, 4), " ");
        assert_eq!(follow_line(1, &key(TEXT, 1), &e), Some(1));
        assert_eq!(follow_line(2, &key(TEXT, 2), &e), None);
    }

    #[test]
    fn the_closest_copy_of_a_repeated_line_wins() {
        let text = "a\n}\nb\n}\nc\n";
        let e = edit(
            text,
            Point::new(0, 0)..Point::new(5, 0),
            "a\n}\nx\nb\n}\nc\n",
        );
        assert_eq!(follow_line(3, &key(text, 3), &e), Some(4));
        assert_eq!(follow_line(1, &key(text, 1), &e), Some(1));
    }
}
