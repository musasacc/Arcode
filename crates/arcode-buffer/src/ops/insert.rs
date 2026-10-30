//! Text insertion commands: typing, pasting, auto-close, newline, tab.

use std::ops::Range;

use super::{OpsConfig, commit, indent_lines, prepare, set_from};
use crate::buffer::Buffer;
use crate::changes::{Assoc, map_pos};
use crate::edit::Edit;
use crate::history::UndoGroup;
use crate::line_ending::normalize;
use crate::movement::display_column;
use crate::selection::{Selection, SelectionSet};
use crate::text_util::{is_blank, is_word_char};

/// Replaces every selection with `text`, leaving cursors after it.
///
/// Typing (all selections empty, no line break in `text`) merges into the
/// previous typing undo step.
pub fn insert_text(buffer: &mut Buffer, selections: &mut SelectionSet, text: &str) -> Vec<Edit> {
    let text = normalize(text).into_owned();
    let texts = vec![text; selections.len()];
    insert_per_selection(buffer, selections, texts)
}

/// Pastes `text`. With several selections and exactly as many lines in
/// `text`, each selection receives one line (the inverse of copying with
/// [`super::selected_text`]). Always a new undo step.
pub fn paste(buffer: &mut Buffer, selections: &mut SelectionSet, text: &str) -> Vec<Edit> {
    let text = normalize(text).into_owned();
    let lines: Vec<&str> = text.split('\n').collect();
    let texts = if selections.len() > 1 && lines.len() == selections.len() {
        lines.into_iter().map(str::to_string).collect()
    } else {
        vec![text; selections.len()]
    };
    buffer.break_undo_group();
    let edits = insert_per_selection(buffer, selections, texts);
    buffer.break_undo_group();
    edits
}

fn insert_per_selection(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    texts: Vec<String>,
) -> Vec<Edit> {
    let typing = selections.iter().all(Selection::is_empty)
        && texts.iter().all(|t| !t.contains('\n') && !t.is_empty());
    let lens: Vec<usize> = texts.iter().map(|t| t.chars().count()).collect();
    let raw: Vec<(Range<usize>, String)> =
        selections.iter().map(Selection::range).zip(texts).collect();
    let changes = prepare(buffer, raw);
    let after: Vec<Selection> = selections
        .iter()
        .zip(lens)
        .map(|(s, len)| Selection::cursor(map_pos(s.start(), &changes, Assoc::Before) + len))
        .collect();
    let after = set_from(after, selections.primary_index(), selections);
    let group = if typing {
        UndoGroup::Merge
    } else {
        UndoGroup::New
    };
    commit(buffer, selections, &changes, after, group)
}

/// What typing one char does at one selection.
enum Typed {
    /// Insert `text` over `range`; the cursor lands `cursor` chars after
    /// the start of the change.
    Insert {
        range: Range<usize>,
        text: String,
        cursor: usize,
    },
    /// Wrap the selection in `open`/`close`, keeping it selected.
    Surround {
        sel: Selection,
        open: char,
        close: char,
    },
    /// Step over the existing char at `pos`.
    Skip { pos: usize },
}

fn plan_typed(buffer: &Buffer, sel: &Selection, c: char, cfg: &OpsConfig) -> Typed {
    let ac = &cfg.auto_close;
    let plain = Typed::Insert {
        range: sel.range(),
        text: c.to_string(),
        cursor: 1,
    };
    if !ac.enabled {
        return plain;
    }
    let pos = sel.head;
    let next = buffer.char_at(sel.end());
    let prev = sel.start().checked_sub(1).and_then(|p| buffer.char_at(p));
    if sel.is_empty() && ac.is_closer(c) && next == Some(c) {
        return Typed::Skip { pos };
    }
    let Some(close) = ac.closer_for(c) else {
        return plain;
    };
    if !sel.is_empty() {
        return if ac.surround {
            Typed::Surround {
                sel: *sel,
                open: c,
                close,
            }
        } else {
            plain
        };
    }
    let before_word = next.is_some_and(is_word_char);
    let after_word_quote = close == c && prev.is_some_and(is_word_char);
    if (ac.skip_before_word && before_word) || after_word_quote {
        return plain;
    }
    Typed::Insert {
        range: pos..pos,
        text: format!("{c}{close}"),
        cursor: 1,
    }
}

