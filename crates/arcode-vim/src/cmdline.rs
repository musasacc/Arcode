//! The command line: `:` ex commands and `/` `?` search prompts.
//!
//! Supported ex commands: `:w` `:w!` `:write`, `:q` `:quit`, `:q!`,
//! `:wq` `:x` `:xit` `:exit`, `:{range}` (go to line), `:noh`,
//! `:[range]s/pat/rep/[flags]` (see [`crate::subst`]). Ranges: `%`, `.`,
//! `$`, `N`, `'x` marks (`'<,'>` after Visual mode), each with `+N`/`-N`.
//! Anything else is returned as [`VimCommand::Unknown`].

use crate::key::{Key, Tok};
use crate::motion::Motion;
use crate::normal::status;
use crate::parse::{Op, Target};
use crate::search::query_from_input;
use crate::text::{first_non_blank, last_line, line_of};
use crate::types::{Mode, Register, RegisterKind, VimAction, VimCommand};
use crate::vim::{Ed, Vim};

/// An operator waiting for a search motion (`d/foo<CR>`).
pub(crate) type PendingOp = (Op, Option<usize>, Option<char>);

/// Inclusive 0-based `(first, last)` lines.
type LineRange = (usize, usize);

/// The command line being edited.
#[derive(Debug, Clone)]
pub(crate) struct CmdLine {
    pub prompt: char,
    pub text: String,
    /// Mode to return to.
    pub origin: Mode,
    pub op: Option<PendingOp>,
}

impl Vim {
    /// Opens the command line with `prompt` and initial `text`. `:` from
    /// Visual mode leaves Visual mode first (the range is `'<,'>`).
    pub(crate) fn open_cmdline(
        &mut self,
        prompt: char,
        text: String,
        op: Option<PendingOp>,
        ed: &mut Ed,
    ) {
        if prompt == ':' && self.mode.is_visual() {
            self.exit_visual(ed);
        }
        self.cmdline = Some(CmdLine {
            prompt,
            text,
            origin: self.mode,
            op,
        });
        self.mode = Mode::CommandLine;
    }

    /// Handles a key while the command line is open.
    pub(crate) fn cmdline_key(&mut self, key: &Key, ed: &mut Ed) -> VimAction {
        self.record_key(key);
        let Some(cl) = &mut self.cmdline else {
            self.mode = Mode::Normal;
            return VimAction::None;
        };
        match key.tok() {
            Tok::Char(c) => cl.text.push(c),
            Tok::Tab => cl.text.push('\t'),
            Tok::Ctrl('u') => cl.text.clear(),
            Tok::Ctrl('w') => {
                cl.text.truncate(ctrl_w_keep(&cl.text));
            }
            Tok::Backspace | Tok::Ctrl('h') if !cl.text.is_empty() => {
                cl.text.pop();
            }
            Tok::Backspace | Tok::Ctrl('h') | Tok::Esc => self.close_cmdline(ed),
            Tok::Enter | Tok::Ctrl('m' | 'j') => {
                let Some(cl) = self.cmdline.take() else {
                    return VimAction::None;
                };
                self.mode = cl.origin;
                if self.mode.is_visual() {
                    self.render_visual(ed);
                }
                return match cl.prompt {
                    ':' => self.ex(&cl.text, ed),
                    _ => self.submit_search(&cl, ed),
                };
            }
            _ => {}
        }
        VimAction::None
    }

    fn close_cmdline(&mut self, ed: &mut Ed) {
        if let Some(cl) = self.cmdline.take() {
            self.mode = cl.origin;
            if cl.op.is_some() {
                self.dot = None;
            }
            if self.mode.is_visual() {
                self.render_visual(ed);
            }
        }
    }

    fn submit_search(&mut self, cl: &CmdLine, ed: &mut Ed) -> VimAction {
        let forward = cl.prompt == '/';
        let query = if cl.text.is_empty() {
            match &self.search {
                Some(q) => crate::types::SearchQuery {
                    forward,
                    ..q.clone()
                },
                None => {
                    self.dot = None;
                    return status("E35: No previous regular expression");
                }
            }
        } else {
            query_from_input(&cl.text, forward)
        };
        let pattern = query.pattern.clone();
        self.registers
            .set('/', Register::new(pattern.clone(), RegisterKind::Charwise));
        self.search = Some(query);
        self.highlight = true;
        let motion = Motion::SearchNext(false);
        let action = match cl.op {
            Some((op, count, reg)) => self.run_operate(op, Target::Motion(motion), count, reg, ed),
            None => self.run_motion(motion, None, ed),
        };
        match action {
            VimAction::StatusMessage(_) => action,
            _ => VimAction::Command(VimCommand::Search(pattern)),
        }
    }

