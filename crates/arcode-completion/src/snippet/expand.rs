//! Snippet expansion into text plus tabstop ranges.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use super::ast::{Node, Snippet};
use super::variables::VariableContext;

/// One tabstop after expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabstop {
    /// Tabstop number (`0` = final cursor).
    pub index: u32,
    /// Char ranges in [`ExpandedSnippet::text`], sorted; several = mirrors.
    pub ranges: Vec<Range<usize>>,
    /// Options when this is a choice tabstop.
    pub choices: Option<Vec<String>>,
}

/// The result of [`expand`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExpandedSnippet {
    /// Text to insert.
    pub text: String,
    /// Tabstops in visiting order (ascending, `$0` last; always present).
    pub tabstops: Vec<Tabstop>,
}

/// Parses and expands `snippet`; see [`Snippet::expand`].
pub fn expand(
    snippet: &str,
    ctx: &VariableContext,
    indent: &str,
    tab_unit: &str,
) -> ExpandedSnippet {
    Snippet::parse(snippet).expand(ctx, indent, tab_unit)
}

impl Snippet {
    /// Expands the snippet.
    ///
    /// Variables are resolved from `ctx`; unknown ones become placeholders
    /// holding their name (or default), one tabstop per distinct name,
    /// numbered after the highest explicit tabstop. Every line after the
    /// first is prefixed with `indent`, and tabs in leading whitespace
    /// become `tab_unit`. Mirrors (`$1` repeated) get the text of the first
    /// placeholder/choice with that number. Transformed tabstops are filled
    /// once and not tracked as mirrors.
    pub fn expand(&self, ctx: &VariableContext, indent: &str, tab_unit: &str) -> ExpandedSnippet {
        let mut next_index = max_index(&self.nodes).saturating_add(1).max(1);
        let mut unknown = HashMap::new();
        let nodes = resolve_variables(&self.nodes, ctx, &mut next_index, &mut unknown);

        let mut defs = HashMap::new();
        collect_defs(&nodes, &mut defs);
        let mut r = Renderer {
            defs,
            raw_memo: HashMap::new(),
            visiting: HashSet::new(),
            out: Emitter::new(indent, tab_unit),
            ranges: BTreeMap::new(),
            choices: HashMap::new(),
        };
        r.render(&nodes);

        let end = r.out.chars;
        let mut tabstops: Vec<Tabstop> = r
            .ranges
            .into_iter()
            .map(|(index, mut ranges)| {
                ranges.sort_by_key(|r| (r.start, r.end));
                Tabstop {
                    index,
                    ranges,
                    choices: r.choices.remove(&index),
                }
            })
            .collect();
        if tabstops.first().is_some_and(|t| t.index == 0) {
            let zero = tabstops.remove(0);
            tabstops.push(zero);
        } else {
            tabstops.push(Tabstop {
                index: 0,
                ranges: vec![end..end],
                choices: None,
            });
        }
        ExpandedSnippet {
            text: r.out.text,
            tabstops,
        }
    }
}

fn max_index(nodes: &[Node]) -> u32 {
    nodes
        .iter()
        .map(|n| match n {
            Node::Tabstop { index, .. } | Node::Choice { index, .. } => *index,
            Node::Placeholder { index, children } => (*index).max(max_index(children)),
            Node::Variable {
                default: Some(d), ..
            } => max_index(d),
            Node::Text(_) | Node::Variable { .. } => 0,
        })
        .max()
        .unwrap_or(0)
}

/// Replaces variables by text (known) or placeholders (unknown).
fn resolve_variables(
    nodes: &[Node],
    ctx: &VariableContext,
    next_index: &mut u32,
    unknown: &mut HashMap<String, u32>,
) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Node::Placeholder { index, children } => out.push(Node::Placeholder {
                index: *index,
                children: resolve_variables(children, ctx, next_index, unknown),
            }),
            Node::Variable {
                name,
                default,
                transform,
            } => match ctx.resolve(name) {
                Some(value) if value.is_empty() && default.is_some() => {
                    let d = default.as_deref().unwrap_or_default();
                    out.extend(resolve_variables(d, ctx, next_index, unknown));
                }
                Some(value) => {
                    let value = match transform {
                        Some(t) => t.apply(&value),
                        None => value,
                    };
                    out.push(Node::Text(value));
                }
                None => {
                    let index = *unknown.entry(name.clone()).or_insert_with(|| {
                        let i = *next_index;
                        *next_index = next_index.saturating_add(1);
                        i
                    });
                    let children = match default {
                        Some(d) => resolve_variables(d, ctx, next_index, unknown),
                        None => vec![Node::Text(name.clone())],
                    };
                    out.push(Node::Placeholder { index, children });
                }
            },
            other => out.push(other.clone()),
        }
    }
    out
}

