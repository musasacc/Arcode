//! `.` (repeat last change) and macros (`q{reg}`, `@{reg}`, `@@`).
//!
//! A change is recorded as the keys that produced it (minus count digits,
//! plus everything typed in Insert mode up to `<Esc>`), and replayed key by
//! key as one undo step. A change made in Visual mode also remembers the
//! size of the selection and re-applies it at the cursor.

use crate::key::Key;
use crate::text::{col_of, last_line, line_of, pos_at_col};
use crate::types::{Mode, VimAction};
use crate::vim::{Ed, Vim};

/// Size of the Visual selection a change applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisualExtent {
    pub mode: Mode,
    /// Lines below the start line.
    pub lines: usize,
    /// Chars (one-line charwise), end column (multi-line), or extra
    /// columns (block).
    pub cols: usize,
}

/// A recorded change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DotRec {
    pub keys: Vec<Key>,
    pub count: Option<usize>,
    pub visual: Option<VisualExtent>,
    /// Buffer version when recording started (unchanged = nothing to keep).
    pub version: u64,
}

/// Deepest macro nesting (`@a` calling `@b` ...).
const MAX_MACRO_DEPTH: usize = 16;

impl Vim {
    /// Starts recording a change from the keys of its command.
    pub(crate) fn start_dot(
        &mut self,
        keys: &[Key],
        count_idx: &[usize],
        count: Option<usize>,
        visual: Option<VisualExtent>,
        version: u64,
    ) {
        let keys = keys
            .iter()
            .enumerate()
            .filter(|(i, _)| !count_idx.contains(i))
            .map(|(_, k)| k.clone())
            .collect();
        self.dot = Some(DotRec {
            keys,
            count,
            visual,
            version,
        });
    }

    /// Appends a key typed while a change is being recorded.
    pub(crate) fn record_key(&mut self, key: &Key) {
        if self.replaying {
            return;
        }
        if let Some(d) = &mut self.dot {
            d.keys.push(key.clone());
        }
    }

    /// Ends the recording once the change is complete.
    pub(crate) fn finish_dot(&mut self, ed: &Ed) {
        if let Some(d) = self.dot.take()
            && ed.b.version() != d.version
        {
            self.last_change = Some(d);
        }
    }

    /// `.`: replays the last change (`count` replaces its count).
    pub(crate) fn repeat(&mut self, count: Option<usize>, ed: &mut Ed) -> VimAction {
        if self.replaying {
            return VimAction::None;
        }
        let Some(change) = self.last_change.clone() else {
            return VimAction::None;
        };
        let mut keys: Vec<Key> = Vec::new();
        if let Some(ext) = change.visual {
            self.select_extent(ext, ed);
        } else if let Some(n) = count.or(change.count) {
            keys.extend(n.to_string().chars().map(Key::char));
        }
        keys.extend(change.keys.iter().cloned());
        self.replaying = true;
        let mut action = VimAction::None;
        for k in &keys {
            let a = self.process(k, ed);
            if a != VimAction::None {
                action = a;
            }
        }
        if self.mode != Mode::Normal {
            self.process(&Key::new("escape"), ed);
            self.process(&Key::new("escape"), ed);
        }
        self.replaying = false;
        action
    }

    /// Selects a Visual area of size `ext` starting at the cursor.
    fn select_extent(&mut self, ext: VisualExtent, ed: &mut Ed) {
        let pos = ed.sels.primary().head;
        let line = line_of(ed.b, pos);
        let end_line = (line + ext.lines).min(last_line(ed.b));
        let head = match ext.mode {
            Mode::Visual if ext.lines == 0 => {
                pos_at_col(ed.b, line, col_of(ed.b, pos) + ext.cols.saturating_sub(1))
            }
            Mode::Visual => pos_at_col(ed.b, end_line, ext.cols),
            Mode::VisualBlock => pos_at_col(ed.b, end_line, col_of(ed.b, pos) + ext.cols),
            _ => pos_at_col(ed.b, end_line, col_of(ed.b, pos)),
        };
        self.set_visual(ext.mode, vec![(pos, head)], ed);
    }

    /// `q{reg}` started recording into `reg`.
    pub(crate) fn macro_start(&mut self, reg: char) {
        self.macro_rec = Some((reg, Vec::new()));
    }

    /// `q` stopped recording (the `q` itself is dropped).
    pub(crate) fn macro_stop(&mut self) {
        if let Some((reg, mut keys)) = self.macro_rec.take() {
            keys.pop();
            self.macros.insert(reg, keys);
        }
    }

    /// `@{reg}` (`@@` = last played) `count` times.
    pub(crate) fn macro_play(&mut self, reg: char, count: usize, ed: &mut Ed) -> VimAction {
        let reg = if reg == '@' {
            match self.last_macro {
                Some(r) => r,
                None => return VimAction::None,
            }
        } else {
            reg.to_ascii_lowercase()
        };
        let Some(keys) = self.macros.get(&reg).cloned() else {
            return VimAction::None;
        };
        if self.macro_depth >= MAX_MACRO_DEPTH {
            return VimAction::None;
        }
        self.last_macro = Some(reg);
        self.macro_depth += 1;
        let mut action = VimAction::None;
        for _ in 0..count.max(1) {
            for k in &keys {
                let a = self.feed(k, ed);
                if a != VimAction::None {
                    action = a;
                }
            }
        }
        self.macro_depth -= 1;
        action
    }
}
