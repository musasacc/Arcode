//! Parses pending keys into a command: `[count]["x][count]cmd`.

use crate::key::Tok;
use crate::motion::Motion;
use crate::registers::valid_name;
use crate::textobj::{TextObject, object_for};

mod heads;
#[cfg(test)]
mod tests;
use crate::types::Mode;

/// Largest count accepted (keeps loops bounded).
const MAX_COUNT: usize = 1_000_000;

/// An operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Reindent,
    ToggleCase,
    Lower,
    Upper,
    Comment,
}

impl Op {
    /// True if the operator modifies the buffer (repeatable with `.`).
    pub(crate) fn is_change(self) -> bool {
        self != Op::Yank
    }

    /// The key that doubles it (`dd`, `g~~`, `gcc`).
    pub(super) fn double(self) -> char {
        match self {
            Op::Delete => 'd',
            Op::Change | Op::Comment => 'c',
            Op::Yank => 'y',
            Op::Indent => '>',
            Op::Outdent => '<',
            Op::Reindent => '=',
            Op::ToggleCase => '~',
            Op::Lower => 'u',
            Op::Upper => 'U',
        }
    }

    pub(super) fn is_g(self) -> bool {
        matches!(self, Op::ToggleCase | Op::Lower | Op::Upper | Op::Comment)
    }
}

/// What an operator acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Motion(Motion),
    Object(TextObject),
    /// Doubled operator: `count` whole lines.
    Line,
    /// `d/` (true) or `d?`: the motion comes from a search prompt.
    Search(bool),
}

/// Ways to enter Insert mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertKind {
    Before,
    After,
    LineStart,
    LineEnd,
    Column0,
    LastInsert,
    Below,
    Above,
}

/// `zt` and friends; `true` also moves to the first non-blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scroll {
    Top(bool),
    Center(bool),
    Bottom(bool),
}

/// A command that is neither a motion nor an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Insert(InsertKind),
    Visual(Mode),
    Reselect,
    SwapEnds,
    Replace(char),
    ToggleCaseChar,
    Join(bool),
    Paste { before: bool, cursor_after: bool },
    Undo,
    Redo,
    Repeat,
    ReplaceMode,
    CmdLine,
    SearchPrompt(bool),
    SetMark(char),
    Scroll(Scroll),
    ScrollLine(isize),
    SaveQuit,
    ForceQuit,
    MacroRecord(char),
    MacroStop,
    MacroPlay(char),
    Increment(i64),
    BlockInsert(bool),
    PassThrough,
}

/// A parsed command kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmdKind {
    Move(Motion),
    Operate(Op, Target),
    /// Visual-mode operator; `true` forces linewise (`X D Y C S R`).
    VisualOp(Op, bool),
    /// Visual-mode text object.
    Object(TextObject),
    Act(Act),
}

/// A complete command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    /// Product of all counts, `None` if none was typed.
    pub count: Option<usize>,
    pub register: Option<char>,
    pub kind: CmdKind,
    /// Indices of the keys that were count digits.
    pub count_idx: Vec<usize>,
}

/// Parse result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parse {
    Incomplete { op_pending: bool },
    Invalid,
    Done(Cmd),
}

/// Parser inputs besides the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseCtx {
    pub visual: bool,
    pub recording: bool,
}

pub(super) enum Head {
    Op(Op),
    Short(Op, Target),
    VisualOp(Op, bool),
    Object(TextObject),
    Motion(Motion),
    Act(Act),
}

pub(super) const MORE: Parse = Parse::Incomplete { op_pending: false };
pub(super) const MORE_OP: Parse = Parse::Incomplete { op_pending: true };

fn mul(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some((a.unwrap_or(1).saturating_mul(b.unwrap_or(1))).min(MAX_COUNT)),
    }
}

/// Parses `toks` as one command.
pub(crate) fn parse(toks: &[Tok], cx: ParseCtx) -> Parse {
    let mut p = P {
        toks,
        i: 0,
        count_idx: Vec::new(),
    };
    let c1 = p.count();
    let mut register = None;
    if p.peek() == Some(Tok::Char('"')) {
        p.i += 1;
        match p.next() {
            None => return MORE,
            Some(Tok::Char(c)) if valid_name(c) => register = Some(c),
            _ => return Parse::Invalid,
        }
    }
    let mut count = mul(c1, p.count());
    let plain = p.i == 0;
    let head = match p.head(cx, plain) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let kind = match head {
        Head::Motion(m) => CmdKind::Move(m),
        Head::Object(o) => CmdKind::Object(o),
        Head::Act(a) => CmdKind::Act(a),
        Head::Short(op, t) => CmdKind::Operate(op, t),
        Head::VisualOp(op, line) => CmdKind::VisualOp(op, line),
        Head::Op(op) if cx.visual => CmdKind::VisualOp(op, false),
        Head::Op(op) => {
            count = mul(count, p.count());
            match p.target(op) {
                Ok(t) => CmdKind::Operate(op, t),
                Err(e) => return e,
            }
        }
    };
    Parse::Done(Cmd {
        count,
        register,
        kind,
        count_idx: p.count_idx,
    })
}

pub(super) struct P<'a> {
    pub(super) toks: &'a [Tok],
    pub(super) i: usize,
    pub(super) count_idx: Vec<usize>,
}

impl P<'_> {
    pub(super) fn peek(&self) -> Option<Tok> {
        self.toks.get(self.i).copied()
    }

    pub(super) fn next(&mut self) -> Option<Tok> {
        let t = self.peek();
        self.i += usize::from(t.is_some());
        t
    }

    pub(super) fn need(&mut self, op_pending: bool) -> Result<Tok, Parse> {
        self.next().ok_or(if op_pending { MORE_OP } else { MORE })
    }

    fn count(&mut self) -> Option<usize> {
        let mut n: Option<usize> = None;
        while let Some(Tok::Char(c)) = self.peek() {
            let Some(d) = c.to_digit(10) else { break };
            if d == 0 && n.is_none() {
                break;
            }
            n = Some((n.unwrap_or(0).saturating_mul(10) + d as usize).min(MAX_COUNT));
            self.count_idx.push(self.i);
            self.i += 1;
        }
        n
    }

    /// The char argument of `f`, `r`, `m`, ... (`<Tab>` reads as `\t`).
    pub(super) fn char_arg(&mut self, op_pending: bool) -> Result<char, Parse> {
        match self.need(op_pending)? {
            Tok::Char(c) => Ok(c),
            Tok::Tab => Ok('\t'),
            _ => Err(Parse::Invalid),
        }
    }

    fn target(&mut self, op: Op) -> Result<Target, Parse> {
        let t = self.need(true)?;
        match t {
            Tok::Char(c) if c == op.double() => Ok(Target::Line),
            Tok::Char('g') if op.is_g() && self.peek() == Some(Tok::Char(op.double())) => {
                self.i += 1;
                Ok(Target::Line)
            }
            Tok::Char(c @ ('i' | 'a')) => {
                let o = self.char_arg(true)?;
                object_for(o, c == 'a')
                    .map(Target::Object)
                    .ok_or(Parse::Invalid)
            }
            Tok::Char('/') => Ok(Target::Search(true)),
            Tok::Char('?') => Ok(Target::Search(false)),
            _ => match self.motion(t, true)? {
                Some(m) => Ok(Target::Motion(m)),
                None => Err(Parse::Invalid),
            },
        }
    }
}
