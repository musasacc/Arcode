//! The [`Vim`] state machine: key dispatch, undo grouping, edits.

use std::collections::HashMap;
use std::ops::Range;

use arcode_buffer::{Buffer, Edit, Selection, SelectionSet, UndoGroup};

use crate::cmdline::CmdLine;
use crate::insert::InsertSession;
use crate::key::{Key, Tok};
use crate::motion::{Ctx, Env, FindSpec};
use crate::parse::{Parse, ParseCtx, parse};
use crate::registers::Registers;
use crate::repeat::DotRec;
use crate::text::clamp_normal;
use crate::types::{Mode, Register, SearchQuery, VimAction, VimContext};
use crate::visual::{LastVisual, VisualState};

/// The buffer, selections and settings of one keystroke.
pub(crate) struct Ed<'a> {
    pub b: &'a mut Buffer,
    pub sels: &'a mut SelectionSet,
    pub ctx: &'a VimContext,
}

/// A clipboard paste waiting for [`Vim::paste_clipboard`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingPaste {
    pub before: bool,
    pub cursor_after: bool,
    pub count: usize,
    /// The `.` recording of the paste command, kept once the paste edits.
    pub dot: Option<DotRec>,
}

/// Vim mode state for one editor (see the crate docs).
#[derive(Debug, Default)]
pub struct Vim {
    pub(crate) mode: Mode,
    pub(crate) pending: Vec<Key>,
    pub(crate) op_pending: bool,
    pub(crate) registers: Registers,
    pub(crate) marks: HashMap<char, usize>,
    pub(crate) last_find: Option<FindSpec>,
    pub(crate) search: Option<SearchQuery>,
    pub(crate) highlight: bool,
    pub(crate) visual: VisualState,
    pub(crate) last_visual: Option<LastVisual>,
    pub(crate) cmdline: Option<CmdLine>,
    pub(crate) insert: Option<InsertSession>,
    pub(crate) dot: Option<DotRec>,
    pub(crate) last_change: Option<DotRec>,
    pub(crate) replaying: bool,
    pub(crate) macro_rec: Option<(char, Vec<Key>)>,
    pub(crate) macros: HashMap<char, Vec<Key>>,
    pub(crate) last_macro: Option<char>,
    pub(crate) macro_depth: usize,
    pub(crate) pending_paste: Option<PendingPaste>,
    /// Edits applied since the last [`Vim::take_edits`].
    pub(crate) edit_log: Vec<Edit>,
    /// Refuse every change (see [`Vim::set_read_only`]).
    pub(crate) read_only: bool,
}

impl Vim {
    /// A Vim state machine in Normal mode.
    pub fn new() -> Self {
        Vim::default()
    }

    /// The current mode (`OperatorPending` while an operator waits).
    pub fn mode(&self) -> Mode {
        if self.op_pending && !self.pending.is_empty() {
            Mode::OperatorPending
        } else {
            self.mode
        }
    }

    /// Handles one keystroke. Buffer and selections are updated in place;
    /// the returned action says what else the editor should do.
    pub fn handle_key(
        &mut self,
        key: &Key,
        buffer: &mut Buffer,
        selections: &mut SelectionSet,
        ctx: &VimContext,
    ) -> VimAction {
        let mut ed = Ed {
            b: buffer,
            sels: selections,
            ctx,
        };
        self.sync(&mut ed);
        if let Some((_, keys)) = &mut self.macro_rec {
            keys.push(key.clone());
        }
        let before = self.mode();
        let action = self.feed(key, &mut ed);
        if action == VimAction::None && self.mode() != before {
            return VimAction::ModeChanged(self.mode());
        }
        action
    }

    /// Handles keys in Vim notation (see [`Key::parse_keys`]) and returns
    /// every action that is not [`VimAction::None`].
    pub fn handle_keys(
        &mut self,
        notation: &str,
        buffer: &mut Buffer,
        selections: &mut SelectionSet,
        ctx: &VimContext,
    ) -> Vec<VimAction> {
        Key::parse_keys(notation)
            .iter()
            .map(|k| self.handle_key(k, buffer, selections, ctx))
            .filter(|a| *a != VimAction::None)
            .collect()
    }

    /// One key with undo grouping and `.` bookkeeping.
    pub(crate) fn feed(&mut self, key: &Key, ed: &mut Ed) -> VimAction {
        if self.is_idle() {
            ed.b.break_undo_group();
        }
        let action = self.process(key, ed);
        if self.is_idle() {
            ed.b.break_undo_group();
            self.finish_dot(ed);
        }
        action
    }

