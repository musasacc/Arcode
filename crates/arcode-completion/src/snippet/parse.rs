//! Fail-soft recursive-descent parser for snippet syntax.
//!
//! Anything that does not parse as a construct is kept as literal text,
//! like VS Code does. Escapes: `\$`, `\}`, `\\` in text; additionally `\,`
//! and `\|` in choices and `\/` in transforms.

use std::collections::HashSet;

use super::ast::{CaseOp, FormatItem, Node, Snippet, Transform};

/// Parses `source` (see the module docs).
pub fn parse(source: &str) -> Snippet {
    let mut p = Parser {
        chars: source.chars().filter(|&c| c != '\r').collect(),
        pos: 0,
        failed: HashSet::new(),
    };
    let mut nodes = Vec::new();
    p.parse_nodes(false, &mut nodes);
    Snippet { nodes }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    /// `$` positions known not to start a construct (keeps unclosed,
    /// deeply nested input from going exponential).
    failed: HashSet<usize>,
}

fn push_text(nodes: &mut Vec<Node>, c: char) {
    if let Some(Node::Text(t)) = nodes.last_mut() {
        t.push(c);
    } else {
        nodes.push(Node::Text(c.to_string()));
    }
}

fn is_var_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, s: &str) -> bool {
        let n = s.chars().count();
        let matches = s.chars().enumerate().all(|(i, c)| self.peek_at(i) == Some(c));
        if matches {
            self.pos += n;
        }
        matches
    }

    /// Parses nodes until EOF, or until an unconsumed `}` when `nested`.
    fn parse_nodes(&mut self, nested: bool, nodes: &mut Vec<Node>) {
        while let Some(c) = self.peek() {
            match c {
                '$' => {
                    let save = self.pos;
                    let parsed = if self.failed.contains(&save) {
                        None
                    } else {
                        self.parse_dollar()
                    };
                    match parsed {
                        Some(node) => nodes.push(node),
                        None => {
                            self.failed.insert(save);
                            self.pos = save + 1;
                            push_text(nodes, '$');
                        }
                    }
                }
                '\\' => {
                    let next = self.peek_at(1);
                    if let Some(e @ ('$' | '}' | '\\')) = next {
                        push_text(nodes, e);
                        self.pos += 2;
                    } else {
                        push_text(nodes, '\\');
                        self.pos += 1;
                    }
                }
                '}' if nested => return,
                _ => {
                    push_text(nodes, c);
                    self.pos += 1;
                }
            }
        }
    }

    fn parse_int(&mut self) -> Option<u32> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if start == self.pos {
            return None;
        }
        let digits: String = self.chars[start..self.pos].iter().collect();
        digits.parse().ok()
    }

    fn parse_var_name(&mut self) -> Option<String> {
        if !self.peek().is_some_and(is_var_start) {
            return None;
        }
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            self.pos += 1;
        }
        Some(self.chars[start..self.pos].iter().collect())
    }

    /// Parses a construct starting at `$`; `None` if it is not one.
    fn parse_dollar(&mut self) -> Option<Node> {
        self.pos += 1; // '$'
        let c = self.peek()?;
        if c.is_ascii_digit() {
            let index = self.parse_int()?;
            return Some(Node::Tabstop {
                index,
                transform: None,
            });
        }
        if is_var_start(c) {
            let name = self.parse_var_name()?;
            return Some(Node::Variable {
                name,
                default: None,
                transform: None,
            });
        }
        if !self.eat('{') {
            return None;
        }
        if self.peek()?.is_ascii_digit() {
            let index = self.parse_int()?;
            self.parse_braced_tabstop(index)
        } else {
            let name = self.parse_var_name()?;
            self.parse_braced_variable(name)
        }
    }

    fn parse_braced_tabstop(&mut self, index: u32) -> Option<Node> {
        match self.peek()? {
            '}' => {
                self.pos += 1;
                Some(Node::Tabstop {
                    index,
                    transform: None,
                })
            }
            ':' => {
                self.pos += 1;
                let mut children = Vec::new();
                self.parse_nodes(true, &mut children);
                self.eat('}')
                    .then_some(Node::Placeholder { index, children })
            }
            '|' => {
                self.pos += 1;
                let options = self.parse_choice_options()?;
                Some(Node::Choice { index, options })
            }
            '/' => {
                let transform = Some(self.parse_transform()?);
                self.eat('}').then_some(Node::Tabstop { index, transform })
            }
            _ => None,
        }
    }

    fn parse_braced_variable(&mut self, name: String) -> Option<Node> {
        match self.peek()? {
            '}' => {
                self.pos += 1;
                Some(Node::Variable {
                    name,
                    default: None,
                    transform: None,
                })
            }
            ':' => {
                self.pos += 1;
                let mut children = Vec::new();
                self.parse_nodes(true, &mut children);
                self.eat('}').then_some(Node::Variable {
                    name,
                    default: Some(children),
                    transform: None,
                })
            }
            '/' => {
                let transform = Some(self.parse_transform()?);
                self.eat('}').then_some(Node::Variable {
                    name,
                    default: None,
                    transform,
                })
            }
            _ => None,
        }
    }

    /// After `${N|`: options separated by `,`, closed by `|}`.
    fn parse_choice_options(&mut self) -> Option<Vec<String>> {
        let mut options = Vec::new();
        let mut current = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                '\\' => match self.peek() {
                    Some(e @ ('$' | '}' | '\\' | ',' | '|')) => {
                        current.push(e);
                        self.pos += 1;
                    }
                    _ => current.push('\\'),
                },
                ',' => options.push(std::mem::take(&mut current)),
                '|' => {
                    if !self.eat('}') {
                        return None;
                    }
                    options.push(current);
                    return Some(options);
                }
                _ => current.push(c),
            }
        }
    }

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
    fn parse_transform(&mut self) -> Option<Transform> {
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
            (self.read_format_text(&['}'])?, String::new())
        } else if self.eat('?') {
            let if_text = self.read_format_text(&[':'])?;
            self.pos += 1;
            (if_text, self.read_format_text(&['}'])?)
        } else {
            self.eat('-');
            (String::new(), self.read_format_text(&['}'])?)
        };
        self.pos += 1; // '}'
        Some(FormatItem::Conditional {
            group,
            if_text,
            else_text,
        })
    }

    /// Text up to (not including) an unescaped char from `stops`.
    fn read_format_text(&mut self, stops: &[char]) -> Option<String> {
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            if stops.contains(&c) {
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

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
