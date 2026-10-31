//! Pure cursor-movement functions on `(&Buffer, char position)`.
//!
//! Horizontal and word moves step over whole grapheme clusters (e.g. `e` +
//! combining accent, flag emoji). Display columns count one per char, except tabs,
//! which advance to the next multiple of `tab_width`.

use crate::buffer::Buffer;
use crate::selection::{Selection, SelectionSet};
use crate::text_util::{is_blank, is_word_char, next_grapheme_boundary, prev_grapheme_boundary};

/// One grapheme to the left.
pub fn left(buffer: &Buffer, pos: usize) -> usize {
    prev_grapheme_boundary(buffer.text(), pos)
}

/// One grapheme to the right.
pub fn right(buffer: &Buffer, pos: usize) -> usize {
    next_grapheme_boundary(buffer.text(), pos)
}

/// Start of the buffer.
pub fn buffer_start(_buffer: &Buffer) -> usize {
    0
}

/// End of the buffer.
pub fn buffer_end(buffer: &Buffer) -> usize {
    buffer.len_chars()
}

/// Start of the line containing `pos`.
pub fn line_start(buffer: &Buffer, pos: usize) -> usize {
    buffer.line_start(buffer.char_to_line(pos))
}

/// End of the line containing `pos` (before the line break).
pub fn line_end(buffer: &Buffer, pos: usize) -> usize {
    buffer.line_end(buffer.char_to_line(pos))
}

/// First non-blank char of `line` (the line end if the line is blank).
pub fn first_non_whitespace(buffer: &Buffer, line: usize) -> usize {
    let start = buffer.line_start(line);
    let end = buffer.line_end(line);
    let blanks = buffer
        .text()
        .slice(start..end)
        .chars()
        .take_while(|&c| is_blank(c))
        .count();
    start + blanks
}

/// Smart Home: jumps to the first non-blank char, or to column 0 if
/// already there.
pub fn line_start_smart(buffer: &Buffer, pos: usize) -> usize {
    let pos = buffer.clamp_char(pos);
    let line = buffer.char_to_line(pos);
    let first = first_non_whitespace(buffer, line);
    if pos == first {
        buffer.line_start(line)
    } else {
        first
    }
}

fn tab_width(tab_width: usize) -> usize {
    tab_width.max(1)
}

fn advance(col: usize, c: char, tw: usize) -> usize {
    if c == '\t' {
        (col / tw + 1) * tw
    } else {
        col + 1
    }
}

/// Display column of `pos` within its line.
pub fn display_column(buffer: &Buffer, pos: usize, tab_width_: usize) -> usize {
    let tw = tab_width(tab_width_);
    let pos = buffer.clamp_char(pos);
    let start = line_start(buffer, pos);
    buffer
        .text()
        .slice(start..pos)
        .chars()
        .fold(0, |col, c| advance(col, c, tw))
}

/// Char index on `line` (clamped) at display column `display_col`.
///
/// A column inside a tab rounds down to the tab's start; a column past the
/// line end clamps to the line end; the result is snapped to a grapheme
/// boundary.
pub fn char_for_display_column(
    buffer: &Buffer,
    line: usize,
    display_col: usize,
    tab_width_: usize,
) -> usize {
    let tw = tab_width(tab_width_);
    let line = buffer.clamp_line(line);
    let start = buffer.line_start(line);
    let end = buffer.line_end(line);
    let mut col = 0;
    let mut pos = start;
    for c in buffer.text().slice(start..end).chars() {
        let next = advance(col, c, tw);
        if next > display_col {
            break;
        }
        col = next;
        pos += 1;
    }
    snap_to_grapheme(buffer, pos)
}

fn snap_to_grapheme(buffer: &Buffer, pos: usize) -> usize {
    if pos >= buffer.len_chars() {
        return pos;
    }
    prev_grapheme_boundary(buffer.text(), pos + 1)
}

/// Direction of a vertical move.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
}

fn vertical(
    buffer: &Buffer,
    pos: usize,
    goal: Option<usize>,
    dir: Dir,
    lines: usize,
    tw: usize,
) -> (usize, usize) {
    let pos = buffer.clamp_char(pos);
    let goal = goal.unwrap_or_else(|| display_column(buffer, pos, tw));
    let line = buffer.char_to_line(pos);
    let last = buffer.line_count().saturating_sub(1);
    let target = match dir {
        Dir::Up => line.saturating_sub(lines),
        Dir::Down => line.saturating_add(lines).min(last),
    };
    if target == line {
        // Cannot move further: go to the buffer edge in that direction.
        let edge = match dir {
            Dir::Up => 0,
            Dir::Down => buffer.len_chars(),
        };
        return (edge, goal);
    }
    (char_for_display_column(buffer, target, goal, tw), goal)
}

