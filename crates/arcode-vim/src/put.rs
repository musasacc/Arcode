//! Pasting: `p P gp gP`, Visual `p`, block pastes and clipboard pastes.

use arcode_buffer::{Buffer, SelectionSet, movement};

use crate::normal::status;
use crate::ops::Region;
use crate::registers::is_clipboard;
use crate::text::{
    char_at_display_col, display_col, first_non_blank, has_line_break, last_line, line_of,
};
use crate::types::{Register, RegisterKind, VimAction, VimContext};
use crate::vim::{Ed, PendingPaste, Vim};

impl Vim {
    /// Register content by name, including the read-only `/` register.
    fn read_register(&self, name: char) -> Option<Register> {
        if name == '/' {
            let q = self.search.as_ref()?;
            return Some(Register::new(q.pattern.clone(), RegisterKind::Charwise));
        }
        self.registers.get(name).cloned()
    }

    /// `p` / `P` (`cursor_after` for `gp` / `gP`).
    pub(crate) fn paste(
        &mut self,
        before: bool,
        cursor_after: bool,
        count: usize,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        let name = reg.unwrap_or('"');
        if is_clipboard(name) {
            // The edit happens later, in `paste_clipboard`, which keeps
            // the `.` recording once the buffer changed.
            let dot = self.dot.take();
            self.pending_paste = Some(PendingPaste {
                before,
                cursor_after,
                count,
                dot,
            });
            return VimAction::PasteRequest;
        }
        let Some(r) = self.read_register(name) else {
            self.dot = None;
            return status(&format!("E353: Nothing in register {name}"));
        };
        if self.mode.is_visual() {
            self.visual_put(&r, before, ed);
        } else {
            self.put(&r, before, cursor_after, count, ed);
        }
        VimAction::None
    }

    /// Completes a [`VimAction::PasteRequest`] with the clipboard `text`
    /// (linewise if it ends with a line break). One undo step.
    pub fn paste_clipboard(
        &mut self,
        text: &str,
        buffer: &mut Buffer,
        selections: &mut SelectionSet,
        ctx: &VimContext,
    ) {
        let Some(p) = self.pending_paste.take() else {
            return;
        };
        if self.read_only {
            return;
        }
        let kind = if text.ends_with('\n') {
            RegisterKind::Linewise
        } else {
            RegisterKind::Charwise
        };
        let r = Register::new(text, kind);
        self.registers.set('+', r.clone());
        let mut ed = Ed {
            b: buffer,
            sels: selections,
            ctx,
        };
        ed.b.break_undo_group();
        if self.mode.is_visual() {
            self.visual_put(&r, p.before, &mut ed);
        } else {
            self.put(&r, p.before, p.cursor_after, p.count, &mut ed);
        }
        ed.b.break_undo_group();
        if let Some(d) = p.dot
            && ed.b.version() != d.version
        {
            self.last_change = Some(d);
        }
    }

    /// Pastes `r` `count` times at every cursor.
    pub(crate) fn put(
        &mut self,
        r: &Register,
        before: bool,
        cursor_after: bool,
        count: usize,
        ed: &mut Ed,
    ) {
        if r.kind == RegisterKind::Blockwise {
            self.put_block(r, before, count, ed);
            return;
        }
        let heads = Self::heads(ed);
        let distribute = heads.len() > 1 && r.pieces.len() == heads.len();
        let mut edits = Vec::new();
        let mut skip = Vec::new();
        for (i, &p) in heads.iter().enumerate() {
            let piece = if distribute {
                r.pieces[i].clone()
            } else {
                r.text()
            };
            let line = line_of(ed.b, p);
            if r.kind == RegisterKind::Linewise {
                let mut t = piece;
                if !t.ends_with('\n') {
                    t.push('\n');
                }
                let t = t.repeat(count);
                if before || has_line_break(ed.b, line) {
                    let at = ed.b.line_start(if before { line } else { line + 1 });
                    edits.push((at..at, t));
                    skip.push(0);
                } else {
                    let at = ed.b.len_chars();
                    edits.push((at..at, format!("\n{}", &t[..t.len() - 1])));
                    skip.push(1);
                }
            } else {
                let at = if before || ed.b.line_len(line) == 0 {
                    p
                } else {
                    movement::right(ed.b, p).min(ed.b.line_end(line))
                };
                edits.push((at..at, piece.repeat(count)));
                skip.push(0);
            }
        }
        let multiline: Vec<bool> = edits.iter().map(|(_, t)| t.contains('\n')).collect();
        let out = self.apply_edits(ed, edits);
        let cursors = out
            .iter()
            .enumerate()
            .map(|(i, &(s, e))| {
                if r.kind == RegisterKind::Linewise {
                    if cursor_after {
                        e
                    } else {
                        first_non_blank(ed.b, line_of(ed.b, s + skip[i]))
                    }
                } else if cursor_after {
                    e
                } else if multiline[i] {
                    s
                } else {
                    e.saturating_sub(1).max(s)
                }
            })
            .collect();
        self.set_cursors(ed, cursors);
    }

