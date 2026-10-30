//! Insert-mode editing keys: typing, Enter, Tab, Backspace, deletes and
//! cursor moves.

use std::ops::Range;

use arcode_buffer::ops::IndentUnit;
use arcode_buffer::{Selection, SelectionSet, movement};

use crate::key::{Key, Tok};
use crate::parse::InsertKind;
use crate::text::{ch, display_col, is_blank, line_of};
use crate::types::Mode;
use crate::vim::{Ed, Vim};

impl Vim {
    /// Replaces every selection with `text` (overwriting in Replace mode).
    pub(super) fn type_text(&mut self, text: &str, ed: &mut Ed) {
        if self.mode == Mode::Replace {
            for c in text.chars() {
                self.replace_char(c, ed);
            }
        } else {
            let edits = ed
                .sels
                .iter()
                .map(|s| (s.range(), text.to_string()))
                .collect();
            let out = self.apply_edits(ed, edits);
            self.set_insert_cursors(ed, out.into_iter().map(|(_, e)| e).collect());
        }
        if let Some(s) = &mut self.insert {
            s.text.push_str(text);
        }
    }

    pub(super) fn replace_char(&mut self, c: char, ed: &mut Ed) {
        let mut step = Vec::new();
        let mut edits = Vec::new();
        for s in ed.sels.iter() {
            let p = s.head;
            let at_end = p >= ed.b.line_end(line_of(ed.b, p)) || c == '\n';
            step.push(if at_end { None } else { ed.b.char_at(p) });
            let end = if at_end { p } else { p + 1 };
            edits.push((p..end, c.to_string()));
        }
        let out = self.apply_edits(ed, edits);
        self.set_insert_cursors(ed, out.into_iter().map(|(_, e)| e).collect());
        if let Some(s) = &mut self.insert {
            s.replaced.push(step);
        }
    }

    pub(crate) fn set_insert_cursors(&self, ed: &mut Ed, cursors: Vec<usize>) {
        let sels: Vec<Selection> = cursors.into_iter().map(Selection::cursor).collect();
        if let Some(set) = SelectionSet::from_vec(sels, ed.sels.primary_index()) {
            *ed.sels = set;
        }
    }

    pub(super) fn delete_ranges(
        &mut self,
        ed: &mut Ed,
        f: impl Fn(&arcode_buffer::Buffer, usize) -> Range<usize>,
    ) {
        let edits = ed
            .sels
            .iter()
            .map(|s| (f(ed.b, s.head), String::new()))
            .collect();
        let out = self.apply_edits(ed, edits);
        self.set_insert_cursors(ed, out.into_iter().map(|(s, _)| s).collect());
        if let Some(s) = &mut self.insert {
            s.text.pop();
        }
    }

    pub(super) fn newline(&mut self, ed: &mut Ed) {
        if self.mode == Mode::Replace {
            self.type_text("\n", ed);
            return;
        }
        let unit = ed.ctx.indent.text();
        let mut edits = Vec::new();
        let mut offsets = Vec::new();
        for s in ed.sels.iter() {
            let p = s.head;
            let ls = ed.b.line_start(line_of(ed.b, p));
            let before = ed.b.slice_to_string(ls..p);
            let indent: String = before.chars().take_while(|&c| is_blank(c)).collect();
            let prev = before.trim_end_matches(is_blank).chars().last();
            let mut text = format!("\n{indent}");
            if matches!(prev, Some('{' | '(' | '[')) {
                text.push_str(&unit);
            }
            let cursor = text.chars().count();
            let next = ed.b.char_at(p);
            if matches!(
                (prev, next),
                (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']'))
            ) {
                text.push('\n');
                text.push_str(&indent);
            }
            edits.push((p..p, text));
            offsets.push(cursor);
        }
        let out = self.apply_edits(ed, edits);
        let cursors = out.iter().zip(offsets).map(|(&(s, _), o)| s + o).collect();
        self.set_insert_cursors(ed, cursors);
        if let Some(s) = &mut self.insert {
            s.text.push('\n');
        }
    }

    pub(super) fn tab(&mut self, ed: &mut Ed) {
        let tab = ed.ctx.tab_size.max(1);
        let edits = ed
            .sels
            .iter()
            .map(|s| {
                let text = match ed.ctx.indent {
                    IndentUnit::Tab => "\t".to_string(),
                    IndentUnit::Spaces(w) => {
                        let w = w.max(1);
                        " ".repeat(w - display_col(ed.b, s.head, tab) % w)
                    }
                };
                (s.range(), text)
            })
            .collect();
        let out = self.apply_edits(ed, edits);
        self.set_insert_cursors(ed, out.into_iter().map(|(_, e)| e).collect());
    }

    pub(super) fn backspace(&mut self, ed: &mut Ed) {
        if self.mode == Mode::Replace {
            let step = self.insert.as_mut().and_then(|s| s.replaced.pop());
            let heads = Self::heads(ed);
            let edits = match step {
                Some(step) if step.len() == heads.len() => heads
                    .iter()
                    .zip(step)
                    .map(|(&p, orig)| {
                        (
                            p.saturating_sub(1)..p,
                            orig.map(String::from).unwrap_or_default(),
                        )
                    })
                    .collect(),
                _ => {
                    let cursors = heads.iter().map(|&p| movement::left(ed.b, p)).collect();
                    self.set_insert_cursors(ed, cursors);
                    return;
                }
            };
            let out = self.apply_edits(ed, edits);
            self.set_insert_cursors(ed, out.into_iter().map(|(s, _)| s).collect());
            return;
        }
        let soft = match ed.ctx.indent {
            IndentUnit::Spaces(w) => w.max(1),
            IndentUnit::Tab => 0,
        };
        self.delete_ranges(ed, |b, p| {
            let ls = b.line_start(line_of(b, p));
            let col = p - ls;
            if soft > 0 && col > 0 && (ls..p).all(|i| ch(b, i) == ' ') {
                let n = (col - 1) % soft + 1;
                return p - n..p;
            }
            movement::left(b, p)..p
        });
    }

    /// Arrow keys in Insert mode: move, start a new undo step, and restart
    /// the `.` recording as a plain `i`.
    pub(super) fn insert_move(&mut self, tok: Tok, ed: &mut Ed) {
        let tab = ed.ctx.tab_size;
        let b = &*ed.b;
        ed.sels.transform(|mut s| {
            let p = s.head;
            let line = line_of(b, p);
            match tok {
                Tok::Left => s.move_to(movement::left(b, p).max(b.line_start(line)), false),
                Tok::Right => s.move_to(movement::right(b, p).min(b.line_end(line)), false),
                Tok::Home => s.move_to(b.line_start(line), false),
                Tok::End => s.move_to(b.line_end(line), false),
                Tok::Up | Tok::Down => {
                    let (np, goal) = if tok == Tok::Up {
                        movement::up(b, p, s.goal_column, tab)
                    } else {
                        movement::down(b, p, s.goal_column, tab)
                    };
                    s.move_to(np, false);
                    s.goal_column = Some(goal);
                }
                _ => {}
            }
            s
        });
        ed.b.break_undo_group();
        if !self.replaying {
            self.finish_dot(ed);
            if self.mode == Mode::Insert {
                self.start_dot(&[Key::char('i')], &[], None, None, ed.b.version());
            }
        }
        if let Some(s) = &mut self.insert {
            s.keys.clear();
            s.count = 1;
            s.kind = Some(InsertKind::Before);
        }
    }
}
