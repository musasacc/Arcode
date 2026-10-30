//! Visual modes: state, rendering to selections, and the region an
//! operator acts on.
//!
//! A Visual selection is inclusive in Vim. It is rendered to the editor's
//! [`SelectionSet`] as half-open ranges: charwise `[min, max + 1)`,
//! linewise from the first line's start to the start of the line after the
//! last, and blockwise as one selection per line (multiple cursors).

mod exec;

use std::ops::Range;

use arcode_buffer::{Selection, SelectionSet, movement};

use crate::ops::Region;
use crate::repeat::VisualExtent;
use crate::text::{ch, char_at_display_col, clamp_normal, col_of, display_col, last_line, line_of};
use crate::types::Mode;
use crate::vim::{Ed, Vim};

/// One Visual selection: inclusive `anchor` and `head` chars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VRegion {
    pub anchor: usize,
    pub head: usize,
    pub goal: Option<usize>,
}

/// Visual mode state.
#[derive(Debug, Clone, Default)]
pub(crate) struct VisualState {
    pub regions: Vec<VRegion>,
    /// Block extends to every line's end (`$`).
    pub eol: bool,
    /// The selection set last written, to notice outside changes.
    pub rendered: Option<SelectionSet>,
}

/// The last Visual selection, for `gv` and the `'<` `'>` marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LastVisual {
    pub mode: Mode,
    pub anchor: usize,
    pub head: usize,
    pub eol: bool,
}

impl Vim {
    /// Enters (or switches to) Visual `mode` with `(anchor, head)` regions.
    pub(crate) fn set_visual(&mut self, mode: Mode, regions: Vec<(usize, usize)>, ed: &mut Ed) {
        let mut regions: Vec<VRegion> = regions
            .into_iter()
            .map(|(anchor, head)| VRegion {
                anchor: ed.b.clamp_char(anchor),
                head: ed.b.clamp_char(head),
                goal: None,
            })
            .collect();
        if mode == Mode::VisualBlock {
            let primary = ed.sels.primary_index().min(regions.len().saturating_sub(1));
            regions = regions.into_iter().skip(primary).take(1).collect();
        }
        if mode != Mode::VisualBlock {
            self.visual.eol = false;
        }
        self.visual.regions = regions;
        self.mode = mode;
        self.render_visual(ed);
    }

    /// Writes the Visual regions to the selection set.
    pub(crate) fn render_visual(&mut self, ed: &mut Ed) {
        let b = &*ed.b;
        let len = b.len_chars();
        let next = |p: usize| if p >= len { len } else { movement::right(b, p) };
        let mut primary = 0;
        let sels: Vec<Selection> = match self.mode {
            Mode::VisualLine => self
                .visual
                .regions
                .iter()
                .map(|r| {
                    let (f, l) = (
                        line_of(b, r.anchor.min(r.head)),
                        line_of(b, r.anchor.max(r.head)),
                    );
                    let start = b.line_start(f);
                    let end = if l < last_line(b) {
                        b.line_start(l + 1)
                    } else {
                        len
                    };
                    if r.head < r.anchor {
                        Selection::new(end, start)
                    } else {
                        Selection::new(start, end)
                    }
                })
                .collect(),
            Mode::VisualBlock => {
                let rows = self.block_rows(ed);
                let head_line = self
                    .visual
                    .regions
                    .first()
                    .map_or(0, |r| line_of(b, r.head));
                primary = rows
                    .iter()
                    .position(|r| line_of(b, r.start) == head_line)
                    .unwrap_or(0);
                let fallback = self.visual.regions.first().map_or(0, |r| r.head);
                if rows.is_empty() {
                    vec![Selection::cursor(fallback)]
                } else {
                    rows.into_iter()
                        .map(|r| Selection::new(r.start, r.end))
                        .collect()
                }
            }
            _ => self
                .visual
                .regions
                .iter()
                .map(|r| {
                    if r.head >= r.anchor {
                        Selection::new(r.anchor, next(r.head))
                    } else {
                        Selection::new(next(r.anchor), r.head)
                    }
                })
                .collect(),
        };
        if let Some(set) = SelectionSet::from_vec(sels, primary) {
            *ed.sels = set;
        }
        self.visual.rendered = Some(ed.sels.clone());
    }

