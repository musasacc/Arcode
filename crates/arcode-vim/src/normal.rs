//! Normal-mode execution: motions and operator targets.

use arcode_buffer::{Buffer, Selection, SelectionSet};

use crate::key::Key;
use crate::motion::word::next_word_end;
use crate::motion::{Ctx, Kind, Motion, Target as MTarget, eval, scroll_amount};
use crate::ops::Region;
use crate::parse::{Act, Cmd, CmdKind, Op, Target};
use crate::search::word_under;
use crate::text::{Class, ch, clamp_normal, class, col_of, last_line, line_of, lines_of_range};
use crate::textobj;
use crate::types::{SearchQuery, VimAction, VimCommand};
use crate::vim::{Ed, Vim};
use arcode_buffer::movement;

/// True if the command changes the buffer (recorded for `.`).
fn is_change(kind: &CmdKind) -> bool {
    match kind {
        CmdKind::Operate(op, _) | CmdKind::VisualOp(op, _) => op.is_change(),
        CmdKind::Act(a) => matches!(
            a,
            Act::Insert(_)
                | Act::Replace(_)
                | Act::ToggleCaseChar
                | Act::Join(_)
                | Act::Paste { .. }
                | Act::ReplaceMode
                | Act::Increment(_)
                | Act::BlockInsert(_)
        ),
        _ => false,
    }
}

impl Vim {
    /// Runs a parsed command.
    pub(crate) fn execute(&mut self, cmd: Cmd, keys: Vec<Key>, ed: &mut Ed) -> VimAction {
        if !self.replaying && is_change(&cmd.kind) {
            let visual = self.mode.is_visual().then(|| self.visual_extent(ed));
            self.start_dot(&keys, &cmd.count_idx, cmd.count, visual, ed.b.version());
        }
        if self.mode.is_visual() {
            return self.exec_visual(cmd, ed);
        }
        match cmd.kind {
            CmdKind::Move(m) => self.do_move(m, cmd.count, ed),
            CmdKind::Operate(op, t) => self.run_operate(op, t, cmd.count, cmd.register, ed),
            CmdKind::Act(a) => self.do_act(a, cmd.count, cmd.register, ed),
            CmdKind::VisualOp(..) | CmdKind::Object(_) => VimAction::None,
        }
    }

    /// Records state a motion depends on (`f`, `*`, `n`) and returns the
    /// motion to evaluate plus the action to report, or an error message.
    pub(crate) fn resolve_motion(
        &mut self,
        m: Motion,
        count: Option<usize>,
        ed: &Ed,
    ) -> Result<(Motion, VimAction), VimAction> {
        match m {
            Motion::Find(spec) => self.last_find = Some(spec),
            Motion::SearchWord { forward, whole } => {
                let pos = ed.sels.primary().head;
                let Some(r) = word_under(ed.b, pos) else {
                    return Err(status("E348: No string under cursor"));
                };
                let pattern = ed.b.slice_to_string(r);
                self.search = Some(SearchQuery {
                    pattern: pattern.clone(),
                    whole_word: whole,
                    case_sensitive: true,
                    forward,
                });
                self.highlight = true;
                return Ok((
                    Motion::SearchNext(false),
                    VimAction::Command(VimCommand::Search(pattern)),
                ));
            }
            Motion::SearchNext(_) => {
                if self.search.is_none() {
                    return Err(status("E35: No previous regular expression"));
                }
                self.highlight = true;
            }
            Motion::HalfPageDown | Motion::PageDown => {
                let n = scroll_amount(m, count, ed.ctx) as isize;
                return Ok((m, VimAction::ScrollLines(n)));
            }
            Motion::HalfPageUp | Motion::PageUp => {
                let n = scroll_amount(m, count, ed.ctx) as isize;
                return Ok((m, VimAction::ScrollLines(-n)));
            }
            _ => {}
        }
        Ok((m, VimAction::None))
    }

    /// The message for a failed motion (search misses), else `None`.
    pub(crate) fn motion_failed(&self, m: Motion) -> VimAction {
        match (m, &self.search) {
            (Motion::SearchNext(_), Some(q)) => {
                status(&format!("E486: Pattern not found: {}", q.pattern))
            }
            _ => VimAction::None,
        }
    }

    pub(crate) fn do_move(&mut self, m: Motion, count: Option<usize>, ed: &mut Ed) -> VimAction {
        let (m, action) = match self.resolve_motion(m, count, ed) {
            Ok(x) => x,
            Err(a) => return a,
        };
        let targets: Vec<Option<MTarget>> = {
            let env = self.env(ed.ctx, Ctx::Normal);
            ed.sels
                .iter()
                .map(|s| eval(ed.b, s.head, s.goal_column, m, count, &env))
                .collect()
        };
        if targets.iter().all(Option::is_none) {
            return self.motion_failed(m);
        }
        let old = ed.sels.primary().head;
        let mut jumped = false;
        let sels: Vec<Selection> = ed
            .sels
            .iter()
            .zip(&targets)
            .map(|(s, t)| match t {
                Some(t) => {
                    jumped |= t.jump;
                    let p = clamp_normal(ed.b, t.pos);
                    Selection {
                        anchor: p,
                        head: p,
                        goal_column: t.goal,
                    }
                }
                None => *s,
            })
            .collect();
        if let Some(set) = SelectionSet::from_vec(sels, ed.sels.primary_index()) {
            *ed.sels = set;
        }
        if jumped {
            self.marks.insert('\'', old);
        }
        action
    }

