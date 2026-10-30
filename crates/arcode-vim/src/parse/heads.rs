//! Command heads: the key(s) that select a motion, operator or action.

use super::{Act, Head, InsertKind, Op, P, Parse, ParseCtx, Scroll, Target};
use crate::key::Tok;
use crate::motion::{FindSpec, Motion};
use crate::textobj::object_for;
use crate::types::Mode;

impl P<'_> {
    pub(super) fn motion(&mut self, t: Tok, op_pending: bool) -> Result<Option<Motion>, Parse> {
        use Motion as M;
        let find = |c: char, ch: char| {
            M::Find(FindSpec {
                ch,
                forward: c == 'f' || c == 't',
                till: c == 't' || c == 'T',
            })
        };
        Ok(Some(match t {
            Tok::Char('h') | Tok::Left | Tok::Backspace | Tok::Ctrl('h') => M::Left,
            Tok::Char('l' | ' ') | Tok::Right => M::Right,
            Tok::Char('j') | Tok::Down | Tok::Ctrl('n' | 'j') => M::Down,
            Tok::Char('k') | Tok::Up | Tok::Ctrl('p') => M::Up,
            Tok::Char('w') => M::WordStart(false),
            Tok::Char('W') => M::WordStart(true),
            Tok::Char('e') => M::WordEnd(false),
            Tok::Char('E') => M::WordEnd(true),
            Tok::Char('b') => M::WordBack(false),
            Tok::Char('B') => M::WordBack(true),
            Tok::Char('0') | Tok::Home => M::LineStart,
            Tok::Char('^') => M::FirstNonBlank,
            Tok::Char('$') | Tok::End => M::LineEnd,
            Tok::Char('|') => M::Column,
            Tok::Char('+') | Tok::Enter | Tok::Ctrl('m') => M::NextLine,
            Tok::Char('-') => M::PrevLine,
            Tok::Char('_') => M::CurrentLine,
            Tok::Char('G') => M::FileEnd,
            Tok::Char(c @ ('f' | 'F' | 't' | 'T')) => find(c, self.char_arg(op_pending)?),
            Tok::Char(';') => M::RepeatFind(false),
            Tok::Char(',') => M::RepeatFind(true),
            Tok::Char('%') => M::MatchPair,
            Tok::Char('}') => M::ParagraphForward,
            Tok::Char('{') => M::ParagraphBackward,
            Tok::Char('H') => M::ScreenTop,
            Tok::Char('M') => M::ScreenMiddle,
            Tok::Char('L') => M::ScreenBottom,
            Tok::Ctrl('d') => M::HalfPageDown,
            Tok::Ctrl('u') => M::HalfPageUp,
            Tok::Ctrl('f') | Tok::PageDown => M::PageDown,
            Tok::Ctrl('b') | Tok::PageUp => M::PageUp,
            Tok::Char('*') => M::SearchWord {
                forward: true,
                whole: true,
            },
            Tok::Char('#') => M::SearchWord {
                forward: false,
                whole: true,
            },
            Tok::Char('n') => M::SearchNext(false),
            Tok::Char('N') => M::SearchNext(true),
            Tok::Char(c @ ('\'' | '`')) => M::Mark {
                name: self.char_arg(op_pending)?,
                linewise: c == '\'',
            },
            Tok::Char('g') => match g_motion(self.char_arg(op_pending)?) {
                Some(m) => m,
                None => return Err(Parse::Invalid),
            },
            _ => return Ok(None),
        }))
    }

    pub(super) fn head(&mut self, cx: ParseCtx, plain: bool) -> Result<Head, Parse> {
        let t = self.need(false)?;
        if let Some(h) = self.common(t, cx)? {
            return Ok(h);
        }
        let h = if cx.visual {
            self.visual_head(t)?
        } else {
            normal_head(t)
        };
        if let Some(h) = h {
            return Ok(h);
        }
        if let Some(m) = self.motion(t, false)? {
            return Ok(Head::Motion(m));
        }
        match t {
            Tok::Ctrl(_) | Tok::Other | Tok::Tab if plain => Ok(Head::Act(Act::PassThrough)),
            _ => Err(Parse::Invalid),
        }
    }

    /// Commands shared by Normal and Visual mode.
    fn common(&mut self, t: Tok, cx: ParseCtx) -> Result<Option<Head>, Parse> {
        let op = |o| Ok(Some(Head::Op(o)));
        let act = |a| Ok(Some(Head::Act(a)));
        match t {
            Tok::Char('d') => op(Op::Delete),
            Tok::Char('c') => op(Op::Change),
            Tok::Char('y') => op(Op::Yank),
            Tok::Char('>') => op(Op::Indent),
            Tok::Char('<') => op(Op::Outdent),
            Tok::Char('=') => op(Op::Reindent),
            Tok::Char('g') => {
                let c = self.char_arg(false)?;
                if let Some(m) = g_motion(c) {
                    return Ok(Some(Head::Motion(m)));
                }
                match c {
                    '~' => op(Op::ToggleCase),
                    'u' => op(Op::Lower),
                    'U' => op(Op::Upper),
                    'c' => op(Op::Comment),
                    'v' => act(Act::Reselect),
                    'i' => act(Act::Insert(InsertKind::LastInsert)),
                    'I' => act(Act::Insert(InsertKind::Column0)),
                    'J' => act(Act::Join(false)),
                    'p' | 'P' => act(Act::Paste {
                        before: c == 'P',
                        cursor_after: true,
                    }),
                    _ => Err(Parse::Invalid),
                }
            }
            Tok::Char('z') => {
                let s = match self.need(false)? {
                    Tok::Char('t') => Scroll::Top(false),
                    Tok::Enter => Scroll::Top(true),
                    Tok::Char('z') => Scroll::Center(false),
                    Tok::Char('.') => Scroll::Center(true),
                    Tok::Char('b') => Scroll::Bottom(false),
                    Tok::Char('-') => Scroll::Bottom(true),
                    _ => return Err(Parse::Invalid),
                };
                act(Act::Scroll(s))
            }
            Tok::Char('Z') => match self.char_arg(false)? {
                'Z' => act(Act::SaveQuit),
                'Q' => act(Act::ForceQuit),
                _ => Err(Parse::Invalid),
            },
            Tok::Char('r') => match self.need(false)? {
                Tok::Char(c) => act(Act::Replace(c)),
                Tok::Enter => act(Act::Replace('\n')),
                Tok::Tab => act(Act::Replace('\t')),
                _ => Err(Parse::Invalid),
            },
            Tok::Char('m') => match self.char_arg(false)? {
                c if c.is_ascii_lowercase() || matches!(c, '\'' | '`' | '<' | '>') => {
                    act(Act::SetMark(c))
                }
                _ => Err(Parse::Invalid),
            },
            Tok::Char('q') if cx.recording => act(Act::MacroStop),
            Tok::Char('q') => match self.char_arg(false)? {
                c if c.is_ascii_alphanumeric() => act(Act::MacroRecord(c.to_ascii_lowercase())),
                _ => Err(Parse::Invalid),
            },
            Tok::Char('@') => act(Act::MacroPlay(self.char_arg(false)?)),
            Tok::Char(':') => act(Act::CmdLine),
            Tok::Char('/') => act(Act::SearchPrompt(true)),
            Tok::Char('?') => act(Act::SearchPrompt(false)),
            Tok::Ctrl('e') => act(Act::ScrollLine(1)),
            Tok::Ctrl('y') => act(Act::ScrollLine(-1)),
            Tok::Ctrl('v' | 'q') => act(Act::Visual(Mode::VisualBlock)),
            Tok::Char('v') => act(Act::Visual(Mode::Visual)),
            Tok::Char('V') => act(Act::Visual(Mode::VisualLine)),
            _ => Ok(None),
        }
    }

    fn visual_head(&mut self, t: Tok) -> Result<Option<Head>, Parse> {
        let vop = |o, line| Ok(Some(Head::VisualOp(o, line)));
        let act = |a| Ok(Some(Head::Act(a)));
        match t {
            Tok::Char(c @ ('i' | 'a')) => {
                let o = self.char_arg(false)?;
                match object_for(o, c == 'a') {
                    Some(obj) => Ok(Some(Head::Object(obj))),
                    None => Err(Parse::Invalid),
                }
            }
            Tok::Char('x') | Tok::Delete => vop(Op::Delete, false),
            Tok::Char('X' | 'D') => vop(Op::Delete, true),
            Tok::Char('s') => vop(Op::Change, false),
            Tok::Char('S' | 'C' | 'R') => vop(Op::Change, true),
            Tok::Char('Y') => vop(Op::Yank, true),
            Tok::Char('~') => vop(Op::ToggleCase, false),
            Tok::Char('u') => vop(Op::Lower, false),
            Tok::Char('U') => vop(Op::Upper, false),
            Tok::Char('o' | 'O') => act(Act::SwapEnds),
            Tok::Char('J') => act(Act::Join(true)),
            Tok::Char('I') => act(Act::BlockInsert(false)),
            Tok::Char('A') => act(Act::BlockInsert(true)),
            Tok::Char('p') => act(Act::Paste {
                before: false,
                cursor_after: false,
            }),
            Tok::Char('P') => act(Act::Paste {
                before: true,
                cursor_after: false,
            }),
            _ => Ok(None),
        }
    }
}

