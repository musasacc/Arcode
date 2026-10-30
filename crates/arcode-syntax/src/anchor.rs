//! Resolves `@anchor` captures of runnables queries to the item they
//! annotate, e.g. a Rust `#[test]` attribute to the `fn` after it.
//!
//! A query pattern like `(attribute_item) . (attribute_item)* . (function_item)`
//! is cubic in the length of an attribute run inside tree-sitter, so the
//! queries match the single attribute and the sibling walk happens here,
//! in one forward pass over the tree (linear in the file size).
//!
//! Pattern properties:
//! - `(#set! target "<kinds>")`: node kinds the annotated item may have.
//! - `(#set! skip "<kinds>")`: sibling kinds walked over (more attributes,
//!   comments). Anonymous nodes are always walked over.
//! - `(#set! require "<field>")`: the item must have this field (e.g. a
//!   `mod` with a `body`).

use std::ops::Range;

use tree_sitter::{Node, Query, TreeCursor};

use crate::query_match::property;

/// One `@anchor` capture waiting to be resolved.
pub(crate) struct Anchor<'t> {
    /// The captured node (the attribute).
    pub node: Node<'t>,
    /// Index of the pattern that captured it.
    pub pattern: usize,
}

/// Rules for one pattern's walk, read from its properties.
struct Rules<'q> {
    target: &'q str,
    skip: &'q str,
    require: Option<&'q str>,
}

impl<'q> Rules<'q> {
    fn new(query: &'q Query, pattern: usize) -> Rules<'q> {
        Rules {
            target: property(query, pattern, "target").unwrap_or(""),
            skip: property(query, pattern, "skip").unwrap_or(""),
            require: property(query, pattern, "require"),
        }
    }

    fn skips(&self, node: Node<'_>) -> bool {
        !node.is_named() || has_kind(self.skip, node.kind())
    }

    fn accepts(&self, node: Node<'_>) -> bool {
        has_kind(self.target, node.kind())
            && self
                .require
                .is_none_or(|field| node.child_by_field_name(field).is_some())
    }
}

fn has_kind(kinds: &str, kind: &str) -> bool {
    kinds.split_whitespace().any(|k| k == kind)
}

/// Resolves every anchor to its annotated item (`None` when the first
/// sibling after the run is not a target). The result is index-aligned
/// with `anchors`.
pub(crate) fn resolve<'t>(
    query: &Query,
    root: Node<'t>,
    anchors: &[Anchor<'t>],
) -> Vec<Option<Node<'t>>> {
    let mut out = vec![None; anchors.len()];
    let mut order: Vec<usize> = (0..anchors.len()).collect();
    order.sort_by_key(|&i| (anchors[i].node.start_byte(), anchors[i].pattern));
    // Last walked run per pattern: the covered bytes and its result.
    let mut runs: Vec<(usize, Range<usize>, Option<Node<'t>>)> = Vec::new();
    let mut cursor = root.walk();
    for index in order {
        let Anchor { node, pattern } = anchors[index];
        if let Some((_, _, result)) = runs
            .iter()
            .find(|(p, covered, _)| *p == pattern && covered.contains(&node.start_byte()))
        {
            out[index] = *result;
            continue;
        }
        if !advance_to(&mut cursor, node) {
            continue;
        }
        let (covered, result) = walk_run(cursor.clone(), &Rules::new(query, pattern));
        out[index] = result;
        runs.retain(|(p, _, _)| *p != pattern);
        runs.push((pattern, covered, result));
    }
    out
}

/// Walks the siblings after the cursor's node. Returns the bytes from the
/// anchor to the first non-skipped sibling, and that sibling if accepted.
fn walk_run<'t>(mut cursor: TreeCursor<'t>, rules: &Rules<'_>) -> (Range<usize>, Option<Node<'t>>) {
    let start = cursor.node().start_byte();
    let mut end = cursor.node().end_byte();
    while cursor.goto_next_sibling() {
        let node = cursor.node();
        if rules.skips(node) {
            end = node.end_byte();
            continue;
        }
        let result = rules.accepts(node).then_some(node);
        return (start..node.start_byte().max(start + 1), result);
    }
    (start..end.max(start + 1), None)
}

/// Moves `cursor` forward (in document order) to `target`. Returns `false`
/// if `target` is not found after the cursor's position.
fn advance_to(cursor: &mut TreeCursor<'_>, target: Node<'_>) -> bool {
    loop {
        let node = cursor.node();
        if node.id() == target.id() {
            return true;
        }
        let contains =
            node.start_byte() <= target.start_byte() && target.end_byte() <= node.end_byte();
        if contains && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).expect("rust grammar");
        parser.parse(src, None).expect("tree")
    }

    #[test]
    fn advance_finds_nested_and_later_nodes() {
        let tree = parse("fn a() {}\nmod m { fn b() {} fn c() {} }\nfn d() {}\n");
        let root = tree.root_node();
        let m = root.named_child(1).expect("mod");
        let body = m.child_by_field_name("body").expect("body");
        let b = body.named_child(0).expect("b");
        let c = body.named_child(1).expect("c");
        let d = root.named_child(2).expect("d");
        let mut cursor = root.walk();
        assert!(advance_to(&mut cursor, b));
        assert!(advance_to(&mut cursor, c));
        assert!(advance_to(&mut cursor, d));
        // Moving backwards is not supported.
        assert!(!advance_to(&mut cursor, b));
    }

    #[test]
    fn walk_skips_listed_kinds_only() {
        let tree = parse("#[a]\n// c\n#[b]\nfn f() {}\n#[x]\nstruct S;\n");
        let root = tree.root_node();
        let rules = Rules {
            target: "function_item",
            skip: "attribute_item line_comment",
            require: None,
        };
        let mut cursor = root.walk();
        assert!(advance_to(&mut cursor, root.named_child(0).expect("attr")));
        let (covered, found) = walk_run(cursor.clone(), &rules);
        assert_eq!(found.map(|n| n.kind()), Some("function_item"));
        assert_eq!(covered, 0.."#[a]\n// c\n#[b]\n".len());
        assert!(advance_to(&mut cursor, root.named_child(4).expect("x")));
        let (_, found) = walk_run(cursor, &rules);
        assert!(found.is_none());
    }
}
