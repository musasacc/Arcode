//! Selection commands that do not edit text.

use std::ops::Range;

use crate::buffer::Buffer;
use crate::search::find_next;
use crate::selection::{Selection, SelectionSet};
use crate::text_util::is_word_char;

/// Selects the whole buffer.
pub fn select_all(buffer: &Buffer, selections: &mut SelectionSet) {
    *selections = SelectionSet::from_selection(Selection::new(0, buffer.len_chars()));
}

/// Extends every selection to whole lines (including the line break).
/// Repeating it extends by one more line.
pub fn select_line(buffer: &Buffer, selections: &mut SelectionSet) {
    selections.transform(|s| {
        let start = buffer.line_start(buffer.char_to_line(s.start()));
        let end = buffer.line_start(buffer.char_to_line(s.end()) + 1);
        Selection::new(start, end)
    });
}

/// The selected texts joined by `\n` (one entry per selection, in document
/// order).
pub fn selected_text(buffer: &Buffer, selections: &SelectionSet) -> String {
    selections
        .iter()
        .map(|s| buffer.slice_to_string(s.range()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The word (run of alphanumerics and `_`) touching `pos`, preferring the
/// word after the cursor. `None` if no word char touches `pos`.
pub fn word_range_at(buffer: &Buffer, pos: usize) -> Option<Range<usize>> {
    let pos = buffer.clamp_char(pos);
    let text = buffer.text();
    let mut start = pos;
    let mut back = text.chars_at(pos);
    while let Some(c) = back.prev() {
        if !is_word_char(c) {
            break;
        }
        start -= 1;
    }
    let end = pos + text.chars_at(pos).take_while(|&c| is_word_char(c)).count();
    (start < end).then_some(start..end)
}

/// Ctrl+D. With an empty primary selection, selects the word under it.
/// Otherwise adds a selection at the next occurrence (case-sensitive,
/// wrapping) of the primary's text that is not already selected, and makes
/// it primary. Returns `false` if nothing changed.
pub fn select_next_occurrence(buffer: &Buffer, selections: &mut SelectionSet) -> bool {
    let primary = *selections.primary();
    if primary.is_empty() {
        let Some(word) = word_range_at(buffer, primary.head) else {
            return false;
        };
        let idx = selections.primary_index();
        let mut sels: Vec<Selection> = selections.iter().copied().collect();
        sels[idx] = Selection::new(word.start, word.end);
        if let Some(set) = SelectionSet::from_vec(sels, idx) {
            *selections = set;
        }
        return true;
    }
    let query = buffer.slice_to_string(primary.range());
    let mut from = primary.end();
    let mut seen = std::collections::HashSet::new();
    loop {
        let Some(found) = find_next(buffer, &query, from, true, true) else {
            return false;
        };
        if !seen.insert(found.start) {
            return false;
        }
        let overlaps = selections
            .iter()
            .any(|s| s.start() < found.end && found.start < s.end() || s.range() == found);
        if !overlaps {
            selections.add(Selection::new(found.start, found.end));
            return true;
        }
        from = found.end;
    }
}
