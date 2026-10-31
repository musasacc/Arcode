//! Visual-mode commands: motions, text objects, operators and the rest.

use crate::motion::{Ctx, Motion, Target as MTarget, eval};
use crate::ops::Region;
use crate::parse::{Act, Cmd, CmdKind, InsertKind};
use crate::text::{char_at_display_col, clamp_normal, display_col, line_of};
use crate::textobj::{self, TextObject};
use crate::types::{Mode, VimAction};
use crate::vim::{Ed, Vim};
use arcode_buffer::movement;

impl Vim {
    /// Runs a command in Visual mode.
    pub(crate) fn exec_visual(&mut self, cmd: Cmd, ed: &mut Ed) -> VimAction {
        match cmd.kind {
            CmdKind::Move(m) => self.visual_move(m, cmd.count, ed),
            CmdKind::Object(o) => {
                self.visual_object(o, cmd.count.unwrap_or(1), ed);
                VimAction::None
            }
            CmdKind::VisualOp(op, linewise) => {
                let mut regions = self.visual_regions(ed);
                if linewise {
                    regions = regions
                        .into_iter()
                        .map(|(r, p)| {
                            let (f, l) = r.lines(ed);
                            (Region::Lines(f, l), p)
                        })
                        .collect();
                }
                self.exit_visual(ed);
                self.apply_op(op, regions, cmd.register, cmd.count.unwrap_or(1), ed)
            }
            CmdKind::Act(a) => self.visual_act(a, cmd.count, cmd.register, ed),
            CmdKind::Operate(..) => VimAction::None,
        }
    }

    pub(crate) fn visual_move(
        &mut self,
        m: Motion,
        count: Option<usize>,
        ed: &mut Ed,
    ) -> VimAction {
        let (m, action) = match self.resolve_motion(m, count, ed) {
            Ok(x) => x,
            Err(a) => return a,
        };
        let targets: Vec<Option<MTarget>> = {
            let env = self.env(ed.ctx, Ctx::Visual);
            self.visual
                .regions
                .iter()
                .map(|r| eval(ed.b, r.head, r.goal, m, count, &env))
                .collect()
        };
        if targets.iter().all(Option::is_none) {
            return self.motion_failed(m);
        }
        let old = ed.sels.primary().head;
        let mut jumped = false;
        let vertical = matches!(
            m,
            Motion::Up
                | Motion::Down
                | Motion::HalfPageDown
                | Motion::HalfPageUp
                | Motion::PageDown
                | Motion::PageUp
        );
        let block = self.mode == Mode::VisualBlock;
        for (r, t) in self.visual.regions.iter_mut().zip(targets) {
            if let Some(t) = t {
                // `$` (and vertical moves after it) select through the
                // line break.
                let eol =
                    m == Motion::LineEnd || (!block && vertical && t.goal == Some(usize::MAX));
                r.head = if eol {
                    let line = line_of(ed.b, t.pos);
                    ed.b.line_end(line)
                } else {
                    clamp_normal(ed.b, t.pos)
                };
                r.goal = t.goal;
                jumped |= t.jump;
            }
        }
        if jumped {
            self.marks.insert('\'', old);
        }
        if block {
            self.visual.eol = m == Motion::LineEnd || (self.visual.eol && vertical);
        }
        self.render_visual(ed);
        action
    }

    fn visual_object(&mut self, obj: TextObject, count: usize, ed: &mut Ed) {
        let mut to_line = false;
        for r in &mut self.visual.regions {
            let Some(o) = textobj::select(ed.b, r.head, obj, count) else {
                continue;
            };
            if o.range.is_empty() {
                continue;
            }
            let end = movement::left(ed.b, o.range.end).max(o.range.start);
            to_line |= o.linewise;
            if r.anchor == r.head {
                r.anchor = o.range.start;
                r.head = end;
            } else {
                r.anchor = r.anchor.min(o.range.start);
                r.head = r.head.max(end);
            }
        }
        if to_line && self.mode == Mode::Visual {
            self.mode = Mode::VisualLine;
        }
        self.render_visual(ed);
    }