/// Types `text`. A single char goes through auto-closing (see
/// [`super::AutoClose`]); anything else behaves like [`insert_text`].
pub fn type_text(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    text: &str,
    cfg: &OpsConfig,
) -> Vec<Edit> {
    if text == "\r\n" {
        return newline_and_indent(buffer, selections, cfg);
    }
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return insert_text(buffer, selections, text);
    };
    if c == '\n' || c == '\r' {
        return newline_and_indent(buffer, selections, cfg);
    }
    let plans: Vec<Typed> = selections
        .iter()
        .map(|s| plan_typed(buffer, s, c, cfg))
        .collect();
    let mut raw = Vec::new();
    for p in &plans {
        match p {
            Typed::Insert { range, text, .. } => raw.push((range.clone(), text.clone())),
            Typed::Surround { sel, open, close } => {
                raw.push((sel.start()..sel.start(), open.to_string()));
                raw.push((sel.end()..sel.end(), close.to_string()));
            }
            Typed::Skip { .. } => {}
        }
    }
    let changes = prepare(buffer, raw);
    let after: Vec<Selection> = plans
        .iter()
        .map(|p| match p {
            Typed::Insert { range, cursor, .. } => {
                Selection::cursor(map_pos(range.start, &changes, Assoc::Before) + cursor)
            }
            Typed::Surround { sel, .. } => {
                let start = map_pos(sel.start(), &changes, Assoc::After);
                let end = map_pos(sel.end(), &changes, Assoc::Before);
                if sel.is_reversed() {
                    Selection::new(end, start)
                } else {
                    Selection::new(start, end)
                }
            }
            Typed::Skip { pos } => Selection::cursor(map_pos(pos + 1, &changes, Assoc::Before)),
        })
        .collect();
    let typing = selections.iter().all(Selection::is_empty);
    let after = set_from(after, selections.primary_index(), selections);
    let group = if typing {
        UndoGroup::Merge
    } else {
        UndoGroup::New
    };
    commit(buffer, selections, &changes, after, group)
}

fn closes(open: char, close: char) -> bool {
    matches!((open, close), ('{', '}') | ('(', ')') | ('[', ']'))
}

/// Inserts a line break at every selection, copying the current line's
/// indentation. After a line ending in `{`, `(`, `[` or `:` one indent
/// level is added; between a bracket pair (`{|}`) the closing bracket goes
/// on its own line.
pub fn newline_and_indent(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    cfg: &OpsConfig,
) -> Vec<Edit> {
    let unit = cfg.indent.text();
    let mut raw = Vec::new();
    let mut offsets = Vec::new();
    for sel in selections.iter() {
        let (start, end) = (sel.start(), sel.end());
        let line = buffer.char_to_line(start);
        let line_start = buffer.line_start(line);
        let indent: String = buffer
            .text()
            .slice(line_start..start)
            .chars()
            .take_while(|&c| is_blank(c))
            .collect();
        let before: String = buffer.text().slice(line_start..start).chars().collect();
        let prev = before.trim_end_matches(is_blank).chars().last();
        let next = buffer.char_at(end);
        let opens = matches!(prev, Some('{' | '(' | '[' | ':'));
        let mut text = format!("\n{indent}");
        if opens {
            text.push_str(&unit);
        }
        let cursor = text.chars().count();
        if let (Some(p), Some(n)) = (prev, next)
            && closes(p, n)
        {
            text.push('\n');
            text.push_str(&indent);
        }
        raw.push((start..end, text));
        offsets.push((start, cursor));
    }
    let changes = prepare(buffer, raw);
    let after: Vec<Selection> = offsets
        .into_iter()
        .map(|(start, cursor)| Selection::cursor(map_pos(start, &changes, Assoc::Before) + cursor))
        .collect();
    let after = set_from(after, selections.primary_index(), selections);
    buffer.break_undo_group();
    commit(buffer, selections, &changes, after, UndoGroup::New)
}

/// Tab key: indents the lines of a multi-line selection; otherwise replaces
/// each selection with indentation up to the next indent stop (a tab
/// character for [`super::IndentUnit::Tab`]).
pub fn insert_tab(
    buffer: &mut Buffer,
    selections: &mut SelectionSet,
    cfg: &OpsConfig,
) -> Vec<Edit> {
    let multi_line = selections
        .iter()
        .any(|s| buffer.char_to_line(s.start()) != buffer.char_to_line(s.end()));
    if multi_line {
        return indent_lines(buffer, selections, cfg.indent);
    }
    let width = cfg.indent.width(cfg.tab_width);
    let texts: Vec<String> = selections
        .iter()
        .map(|s| match cfg.indent {
            super::IndentUnit::Tab => "\t".to_string(),
            super::IndentUnit::Spaces(_) => {
                let col = display_column(buffer, s.start(), cfg.tab_width);
                " ".repeat(width - col % width)
            }
        })
        .collect();
    buffer.break_undo_group();
    insert_per_selection(buffer, selections, texts)
}
