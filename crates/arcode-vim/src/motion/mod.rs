//! Motions: what they are and where they go.

pub(crate) mod find;
pub(crate) mod word;

use std::collections::HashMap;

use arcode_buffer::{Buffer, movement};

use crate::search::find_match;
use crate::text::{
    clamp_normal, display_col, first_non_blank, last_char, last_line, line_of, pos_at_col,
};
use crate::types::{SearchQuery, VimContext};
pub(crate) use find::FindSpec;

/// A cursor motion (counts are passed separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordStart(bool),
    WordEnd(bool),
    WordBack(bool),
    WordEndBack(bool),
    LineStart,
    FirstNonBlank,
    LineEnd,
    LastNonBlank,
    Column,
    NextLine,
    PrevLine,
    CurrentLine,
    FileStart,
    FileEnd,
    Find(FindSpec),
    RepeatFind(bool),
    MatchPair,
    ParagraphForward,
    ParagraphBackward,
    ScreenTop,
    ScreenMiddle,
    ScreenBottom,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    /// `*` `#` `g*` `g#` (resolved to a search by the executor).
    SearchWord {
        forward: bool,
        whole: bool,
    },
    /// `n` (false) / `N` (true).
    SearchNext(bool),
    Mark {
        name: char,
        linewise: bool,
    },
}

/// How an operator treats a motion's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

/// Where a motion lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    pub pos: usize,
    pub kind: Kind,
    /// Goal display column to remember (vertical motions, `$`).
    pub goal: Option<usize>,
    /// Sets the `'` mark (a "jump").
    pub jump: bool,
}

/// Who asks: bounds differ for Normal, Visual and operator-pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ctx {
    Normal,
    Visual,
    Op,
}

/// State a motion may read.
pub(crate) struct Env<'a> {
    pub ctx: &'a VimContext,
    pub last_find: Option<FindSpec>,
    pub search: Option<&'a SearchQuery>,
    pub marks: &'a HashMap<char, usize>,
    pub who: Ctx,
}

fn t(pos: usize, kind: Kind) -> Option<Target> {
    Some(Target {
        pos,
        kind,
        goal: None,
        jump: false,
    })
}

fn jump(pos: usize, kind: Kind) -> Option<Target> {
    Some(Target {
        pos,
        kind,
        goal: None,
        jump: true,
    })
}