    /// Normal or Visual mode with no pending keys.
    pub(crate) fn is_idle(&self) -> bool {
        self.pending.is_empty()
            && (self.mode == Mode::Normal || self.mode.is_visual())
            && self.cmdline.is_none()
    }

    /// One key, no grouping (used directly when replaying `.`).
    pub(crate) fn process(&mut self, key: &Key, ed: &mut Ed) -> VimAction {
        if self.read_only && matches!(self.mode, Mode::Insert | Mode::Replace) {
            self.set_read_only(true);
            return self.refuse();
        }
        match self.mode {
            Mode::Insert | Mode::Replace => self.insert_key(key, ed),
            Mode::CommandLine => self.cmdline_key(key, ed),
            _ => self.normal_key(key, ed),
        }
    }

    fn normal_key(&mut self, key: &Key, ed: &mut Ed) -> VimAction {
        if key.tok() == Tok::Esc {
            if !self.pending.is_empty() {
                self.pending.clear();
                self.op_pending = false;
                return VimAction::None;
            }
            if self.mode.is_visual() {
                self.exit_visual(ed);
            }
            return VimAction::None;
        }
        self.pending.push(key.clone());
        let toks: Vec<Tok> = self.pending.iter().map(Key::tok).collect();
        let cx = ParseCtx {
            visual: self.mode.is_visual(),
            recording: self.macro_rec.is_some(),
        };
        match parse(&toks, cx) {
            Parse::Incomplete { op_pending } => {
                self.op_pending = op_pending;
                VimAction::None
            }
            Parse::Invalid => {
                self.pending.clear();
                self.op_pending = false;
                VimAction::None
            }
            Parse::Done(cmd) => {
                self.op_pending = false;
                let keys = std::mem::take(&mut self.pending);
                self.execute(cmd, keys, ed)
            }
        }
    }

    /// Motion environment for `who`.
    pub(crate) fn env<'a>(&'a self, ctx: &'a VimContext, who: Ctx) -> Env<'a> {
        Env {
            ctx,
            last_find: self.last_find,
            search: self.search.as_ref(),
            marks: &self.marks,
            who,
        }
    }

    /// Applies `edits` (any order; overlapping ones are trimmed) as part of
    /// the current undo step. Returns each edit's new `(start, end)` in the
    /// resulting text, in input order.
    pub(crate) fn apply_edits(
        &mut self,
        ed: &mut Ed,
        edits: Vec<(Range<usize>, String)>,
    ) -> Vec<(usize, usize)> {
        let len = ed.b.len_chars();
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by_key(|&i| (edits[i].0.start, edits[i].0.end));
        let mut out = vec![(0, 0); edits.len()];
        let mut fixed: Vec<(Range<usize>, String)> = Vec::with_capacity(edits.len());
        let (mut delta, mut prev_end) = (0isize, 0usize);
        for &i in &order {
            let (r, text) = &edits[i];
            let start = r.start.min(len).max(prev_end);
            let end = r.end.min(len).max(start);
            let new_start = (start as isize + delta) as usize;
            let chars = text.chars().count();
            out[i] = (new_start, new_start + chars);
            delta += chars as isize - (end - start) as isize;
            prev_end = end;
            fixed.push((start..end, text.clone()));
        }
        let recs = ed.b.edit_grouped(fixed, UndoGroup::Merge);
        self.map_marks(&recs);
        self.edit_log.extend(recs);
        if let Some(&(s, _)) = order.first().map(|&i| &out[i]) {
            self.marks.insert('.', s);
        }
        out
    }

    fn map_marks(&mut self, edits: &[Edit]) {
        for e in edits {
            let new_len = e.new_end_char() - e.range.start;
            for p in self.marks.values_mut() {
                if *p >= e.range.end {
                    *p = *p + new_len - e.range.len();
                } else if *p > e.range.start {
                    *p = e.range.start;
                }
            }
        }
    }

    /// Updates marks for edits the editor made itself (completion, LSP
    /// formatting, ...). Vim's own edits are tracked automatically.
    pub fn on_external_edits(&mut self, edits: &[Edit]) {
        self.map_marks(edits);
    }

    /// Sets Normal-mode cursors (clamped) at `positions`.
    pub(crate) fn set_cursors(&self, ed: &mut Ed, positions: Vec<usize>) {
        let primary = ed.sels.primary_index();
        let sels: Vec<Selection> = positions
            .into_iter()
            .map(|p| Selection::cursor(clamp_normal(ed.b, p)))
            .collect();
        if let Some(set) = SelectionSet::from_vec(sels, primary) {
            *ed.sels = set;
        }
    }