    pub(crate) fn run_operate(
        &mut self,
        op: Op,
        target: Target,
        count: Option<usize>,
        reg: Option<char>,
        ed: &mut Ed,
    ) -> VimAction {
        if let Target::Search(forward) = target {
            let prompt = if forward { '/' } else { '?' };
            self.open_cmdline(prompt, String::new(), Some((op, count, reg)), ed);
            return VimAction::None;
        }
        let mut action = VimAction::None;
        let target = match target {
            Target::Motion(m) => match self.resolve_motion(m, count, ed) {
                Ok((m, a)) => {
                    if matches!(a, VimAction::Command(_)) {
                        action = a;
                    }
                    Target::Motion(m)
                }
                Err(a) => {
                    self.dot = None;
                    return a;
                }
            },
            t => t,
        };
        let Some(regions) = self.op_regions(op, target, count, ed) else {
            self.dot = None;
            return match target {
                Target::Motion(m) => self.motion_failed(m),
                _ => VimAction::None,
            };
        };
        let a = self.apply_op(op, regions, reg, 1, ed);
        if a == VimAction::None { action } else { a }
    }

    /// A motion in Normal or Visual mode (for search prompts).
    pub(crate) fn run_motion(&mut self, m: Motion, count: Option<usize>, ed: &mut Ed) -> VimAction {
        if self.mode.is_visual() {
            self.visual_move(m, count, ed)
        } else {
            self.do_move(m, count, ed)
        }
    }

    /// Per-cursor regions for `op` over `target`; `None` if any fails.
    pub(crate) fn op_regions(
        &self,
        op: Op,
        target: Target,
        count: Option<usize>,
        ed: &Ed,
    ) -> Option<Vec<(Region, usize)>> {
        let b: &Buffer = ed.b;
        let env = self.env(ed.ctx, Ctx::Op);
        let n = count.unwrap_or(1).max(1);
        let mut out = Vec::new();
        for s in ed.sels.iter() {
            let pos = s.head;
            let region = match target {
                Target::Line => {
                    let l = line_of(b, pos);
                    Region::Lines(l, (l + n - 1).min(last_line(b)))
                }
                Target::Object(o) => {
                    let r = textobj::select(b, pos, o, n)?;
                    if r.linewise && !r.range.is_empty() {
                        let (f, l) = lines_of_range(b, &r.range);
                        Region::Lines(f, l)
                    } else {
                        Region::Chars(r.range)
                    }
                }
                Target::Motion(Motion::WordStart(big))
                    if op == Op::Change && !ch(b, pos).is_whitespace() =>
                {
                    let end = change_word_end(b, pos, big, n);
                    Region::Chars(pos..(end + 1).min(b.len_chars()))
                }
                Target::Motion(m) => {
                    let t = eval(b, pos, s.goal_column, m, count, &env)?;
                    region_for(b, pos, t)
                }
                Target::Search(_) => return None,
            };
            out.push((region, pos));
        }
        Some(out)
    }
}

/// `cw` on a word: the end of the `n`-th word, not the next word start.
fn change_word_end(b: &Buffer, pos: usize, big: bool, n: usize) -> usize {
    let mut p = pos;
    for i in 0..n {
        let at_end = ch(b, p + 1) == '\n' || class(ch(b, p + 1), big) != class(ch(b, p), big);
        let on_space = class(ch(b, p), big) == Class::Space;
        if !(i == 0 && at_end && !on_space) {
            p = next_word_end(b, p, big);
        }
    }
    p
}

/// The region an operator covers from `pos` to the motion target.
pub(crate) fn region_for(b: &Buffer, pos: usize, t: MTarget) -> Region {
    let (s, e) = (pos.min(t.pos), pos.max(t.pos));
    match t.kind {
        Kind::Linewise => Region::Lines(line_of(b, s), line_of(b, e)),
        Kind::Inclusive => {
            let end = if ch(b, e) == '\n' {
                e
            } else {
                movement::right(b, e)
            };
            Region::Chars(s..end.min(b.len_chars()))
        }
        Kind::Exclusive => {
            let (ls, le) = (line_of(b, s), line_of(b, e));
            if e > s && le > ls && col_of(b, e) == 0 {
                if s <= movement::first_non_whitespace(b, ls) {
                    return Region::Lines(ls, le - 1);
                }
                return Region::Chars(s..b.line_end(le - 1));
            }
            Region::Chars(s..e)
        }
    }
}

/// A status-bar message action.
pub(crate) fn status(msg: &str) -> VimAction {
    VimAction::StatusMessage(msg.to_string())
}
