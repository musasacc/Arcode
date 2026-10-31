//! The debug console's lines and the REPL input history (plain data).

use std::collections::VecDeque;

/// Most lines kept; older ones are dropped.
pub const MAX_LINES: usize = 5000;
/// Longest line in bytes; longer output is wrapped onto more lines.
pub const MAX_LINE_LEN: usize = 4000;
/// Most REPL inputs remembered.
pub const MAX_HISTORY: usize = 100;

/// What a console line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Program standard output.
    Stdout,
    /// Program standard error.
    Stderr,
    /// Adapter or build messages.
    Info,
    /// An expression the user evaluated.
    Input,
    /// Its result.
    Result,
    /// An error.
    Error,
}

impl LineKind {
    /// The kind for a DAP `output` category.
    pub fn from_category(category: Option<&str>) -> LineKind {
        match category {
            Some("stderr") => LineKind::Stderr,
            Some("console") | Some("important") | Some("telemetry") => LineKind::Info,
            _ => LineKind::Stdout,
        }
    }
}

/// One line of the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// The kind (sets the color).
    pub kind: LineKind,
    /// The text, without the line break.
    pub text: String,
}

/// The console's content.
#[derive(Debug, Default)]
pub struct Console {
    lines: VecDeque<ConsoleLine>,
    /// Text of an unfinished output line, per kind.
    partial: Option<ConsoleLine>,
    /// The unfinished line ended with `\r`: the next text replaces it.
    carriage_return: bool,
    history: Vec<String>,
    /// Position while browsing the history (`None`: not browsing).
    history_pos: Option<usize>,
}

impl Console {
    /// The finished lines plus an unfinished one.
    pub fn lines(&self) -> impl Iterator<Item = &ConsoleLine> {
        self.lines.iter().chain(self.partial.iter())
    }

    /// Number of lines (including an unfinished one).
    pub fn len(&self) -> usize {
        self.lines.len() + usize::from(self.partial.is_some())
    }

    /// Whether nothing was printed.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears the lines (the history stays).
    pub fn clear(&mut self) {
        self.lines.clear();
        self.partial = None;
        self.carriage_return = false;
    }

    /// Adds output that may contain several lines or end mid-line (DAP
    /// `output` events split lines anywhere). A carriage return starts the
    /// line over (progress bars), and lines longer than [`MAX_LINE_LEN`]
    /// are wrapped, so output without line breaks stays bounded.
    pub fn output(&mut self, kind: LineKind, text: &str) {
        if self.partial.as_ref().is_some_and(|p| p.kind != kind)
            && let Some(p) = self.partial.take()
        {
            self.push(p);
        }
        let mut pieces = text.split('\n').peekable();
        while let Some(piece) = pieces.next() {
            let finished = pieces.peek().is_some();
            let piece = if finished {
                piece.strip_suffix('\r').unwrap_or(piece)
            } else {
                piece
            };
            if piece.is_empty() && !finished {
                break;
            }
            let partial = self.partial.get_or_insert_with(|| ConsoleLine {
                kind,
                text: String::new(),
            });
            append_with_returns(&mut partial.text, &mut self.carriage_return, piece);
            // Wrap an over-long line (keeps the remainder unfinished).
            while self
                .partial
                .as_ref()
                .is_some_and(|p| p.text.len() > MAX_LINE_LEN)
            {
                let Some(mut p) = self.partial.take() else {
                    break;
                };
                let rest = p.text.split_off(char_boundary(&p.text, MAX_LINE_LEN));
                self.lines_push(p);
                self.partial = Some(ConsoleLine { kind, text: rest });
            }
            if finished && let Some(p) = self.partial.take() {
                self.push(p);
            }
        }
    }

    /// Adds whole lines of one kind.
    pub fn line(&mut self, kind: LineKind, text: impl Into<String>) {
        if let Some(p) = self.partial.take() {
            self.push(p);
        }
        for piece in text.into().split('\n') {
            let mut text = piece.trim_end_matches('\r');
            while text.len() > MAX_LINE_LEN {
                let (head, tail) = text.split_at(char_boundary(text, MAX_LINE_LEN));
                self.lines_push(ConsoleLine {
                    kind,
                    text: head.to_string(),
                });
                text = tail;
            }
            self.push(ConsoleLine {
                kind,
                text: text.to_string(),
            });
        }
    }

    /// Finishes a line (the unfinished line's carriage return is reset).
    fn push(&mut self, line: ConsoleLine) {
        self.carriage_return = false;
        self.lines_push(line);
    }

    fn lines_push(&mut self, line: ConsoleLine) {
        self.lines.push_back(line);
        while self.lines.len() > MAX_LINES {
            self.lines.pop_front();
        }
    }