pub(super) fn normal_head(t: Tok) -> Option<Head> {
    use Head::{Act as A, Short};
    let short = |op, m| Some(Short(op, Target::Motion(m)));
    match t {
        Tok::Char('x') | Tok::Delete => short(Op::Delete, Motion::Right),
        Tok::Char('X') => short(Op::Delete, Motion::Left),
        Tok::Char('s') => short(Op::Change, Motion::Right),
        Tok::Char('S') => Some(Short(Op::Change, Target::Line)),
        Tok::Char('D') => short(Op::Delete, Motion::LineEnd),
        Tok::Char('C') => short(Op::Change, Motion::LineEnd),
        Tok::Char('Y') => Some(Short(Op::Yank, Target::Line)),
        Tok::Char('i') => Some(A(Act::Insert(InsertKind::Before))),
        Tok::Char('a') => Some(A(Act::Insert(InsertKind::After))),
        Tok::Char('I') => Some(A(Act::Insert(InsertKind::LineStart))),
        Tok::Char('A') => Some(A(Act::Insert(InsertKind::LineEnd))),
        Tok::Char('o') => Some(A(Act::Insert(InsertKind::Below))),
        Tok::Char('O') => Some(A(Act::Insert(InsertKind::Above))),
        Tok::Char('~') => Some(A(Act::ToggleCaseChar)),
        Tok::Char('J') => Some(A(Act::Join(true))),
        Tok::Char('p') => Some(A(Act::Paste {
            before: false,
            cursor_after: false,
        })),
        Tok::Char('P') => Some(A(Act::Paste {
            before: true,
            cursor_after: false,
        })),
        Tok::Char('u') => Some(A(Act::Undo)),
        Tok::Ctrl('r') => Some(A(Act::Redo)),
        Tok::Char('.') => Some(A(Act::Repeat)),
        Tok::Char('R') => Some(A(Act::ReplaceMode)),
        Tok::Ctrl('a') => Some(A(Act::Increment(1))),
        Tok::Ctrl('x') => Some(A(Act::Increment(-1))),
        _ => None,
    }
}

/// Motions after `g`.
pub(super) fn g_motion(c: char) -> Option<Motion> {
    Some(match c {
        'g' => Motion::FileStart,
        'e' => Motion::WordEndBack(false),
        'E' => Motion::WordEndBack(true),
        'j' => Motion::Down,
        'k' => Motion::Up,
        '_' => Motion::LastNonBlank,
        '0' => Motion::LineStart,
        '^' => Motion::FirstNonBlank,
        '$' => Motion::LineEnd,
        '*' => Motion::SearchWord {
            forward: true,
            whole: false,
        },
        '#' => Motion::SearchWord {
            forward: false,
            whole: false,
        },
        _ => return None,
    })
}
