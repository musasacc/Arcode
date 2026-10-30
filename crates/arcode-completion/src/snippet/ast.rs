//! Snippet syntax tree (LSP / TextMate snippet grammar).

/// A parsed snippet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snippet {
    /// Top-level nodes in order.
    pub nodes: Vec<Node>,
}

/// One snippet element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Literal text (escapes already resolved).
    Text(String),
    /// `$1`, `${1}` or `${1/regex/format/flags}`.
    Tabstop {
        /// Tabstop number (`0` is the final cursor).
        index: u32,
        /// Optional transform applied to the mirrored text.
        transform: Option<Transform>,
    },
    /// `${1:default}`; the default may nest other nodes.
    Placeholder {
        /// Tabstop number.
        index: u32,
        /// Default content.
        children: Vec<Node>,
    },
    /// `${1|a,b,c|}`.
    Choice {
        /// Tabstop number.
        index: u32,
        /// Options; the first is inserted.
        options: Vec<String>,
    },
    /// `$NAME`, `${NAME}`, `${NAME:default}` or `${NAME/regex/format/flags}`.
    Variable {
        /// Variable name.
        name: String,
        /// Default used when the variable is empty or unknown.
        default: Option<Vec<Node>>,
        /// Optional transform of the resolved value.
        transform: Option<Transform>,
    },
}

/// `/regex/format/flags` part of a tabstop or variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transform {
    /// Regular expression source (Rust `regex` syntax, close to JS).
    pub regex: String,
    /// Replacement format.
    pub format: Vec<FormatItem>,
    /// Flags: `g` (all matches), `i`, `m`, `s`.
    pub flags: String,
}

/// One element of a transform's format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatItem {
    /// Literal text.
    Text(String),
    /// `$1` / `${1}`: a capture group.
    Group(usize),
    /// `${1:/upcase}` and friends.
    Case(usize, CaseOp),
    /// `${1:+if}`, `${1:-else}`, `${1:else}`, `${1:?if:else}`.
    Conditional {
        /// Capture group tested for participation (non-empty match).
        group: usize,
        /// Inserted when the group matched.
        if_text: String,
        /// Inserted otherwise.
        else_text: String,
    },
}

/// Case change in a format item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseOp {
    /// `/upcase`.
    Upcase,
    /// `/downcase`.
    Downcase,
    /// `/capitalize`: first char upper case.
    Capitalize,
    /// `/camelcase`: `foo_bar` -> `fooBar`.
    Camelcase,
    /// `/pascalcase`: `foo_bar` -> `FooBar`.
    Pascalcase,
}

impl Snippet {
    /// Parses snippet syntax; never fails (invalid constructs stay text).
    pub fn parse(source: &str) -> Snippet {
        crate::snippet::parse::parse(source)
    }

    /// The snippet as plain text with placeholders' defaults and variable
    /// names inlined; for previews in a documentation popup.
    pub fn preview_text(&self) -> String {
        let mut out = String::new();
        preview(&self.nodes, &mut out);
        out
    }
}

fn preview(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Tabstop { .. } => {}
            Node::Placeholder { children, .. } => preview(children, out),
            Node::Choice { options, .. } => {
                if let Some(first) = options.first() {
                    out.push_str(first);
                }
            }
            Node::Variable { name, default, .. } => match default {
                Some(children) => preview(children, out),
                None => out.push_str(name),
            },
        }
    }
}
