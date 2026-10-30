//! Validation helpers for [`super::PanelTree`]: the structure walk, child
//! counts per widget and the [`MAX_STRING_BYTES`] accounting.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{MAX_DEPTH, MAX_NODES, MAX_STRING_BYTES, TreeError, invalid};
use crate::bindings::NodeId;
use crate::bindings::ui::{Column, Node, TreeItem, Widget};

pub(super) fn too_many(count: usize) -> TreeError {
    TreeError::TooLarge(format!("{count} nodes (limit {MAX_NODES})"))
}

/// Walks the tree from `root`; returns the reachable ids.
pub(super) fn validate(
    root: NodeId,
    nodes: &HashMap<NodeId, Arc<Node>>,
) -> Result<HashSet<NodeId>, TreeError> {
    if !nodes.contains_key(&root) {
        return Err(invalid(format!("root {root} does not exist")));
    }
    let mut seen = HashSet::with_capacity(nodes.len());
    seen.insert(root);
    let mut stack = vec![(root, 1usize)];
    while let Some((id, depth)) = stack.pop() {
        let Some(node) = nodes.get(&id) else {
            return Err(invalid(format!("node {id} does not exist")));
        };
        if node.id != id {
            return Err(invalid(format!("node {id} carries id {}", node.id)));
        }
        check_children(node)?;
        if depth >= MAX_DEPTH && !node.children.is_empty() {
            return Err(invalid(format!("deeper than {MAX_DEPTH} levels")));
        }
        for &child in &node.children {
            if !nodes.contains_key(&child) {
                return Err(invalid(format!("node {id} refers to missing node {child}")));
            }
            if !seen.insert(child) {
                return Err(invalid(format!(
                    "node {child} has two parents or forms a cycle"
                )));
            }
            stack.push((child, depth + 1));
        }
        if seen.len() > MAX_NODES {
            return Err(too_many(seen.len()));
        }
    }
    Ok(seen)
}

/// Child-count and value rules per widget (§7 widget schema).
fn check_children(node: &Node) -> Result<(), TreeError> {
    let have = node.children.len();
    let (want, what) = match &node.widget {
        Widget::Stack(_) => return Ok(()),
        Widget::Split(s) => {
            if !s.ratio.is_finite() {
                return Err(invalid(format!("split {} has a non-finite ratio", node.id)));
            }
            (2, "split")
        }
        Widget::Tabs(t) => (t.titles.len(), "tabs (one child per title)"),
        Widget::Scroll => (1, "scroll"),
        _ => (0, "a leaf widget"),
    };
    if have == want {
        Ok(())
    } else {
        Err(invalid(format!(
            "node {} is {what} and needs {want} children, has {have}",
            node.id
        )))
    }
}

pub(super) fn check_strings<'a>(
    widgets: impl Iterator<Item = &'a Widget>,
) -> Result<(), TreeError> {
    let bytes: usize = widgets.map(widget_bytes).sum();
    if bytes > MAX_STRING_BYTES {
        return Err(TreeError::TooLarge(format!(
            "{bytes} bytes of text (limit {MAX_STRING_BYTES})"
        )));
    }
    Ok(())
}

/// What one widget costs against [`MAX_STRING_BYTES`]: its text bytes plus
/// the host size of every list element (tab title, option, key-value pair,
/// column, tree item), so empty strings are not free.
pub fn widget_bytes(w: &Widget) -> usize {
    use std::mem::size_of;
    fn opt(s: &Option<String>) -> usize {
        s.as_ref().map_or(0, String::len)
    }
    fn strings(list: &[String]) -> usize {
        list.iter().map(|s| s.len() + size_of::<String>()).sum()
    }
    match w {
        Widget::Stack(_) | Widget::Split(_) | Widget::Scroll | Widget::Divider | Widget::Spacer => {
            0
        }
        Widget::Tabs(t) => strings(&t.titles),
        Widget::Text(t) => t.text.len(),
        Widget::Badge(b) => b.text.len(),
        Widget::Button(b) => b.label.len() + opt(&b.icon),
        Widget::Input(i) => i.value.len() + i.placeholder.len() + opt(&i.language),
        Widget::Select(s) => strings(&s.options),
        Widget::Checkbox(c) => c.label.len(),
        Widget::KeyValues(kv) => kv
            .iter()
            .map(|(k, v)| k.len() + v.len() + size_of::<(String, String)>())
            .sum(),
        Widget::Table(t) => t
            .columns
            .iter()
            .map(|c| c.title.len() + size_of::<Column>())
            .sum(),
        Widget::Tree(items) => items
            .iter()
            .map(|i| {
                i.key.len() + i.label.len() + opt(&i.detail) + opt(&i.icon) + size_of::<TreeItem>()
            })
            .sum(),
        Widget::Code(c) => c.text.len() + opt(&c.language),
        Widget::Json(j) | Widget::Spinner(j) => j.len(),
        Widget::EmptyState((a, b)) => a.len() + b.len(),
    }
}
