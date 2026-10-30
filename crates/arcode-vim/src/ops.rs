//! Applying operators to regions: yank, delete, change, case.

use std::ops::Range;

use crate::parse::Op;
use crate::registers::is_clipboard;
use crate::text::{
    col_of, first_non_blank, indent_of, line_delete_range, line_of, lines_text, lower, map_case,
    pos_at_col, toggle, upper,
};
use crate::types::{Register, RegisterKind, VimAction};
use crate::vim::{Ed, Vim};

/// What an operator acts on, per cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Region {
    /// A char range.
    Chars(Range<usize>),
    /// Lines `first..=last`.
    Lines(usize, usize),
    /// One char range per line of a visual block.
    Block(Vec<Range<usize>>),
}

impl Region {
    /// First char of the region.
    pub(crate) fn start(&self, ed: &Ed) -> usize {
        match self {
            Region::Chars(r) => r.start,
            Region::Lines(f, _) => ed.b.line_start(*f),
            Region::Block(rows) => rows.first().map_or(0, |r| r.start),
        }
    }

    /// Lines `first..=last` the region touches.
    pub(crate) fn lines(&self, ed: &Ed) -> (usize, usize) {
        match self {
            Region::Chars(r) => crate::text::lines_of_range(ed.b, r),
            Region::Lines(f, l) => (*f, *l),
            Region::Block(rows) => {
                let first = rows.first().map_or(0, |r| line_of(ed.b, r.start));
                let last = rows.last().map_or(first, |r| line_of(ed.b, r.start));
                (first, last)
            }
        }
    }

    /// The char ranges holding the region's text (line contents for
    /// `Lines`).
    pub(crate) fn ranges(&self, ed: &Ed) -> Vec<Range<usize>> {
        match self {
            Region::Chars(r) => vec![r.clone()],
            Region::Lines(f, l) => {
                std::iter::once(ed.b.line_start(*f)..ed.b.line_end(*l)).collect()
            }
            Region::Block(rows) => rows.clone(),
        }
    }
}

/// The register content for `regions`.
fn register_for(ed: &Ed, regions: &[(Region, usize)]) -> Register {
    let kind = match regions.first().map(|r| &r.0) {
        Some(Region::Lines(..)) => RegisterKind::Linewise,
        Some(Region::Block(_)) => RegisterKind::Blockwise,
        _ => RegisterKind::Charwise,
    };
    let mut pieces = Vec::new();
    for (r, _) in regions {
        match r {
            Region::Chars(range) => pieces.push(ed.b.slice_to_string(range.clone())),
            Region::Lines(f, l) => pieces.push(lines_text(ed.b, *f, *l)),
            Region::Block(rows) => {
                pieces.extend(rows.iter().map(|r| ed.b.slice_to_string(r.clone())));
            }
        }
    }
    Register { pieces, kind }
}

impl Vim {
    /// Stores `value` in register `name` (yank or delete semantics);
    /// clipboard registers also produce a [`VimAction::Yank`].
    pub(crate) fn store_register(
        &mut self,
        name: Option<char>,
        value: Register,
        yank: bool,
    ) -> VimAction {
        let text = value.text();
        self.registers.store(name, value, yank);
        match name {
            Some(c) if is_clipboard(c) => VimAction::Yank(text),
            _ => VimAction::None,
        }
    }

    /// Applies `op` to `regions` (each with the cursor position it came
    /// from). `levels` is the shift amount for `>`/`<`.
    pub(crate) fn apply_op(
        &mut self,
        op: Op,
        regions: Vec<(Region, usize)>,
        reg: Option<char>,
        levels: usize,
        ed: &mut Ed,
    ) -> VimAction {
        if regions.is_empty() {
            return VimAction::None;
        }
        match op {
            Op::Yank => self.op_yank(regions, reg, ed),
            Op::Delete => self.op_delete(regions, reg, ed),
            Op::Change => self.op_change(regions, reg, ed),
            Op::Indent | Op::Outdent => {
                self.op_shift(&regions, op == Op::Indent, levels.max(1), ed);
                VimAction::None
            }
            Op::Reindent => {
                self.op_reindent(&regions, ed);
                VimAction::None
            }
            Op::Comment => self.op_comment(&regions, ed),
            Op::ToggleCase | Op::Lower | Op::Upper => {
                self.op_case(regions, op, ed);
                VimAction::None
            }
        }
    }