    fn visual_act(
        &mut self,
        a: Act,
        count: Option<usize>,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        match a {
            Act::Visual(m) if m == self.mode => {
                self.exit_visual(ed);
                VimAction::None
            }
            Act::Visual(m) => {
                let regions = self
                    .visual
                    .regions
                    .iter()
                    .map(|r| (r.anchor, r.head))
                    .collect();
                self.set_visual(m, regions, ed);
                VimAction::None
            }
            Act::SwapEnds => {
                for r in &mut self.visual.regions {
                    std::mem::swap(&mut r.anchor, &mut r.head);
                    r.goal = None;
                }
                self.render_visual(ed);
                VimAction::None
            }
            Act::Replace(c) => {
                let regions = self.visual_regions(ed);
                self.exit_visual(ed);
                let mut edits = Vec::new();
                for (r, _) in &regions {
                    for range in r.ranges(ed) {
                        let text: String =
                            ed.b.slice_to_string(range.clone())
                                .chars()
                                .map(|x| if x == '\n' { '\n' } else { c })
                                .collect();
                        edits.push((range, text));
                    }
                }
                let starts: Vec<usize> = regions.iter().map(|(r, _)| r.start(ed)).collect();
                self.apply_edits(ed, edits);
                self.set_cursors(ed, starts);
                VimAction::None
            }
            Act::Join(spaces) => {
                let regions = self.visual_regions(ed);
                self.exit_visual(ed);
                let blocks = regions
                    .iter()
                    .map(|(r, _)| {
                        let (f, l) = r.lines(ed);
                        (f, l.max(f + 1))
                    })
                    .collect();
                self.join_range(blocks, spaces, ed)
            }
            Act::Paste { before, .. } => self.paste(before, false, 1, reg, ed),
            Act::BlockInsert(append) => {
                self.block_insert(append, ed);
                VimAction::None
            }
            Act::CmdLine => {
                self.open_cmdline(':', "'<,'>".to_string(), None, ed);
                VimAction::None
            }
            Act::Reselect => {
                self.reselect(ed);
                VimAction::None
            }
            other => self.do_act(other, count, reg, ed),
        }
    }

    /// `I` / `A` in Visual mode. In block mode, inserts on every line of
    /// the block with one cursor per line (`A` pads short lines).
    fn block_insert(&mut self, append: bool, ed: &mut Ed) {
        if self.mode != Mode::VisualBlock {
            let regions = self.visual_regions(ed);
            self.exit_visual(ed);
            let cursors = regions
                .iter()
                .map(|(r, _)| match (r, append) {
                    (Region::Chars(x), true) => x.end,
                    (Region::Lines(_, l), true) => ed.b.line_end(*l),
                    (other, _) => other.start(ed),
                })
                .collect();
            self.start_insert(Some(InsertKind::Before), 1, cursors, false, false, ed);
            return;
        }
        let tab = ed.ctx.tab_size;
        let Some(r) = self.visual.regions.first().copied() else {
            return;
        };
        let eol = self.visual.eol;
        let (ca, chd) = (
            display_col(ed.b, r.anchor, tab),
            display_col(ed.b, r.head, tab),
        );
        let (left, right) = (ca.min(chd), ca.max(chd));
        let (f, l) = (
            line_of(ed.b, r.anchor.min(r.head)),
            line_of(ed.b, r.anchor.max(r.head)),
        );
        self.exit_visual(ed);
        let mut edits = Vec::new();
        for line in f..=l {
            if !append {
                if let Some(p) = char_at_display_col(ed.b, line, left, tab) {
                    edits.push((p..p, String::new()));
                }
                continue;
            }
            let end = ed.b.line_end(line);
            let (at, pad) = if eol {
                (end, 0)
            } else {
                match char_at_display_col(ed.b, line, right, tab) {
                    Some(p) => (movement::right(ed.b, p), 0),
                    None => (end, (right + 1).saturating_sub(display_col(ed.b, end, tab))),
                }
            };
            edits.push((at..at, " ".repeat(pad)));
        }
        let out = self.apply_edits(ed, edits);
        let cursors = out.into_iter().map(|(_, e)| e).collect();
        self.start_insert(Some(InsertKind::Before), 1, cursors, true, false, ed);
    }
}