    /// Pastes a block register as a block at the primary cursor.
    fn put_block(&mut self, r: &Register, before: bool, count: usize, ed: &mut Ed) {
        let tab = ed.ctx.tab_size;
        let p = ed.sels.primary().head;
        let line0 = line_of(ed.b, p);
        let start = if before || ed.b.line_len(line0) == 0 {
            p
        } else {
            movement::right(ed.b, p).min(ed.b.line_end(line0))
        };
        let col = display_col(ed.b, start, tab);
        let rows: Vec<String> = r.pieces.iter().map(|row| row.repeat(count)).collect();
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let last = last_line(ed.b);
        let mut edits = Vec::new();
        let mut extra = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let line = line0 + i;
            if line > last {
                extra.push(format!("{}{row}", " ".repeat(col)));
                continue;
            }
            let (at, pad) = match char_at_display_col(ed.b, line, col, tab) {
                Some(at) => (at, 0),
                None => {
                    let end = ed.b.line_end(line);
                    (end, col.saturating_sub(display_col(ed.b, end, tab)))
                }
            };
            let fill = if at < ed.b.line_end(line) {
                " ".repeat(width - row.chars().count())
            } else {
                String::new()
            };
            edits.push((at..at, format!("{}{row}{fill}", " ".repeat(pad))));
        }
        if !extra.is_empty() {
            let at = ed.b.len_chars();
            let text = if has_line_break(ed.b, last) {
                format!("{}\n", extra.join("\n"))
            } else {
                format!("\n{}", extra.join("\n"))
            };
            edits.push((at..at, text));
        }
        let first_pad = edits
            .first()
            .map_or(0, |(_, t)| t.chars().take_while(|&c| c == ' ').count());
        let out = self.apply_edits(ed, edits);
        let cursor = out.first().map_or(p, |&(s, _)| s + first_pad.min(col));
        self.set_cursors(ed, vec![cursor]);
    }

    /// Visual `p` / `P`: replaces the selection with `r` (`p` also puts the
    /// replaced text in the unnamed register).
    pub(crate) fn visual_put(&mut self, r: &Register, keep_register: bool, ed: &mut Ed) {
        let regions = self.visual_regions(ed);
        self.exit_visual(ed);
        if !keep_register {
            let old = Register {
                pieces: regions
                    .iter()
                    .map(|(rg, _)| match rg {
                        Region::Lines(f, l) => crate::text::lines_text(ed.b, *f, *l),
                        Region::Chars(c) => ed.b.slice_to_string(c.clone()),
                        Region::Block(rows) => rows
                            .iter()
                            .map(|x| ed.b.slice_to_string(x.clone()))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    })
                    .collect(),
                kind: match regions.first() {
                    Some((Region::Lines(..), _)) => RegisterKind::Linewise,
                    _ => RegisterKind::Charwise,
                },
            };
            self.registers.store(None, old, false);
        }
        let linewise = r.kind == RegisterKind::Linewise;
        let text = r.text();
        let mut edits = Vec::new();
        let mut line_cursor = Vec::new();
        for (rg, _) in &regions {
            match rg {
                Region::Chars(c) if linewise => {
                    edits.push((c.clone(), format!("\n{text}")));
                    line_cursor.push(Some(1));
                }
                Region::Chars(c) => {
                    edits.push((c.clone(), text.clone()));
                    line_cursor.push(None);
                }
                Region::Lines(f, l) => {
                    let body = text.strip_suffix('\n').unwrap_or(&text).to_string();
                    edits.push((ed.b.line_start(*f)..ed.b.line_end(*l), body));
                    line_cursor.push(Some(0));
                }
                Region::Block(rows) => {
                    for (k, row) in rows.iter().enumerate() {
                        let t = if k == 0 { text.clone() } else { String::new() };
                        edits.push((row.clone(), t));
                        line_cursor.push(None);
                    }
                }
            }
        }
        let out = self.apply_edits(ed, edits);
        let cursors = out
            .iter()
            .zip(&line_cursor)
            .map(|(&(s, e), lc)| match lc {
                Some(skip) => first_non_blank(ed.b, line_of(ed.b, s + skip)),
                None => e.saturating_sub(1).max(s),
            })
            .collect();
        self.set_cursors(ed, cursors);
    }
}
