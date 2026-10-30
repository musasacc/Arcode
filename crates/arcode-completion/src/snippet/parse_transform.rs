//! Transform parsing: `/regex/format/flags`.

use super::{Parser, stop_table};
use crate::snippet::ast::{CaseOp, FormatItem, Transform};

impl Parser {
    /// Reads up to an unescaped `/`, consuming it. `\/` becomes `/`; other
    /// escapes are kept verbatim (they matter to the regex).
    fn read_until_slash(&mut self) -> Option<String> {
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                '/' => return Some(out),
                '\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    if e != '/' {
                        out.push('\\');
                    }
                    out.push(e);
                }
                _ => out.push(c),
            }
        }
    }

    /// `/regex/format/flags` (the closing `}` is left to the caller).
    pub(super) fn parse_transform(&mut self) -> Option<Transform> {
        if !self.eat('/') {
            return None;
        }
        let regex = self.read_until_slash()?;
        let format = self.parse_format()?;
        let mut flags = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_ascii_alphabetic()) {
            flags.push(c);
            self.pos += 1;
        }
        Some(Transform {
            regex,
            format,
            flags,
        })
    }

    /// Format items up to and including the closing `/`.
    fn parse_format(&mut self) -> Option<Vec<FormatItem>> {
        let mut items = Vec::new();
        let mut text = String::new();
        loop {
            let c = self.peek()?;
            match c {
                '/' => {
                    self.pos += 1;
                    break;
                }
                '\\' => {
                    self.pos += 1;
                    match self.peek()? {
                        e @ ('/' | '$' | '\\' | '}') => text.push(e),
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        e => {
                            text.push('\\');
                            text.push(e);
                        }
                    }
                    self.pos += 1;
                }
                '$' => {
                    let save = self.pos;
                    match self.parse_format_item() {
                        Some(item) => {
                            if !text.is_empty() {
                                items.push(FormatItem::Text(std::mem::take(&mut text)));
                            }
                            items.push(item);
                        }
                        None => {
                            self.pos = save + 1;
                            text.push('$');
                        }
                    }
                }
                _ => {
                    text.push(c);
                    self.pos += 1;
                }
            }
        }
        if !text.is_empty() {
            items.push(FormatItem::Text(text));
        }
        Some(items)
    }

    fn parse_format_item(&mut self) -> Option<FormatItem> {
        self.pos += 1; // '$'
        if self.peek()?.is_ascii_digit() {
            return Some(FormatItem::Group(self.parse_int()? as usize));
        }
        if !self.eat('{') {
            return None;
        }
        let group = self.parse_int()? as usize;
        if self.eat('}') {
            return Some(FormatItem::Group(group));
        }
        if !self.eat(':') {
            return None;
        }
        if self.eat('/') {
            let op = [
                ("upcase", CaseOp::Upcase),
                ("downcase", CaseOp::Downcase),
                ("capitalize", CaseOp::Capitalize),
                ("camelcase", CaseOp::Camelcase),
                ("pascalcase", CaseOp::Pascalcase),
            ]
            .into_iter()
            .find_map(|(name, op)| self.eat_str(name).then_some(op))?;
            return self.eat('}').then_some(FormatItem::Case(group, op));
        }
        let (if_text, else_text) = if self.eat('+') {
            (self.read_format_text('}')?, String::new())
        } else if self.eat('?') {
            let if_text = self.read_format_text(':')?;
            self.pos += 1;
            (if_text, self.read_format_text('}')?)
        } else {
            self.eat('-');
            (String::new(), self.read_format_text('}')?)
        };
        self.pos += 1; // '}'
        Some(FormatItem::Conditional {
            group,
            if_text,
            else_text,
        })
    }

    /// Text up to (not including) an unescaped `stop` (`}` or `:`).
    /// Fails in O(1) when there is none, so unclosed items stay linear.
    fn read_format_text(&mut self, stop: char) -> Option<String> {
        let chars = &self.chars;
        let (close, colon) = self.format_stops.get_or_insert_with(|| {
            (
                stop_table(chars, '}', |_| true),
                stop_table(chars, ':', |_| true),
            )
        });
        let table = if stop == '}' { close } else { colon };
        if table
            .get(self.pos)
            .is_none_or(|&end| end >= self.chars.len())
        {
            self.pos = self.chars.len();
            return None;
        }
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            if c == stop {
                return Some(out);
            }
            self.pos += 1;
            if c == '\\' {
                out.push(self.peek()?);
                self.pos += 1;
            } else {
                out.push(c);
            }
        }
    }
}
