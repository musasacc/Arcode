//! Line-oriented commands: indent, outdent, duplicate, move, comment.

use std::ops::Range;

use super::{IndentUnit, clamp_stale, commit, map_selection, prepare, set_from};
use crate::buffer::Buffer;
use crate::changes::{Change, ChangeMap};
use crate::edit::Edit;
use crate::history::UndoGroup;
use crate::selection::{Selection, SelectionSet};
use crate::text_util::is_blank;

/// Inclusive `(first, last)` line blocks covered by the selections, sorted
/// and merged when overlapping or adjacent. A non-empty selection ending at
/// column 0 does not include that last line.
pub(crate) fn line_blocks(buffer: &Buffer, selections: &SelectionSet) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for s in selections.iter() {
        let first = buffer.char_to_line(s.start());
        let mut last = buffer.char_to_line(s.end());
        if !s.is_empty() && last > first && buffer.line_start(last) == s.end() {
            last -= 1;
        }
        match blocks.last_mut() {
            Some(prev) if first <= prev.1 + 1 => prev.1 = prev.1.max(last),
            _ => blocks.push((first, last)),
        }
    }
    blocks
}

/// Applies line changes and maps every selection through them.
fn commit_mapped(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    changes: Vec<Change>,
) -> Vec<Edit> {
    let map = ChangeMap::new(&changes);
    let after: Vec<Selection> = selections.iter().map(|s| map_selection(s, &map)).collect();
    let after = set_from(after, selections.primary_index(), selections);
    buffer.break_undo_group();
    commit(buffer, selections, &changes, after, UndoGroup::New)
}

fn leading_blanks(buffer: &Buffer, line: usize) -> usize {
    let start = buffer.line_start(line);
    let end = buffer.line_end(line);
    buffer
        .text()
        .slice(start..end)
        .chars()
        .take_while(|&c| is_blank(c))
        .count()
}

/// Adds one indent level at the start of every selected line (empty lines
/// are skipped in multi-line blocks).
pub fn indent_lines(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    unit: IndentUnit,
) -> Vec<Edit> {
    clamp_stale(buffer, selections);
    let text = unit.text();
    let mut raw = Vec::new();
    for (first, last) in line_blocks(buffer, selections) {
        for line in first..=last {
            if first != last && buffer.line_len(line) == 0 {
                continue;
            }
            let at = buffer.line_start(line);
            raw.push((at..at, text.clone()));
        }
    }
    let changes = prepare(buffer, raw);
    commit_mapped(buffer, selections, changes)
}

/// Removes up to one indent level from every selected line: a leading tab,
/// or up to one indent width of leading spaces.
pub fn outdent_lines(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    unit: IndentUnit,
    tab_width: usize,
) -> Vec<Edit> {
    clamp_stale(buffer, selections);
    let width = unit.width(tab_width);
    let mut raw = Vec::new();
    for (first, last) in line_blocks(buffer, selections) {
        for line in first..=last {
            let start = buffer.line_start(line);
            let end = buffer.line_end(line);
            let mut chars = buffer.text().slice(start..end).chars();
            let n = match chars.next() {
                Some('\t') => 1,
                Some(' ') => 1 + chars.take_while(|&c| c == ' ').take(width - 1).count(),
                _ => 0,
            };
            if n > 0 {
                raw.push((start..start + n, String::new()));
            }
        }
    }
    let changes = prepare(buffer, raw);
    commit_mapped(buffer, selections, changes)
}

