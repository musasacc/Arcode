//! Normal-mode commands that are not operators.

use arcode_buffer::movement;

use crate::parse::{Act, InsertKind, Scroll};
use crate::text::{
    ch, clamp_normal, first_non_blank, indent_of, is_blank, last_line, line_of, toggle,
};
use crate::types::{VimAction, VimCommand};
use crate::vim::{Ed, Vim};

impl Vim {
    /// Runs a Normal-mode [`Act`].
    pub(crate) fn do_act(
        &mut self,
        act: Act,
        count: Option<usize>,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        let n = count.unwrap_or(1).max(1);
        match act {
            Act::Insert(kind) => {
                self.enter_insert(kind, n, ed);
                VimAction::None
            }
            Act::ReplaceMode => {
                let heads = Self::heads(ed);
                self.start_insert(Some(InsertKind::Before), n, heads, false, true, ed);
                VimAction::None
            }
            Act::Visual(mode) => {
                let regions = ed.sels.iter().map(|s| (s.head, s.head)).collect();
                self.set_visual(mode, regions, ed);
                VimAction::None
            }
            Act::Reselect => {
                self.reselect(ed);
                VimAction::None
            }
            Act::Replace(c) => self.replace_chars(c, n, ed),
            Act::ToggleCaseChar => self.toggle_chars(n, ed),
            Act::Join(spaces) => self.join_lines(n, spaces, ed),
            Act::Paste {
                before,
                cursor_after,
            } => self.paste(before, cursor_after, n, reg, ed),
            Act::Undo => self.undo_redo(n, false, ed),
            Act::Redo => self.undo_redo(n, true, ed),
            Act::Repeat => self.repeat(count, ed),
            Act::CmdLine => {
                let text = match count {
                    Some(c) if c > 1 => format!(".,.+{}", c - 1),
                    _ => String::new(),
                };
                self.open_cmdline(':', text, None, ed);
                VimAction::None
            }
            Act::SearchPrompt(forward) => {
                self.open_cmdline(if forward { '/' } else { '?' }, String::new(), None, ed);
                VimAction::None
            }
            Act::SetMark(c) => {
                let name = if c == '`' { '\'' } else { c };
                self.marks.insert(name, ed.sels.primary().head);
                VimAction::None
            }
            Act::Scroll(s) => self.scroll(s, ed),
            Act::ScrollLine(d) => VimAction::ScrollLines(d * n as isize),
            Act::SaveQuit => VimAction::Command(VimCommand::SaveQuit),
            Act::ForceQuit => VimAction::Command(VimCommand::ForceQuit),
            Act::MacroRecord(c) => {
                self.macro_start(c);
                VimAction::StatusMessage(format!("recording @{c}"))
            }
            Act::MacroStop => {
                self.macro_stop();
                VimAction::StatusMessage(String::new())
            }
            Act::MacroPlay(c) => self.macro_play(c, n, ed),
            Act::Increment(d) => self.increment(d * n as i64, ed),
            Act::PassThrough => VimAction::PassThrough,
            Act::SwapEnds | Act::BlockInsert(_) => VimAction::None,
        }
    }

    fn enter_insert(&mut self, kind: InsertKind, n: usize, ed: &mut Ed) {
        if matches!(kind, InsertKind::Below | InsertKind::Above) {
            let cursors = self.open_lines(kind == InsertKind::Below, ed);
            self.start_insert(Some(kind), n, cursors, false, false, ed);
            return;
        }
        let b = &*ed.b;
        let cursors: Vec<usize> = ed
            .sels
            .iter()
            .map(|s| {
                let p = s.head;
                let line = line_of(b, p);
                match kind {
                    InsertKind::After if b.line_len(line) > 0 => movement::right(b, p),
                    InsertKind::LineStart => movement::first_non_whitespace(b, line),
                    InsertKind::LineEnd => b.line_end(line),
                    InsertKind::Column0 => b.line_start(line),
                    InsertKind::LastInsert => self.marks.get(&'^').copied().unwrap_or(p),
                    _ => p,
                }
            })
            .collect();
        let cursors = cursors.into_iter().map(|p| b.clamp_char(p)).collect();
        self.start_insert(Some(kind), n, cursors, false, false, ed);
    }

