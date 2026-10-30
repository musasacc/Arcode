//! Insert and Replace mode.
//!
//! Text insertion is handled here (not by the editor): typed chars,
//! Enter (with auto-indent), Tab, Backspace, Delete, Ctrl-W, Ctrl-U and
//! Ctrl-R {reg} are applied at every cursor, so `.` and counts can replay
//! them. Arrow keys move and start a new undo step. Other Ctrl/Alt keys
//! are passed through to the editor.

use arcode_buffer::{Selection, SelectionSet, movement};

use crate::key::{Key, Tok};
use crate::parse::InsertKind;
use crate::text::line_of;
use crate::types::{Mode, Register, RegisterKind, VimAction};
use crate::vim::{Ed, Vim};

mod keys;

/// Prefix marking an internal key that carries literal text.
pub(crate) const TEXT_KEY: char = '\u{E000}';

/// State of one Insert/Replace session.
#[derive(Debug, Clone, Default)]
pub(crate) struct InsertSession {
    /// How it was entered (`None` for `c`); used for count repeats.
    pub kind: Option<InsertKind>,
    pub count: usize,
    /// Keys typed, replayed `count - 1` more times on `<Esc>`.
    pub keys: Vec<Key>,
    /// Inserted text (for the `.` register).
    pub text: String,
    /// Block insert: collapse to the first cursor on `<Esc>`.
    pub block: bool,
    /// Replace mode: replaced chars per step, for Backspace.
    pub replaced: Vec<Vec<Option<char>>>,
    pub ctrl_r: bool,
    pub repeating: bool,
}

impl Vim {
    /// Enters Insert (or Replace) mode with cursors at `cursors`.
    pub(crate) fn start_insert(
        &mut self,
        kind: Option<InsertKind>,
        count: usize,
        cursors: Vec<usize>,
        block: bool,
        replace: bool,
        ed: &mut Ed,
    ) {
        let sels: Vec<Selection> = cursors.into_iter().map(Selection::cursor).collect();
        if let Some(set) = SelectionSet::from_vec(sels, 0) {
            *ed.sels = set;
        }
        self.visual.rendered = None;
        self.mode = if replace { Mode::Replace } else { Mode::Insert };
        self.insert = Some(InsertSession {
            kind,
            count: count.max(1),
            block,
            ..InsertSession::default()
        });
    }

    /// Inserts `text` as if typed (for IME commits or pastes the editor
    /// receives while in Insert/Replace mode); recorded for `.`. Does
    /// nothing in other modes.
    pub fn insert_text(
        &mut self,
        text: &str,
        buffer: &mut arcode_buffer::Buffer,
        selections: &mut SelectionSet,
        ctx: &crate::types::VimContext,
    ) {
        if !matches!(self.mode, Mode::Insert | Mode::Replace) || text.is_empty() {
            return;
        }
        let key = Key::new(format!("{TEXT_KEY}{text}"));
        let mut ed = Ed {
            b: buffer,
            sels: selections,
            ctx,
        };
        self.process(&key, &mut ed);
    }

    fn record_insert(&mut self, key: &Key) {
        self.record_key(key);
        if let Some(s) = &mut self.insert
            && !s.repeating
        {
            s.keys.push(key.clone());
        }
    }

    /// Handles a key in Insert/Replace mode.
    pub(crate) fn insert_key(&mut self, key: &Key, ed: &mut Ed) -> VimAction {
        if let Some(text) = key.key.strip_prefix(TEXT_KEY) {
            self.record_insert(key);
            self.type_text(text, ed);
            return VimAction::None;
        }
        let tok = key.tok();
        if self.insert.as_ref().is_some_and(|s| s.ctrl_r) {
            if let Some(s) = &mut self.insert {
                s.ctrl_r = false;
            }
            self.record_insert(key);
            if let Tok::Char(c) = tok
                && let Some(r) = self.registers.get(c).cloned()
            {
                self.type_text(&r.text(), ed);
            }
            return VimAction::None;
        }
        match tok {
            Tok::Esc => {
                self.record_key(key);
                self.leave_insert(ed);
                return VimAction::None;
            }
            Tok::Left | Tok::Right | Tok::Up | Tok::Down | Tok::Home | Tok::End => {
                self.insert_move(tok, ed);
                return VimAction::None;
            }
            Tok::Other => return VimAction::PassThrough,
            Tok::Ctrl(c) if !matches!(c, 'h' | 'w' | 'u' | 'r' | 'j' | 'm') => {
                return VimAction::PassThrough;
            }
            _ => {}
        }
        self.record_insert(key);
        match tok {
            Tok::Char(c) => self.type_text(&c.to_string(), ed),
            Tok::Enter | Tok::Ctrl('j' | 'm') => self.newline(ed),
            Tok::Tab => self.tab(ed),
            Tok::Backspace | Tok::Ctrl('h') => self.backspace(ed),
            Tok::Delete => self.delete_ranges(ed, |b, p| p..movement::right(b, p)),
            Tok::Ctrl('w') => self.delete_ranges(ed, |b, p| {
                let ls = b.line_start(line_of(b, p));
                if p == ls {
                    p.saturating_sub(1)..p
                } else {
                    movement::word_left(b, p).max(ls)..p
                }
            }),
            Tok::Ctrl('u') => self.delete_ranges(ed, |b, p| {
                let line = line_of(b, p);
                let first = movement::first_non_whitespace(b, line);
                if p > first {
                    first..p
                } else {
                    b.line_start(line)..p
                }
            }),
            Tok::Ctrl('r') => {
                if let Some(s) = &mut self.insert {
                    s.ctrl_r = true;
                }
            }
            _ => {}
        }
        VimAction::None
    }

    /// `<Esc>`: repeats the insert for a count, stores the `.` register,
    /// returns to Normal mode one char left.
    fn leave_insert(&mut self, ed: &mut Ed) {
        let Some(session) = self.insert.take() else {
            self.mode = Mode::Normal;
            return;
        };
        if session.count > 1 && session.kind.is_some() {
            self.insert = Some(InsertSession {
                repeating: true,
                ..session.clone()
            });
            // The repetitions are not typed keys: keep them out of the `.`
            // recording (it already ends with this `<Esc>`).
            let replaying = std::mem::replace(&mut self.replaying, true);
            for _ in 1..session.count {
                if matches!(session.kind, Some(InsertKind::Below | InsertKind::Above)) {
                    let cursors = self.open_lines(true, ed);
                    self.set_insert_cursors(ed, cursors);
                }
                for k in &session.keys {
                    self.insert_key(k, ed);
                }
            }
            self.replaying = replaying;
            self.insert = None;
        }
        self.registers.set(
            '.',
            Register::new(session.text.clone(), RegisterKind::Charwise),
        );
        self.marks.insert('^', ed.sels.primary().head);
        self.mode = Mode::Normal;
        if session.block {
            let first = ed.sels.iter().next().map_or(0, |s| s.head);
            *ed.sels = SelectionSet::single(first);
        }
        let b = &*ed.b;
        let cursors = ed
            .sels
            .iter()
            .map(|s| {
                let ls = b.line_start(line_of(b, s.head));
                if s.head > ls {
                    movement::left(b, s.head)
                } else {
                    s.head
                }
            })
            .collect();
        self.set_cursors(ed, cursors);
    }
}