/// One line up, keeping `goal` (or the current display column). Returns
/// `(new position, goal column)`. On the first line moves to the start.
pub fn up(buffer: &Buffer, pos: usize, goal: Option<usize>, tab_width: usize) -> (usize, usize) {
    vertical(buffer, pos, goal, Dir::Up, 1, tab_width)
}

/// One line down; on the last line moves to the end. See [`up`].
pub fn down(buffer: &Buffer, pos: usize, goal: Option<usize>, tab_width: usize) -> (usize, usize) {
    vertical(buffer, pos, goal, Dir::Down, 1, tab_width)
}

/// `lines` lines up (at least one; clamped to the first line, any count
/// such as `usize::MAX` is fine). See [`up`].
pub fn page_up(
    buffer: &Buffer,
    pos: usize,
    goal: Option<usize>,
    lines: usize,
    tab_width: usize,
) -> (usize, usize) {
    vertical(buffer, pos, goal, Dir::Up, lines.max(1), tab_width)
}

/// `lines` lines down (at least one; clamped to the last line). See
/// [`page_up`].
pub fn page_down(
    buffer: &Buffer,
    pos: usize,
    goal: Option<usize>,
    lines: usize,
    tab_width: usize,
) -> (usize, usize) {
    vertical(buffer, pos, goal, Dir::Down, lines.max(1), tab_width)
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Space,
    Word,
    Punct,
}

/// Class of the grapheme cluster starting at `pos` (by its base char, so
/// `e` + U+0301 is a word char).
fn class_at(buffer: &Buffer, pos: usize) -> Option<Class> {
    let c = buffer.char_at(pos)?;
    Some(if c.is_whitespace() {
        Class::Space
    } else if is_word_char(c) {
        Class::Word
    } else {
        Class::Punct
    })
}

/// Next word end: skips whitespace, then a run of word chars or a run of
/// punctuation (VS Code style). Works on grapheme clusters, so it never
/// stops inside one.
pub fn word_right(buffer: &Buffer, pos: usize) -> usize {
    let rope = buffer.text();
    let mut pos = buffer.clamp_char(pos);
    while class_at(buffer, pos) == Some(Class::Space) {
        pos = next_grapheme_boundary(rope, pos);
    }
    if let Some(run) = class_at(buffer, pos) {
        while class_at(buffer, pos) == Some(run) {
            pos = next_grapheme_boundary(rope, pos);
        }
    }
    pos
}

/// Previous word start: mirror of [`word_right`].
pub fn word_left(buffer: &Buffer, pos: usize) -> usize {
    let rope = buffer.text();
    let mut pos = buffer.clamp_char(pos);
    let prev_class = |pos: usize| {
        (pos > 0).then(|| {
            let prev = prev_grapheme_boundary(rope, pos);
            (prev, class_at(buffer, prev))
        })
    };
    while let Some((prev, Some(Class::Space))) = prev_class(pos) {
        pos = prev;
    }
    if let Some((_, Some(run))) = prev_class(pos) {
        while let Some((prev, class)) = prev_class(pos) {
            if class != Some(run) {
                break;
            }
            pos = prev;
        }
    }
    pos
}

/// Moves every selection's head with `f`; without `extend` selections
/// collapse to cursors. Clears goal columns.
pub fn move_selections(
    buffer: &Buffer,
    selections: &mut SelectionSet,
    extend: bool,
    f: impl Fn(&Buffer, usize) -> usize,
) {
    selections.transform(|mut s| {
        s.move_to(f(buffer, s.head), extend);
        s
    });
}

/// Like [`move_selections`] for vertical moves, which keep goal columns.
pub fn move_selections_vertically(
    buffer: &Buffer,
    selections: &mut SelectionSet,
    extend: bool,
    f: impl Fn(&Buffer, usize, Option<usize>) -> (usize, usize),
) {
    selections.transform(|mut s| {
        let (pos, goal) = f(buffer, s.head, s.goal_column);
        s.move_to(pos, extend);
        s.goal_column = Some(goal);
        s
    });
}

/// Left arrow: collapses a non-empty selection to its start unless
/// extending, otherwise moves one grapheme left.
pub fn move_left(buffer: &Buffer, selections: &mut SelectionSet, extend: bool) {
    selections.transform(|mut s| {
        if !extend && !s.is_empty() {
            return Selection::cursor(s.start());
        }
        s.move_to(left(buffer, s.head), extend);
        s
    });
}

/// Right arrow: mirror of [`move_left`].
pub fn move_right(buffer: &Buffer, selections: &mut SelectionSet, extend: bool) {
    selections.transform(|mut s| {
        if !extend && !s.is_empty() {
            return Selection::cursor(s.end());
        }
        s.move_to(right(buffer, s.head), extend);
        s
    });
}

#[cfg(test)]
#[path = "movement_tests.rs"]
mod tests;
