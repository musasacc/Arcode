//! Line diff for the editor's change gutter.

use std::collections::HashMap;
use std::ops::Range;

use crate::myers::{self, Op};

/// How a block of lines changed relative to the old text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// Lines exist only in the new text.
    Added,
    /// Old lines were replaced by these new lines.
    Modified,
    /// Old lines were removed; `new_lines` is empty and starts at the line
    /// that follows the removed block.
    Deleted,
}

/// One contiguous change, expressed in zero-based line indices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineChange {
    /// What kind of change this is.
    pub kind: ChangeKind,
    /// Affected lines in the new text. Empty for [`ChangeKind::Deleted`],
    /// where `start` is the line after the deletion (this equals the new
    /// line count when the deletion is at the end of the file; clamp it when
    /// drawing a marker).
    pub new_lines: Range<usize>,
    /// The corresponding lines in the old text (empty for
    /// [`ChangeKind::Added`], starting at the old line the insert precedes).
    pub old_lines: Range<usize>,
}

/// Diffs `old` against `new` line by line and returns the changed blocks in
/// order.
///
/// Lines are split like [`str::lines`]: a trailing newline does not start an
/// extra line and `\r\n` endings compare equal to `\n`, so a file that only
/// gained or lost its final newline (or switched line endings) is unchanged.
/// A block where lines were both removed and inserted is reported as
/// [`ChangeKind::Modified`] over the inserted lines.
pub fn line_diff(old: &str, new: &str) -> Vec<LineChange> {
    let (a, b) = intern(old, new);
    coalesce(myers::diff(&a, &b))
}

/// Maps every distinct line to a small integer so comparisons are cheap.
fn intern(old: &str, new: &str) -> (Vec<u32>, Vec<u32>) {
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let mut id_of = |line| {
        let next = ids.len() as u32;
        *ids.entry(line).or_insert(next)
    };
    let a = old.lines().map(&mut id_of).collect();
    let b = new.lines().map(&mut id_of).collect();
    (a, b)
}

/// Merges adjacent edit operations (no unchanged line between them) into
/// gutter-sized blocks.
fn coalesce(ops: Vec<Op>) -> Vec<LineChange> {
    let mut changes: Vec<LineChange> = Vec::new();
    for op in ops {
        let (old, new) = match op {
            Op::Delete { old, new_pos } => (old, new_pos..new_pos),
            Op::Insert { old_pos, new } => (old_pos..old_pos, new),
        };
        match changes.last_mut() {
            Some(last) if last.new_lines.end == new.start && last.old_lines.end == old.start => {
                last.new_lines.end = new.end;
                last.old_lines.end = old.end;
                last.kind = kind_of(&last.old_lines, &last.new_lines);
            }
            _ => changes.push(LineChange {
                kind: kind_of(&old, &new),
                new_lines: new,
                old_lines: old,
            }),
        }
    }
    changes
}

fn kind_of(old: &Range<usize>, new: &Range<usize>) -> ChangeKind {
    match (old.is_empty(), new.is_empty()) {
        (true, _) => ChangeKind::Added,
        (false, true) => ChangeKind::Deleted,
        (false, false) => ChangeKind::Modified,
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