    /// Opens a line below/above each cursor line with its indentation (one
    /// unit deeper below a line ending in `{ ( [ :`); returns the cursors.
    pub(crate) fn open_lines(&mut self, below: bool, ed: &mut Ed) -> Vec<usize> {
        let edits = ed
            .sels
            .iter()
            .map(|s| {
                let line = line_of(ed.b, s.head);
                let mut indent = indent_of(ed.b, line);
                if below {
                    if ed
                        .b
                        .line_text(line)
                        .trim_end()
                        .ends_with(['{', '(', '[', ':'])
                    {
                        indent.push_str(&ed.ctx.indent.text());
                    }
                    let at = ed.b.line_end(line);
                    (at..at, format!("\n{indent}"))
                } else {
                    let at = ed.b.line_start(line);
                    (at..at, format!("{indent}\n"))
                }
            })
            .collect();
        let out = self.apply_edits(ed, edits);
        out.into_iter()
            .map(|(_, e)| if below { e } else { e.saturating_sub(1) })
            .collect()
    }

    fn replace_chars(&mut self, c: char, n: usize, ed: &mut Ed) -> VimAction {
        // Each cursor replaces `n` grapheme clusters; all must fit.
        let mut edits = Vec::new();
        for p in Self::heads(ed) {
            let Some(end) = grapheme_end(ed.b, p, n) else {
                self.dot = None;
                return VimAction::None;
            };
            let text = if c == '\n' {
                "\n".to_string()
            } else {
                c.to_string().repeat(n)
            };
            edits.push((p..end, text));
        }
        let out = self.apply_edits(ed, edits);
        let cursors = out
            .into_iter()
            .map(|(_, e)| if c == '\n' { e } else { e.saturating_sub(1) })
            .collect();
        self.set_cursors(ed, cursors);
        VimAction::None
    }

    fn toggle_chars(&mut self, n: usize, ed: &mut Ed) -> VimAction {
        let mut edits = Vec::new();
        for p in Self::heads(ed) {
            let le = ed.b.line_end(line_of(ed.b, p));
            let mut end = p;
            for _ in 0..n {
                if end >= le {
                    break;
                }
                end = movement::right(ed.b, end).min(le);
            }
            let text: String = ed.b.slice_to_string(p..end).chars().map(toggle).collect();
            edits.push((p..end, text));
        }
        let out = self.apply_edits(ed, edits);
        self.set_cursors(ed, out.into_iter().map(|(_, e)| e).collect());
        VimAction::None
    }

    /// `J` / `gJ`: joins `max(n, 2)` lines at each cursor.
    pub(crate) fn join_lines(&mut self, n: usize, spaces: bool, ed: &mut Ed) -> VimAction {
        let lines: Vec<usize> = Self::heads(ed).iter().map(|&p| line_of(ed.b, p)).collect();
        self.join_range(
            lines.into_iter().map(|l| (l, l + n.max(2) - 1)).collect(),
            spaces,
            ed,
        )
    }

    /// Joins lines `first..=last` of each block.
    pub(crate) fn join_range(
        &mut self,
        blocks: Vec<(usize, usize)>,
        spaces: bool,
        ed: &mut Ed,
    ) -> VimAction {
        let last_line = last_line(ed.b);
        if blocks.iter().all(|&(f, _)| f >= last_line) {
            self.dot = None;
            return VimAction::None;
        }
        let mut edits = Vec::new();
        let mut owner = Vec::new();
        for (i, &(first, last)) in blocks.iter().enumerate() {
            let last = last.min(last_line);
            let mut acc_empty = ed.b.line_len(first) == 0;
            let mut acc_blank_end = ed.b.line_text(first).ends_with(is_blank);
            for k in first..last {
                let next = ed.b.line_text(k + 1);
                let lead = if spaces {
                    next.chars().take_while(|&c| is_blank(c)).count()
                } else {
                    0
                };
                let rest = &next[next.char_indices().nth(lead).map_or(next.len(), |(b, _)| b)..];
                let start = ed.b.line_end(k);
                let sep = if !spaces
                    || rest.is_empty()
                    || acc_empty
                    || acc_blank_end
                    || rest.starts_with(')')
                {
                    ""
                } else {
                    " "
                };
                edits.push((start..ed.b.line_start(k + 1) + lead, sep.to_string()));
                owner.push(i);
                if !rest.is_empty() {
                    acc_empty = false;
                    acc_blank_end = rest.ends_with(is_blank);
                }
            }
        }
        // A block with nothing to join keeps its cursor (shifted by the
        // other blocks' edits).
        let heads = Self::heads(ed);
        let mut stay: Vec<usize> = blocks
            .iter()
            .enumerate()
            .map(|(i, &(first, _))| match heads.get(i) {
                Some(&h) if heads.len() == blocks.len() => h,
                _ => ed.b.line_start(first),
            })
            .collect();
        let ranges: Vec<std::ops::Range<usize>> = edits.iter().map(|(r, _)| r.clone()).collect();
        let out = self.apply_edits(ed, edits);
        for p in &mut stay {
            let orig = *p;
            for (r, &(s, e)) in ranges.iter().zip(&out) {
                if r.end <= orig {
                    *p = (*p + (e - s)).saturating_sub(r.len());
                }
            }
        }
        let mut cursors = stay;
        for (k, &i) in owner.iter().enumerate() {
            cursors[i] = out[k].0;
        }
        self.set_cursors(ed, cursors);
        VimAction::None
    }