    /// Cursor heads of the current selections.
    pub(crate) fn heads(ed: &Ed) -> Vec<usize> {
        ed.sels.iter().map(|s| s.head).collect()
    }

    /// Leaves any mode for Normal mode (like pressing Escape until idle),
    /// e.g. when the editor loses focus.
    pub fn force_normal(
        &mut self,
        buffer: &mut Buffer,
        selections: &mut SelectionSet,
        ctx: &VimContext,
    ) {
        let esc = Key::new("escape");
        for _ in 0..3 {
            if self.is_idle() && self.mode == Mode::Normal {
                break;
            }
            self.handle_key(&esc, buffer, selections, ctx);
        }
    }

    /// Re-reads the selections after the editor changed them (mouse
    /// clicks, drags): a non-empty selection in Normal mode enters Visual
    /// mode, empty ones in Visual mode return to Normal; Normal cursors are
    /// clamped onto a char. Called by [`Vim::handle_key`] as well.
    pub fn sync_selections(
        &mut self,
        buffer: &mut Buffer,
        selections: &mut SelectionSet,
        ctx: &VimContext,
    ) -> Mode {
        let mut ed = Ed {
            b: buffer,
            sels: selections,
            ctx,
        };
        self.sync(&mut ed);
        self.mode()
    }

    fn sync(&mut self, ed: &mut Ed) {
        let len = ed.b.len_chars();
        if ed.sels.iter().any(|s| s.anchor > len || s.head > len) {
            ed.sels.clamp(len);
        }
        match self.mode {
            Mode::Normal if self.pending.is_empty() => {
                if ed.sels.iter().any(|s| !s.is_empty()) {
                    self.visual_from_selections(ed, Mode::Visual);
                } else {
                    let b = &*ed.b;
                    ed.sels.transform(|s| {
                        let p = clamp_normal(b, s.head);
                        Selection {
                            anchor: p,
                            head: p,
                            goal_column: s.goal_column,
                        }
                    });
                }
            }
            m if m.is_visual() && self.visual.rendered.as_ref() != Some(&*ed.sels) => {
                if ed.sels.iter().all(Selection::is_empty) {
                    self.mode = Mode::Normal;
                    self.visual.rendered = None;
                } else {
                    self.visual_from_selections(ed, m);
                }
            }
            _ => {}
        }
    }

    /// Text and kind of register `name` (`"` is the unnamed register).
    pub fn register(&self, name: char) -> Option<&Register> {
        self.registers.get(name)
    }

    /// Sets register `name` (e.g. to mirror the system clipboard into `+`).
    pub fn set_register(&mut self, name: char, register: Register) {
        self.registers.set(name, register);
    }

    /// Keys typed so far of an unfinished command (for a "showcmd" area).
    pub fn pending_keys(&self) -> String {
        self.pending
            .iter()
            .map(|k| match k.tok() {
                Tok::Char(c) => c.to_string(),
                Tok::Ctrl(c) => format!("^{}", c.to_ascii_uppercase()),
                _ => String::new(),
            })
            .collect()
    }

    /// The command line being typed, with its prompt (`":%s/a"`, `"/foo"`).
    pub fn command_line(&self) -> Option<String> {
        self.cmdline
            .as_ref()
            .map(|c| format!("{}{}", c.prompt, c.text))
    }

    /// The last search, if its matches should be highlighted.
    pub fn search_query(&self) -> Option<&SearchQuery> {
        self.search.as_ref().filter(|_| self.highlight)
    }

    /// The register a macro is being recorded into (`q{reg}`).
    pub fn recording_macro(&self) -> Option<char> {
        self.macro_rec.as_ref().map(|(c, _)| *c)
    }

    /// Position of mark `name` (`a`-`z`, `'`, `.`, `^`, `<`, `>`).
    pub fn mark(&self, name: char) -> Option<usize> {
        self.marks.get(&name).copied()
    }

    /// Where to draw block cursors: the char each cursor sits on. In
    /// Visual modes this is the moving end of each selection (which the
    /// selection ranges alone cannot tell for inclusive selections).
    pub fn cursor_positions(&self, selections: &SelectionSet) -> Vec<usize> {
        if self.mode.is_visual() && self.visual.rendered.as_ref() == Some(selections) {
            return self.visual.regions.iter().map(|r| r.head).collect();
        }
        selections.iter().map(|s| s.head).collect()
    }
}
