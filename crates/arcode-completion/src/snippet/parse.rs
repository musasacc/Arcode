//! Fail-soft recursive-descent parser for snippet syntax.
//!
//! Anything that does not parse as a construct is kept as literal text,
//! like VS Code does. Escapes: `\$`, `\}`, `\\` in text; additionally `\,`
//! and `\|` in choices and `\/` in transforms.

use std::cell::Cell;
use std::collections::HashSet;

use super::ast::{Node, Snippet};

/// Parses `source` (see the module docs).
///
/// Runs in linear time: a step budget bounds pathological input (e.g.
/// thousands of unclosed constructs); when it runs out, the whole snippet
/// is kept as literal text.
pub fn parse(source: &str) -> Snippet {
    let chars: Vec<char> = source.chars().filter(|&c| c != '\r').collect();
    let budget = chars
        .len()
        .saturating_mul(STEPS_PER_CHAR)
        .saturating_add(BASE_STEPS);
    let mut p = Parser {
        text_close: stop_table(&chars, '}', |e| matches!(e, '$' | '}' | '\\')),
        format_stops: None,
        chars,
        pos: 0,
        failed: HashSet::new(),
        depth: 0,
        steps: Cell::new(0),
        budget,
    };
    let mut nodes = Vec::new();
    p.parse_nodes(false, &mut nodes);
    if p.steps.get() > p.budget {
        return Snippet {
            nodes: vec![Node::Text(literal(&p.chars))],
        };
    }
    Snippet { nodes }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    /// `$` positions known not to start a construct (keeps unclosed,
    /// deeply nested input from going exponential).
    failed: HashSet<usize>,
    /// Current placeholder nesting depth (bounded by [`MAX_DEPTH`]).
    depth: usize,
    /// Per position: where plain text scanning from it stops at an
    /// unescaped `}` (`chars.len()` if never).
    text_close: Vec<usize>,
    /// Same for transform format text, for `}` and `:` (built lazily).
    format_stops: Option<(Vec<usize>, Vec<usize>)>,
    /// Characters looked at so far; parsing gives up past `budget`.
    steps: Cell<usize>,
    budget: usize,
}

/// Step budget per input char (legit snippets need a handful).
const STEPS_PER_CHAR: usize = 128;
/// Step budget independent of the input length.
const BASE_STEPS: usize = 4096;

/// For every position `x` (and `chars.len()`), the first `stop` reached
/// when scanning from `x`, where `\` followed by a char accepted by
/// `escapes` skips both; `chars.len()` when there is none.
fn stop_table(chars: &[char], stop: char, escapes: impl Fn(char) -> bool) -> Vec<usize> {
    let n = chars.len();
    let mut table = vec![n; n + 1];
    for x in (0..n).rev() {
        table[x] = match chars[x] {
            c if c == stop => x,
            '\\' if chars.get(x + 1).copied().is_some_and(&escapes) => table[x + 2],
            '\\' if x + 1 == n => n,
            _ => table[x + 1],
        };
    }
    table
}

/// `chars` as literal text with top-level escapes resolved.
fn literal(chars: &[char]) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match (c, chars.get(i + 1)) {
            ('\\', Some(&e @ ('$' | '}' | '\\'))) => {
                out.push(e);
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Deeper nesting is kept as text so recursion stays bounded.
const MAX_DEPTH: usize = 64;

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
    /// The current char; `None` at EOF or once the step budget is spent
    /// (which unwinds every loop).
    fn peek(&self) -> Option<char> {
        let steps = self.steps.get() + 1;
        self.steps.set(steps);
        if steps > self.budget {
            return None;
        }
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
        let matches = s
            .chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c));
        if matches {
            self.pos += n;
        }
        matches
    }

    /// Parses nodes until EOF, or until an unconsumed `}` when `nested`.
    fn parse_nodes(&mut self, nested: bool, nodes: &mut Vec<Node>) {
        // At the depth cap everything is text: an unclosed run fails in O(1)
        // instead of rescanning to EOF for every enclosing `$`.
        if nested && self.depth >= MAX_DEPTH && self.text_close[self.pos] == self.chars.len() {
            self.pos = self.chars.len();
            return;
        }
        while let Some(c) = self.peek() {
            match c {
                '$' => {
                    let save = self.pos;
                    let parsed = if self.failed.contains(&save) || self.depth >= MAX_DEPTH {
                        None
                    } else {
                        self.parse_dollar()
                    };
                    match parsed {
                        Some(node) => nodes.push(node),
                        None => {
                            if self.depth < MAX_DEPTH {
                                self.failed.insert(save);
                            }
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
                self.depth += 1;
                self.parse_nodes(true, &mut children);
                self.depth -= 1;
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
                self.depth += 1;
                self.parse_nodes(true, &mut children);
                self.depth -= 1;
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
}

#[path = "parse_transform.rs"]
mod transform;

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