/// What defines a tabstop's text: the first placeholder or choice.
#[derive(Clone, Copy)]
enum Def<'a> {
    Children(&'a [Node]),
    Choice(&'a [String]),
}

fn collect_defs<'a>(nodes: &'a [Node], defs: &mut HashMap<u32, Def<'a>>) {
    for node in nodes {
        match node {
            Node::Placeholder { index, children } => {
                defs.entry(*index).or_insert(Def::Children(children));
                collect_defs(children, defs);
            }
            Node::Choice { index, options } => {
                defs.entry(*index).or_insert(Def::Choice(options));
            }
            _ => {}
        }
    }
}

/// Builds the output text, applying indentation and counting chars.
struct Emitter<'s> {
    text: String,
    chars: usize,
    leading_ws: bool,
    indent: &'s str,
    tab_unit: &'s str,
}

impl<'s> Emitter<'s> {
    fn new(indent: &'s str, tab_unit: &'s str) -> Self {
        Emitter {
            text: String::new(),
            chars: 0,
            leading_ws: true,
            indent,
            tab_unit,
        }
    }

    fn push_str(&mut self, s: &str) {
        let start_len = self.text.len();
        for c in s.chars() {
            match c {
                '\r' => {}
                '\n' => {
                    self.text.push('\n');
                    self.text.push_str(self.indent);
                    self.leading_ws = true;
                }
                '\t' if self.leading_ws => self.text.push_str(self.tab_unit),
                ' ' => self.text.push(' '),
                _ => {
                    self.leading_ws = false;
                    self.text.push(c);
                }
            }
        }
        self.chars += self.text[start_len..].chars().count();
    }
}

struct Renderer<'a, 's> {
    defs: HashMap<u32, Def<'a>>,
    raw_memo: HashMap<u32, String>,
    visiting: HashSet<u32>,
    out: Emitter<'s>,
    ranges: BTreeMap<u32, Vec<Range<usize>>>,
    choices: HashMap<u32, Vec<String>>,
}

fn is_def_children(def: Option<&Def<'_>>, children: &[Node]) -> bool {
    matches!(def, Some(Def::Children(c)) if std::ptr::eq(*c, children))
}

fn is_def_choice(def: Option<&Def<'_>>, options: &[String]) -> bool {
    matches!(def, Some(Def::Choice(o)) if std::ptr::eq(*o, options))
}

impl<'a> Renderer<'a, '_> {
    fn record(&mut self, index: u32, range: Range<usize>) {
        self.ranges.entry(index).or_default().push(range);
    }

    fn render(&mut self, nodes: &'a [Node]) {
        for node in nodes {
            let start = self.out.chars;
            match node {
                Node::Text(t) => self.out.push_str(t),
                Node::Tabstop { index, transform } => {
                    let text = self.raw_text(*index);
                    match transform {
                        Some(t) => self.out.push_str(&t.apply(&text)),
                        None => {
                            self.out.push_str(&text);
                            self.record(*index, start..self.out.chars);
                        }
                    }
                }
                Node::Placeholder { index, children } => {
                    if is_def_children(self.defs.get(index), children) {
                        self.render(children);
                    } else {
                        let text = self.raw_text(*index);
                        self.out.push_str(&text);
                    }
                    self.record(*index, start..self.out.chars);
                }
                Node::Choice { index, options } => {
                    if is_def_choice(self.defs.get(index), options) {
                        self.out.push_str(options.first().map_or("", String::as_str));
                        self.choices.insert(*index, options.clone());
                    } else {
                        let text = self.raw_text(*index);
                        self.out.push_str(&text);
                    }
                    self.record(*index, start..self.out.chars);
                }
                // Variables were resolved before rendering.
                Node::Variable { .. } => {}
            }
        }
    }

    /// The unindented default text of tabstop `index` (empty on cycles).
    fn raw_text(&mut self, index: u32) -> String {
        if let Some(t) = self.raw_memo.get(&index) {
            return t.clone();
        }
        if !self.visiting.insert(index) {
            return String::new();
        }
        let text = match self.defs.get(&index).copied() {
            Some(Def::Children(children)) => self.raw_nodes(children),
            Some(Def::Choice(options)) => options.first().cloned().unwrap_or_default(),
            None => String::new(),
        };
        self.visiting.remove(&index);
        self.raw_memo.insert(index, text.clone());
        text
    }

    fn raw_nodes(&mut self, nodes: &'a [Node]) -> String {
        let mut out = String::new();
        for node in nodes {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Tabstop { index, transform } => {
                    let text = self.raw_text(*index);
                    match transform {
                        Some(t) => out.push_str(&t.apply(&text)),
                        None => out.push_str(&text),
                    }
                }
                Node::Placeholder { index, children } => {
                    if is_def_children(self.defs.get(index), children) {
                        let text = self.raw_nodes(children);
                        out.push_str(&text);
                    } else {
                        out.push_str(&self.raw_text(*index));
                    }
                }
                Node::Choice { index, .. } => out.push_str(&self.raw_text(*index)),
                Node::Variable { .. } => {}
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "expand_tests.rs"]
mod tests;