/// Evaluates `m` from `pos`. `None` means the motion failed (beep).
pub(crate) fn eval(
    b: &Buffer,
    pos: usize,
    goal: Option<usize>,
    m: Motion,
    count: Option<usize>,
    env: &Env,
) -> Option<Target> {
    use Kind::*;
    let n = count.unwrap_or(1).max(1);
    let line = line_of(b, pos);
    let (ls, le) = (b.line_start(line), b.line_end(line));
    match m {
        Motion::Left => {
            if pos <= ls {
                return None;
            }
            let mut p = pos;
            for _ in 0..n {
                p = movement::left(b, p).max(ls);
            }
            t(p, Exclusive)
        }
        Motion::Right => {
            let bound = if env.who == Ctx::Op {
                le
            } else {
                last_char(b, line)
            };
            if pos >= bound {
                return None;
            }
            let mut p = pos;
            for _ in 0..n {
                p = movement::right(b, p).min(bound);
            }
            t(p, Exclusive)
        }
        Motion::Up => vertical(b, pos, goal, -(n as isize), env),
        Motion::Down => vertical(b, pos, goal, n as isize, env),
        Motion::WordStart(big) => {
            let mut p = pos;
            for i in 0..n {
                p = word::next_word_start(b, p, big, env.who == Ctx::Op && i + 1 == n);
            }
            t(p, Exclusive)
        }
        Motion::WordEnd(big) => {
            let mut p = pos;
            for _ in 0..n {
                p = word::next_word_end(b, p, big);
            }
            t(p, Inclusive)
        }
        Motion::WordBack(big) => {
            let mut p = pos;
            for _ in 0..n {
                p = word::prev_word_start(b, p, big);
            }
            t(p, Exclusive)
        }
        Motion::WordEndBack(big) => {
            let mut p = pos;
            for _ in 0..n {
                p = word::prev_word_end(b, p, big);
            }
            t(p, Inclusive)
        }
        Motion::LineStart => t(ls, Exclusive),
        Motion::FirstNonBlank => t(first_non_blank(b, line), Exclusive),
        Motion::LineEnd => {
            let target = (line + n - 1).min(last_line(b));
            let p = if env.who == Ctx::Visual {
                b.line_end(target)
            } else {
                last_char(b, target)
            };
            Some(Target {
                pos: p,
                kind: Inclusive,
                goal: Some(usize::MAX),
                jump: false,
            })
        }
        Motion::LastNonBlank => {
            let target = (line + n - 1).min(last_line(b));
            let text = b.line_text(target);
            let trimmed = text.trim_end_matches(crate::text::is_blank).chars().count();
            t(b.line_start(target) + trimmed.saturating_sub(1), Inclusive)
        }
        Motion::Column => t(
            pos_at_col(b, line, n - 1).min(last_char(b, line)),
            Exclusive,
        ),
        Motion::NextLine => {
            if line == last_line(b) {
                return None;
            }
            t(first_non_blank(b, line + n), Linewise)
        }
        Motion::PrevLine => {
            if line == 0 {
                return None;
            }
            t(first_non_blank(b, line.saturating_sub(n)), Linewise)
        }
        Motion::CurrentLine => t(first_non_blank(b, line + n - 1), Linewise),
        Motion::FileStart => jump(first_non_blank(b, count.unwrap_or(1) - 1), Linewise),
        Motion::FileEnd => {
            let target = count.map_or(last_line(b), |c| c.saturating_sub(1));
            jump(first_non_blank(b, target), Linewise)
        }
        Motion::Find(spec) => find::find_char(b, pos, spec, n, false).and_then(|p| find_t(p, spec)),
        Motion::RepeatFind(reverse) => {
            let spec = env.last_find?;
            let spec = if reverse { spec.reversed() } else { spec };
            find::find_char(b, pos, spec, n, true).and_then(|p| find_t(p, spec))
        }
        Motion::MatchPair => match count {
            Some(c) if c <= 100 => {
                let lines = b.line_count();
                let target = (c * lines).div_ceil(100).saturating_sub(1);
                jump(first_non_blank(b, target), Linewise)
            }
            Some(_) => None,
            None => find::match_pair(b, pos).and_then(|p| jump(p, Inclusive)),
        },
        Motion::ParagraphForward => jump(find::paragraph_forward(b, pos, n), Exclusive),
        Motion::ParagraphBackward => jump(find::paragraph_backward(b, pos, n), Exclusive),
        Motion::ScreenTop | Motion::ScreenMiddle | Motion::ScreenBottom => {
            let top = env.ctx.viewport_top.min(last_line(b));
            let bottom = (top + env.ctx.viewport_lines.max(1) - 1).min(last_line(b));
            let target = match m {
                Motion::ScreenTop => (top + n - 1).min(bottom),
                Motion::ScreenBottom => bottom.saturating_sub(n - 1).max(top),
                _ => top + (bottom - top) / 2,
            };
            jump(first_non_blank(b, target), Linewise)
        }
        Motion::HalfPageDown | Motion::HalfPageUp | Motion::PageDown | Motion::PageUp => {
            let lines = scroll_amount(m, count, env.ctx) as isize;
            let down = matches!(m, Motion::HalfPageDown | Motion::PageDown);
            vertical(b, pos, goal, if down { lines } else { -lines }, env)
        }
        Motion::SearchWord { .. } => None,
        Motion::SearchNext(reverse) => {
            let q = env.search?;
            let forward = q.forward != reverse;
            let mut p = pos;
            for _ in 0..n {
                p = find_match(b, q, p, forward)?.start;
            }
            jump(p, Exclusive)
        }
        Motion::Mark { name, linewise } => {
            let name = if name == '`' { '\'' } else { name };
            let p = b.clamp_char(*env.marks.get(&name)?);
            if linewise {
                jump(first_non_blank(b, line_of(b, p)), Linewise)
            } else {
                jump(p, Exclusive)
            }
        }
    }
}

/// Lines moved by Ctrl-D/U (half a page or the count) and Ctrl-F/B.
pub(crate) fn scroll_amount(m: Motion, count: Option<usize>, ctx: &VimContext) -> usize {
    let page = ctx.viewport_lines.max(1);
    match m {
        Motion::HalfPageDown | Motion::HalfPageUp => count.unwrap_or(page / 2).max(1),
        _ => count.unwrap_or(1) * page.saturating_sub(2).max(1),
    }
}

/// `f`/`t` are inclusive, `F`/`T` exclusive.
fn find_t(p: usize, spec: FindSpec) -> Option<Target> {
    t(
        p,
        if spec.forward {
            Kind::Inclusive
        } else {
            Kind::Exclusive
        },
    )
}

/// `j`/`k`-style move by `delta` lines keeping the goal column. Fails when
/// already on the first/last line.
fn vertical(
    b: &Buffer,
    pos: usize,
    goal: Option<usize>,
    delta: isize,
    env: &Env,
) -> Option<Target> {
    let line = line_of(b, pos);
    let last = last_line(b);
    let target = if delta > 0 {
        if line == last {
            return None;
        }
        line.saturating_add(delta.unsigned_abs()).min(last)
    } else {
        if line == 0 {
            return None;
        }
        line.saturating_sub(delta.unsigned_abs())
    };
    let tab = env.ctx.tab_size;
    let goal = goal.unwrap_or_else(|| display_col(b, pos, tab));
    let p = movement::char_for_display_column(b, target, goal, tab);
    Some(Target {
        pos: clamp_normal(b, p),
        kind: Kind::Linewise,
        goal: Some(goal),
        jump: false,
    })
}