    /// The char range of each line of the (primary) block. Lines too short
    /// to reach the block are skipped.
    pub(crate) fn block_rows(&self, ed: &Ed) -> Vec<Range<usize>> {
        let Some(r) = self.visual.regions.first() else {
            return Vec::new();
        };
        let b = &*ed.b;
        let tab = ed.ctx.tab_size;
        let (ca, ch_) = (display_col(b, r.anchor, tab), display_col(b, r.head, tab));
        let (left, right) = (ca.min(ch_), ca.max(ch_));
        let (f, l) = (
            line_of(b, r.anchor.min(r.head)),
            line_of(b, r.anchor.max(r.head)),
        );
        (f..=l)
            .filter_map(|line| {
                let start = char_at_display_col(b, line, left, tab)?;
                let end = if self.visual.eol {
                    b.line_end(line)
                } else {
                    char_at_display_col(b, line, right, tab)
                        .map_or(b.line_end(line), |p| movement::right(b, p))
                };
                Some(start..end)
            })
            .collect()
    }

    /// Leaves Visual mode: remembers it for `gv`, sets `'<` `'>`, puts the
    /// cursors on the heads.
    pub(crate) fn exit_visual(&mut self, ed: &mut Ed) {
        if let Some(r) = self.visual.regions.first().copied() {
            self.last_visual = Some(LastVisual {
                mode: self.mode,
                anchor: r.anchor,
                head: r.head,
                eol: self.visual.eol,
            });
            self.marks.insert('<', r.anchor.min(r.head));
            self.marks.insert('>', r.anchor.max(r.head));
        }
        let heads: Vec<usize> = self.visual.regions.iter().map(|r| r.head).collect();
        self.mode = Mode::Normal;
        self.visual.rendered = None;
        self.visual.eol = false;
        if !heads.is_empty() {
            self.set_cursors(ed, heads);
        }
    }

    /// Re-derives Visual regions from selections the editor changed.
    pub(crate) fn visual_from_selections(&mut self, ed: &mut Ed, mode: Mode) {
        let b = &*ed.b;
        let regions = ed
            .sels
            .iter()
            .map(|s| {
                if s.head > s.anchor {
                    (s.anchor, movement::left(b, s.head))
                } else if s.head < s.anchor {
                    (movement::left(b, s.anchor), s.head)
                } else {
                    (s.head, s.head)
                }
            })
            .collect();
        let mode = if mode == Mode::VisualBlock {
            Mode::Visual
        } else {
            mode
        };
        self.set_visual(mode, regions, ed);
    }

    /// Size of the primary region (for `.` after a Visual change).
    pub(crate) fn visual_extent(&self, ed: &Ed) -> VisualExtent {
        let r = self.visual.regions.first().copied().unwrap_or(VRegion {
            anchor: 0,
            head: 0,
            goal: None,
        });
        let (s, e) = (r.anchor.min(r.head), r.anchor.max(r.head));
        let lines = line_of(ed.b, e) - line_of(ed.b, s);
        let cols = match self.mode {
            Mode::Visual if lines == 0 => e - s + 1,
            Mode::Visual => col_of(ed.b, e),
            Mode::VisualBlock => col_of(ed.b, r.head).abs_diff(col_of(ed.b, r.anchor)),
            _ => 0,
        };
        VisualExtent {
            mode: self.mode,
            lines,
            cols,
        }
    }

    /// The regions an operator acts on, with the selection start.
    pub(crate) fn visual_regions(&self, ed: &Ed) -> Vec<(Region, usize)> {
        let b = &*ed.b;
        let len = b.len_chars();
        match self.mode {
            Mode::VisualBlock => {
                let rows = self.block_rows(ed);
                let start = rows.first().map_or(0, |r| r.start);
                vec![(Region::Block(rows), start)]
            }
            Mode::VisualLine => self
                .visual
                .regions
                .iter()
                .map(|r| {
                    let s = r.anchor.min(r.head);
                    (
                        Region::Lines(line_of(b, s), line_of(b, r.anchor.max(r.head))),
                        s,
                    )
                })
                .collect(),
            _ => self
                .visual
                .regions
                .iter()
                .map(|r| {
                    let (s, e) = (r.anchor.min(r.head), r.anchor.max(r.head));
                    let end = if e >= len || ch(b, e) == '\n' {
                        (e + 1).min(len)
                    } else {
                        movement::right(b, e)
                    };
                    (Region::Chars(s..end), s)
                })
                .collect(),
        }
    }

    /// `gv`: restores the last Visual selection.
    pub(crate) fn reselect(&mut self, ed: &mut Ed) {
        let Some(lv) = self.last_visual else { return };
        if self.mode.is_visual() {
            self.exit_visual(ed);
        }
        self.visual.eol = lv.eol;
        let anchor = clamp_normal(ed.b, lv.anchor);
        let head = clamp_normal(ed.b, lv.head);
        self.set_visual(lv.mode, vec![(anchor, head)], ed);
        self.visual.eol = lv.eol;
        self.render_visual(ed);
    }
}
