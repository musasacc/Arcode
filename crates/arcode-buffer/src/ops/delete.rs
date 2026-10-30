//! Deletion commands.

use std::ops::Range;

use super::{OpsConfig, clamp_stale, commit, prepare, set_from};
use crate::buffer::Buffer;
use crate::changes::{Assoc, ChangeMap};
use crate::edit::Edit;
use crate::history::UndoGroup;
use crate::movement::{self, display_column};
use crate::selection::{Selection, SelectionSet};
use crate::text_util::is_blank;

/// Deletes, for each selection, the selection itself if non-empty, else the
/// range `f` returns for its cursor. Cursors land at the deletion start.
fn delete_with(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    f: impl Fn(&Buffer, usize) -> Range<usize>,
) -> Vec<Edit> {
    clamp_stale(buffer, selections);
    let ranges: Vec<Range<usize>> = selections
        .iter()
        .map(|s| {
            if s.is_empty() {
                f(buffer, s.head)
            } else {
                s.range()
            }
        })
        .collect();
    let raw = ranges.iter().map(|r| (r.clone(), String::new())).collect();
    let changes = prepare(buffer, raw);
    let map = ChangeMap::new(&changes);
    let after: Vec<Selection> = ranges
        .iter()
        .map(|r| Selection::cursor(map.map(r.start, Assoc::Before)))
        .collect();
    let after = set_from(after, selections.primary_index(), selections);
    buffer.break_undo_group();
    commit(buffer, selections, &changes, after, UndoGroup::New)
}

/// Backspace: deletes each selection, or else the grapheme before the
/// cursor. Between an auto-close pair (`(|)`) both chars go; inside leading
/// spaces it deletes back to the previous indent stop.
pub fn backspace(buffer: &mut Buffer, selections: &mut SelectionSet, cfg: &OpsConfig) -> Vec<Edit> {
    delete_with(buffer, selections, |b, pos| backspace_range(b, pos, cfg))
}

fn backspace_range(buffer: &Buffer, pos: usize, cfg: &OpsConfig) -> Range<usize> {
    let pos = buffer.clamp_char(pos);
    if pos == 0 {
        return 0..0;
    }
    let prev = buffer.char_at(pos - 1);
    let next = buffer.char_at(pos);
    if cfg.auto_close.enabled
        && let (Some(p), Some(n)) = (prev, next)
        && cfg.auto_close.closer_for(p) == Some(n)
    {
        return pos - 1..pos + 1;
    }
    let line_start = movement::line_start(buffer, pos);
    let in_indent = buffer.text().slice(line_start..pos).chars().all(is_blank);
    if in_indent && prev == Some(' ') {
        let width = cfg.indent.width(cfg.tab_width);
        let col = display_column(buffer, pos, cfg.tab_width);
        let target = (col - 1) / width * width;
        let mut start = pos;
        let mut c = col;
        while start > line_start && c > target && buffer.char_at(start - 1) == Some(' ') {
            start -= 1;
            c -= 1;
        }
        return start..pos;
    }
    movement::left(buffer, pos)..pos
}

/// Delete key: deletes each selection, or else the grapheme after the
/// cursor.
pub fn delete_forward(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    delete_with(buffer, selections, |b, pos| pos..movement::right(b, pos))
}

/// Deletes each selection, or else back to the previous word start.
pub fn delete_word_left(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    delete_with(buffer, selections, |b, pos| {
        movement::word_left(b, pos)..pos
    })
}

/// Deletes each selection, or else forward to the next word end.
pub fn delete_word_right(buffer: &mut Buffer, selections: &mut SelectionSet) -> Vec<Edit> {
    delete_with(buffer, selections, |b, pos| {
        pos..movement::word_right(b, pos)
    })
}
