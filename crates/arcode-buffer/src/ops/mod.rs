//! Multi-cursor editing commands.
//!
//! Every command takes the buffer and the current [`SelectionSet`], applies
//! its change at **all** selections as one undo step (recording the
//! selections before and after, so undo/redo restore them), updates the
//! selections and returns the applied [`Edit`]s (for syntax/LSP sync).

mod delete;
mod insert;
mod lines;
mod select;

use std::ops::Range;

pub use delete::{backspace, delete_forward, delete_word_left, delete_word_right};
pub use insert::{insert_tab, insert_text, newline_and_indent, paste, type_text};
pub use lines::{
    duplicate_line, indent_lines, move_lines_down, move_lines_up, outdent_lines,
    toggle_line_comment,
};
pub use select::{select_all, select_line, select_next_occurrence, selected_text, word_range_at};

use crate::buffer::Buffer;
use crate::changes::{Assoc, Change, map_pos, normalize_changes};
use crate::edit::Edit;
use crate::history::UndoGroup;
use crate::selection::{Selection, SelectionSet};

/// One level of indentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndentUnit {
    /// `n` spaces.
    Spaces(usize),
    /// A tab character.
    Tab,
}

impl Default for IndentUnit {
    fn default() -> Self {
        IndentUnit::Spaces(4)
    }
}

impl IndentUnit {
    /// The text of one indent level.
    pub fn text(self) -> String {
        match self {
            IndentUnit::Spaces(n) => " ".repeat(n.max(1)),
            IndentUnit::Tab => "\t".to_string(),
        }
    }

    /// Display width of one indent level.
    pub fn width(self, tab_width: usize) -> usize {
        match self {
            IndentUnit::Spaces(n) => n.max(1),
            IndentUnit::Tab => tab_width.max(1),
        }
    }
}

/// Auto-closing of bracket and quote pairs while typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoClose {
    /// Master switch.
    pub enabled: bool,
    /// `(open, close)` pairs; quotes use the same char twice.
    pub pairs: Vec<(char, char)>,
    /// Do not auto-close when the char after the cursor is a word char.
    pub skip_before_word: bool,
    /// Typing an opener with a non-empty selection wraps the selection.
    pub surround: bool,
}

impl Default for AutoClose {
    fn default() -> Self {
        AutoClose {
            enabled: true,
            pairs: vec![('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')],
            skip_before_word: true,
            surround: true,
        }
    }
}

impl AutoClose {
    /// Auto-closing turned off.
    pub fn disabled() -> Self {
        AutoClose {
            enabled: false,
            ..AutoClose::default()
        }
    }

    pub(crate) fn closer_for(&self, open: char) -> Option<char> {
        self.pairs.iter().find(|(o, _)| *o == open).map(|(_, c)| *c)
    }

    pub(crate) fn is_closer(&self, close: char) -> bool {
        self.pairs.iter().any(|(_, c)| *c == close)
    }
}

/// Settings shared by the editing commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsConfig {
    /// Indentation inserted by Tab / newline and removed by outdent.
    pub indent: IndentUnit,
    /// Display width of a tab character.
    pub tab_width: usize,
    /// Bracket/quote auto-closing.
    pub auto_close: AutoClose,
}

impl Default for OpsConfig {
    fn default() -> Self {
        OpsConfig {
            indent: IndentUnit::default(),
            tab_width: 4,
            auto_close: AutoClose::default(),
        }
    }
}

/// Raw changes in pre-edit coordinates, normalized for `buffer`.
pub(crate) fn prepare(buffer: &Buffer, raw: Vec<(Range<usize>, String)>) -> Vec<Change> {
    normalize_changes(raw, buffer.len_chars())
}

/// Maps a selection through `changes`: cursors follow inserted text, a
/// non-empty selection grows to include text inserted at its edges.
pub(crate) fn map_selection(sel: &Selection, changes: &[Change]) -> Selection {
    if sel.is_empty() {
        return Selection::cursor(map_pos(sel.head, changes, Assoc::After));
    }
    let start = map_pos(sel.start(), changes, Assoc::Before);
    let end = map_pos(sel.end(), changes, Assoc::After);
    if sel.is_reversed() {
        Selection::new(end, start)
    } else {
        Selection::new(start, end)
    }
}

/// Builds a selection set from `sels` (falls back to `fallback`).
pub(crate) fn set_from(
    sels: Vec<Selection>,
    primary: usize,
    fallback: &SelectionSet,
) -> SelectionSet {
    SelectionSet::from_vec(sels, primary).unwrap_or_else(|| fallback.clone())
}

/// Applies `changes` as one undo step and installs `after`.
pub(crate) fn commit(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    changes: &[Change],
    mut after: SelectionSet,
    group: UndoGroup,
) -> Vec<Edit> {
    let len_after = changes.iter().fold(buffer.len_chars() as isize, |acc, c| {
        acc + c.text_chars as isize - c.range.len() as isize
    });
    after.clamp(len_after.max(0) as usize);
    let edits = if changes.is_empty() {
        Vec::new()
    } else {
        buffer.apply_changes(changes, group, Some((selections.clone(), after.clone())))
    };
    *selections = after;
    edits
}

#[cfg(test)]
mod test_util;
#[cfg(test)]
mod tests_delete;
#[cfg(test)]
mod tests_insert;
#[cfg(test)]
mod tests_lines;
#[cfg(test)]
mod tests_select;