    fn op_yank(
        &mut self,
        regions: Vec<(Region, usize)>,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        let value = register_for(ed, &regions);
        let action = self.store_register(reg, value, true);
        let cursors = regions
            .iter()
            .map(|(r, pos)| match r {
                Region::Lines(f, _) if line_of(ed.b, *pos) != *f => {
                    pos_at_col(ed.b, *f, col_of(ed.b, *pos))
                }
                Region::Lines(..) => *pos,
                other => other.start(ed),
            })
            .collect();
        self.set_cursors(ed, cursors);
        action
    }

    /// Deletes the regions' text; returns each region's new start.
    fn delete_regions(&mut self, regions: &[(Region, usize)], ed: &mut Ed) -> Vec<usize> {
        let mut edits = Vec::new();
        let mut owner = Vec::new();
        for (i, (r, _)) in regions.iter().enumerate() {
            let ranges = match r {
                Region::Lines(f, l) => vec![line_delete_range(ed.b, *f, *l)],
                other => other.ranges(ed),
            };
            for range in ranges {
                edits.push((range, String::new()));
                owner.push(i);
            }
        }
        let out = self.apply_edits(ed, edits);
        let mut starts = vec![usize::MAX; regions.len()];
        for (k, &i) in owner.iter().enumerate() {
            starts[i] = starts[i].min(out[k].0);
        }
        starts
    }

    fn op_delete(
        &mut self,
        regions: Vec<(Region, usize)>,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        let value = register_for(ed, &regions);
        let action = self.store_register(reg, value, false);
        let starts = self.delete_regions(&regions, ed);
        let cursors = regions
            .iter()
            .zip(starts)
            .map(|((r, _), s)| match r {
                Region::Lines(..) => {
                    let s = ed.b.clamp_char(s);
                    first_non_blank(ed.b, line_of(ed.b, s))
                }
                _ => s,
            })
            .collect();
        self.set_cursors(ed, cursors);
        action
    }

    fn op_change(
        &mut self,
        regions: Vec<(Region, usize)>,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        let value = register_for(ed, &regions);
        let action = self.store_register(reg, value, false);
        let block = regions.iter().any(|(r, _)| matches!(r, Region::Block(_)));
        let mut edits = Vec::new();
        for (r, _) in &regions {
            match r {
                Region::Lines(f, l) => {
                    edits.push((ed.b.line_start(*f)..ed.b.line_end(*l), indent_of(ed.b, *f)))
                }
                other => edits.extend(other.ranges(ed).into_iter().map(|r| (r, String::new()))),
            }
        }
        let out = self.apply_edits(ed, edits);
        let cursors = out.iter().map(|&(_, end)| end).collect();
        self.start_insert(None, 1, cursors, block, false, ed);
        action
    }

    fn op_case(&mut self, regions: Vec<(Region, usize)>, op: Op, ed: &mut Ed) {
        let f: fn(char) -> String = match op {
            Op::Lower => lower,
            Op::Upper => upper,
            _ => toggle,
        };
        let mut edits = Vec::new();
        for (r, _) in &regions {
            for range in r.ranges(ed) {
                let text = ed.b.slice_to_string(range.clone());
                edits.push((range, map_case(&text, f)));
            }
        }
        let cursors: Vec<usize> = regions
            .iter()
            .map(|(r, pos)| match r {
                Region::Lines(..) => *pos,
                other => other.start(ed),
            })
            .collect();
        self.apply_edits(ed, edits);
        self.set_cursors(ed, cursors);
    }

    /// Lines touched by `regions`, sorted and deduplicated.
    pub(crate) fn region_lines(regions: &[(Region, usize)], ed: &Ed) -> Vec<usize> {
        let mut lines: Vec<usize> = regions
            .iter()
            .flat_map(|(r, _)| {
                let (f, l) = r.lines(ed);
                f..=l
            })
            .collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    /// Cursor on the first non-blank of the first region's first line.
    pub(crate) fn cursor_to_first_line(&self, regions: &[(Region, usize)], ed: &mut Ed) {
        if let Some((r, _)) = regions.first() {
            let p = first_non_blank(ed.b, r.lines(ed).0);
            self.set_cursors(ed, vec![p]);
        }
    }
}
