//! Edit description passed to [`Syntax::edit`](crate::Syntax::edit).

use tree_sitter::{InputEdit, Point};

/// One text edit in tree-sitter coordinates.
///
/// Callers convert `arcode_buffer::Edit` into this type; this crate does not
/// depend on the buffer crate. Offsets are **bytes** into the text; positions
/// are `(row, byte column)` where rows are separated by `'\n'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct InputEditData {
    /// Byte offset where the edit starts.
    pub start_byte: usize,
    /// Byte offset where the replaced text ended (in the old text).
    pub old_end_byte: usize,
    /// Byte offset where the inserted text ends (in the new text).
    pub new_end_byte: usize,
    /// `(row, byte column)` of `start_byte`.
    pub start_position: (usize, usize),
    /// `(row, byte column)` of `old_end_byte` in the old text.
    pub old_end_position: (usize, usize),
    /// `(row, byte column)` of `new_end_byte` in the new text.
    pub new_end_position: (usize, usize),
}

fn point((row, column): (usize, usize)) -> Point {
    Point { row, column }
}

impl From<InputEditData> for InputEdit {
    fn from(edit: InputEditData) -> Self {
        InputEdit {
            start_byte: edit.start_byte,
            old_end_byte: edit.old_end_byte,
            new_end_byte: edit.new_end_byte,
            start_position: point(edit.start_position),
            old_end_position: point(edit.old_end_position),
            new_end_position: point(edit.new_end_position),
        }
    }
}

impl From<&InputEditData> for InputEdit {
    fn from(edit: &InputEditData) -> Self {
        (*edit).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_all_fields() {
        let data = InputEditData {
            start_byte: 1,
            old_end_byte: 2,
            new_end_byte: 3,
            start_position: (0, 1),
            old_end_position: (0, 2),
            new_end_position: (1, 0),
        };
        let edit: InputEdit = data.into();
        assert_eq!(edit.start_byte, 1);
        assert_eq!(edit.old_end_byte, 2);
        assert_eq!(edit.new_end_byte, 3);
        assert_eq!(edit.start_position, Point { row: 0, column: 1 });
        assert_eq!(edit.old_end_position, Point { row: 0, column: 2 });
        assert_eq!(edit.new_end_position, Point { row: 1, column: 0 });
    }
}
