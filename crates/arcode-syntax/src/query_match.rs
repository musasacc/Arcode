//! Helpers shared by the runnables and outline queries: run a query over a
//! whole tree, read captures by name, read `#set!` properties, node text.

use std::ops::Range;

use ropey::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

use crate::rope_text::{byte_chunks, node_text};

/// One query match with its captures resolved to names.
pub(crate) struct Match<'q, 't> {
    /// Index of the pattern that matched.
    pub pattern: usize,
    /// `(capture name, node)` pairs in the order tree-sitter reported them.
    pub captures: Vec<(&'q str, Node<'t>)>,
}

impl<'t> Match<'_, 't> {
    /// The first node captured as `name`.
    pub fn node(&self, name: &str) -> Option<Node<'t>> {
        self.nodes(name).next()
    }

    /// Every node captured as `name`, in document order.
    pub fn nodes<'s>(&'s self, name: &'s str) -> impl Iterator<Item = Node<'t>> + 's {
        self.captures
            .iter()
            .filter(move |(capture, _)| *capture == name)
            .map(|(_, node)| *node)
    }
}

/// Runs `query` over the subtree at `root` and collects every match.
///
/// Text predicates (`#eq?`, `#match?`, `#any-of?`) are checked against `text`.
pub(crate) fn matches<'q, 't>(query: &'q Query, root: Node<'t>, text: &Rope) -> Vec<Match<'q, 't>> {
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut iter = cursor.matches(query, root, node_text(text));
    let mut out = Vec::new();
    while let Some(found) = iter.next() {
        let mut captures: Vec<(&'q str, Node<'t>)> = found
            .captures
            .iter()
            .filter_map(|capture| {
                let name = names.get(capture.index as usize)?;
                Some((*name, capture.node))
            })
            .collect();
        captures.sort_by_key(|(_, node)| node.start_byte());
        out.push(Match {
            pattern: found.pattern_index,
            captures,
        });
    }
    out
}

/// The value of `(#set! key "value")` on `pattern`, if any.
pub(crate) fn property<'q>(query: &'q Query, pattern: usize, key: &str) -> Option<&'q str> {
    query
        .property_settings(pattern)
        .iter()
        .find(|prop| &*prop.key == key)
        .and_then(|prop| prop.value.as_deref())
}

/// The text of `range` (bytes, clamped to the text) as a `String`.
pub(crate) fn text_of(text: &Rope, range: Range<usize>) -> String {
    let bytes: Vec<u8> = byte_chunks(text, range).flatten().copied().collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Node text with runs of whitespace collapsed to one space.
pub(crate) fn compact_text(text: &Rope, node: Node<'_>) -> String {
    let raw = text_of(text, node.byte_range());
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The display name of a `@name` node. String literals lose their quotes
/// and keep their inner whitespace (a JS test title must match the real
/// title exactly); other nodes have whitespace runs collapsed.
pub(crate) fn name_text(text: &Rope, node: Node<'_>) -> String {
    if is_string_literal(node.kind()) {
        let raw = text_of(text, node.byte_range());
        return unquote(&raw).to_string();
    }
    compact_text(text, node)
}

/// Whether `kind` is a quoted string literal node in one of our grammars.
fn is_string_literal(kind: &str) -> bool {
    matches!(
        kind,
        "string" | "template_string" | "string_literal" | "interpreted_string_literal"
    )
}

/// Strips one pair of matching string quotes (`"`, `'` or `` ` ``).
pub(crate) fn unquote(name: &str) -> &str {
    for quote in ['"', '\'', '`'] {
        if let Some(inner) = name
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    name
}

/// Whether `outer` strictly contains `inner` (contains and is not equal).
pub(crate) fn strictly_contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end && outer != inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_strips_one_matching_pair() {
        assert_eq!(unquote("\"adds\""), "adds");
        assert_eq!(unquote("'x'"), "x");
        assert_eq!(unquote("`t`"), "t");
        assert_eq!(unquote("\"mixed'"), "\"mixed'");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(unquote("plain"), "plain");
        assert_eq!(unquote("\"\""), "");
    }

    #[test]
    fn containment_is_strict() {
        assert!(strictly_contains(&(0..10), &(2..5)));
        assert!(strictly_contains(&(0..10), &(0..5)));
        assert!(!strictly_contains(&(0..10), &(0..10)));
        assert!(!strictly_contains(&(2..5), &(0..10)));
        assert!(!strictly_contains(&(0..5), &(4..8)));
    }

    #[test]
    fn text_of_clamps() {
        let rope = Rope::from_str("héllo");
        assert_eq!(text_of(&rope, 0..3), "hé");
        assert_eq!(text_of(&rope, 3..100), "llo");
        assert_eq!(text_of(&rope, 100..200), "");
    }
}