/// Text of lines `first..=last` including a final `\n` (added if the last
/// line has none).
fn block_text(buffer: &Buffer, first: usize, last: usize) -> String {
    let mut text = buffer.slice_to_string(buffer.line_start(first)..buffer.line_start(last + 1));
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Duplicates the selected lines below themselves; selections move to the
/// copy (Copy Line Down).
pub fn duplicate_line(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    duplicate_lines(buffer, selections, false)
}

/// Duplicates the selected lines above themselves; selections stay on the
/// upper copy (Copy Line Up).
pub fn duplicate_line_up(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    duplicate_lines(buffer, selections, true)
}

/// Inserts a copy of each selected line block above it. Selections move
/// by the copies inserted before them: those of earlier blocks, plus their
/// own block's copy unless they stay on the upper copy (`up`).
fn duplicate_lines(buffer: &mut Buffer, selections: &mut SelectionSet, up: bool) -> Vec<Edit> {
    clamp_stale(buffer, selections);
    let blocks = line_blocks(buffer, selections);
    let raw: Vec<(Range<usize>, String)> = blocks
        .iter()
        .map(|&(first, last)| {
            let at = buffer.line_start(first);
            (at..at, block_text(buffer, first, last))
        })
        .collect();
    // Chars inserted above each block (and, last, in total).
    let mut inserted = vec![0];
    for (_, text) in &raw {
        inserted.push(inserted.last().copied().unwrap_or(0) + text.chars().count());
    }
    let after: Vec<Selection> = selections
        .iter()
        .map(|s| {
            let line = buffer.char_to_line(s.start());
            let block = blocks.partition_point(|&(_, last)| last < line);
            let shift = inserted[if up { block } else { block + 1 }];
            Selection::new(s.anchor + shift, s.head + shift)
        })
        .collect();
    let changes = prepare(buffer, raw);
    let after = set_from(after, selections.primary_index(), selections);
    buffer.break_undo_group();
    commit(buffer, selections, &changes, after, UndoGroup::New)
}

/// Swaps each selected line block with the line above (`up`) or below.
fn move_lines(buffer: &mut Buffer, selections: &mut SelectionSet, up: bool) -> Vec<Edit> {
    clamp_stale(buffer, selections);
    let last_line = buffer.line_count() - 1;
    let mut raw = Vec::new();
    // (first, last, signed shift) per moved block.
    let mut moved: Vec<(usize, usize, isize)> = Vec::new();
    for (first, last) in line_blocks(buffer, selections) {
        let block_start = buffer.line_start(first);
        let block = buffer.slice_to_string(block_start..buffer.line_end(last));
        if up && first > 0 {
            let other = buffer.line_text(first - 1);
            let range = buffer.line_start(first - 1)..buffer.line_end(last);
            raw.push((range, format!("{block}\n{other}")));
            moved.push((first, last, -(other.chars().count() as isize + 1)));
        } else if !up && last < last_line {
            let other = buffer.line_text(last + 1);
            let range = block_start..buffer.line_end(last + 1);
            raw.push((range, format!("{other}\n{block}")));
            moved.push((first, last, other.chars().count() as isize + 1));
        }
    }
    if raw.is_empty() {
        return Vec::new();
    }
    let changes = prepare(buffer, raw);
    let after: Vec<Selection> = selections
        .iter()
        .map(|s| {
            let line = buffer.char_to_line(s.start());
            // `moved` is sorted and disjoint: binary search for the block.
            let i = moved.partition_point(|&(_, l, _)| l < line);
            let shift = moved
                .get(i)
                .filter(|&&(f, l, _)| (f..=l).contains(&line))
                .map_or(0, |m| m.2);
            let apply = |p: usize| (p as isize + shift).max(0) as usize;
            Selection::new(apply(s.anchor), apply(s.head))
        })
        .collect();
    let after = set_from(after, selections.primary_index(), selections);
    buffer.break_undo_group();
    commit(buffer, selections, &changes, after, UndoGroup::New)
}

/// Moves the selected lines up by one line, keeping the selections on them.
pub fn move_lines_up(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    move_lines(buffer, selections, true)
}

/// Moves the selected lines down by one line, keeping the selections on
/// them.
pub fn move_lines_down(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    move_lines(buffer, selections, false)
}

/// Toggles a line comment (`prefix`, e.g. `"//"`) on the selected lines.
///
/// If every non-blank selected line is commented, the prefix (and one
/// following space) is removed; otherwise `prefix ` is inserted at the
/// smallest indentation of the non-blank lines.
pub fn toggle_line_comment(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    prefix: &str,
) -> Vec<Edit> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return Vec::new();
    }
    clamp_stale(buffer, selections);
    let lines: Vec<usize> = line_blocks(buffer, selections)
        .into_iter()
        .flat_map(|(f, l)| f..=l)
        .collect();
    let non_blank: Vec<usize> = lines
        .iter()
        .copied()
        .filter(|&l| leading_blanks(buffer, l) < buffer.line_len(l))
        .collect();
    let targets = if non_blank.is_empty() {
        lines
    } else {
        non_blank
    };
    let prefix_chars = prefix.chars().count();
    let commented = |l: usize| {
        let start = buffer.line_start(l) + leading_blanks(buffer, l);
        buffer.slice_to_string(start..start + prefix_chars) == prefix
    };
    let mut raw = Vec::new();
    if targets.iter().all(|&l| commented(l)) {
        for &l in &targets {
            let start = buffer.line_start(l) + leading_blanks(buffer, l);
            let mut end = start + prefix_chars;
            if buffer.char_at(end) == Some(' ') {
                end += 1;
            }
            raw.push((start..end, String::new()));
        }
    } else {
        let indent = targets
            .iter()
            .map(|&l| leading_blanks(buffer, l))
            .min()
            .unwrap_or(0);
        let text = format!("{prefix} ");
        for &l in &targets {
            let at = buffer.line_start(l) + indent;
            raw.push((at..at, text.clone()));
        }
    }
    let changes = prepare(buffer, raw);
    commit_mapped(buffer, selections, changes)
}