    /// Runs an ex command line (without the `:`).
    pub(crate) fn ex(&mut self, text: &str, ed: &mut Ed) -> VimAction {
        let text = text.trim();
        if text.is_empty() {
            return VimAction::None;
        }
        self.registers
            .set(':', Register::new(text.to_string(), RegisterKind::Charwise));
        let (range, rest) = match self.parse_range(text, ed) {
            Ok(x) => x,
            Err(msg) => return status(&msg),
        };
        let rest = rest.trim();
        let cmd = match rest {
            "" => {
                let Some((_, line)) = range else {
                    return VimAction::None;
                };
                let p = first_non_blank(ed.b, line);
                self.marks.insert('\'', ed.sels.primary().head);
                self.set_cursors(ed, vec![p]);
                VimCommand::GotoLine(line + 1)
            }
            "w" | "w!" | "write" | "write!" => VimCommand::Save,
            "q" | "quit" => VimCommand::Quit,
            "q!" | "quit!" => VimCommand::ForceQuit,
            "wq" | "wq!" | "x" | "x!" | "xit" | "exit" => VimCommand::SaveQuit,
            "noh" | "nohl" | "nohlsearch" => {
                self.highlight = false;
                VimCommand::NoHighlight
            }
            r if is_substitute(r) => {
                let cur = line_of(ed.b, ed.sels.primary().head);
                let (first, last) = range.unwrap_or((cur, cur));
                let body = r.trim_start_matches("substitute").trim_start_matches('s');
                return self.substitute(body, first, last, ed);
            }
            _ => VimCommand::Unknown(text.to_string()),
        };
        VimAction::Command(cmd)
    }

    /// Parses a leading range into 0-based `(first, last)` lines.
    fn parse_range<'t>(
        &self,
        text: &'t str,
        ed: &Ed,
    ) -> Result<(Option<LineRange>, &'t str), String> {
        if let Some(rest) = text.strip_prefix('%') {
            return Ok((Some((0, last_line(ed.b))), rest));
        }
        let (first, mut rest) = match self.address(text, ed)? {
            Some(x) => x,
            None => return Ok((None, text)),
        };
        let mut last = first;
        if let Some(r) = rest.strip_prefix([',', ';']) {
            let (l, r) = self.address(r, ed)?.unwrap_or((first, r));
            last = l;
            rest = r;
        }
        let max = last_line(ed.b);
        let (a, b) = (first.min(max), last.min(max));
        Ok((Some((a.min(b), a.max(b))), rest))
    }

    /// One address with optional `+N`/`-N` offsets.
    fn address<'t>(&self, text: &'t str, ed: &Ed) -> Result<Option<(usize, &'t str)>, String> {
        let cur = line_of(ed.b, ed.sels.primary().head) as i64;
        let digits = text.chars().take_while(char::is_ascii_digit).count();
        let (mut line, mut rest) = if digits > 0 {
            let n: i64 = text[..digits]
                .parse()
                .map_err(|_| "E16: Invalid range".to_string())?;
            (n - 1, &text[digits..])
        } else if let Some(r) = text.strip_prefix('.') {
            (cur, r)
        } else if let Some(r) = text.strip_prefix('$') {
            (last_line(ed.b) as i64, r)
        } else if let Some(r) = text.strip_prefix('\'') {
            let mut chars = r.chars();
            let name = chars.next().ok_or("E20: Mark not set")?;
            let pos = self.marks.get(&name).ok_or("E20: Mark not set")?;
            (line_of(ed.b, *pos) as i64, chars.as_str())
        } else if text.starts_with(['+', '-']) {
            (cur, text)
        } else {
            return Ok(None);
        };
        while let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
            let after = &rest[1..];
            let n = after.chars().take_while(char::is_ascii_digit).count();
            let v: i64 = if n == 0 {
                1
            } else {
                after[..n].parse().unwrap_or(1)
            };
            line += if sign == '+' { v } else { -v };
            rest = &after[n..];
        }
        Ok(Some((line.max(0) as usize, rest)))
    }
}

/// True for `s/..`, `s#..`, `substitute/..` (any non-alphanumeric delimiter).
fn is_substitute(r: &str) -> bool {
    let body = r.strip_prefix("substitute").or_else(|| r.strip_prefix('s'));
    matches!(body.and_then(|b| b.chars().next()), Some(c) if !c.is_alphanumeric() && !c.is_whitespace() && c != '"' && c != '|')
}

/// Ctrl-W on the command line: the byte length to keep after deleting
/// trailing blanks and then one run of word chars or of other non-blank
/// chars (always on a char boundary).
fn ctrl_w_keep(text: &str) -> usize {
    let trimmed = text.trim_end();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let Some(last) = trimmed.chars().next_back() else {
        return 0;
    };
    let same = |c: char| !c.is_whitespace() && word(c) == word(last);
    trimmed
        .char_indices()
        .rev()
        .find(|&(_, c)| !same(c))
        .map_or(0, |(i, c)| i + c.len_utf8())
}