    /// Records an evaluated expression in the history (no duplicates in a
    /// row) and stops browsing.
    pub fn remember(&mut self, input: &str) {
        let input = input.trim();
        self.history_pos = None;
        if input.is_empty() || self.history.last().is_some_and(|l| l == input) {
            return;
        }
        self.history.push(input.to_string());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// The previous history entry (Up), staying at the oldest.
    pub fn history_up(&mut self) -> Option<&str> {
        if self.history.is_empty() {
            return None;
        }
        let pos = match self.history_pos {
            None => self.history.len() - 1,
            Some(p) => p.saturating_sub(1),
        };
        self.history_pos = Some(pos);
        self.history.get(pos).map(String::as_str)
    }

    /// The next history entry (Down); `Some("")` past the newest.
    pub fn history_down(&mut self) -> Option<&str> {
        let pos = self.history_pos? + 1;
        if pos >= self.history.len() {
            self.history_pos = None;
            return Some("");
        }
        self.history_pos = Some(pos);
        self.history.get(pos).map(String::as_str)
    }
}

/// Appends `piece` to `line`; text after a carriage return replaces the
/// line (as a terminal shows a progress bar). `cr` carries a `\r` that
/// ended the previous piece.
fn append_with_returns(line: &mut String, cr: &mut bool, piece: &str) {
    for (i, segment) in piece.split('\r').enumerate() {
        if i > 0 {
            *cr = true;
        }
        if segment.is_empty() {
            continue;
        }
        if std::mem::take(cr) {
            line.clear();
        }
        line.push_str(segment);
    }
}

/// The largest char boundary of `text` at or below `at` (at least one
/// char, so wrapping always makes progress).
fn char_boundary(text: &str, at: usize) -> usize {
    let mut i = at.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    if i == 0 {
        text.chars().next().map_or(0, char::len_utf8)
    } else {
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(c: &Console) -> Vec<(LineKind, String)> {
        c.lines().map(|l| (l.kind, l.text.clone())).collect()
    }

    #[test]
    fn output_is_split_into_lines_across_events() {
        let mut c = Console::default();
        c.output(LineKind::Stdout, "hel");
        c.output(LineKind::Stdout, "lo\r\nwor");
        assert_eq!(
            texts(&c),
            [
                (LineKind::Stdout, "hello".into()),
                (LineKind::Stdout, "wor".into())
            ]
        );
        c.output(LineKind::Stderr, "err\n");
        c.output(LineKind::Stdout, "ld\n");
        assert_eq!(
            texts(&c),
            [
                (LineKind::Stdout, "hello".into()),
                (LineKind::Stdout, "wor".into()),
                (LineKind::Stderr, "err".into()),
                (LineKind::Stdout, "ld".into()),
            ]
        );
        assert_eq!(LineKind::from_category(Some("console")), LineKind::Info);
    }

    #[test]
    fn lines_are_capped() {
        let mut c = Console::default();
        for i in 0..MAX_LINES + 10 {
            c.line(LineKind::Info, i.to_string());
        }
        assert_eq!(c.len(), MAX_LINES);
        assert_eq!(c.lines().next().unwrap().text, "10");
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn output_without_line_breaks_stays_bounded() {
        let mut c = Console::default();
        let chunk = "é".repeat(500);
        for _ in 0..1000 {
            c.output(LineKind::Stdout, &chunk);
        }
        assert!(c.lines().all(|l| l.text.len() <= MAX_LINE_LEN));
        let total: usize = c.lines().map(|l| l.text.chars().count()).sum();
        assert_eq!(total, 500 * 1000, "nothing is lost while wrapping");
        c.line(LineKind::Info, "x".repeat(MAX_LINE_LEN * 2 + 1));
        assert!(c.lines().all(|l| l.text.len() <= MAX_LINE_LEN));
    }

    #[test]
    fn carriage_returns_redraw_the_line() {
        let mut c = Console::default();
        for p in 0..=100 {
            c.output(LineKind::Stdout, &format!("{p}%\r"));
        }
        assert_eq!(texts(&c), [(LineKind::Stdout, "100%".into())]);
        c.output(LineKind::Stdout, "\rdone\nnext\r\n");
        assert_eq!(
            texts(&c),
            [
                (LineKind::Stdout, "done".into()),
                (LineKind::Stdout, "next".into())
            ]
        );
        c.output(LineKind::Stdout, "a\rb\n\n");
        assert_eq!(c.lines().nth(2).unwrap().text, "b");
        assert_eq!(c.lines().nth(3).unwrap().text, "");
    }

    #[test]
    fn history_browses_back_and_forth() {
        let mut c = Console::default();
        assert_eq!(c.history_up(), None);
        c.remember("a");
        c.remember("b");
        c.remember("b");
        c.remember("  ");
        assert_eq!(c.history_up(), Some("b"));
        assert_eq!(c.history_up(), Some("a"));
        assert_eq!(c.history_up(), Some("a"));
        assert_eq!(c.history_down(), Some("b"));
        assert_eq!(c.history_down(), Some(""));
        assert_eq!(c.history_down(), None);
    }
}