    fn undo_redo(&mut self, n: usize, redo: bool, ed: &mut Ed) -> VimAction {
        let mut pos = None;
        for _ in 0..n {
            let res = if redo { ed.b.redo() } else { ed.b.undo() };
            let Some(res) = res else { break };
            self.on_external_edits(&res.edits);
            pos = res
                .edits
                .iter()
                .map(|e| e.range.start + usize::from(e.new_text.starts_with('\n')))
                .min()
                .or(pos);
        }
        match pos {
            Some(p) => {
                let p = ed.b.clamp_char(p);
                self.set_cursors(ed, vec![p]);
                VimAction::None
            }
            None if redo => VimAction::StatusMessage("Already at newest change".into()),
            None => VimAction::StatusMessage("Already at oldest change".into()),
        }
    }

    fn scroll(&mut self, s: Scroll, ed: &mut Ed) -> VimAction {
        let (fnb, action) = match s {
            Scroll::Top(f) => (f, VimAction::ScrollCursorTop),
            Scroll::Center(f) => (f, VimAction::ScrollCursorCenter),
            Scroll::Bottom(f) => (f, VimAction::ScrollCursorBottom),
        };
        if fnb {
            let line = line_of(ed.b, ed.sels.primary().head);
            let p = first_non_blank(ed.b, line);
            self.set_cursors(ed, vec![p]);
        }
        action
    }

    /// Ctrl-A / Ctrl-X: adds `delta` to the decimal number at or after the
    /// cursor on its line.
    fn increment(&mut self, delta: i64, ed: &mut Ed) -> VimAction {
        let pos = ed.sels.primary().head;
        let line_end = ed.b.line_end(line_of(ed.b, pos));
        let ls = ed.b.line_start(line_of(ed.b, pos));
        let Some(mut start) = (pos..line_end).find(|&i| ch(ed.b, i).is_ascii_digit()) else {
            self.dot = None;
            return VimAction::None;
        };
        while start > ls && ch(ed.b, start - 1).is_ascii_digit() {
            start -= 1;
        }
        let mut end = start;
        while end < line_end && ch(ed.b, end).is_ascii_digit() {
            end += 1;
        }
        let negative = start > ls && ch(ed.b, start - 1) == '-';
        let digits = ed.b.slice_to_string(start..end);
        let Ok(value) = digits.parse::<i128>() else {
            return VimAction::None;
        };
        let value = if negative { -value } else { value };
        let new = value.saturating_add(delta as i128).to_string();
        let from = if negative { start - 1 } else { start };
        let out = self.apply_edits(ed, vec![(from..end, new)]);
        let cursor = out.first().map_or(from, |&(_, e)| e.saturating_sub(1));
        self.set_cursors(ed, vec![clamp_normal(ed.b, cursor)]);
        VimAction::None
    }
}

/// End of the `n` grapheme clusters starting at `p`, if they all lie on
/// `p`'s line.
fn grapheme_end(b: &arcode_buffer::Buffer, p: usize, n: usize) -> Option<usize> {
    let le = b.line_end(line_of(b, p));
    (0..n).try_fold(p, |q, _| (q < le).then(|| movement::right(b, q).min(le)))
}
