//! Panel trees on the host side (§7 "Pipeline", step 3). No GPUI.
//!
//! A [`PanelTree`] holds the current view of one panel. `ui.render` replaces
//! it and `ui.apply-patches` edits it; both run on the plugin thread, are
//! validated as a whole and applied atomically: on error nothing changes.
//! Validation rejects dangling ids, nodes with two parents, cycles, wrong
//! child counts, non-finite split ratios and cap violations (§5 host caps).
//!
//! After every change, nodes that are no longer reachable from the root are
//! dropped, which is how `remove` takes "every descendant no longer
//! referenced" with it. The app receives an immutable [`PanelSnapshot`]
//! (nodes are `Arc`s, so a snapshot costs one pointer per node) plus the
//! [`Changes`] it needs to redraw only what moved.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::bindings::NodeId;
use crate::bindings::ui::{Node, Patch, UiError, View, Widget};

/// Nodes per panel.
pub const MAX_NODES: usize = 20_000;
/// Levels below and including the root.
pub const MAX_DEPTH: usize = 64;
/// String bytes per `render` or `apply-patches`.
pub const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;

/// An immutable view of a panel, shared with the app.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanelSnapshot {
    /// The root node (0 while nothing was rendered).
    pub root: NodeId,
    /// Every node reachable from `root`.
    pub nodes: HashMap<NodeId, Arc<Node>>,
    /// Bumped on every accepted change.
    pub revision: u64,
}

impl PanelSnapshot {
    /// The node `id`.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).map(Arc::as_ref)
    }

    /// The root node, once something was rendered.
    pub fn root_node(&self) -> Option<&Node> {
        self.node(self.root)
    }

    /// The children of `id`, in order.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = &Node> {
        self.node(id)
            .into_iter()
            .flat_map(|n| n.children.iter().filter_map(|c| self.node(*c)))
    }

    /// The snapshot as a WIT `view` (nodes ordered by id; tests).
    pub fn to_view(&self) -> View {
        let mut nodes: Vec<Node> = self.nodes.values().map(|n| (**n).clone()).collect();
        nodes.sort_by_key(|n| n.id);
        View {
            root: self.root,
            nodes,
        }
    }
}

/// A table whose rows the app must refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableChange {
    /// The table node.
    pub table: NodeId,
    /// Its row count after the change (`None`: unknown).
    pub row_count: Option<u64>,
    /// True: drop every cached page (new generation, new columns,
    /// `rows-changed`). False: only the row count changed.
    pub reset: bool,
}

/// What one accepted change did, so the app redraws only that.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    /// True for `render` (the whole view was replaced).
    pub full: bool,
    /// New or changed nodes, ascending.
    pub dirty: Vec<NodeId>,
    /// Nodes that disappeared, ascending.
    pub removed: Vec<NodeId>,
    /// Inputs whose `value` the plugin set or changed: only these overwrite
    /// the text the user is editing (§7 "Uncontrolled inputs").
    pub inputs: Vec<NodeId>,
    /// Tables whose cached pages or row counts changed.
    pub tables: Vec<TableChange>,
}

/// Why a view or patch set was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// A cap was exceeded (nodes or string bytes).
    TooLarge(String),
    /// The structure is invalid.
    Invalid(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(m) | Self::Invalid(m) => f.write_str(m),
        }
    }
}

impl From<TreeError> for UiError {
    fn from(e: TreeError) -> Self {
        match e {
            TreeError::TooLarge(_) => Self::TooLarge,
            TreeError::Invalid(m) => Self::InvalidTree(m),
        }
    }
}

fn invalid(message: impl Into<String>) -> TreeError {
    TreeError::Invalid(message.into())
}

/// One panel's tree.
#[derive(Debug, Clone, Default)]
pub struct PanelTree {
    current: Arc<PanelSnapshot>,
}

impl PanelTree {
    /// An empty tree (nothing rendered yet).
    pub fn new() -> Self {
        Self::default()
    }

    /// The current snapshot.
    pub fn snapshot(&self) -> Arc<PanelSnapshot> {
        self.current.clone()
    }

    /// `ui.render`: replaces the whole view.
    pub fn render(&mut self, view: View) -> Result<(Arc<PanelSnapshot>, Changes), TreeError> {
        check_strings(view.nodes.iter().map(|n| &n.widget))?;
        if view.nodes.len() > MAX_NODES {
            return Err(too_many(view.nodes.len()));
        }
        let mut nodes = HashMap::with_capacity(view.nodes.len());
        for node in view.nodes {
            let id = node.id;
            if nodes.insert(id, Arc::new(node)).is_some() {
                return Err(invalid(format!("node {id} appears twice")));
            }
        }
        let mut changes = self.commit(view.root, nodes, HashMap::new())?;
        changes.full = true;
        Ok((self.snapshot(), changes))
    }

    /// `ui.apply-patches`: validated and applied atomically.
    pub fn apply(
        &mut self,
        patches: Vec<Patch>,
    ) -> Result<(Arc<PanelSnapshot>, Changes), TreeError> {
        check_strings(patches.iter().filter_map(|p| match p {
            Patch::Upsert(n) => Some(&n.widget),
            _ => None,
        }))?;
        if patches.len() > 2 * MAX_NODES {
            return Err(TreeError::TooLarge(format!(
                "{} patches in one call",
                patches.len()
            )));
        }
        let mut root = self.current.root;
        let mut nodes = self.current.nodes.clone();
        let mut resets: HashMap<NodeId, Option<u64>> = HashMap::new();
        for patch in patches {
            match patch {
                Patch::Upsert(node) => {
                    nodes.insert(node.id, Arc::new(node));
                }
                Patch::Remove(id) => {
                    if nodes.remove(&id).is_none() {
                        return Err(invalid(format!("remove of unknown node {id}")));
                    }
                }
                Patch::SetRoot(id) => root = id,
                Patch::RowsChanged((id, count)) => {
                    let Some(node) = nodes.get_mut(&id) else {
                        return Err(invalid(format!("rows-changed for unknown node {id}")));
                    };
                    let Widget::Table(props) = &node.widget else {
                        return Err(invalid(format!("rows-changed for node {id}, not a table")));
                    };
                    let count = count.or(props.row_count);
                    if let Widget::Table(props) = &mut Arc::make_mut(node).widget {
                        props.row_count = count;
                    }
                    resets.insert(id, count);
                }
            }
        }
        let changes = self.commit(root, nodes, resets)?;
        Ok((self.snapshot(), changes))
    }

    /// Validates `nodes` from `root`, drops unreachable nodes, computes the
    /// changes against the current snapshot and swaps it in.
    fn commit(
        &mut self,
        root: NodeId,
        mut nodes: HashMap<NodeId, Arc<Node>>,
        resets: HashMap<NodeId, Option<u64>>,
    ) -> Result<Changes, TreeError> {
        let reachable = validate(root, &nodes)?;
        nodes.retain(|id, _| reachable.contains(id));
        let old = &self.current;
        let mut changes = Changes::default();
        for (id, node) in &nodes {
            let before = old.nodes.get(id);
            if before.is_some_and(|b| Arc::ptr_eq(b, node) || **b == **node) {
                continue;
            }
            changes.dirty.push(*id);
            let before = before.map(|b| &b.widget);
            match (&node.widget, before) {
                (Widget::Input(new), Some(Widget::Input(old))) if new.value == old.value => {}
                (Widget::Input(_), _) => changes.inputs.push(*id),
                (Widget::Table(new), Some(Widget::Table(old))) => {
                    let reset = new.generation != old.generation || new.columns != old.columns;
                    if reset || new.row_count != old.row_count {
                        changes.tables.push(TableChange {
                            table: *id,
                            row_count: new.row_count,
                            reset,
                        });
                    }
                }
                _ => {}
            }
        }
        for (id, row_count) in resets {
            if !nodes.contains_key(&id) {
                continue;
            }
            match changes.tables.iter_mut().find(|t| t.table == id) {
                Some(t) => t.reset = true,
                None => changes.tables.push(TableChange {
                    table: id,
                    row_count,
                    reset: true,
                }),
            }
        }
        changes.removed = old
            .nodes
            .keys()
            .filter(|id| !nodes.contains_key(id))
            .copied()
            .collect();
        changes.dirty.sort_unstable();
        changes.removed.sort_unstable();
        changes.inputs.sort_unstable();
        changes.tables.sort_by_key(|t| t.table);
        self.current = Arc::new(PanelSnapshot {
            root,
            nodes,
            revision: old.revision + 1,
        });
        Ok(changes)
    }
}

fn too_many(count: usize) -> TreeError {
    TreeError::TooLarge(format!("{count} nodes (limit {MAX_NODES})"))
}

/// Walks the tree from `root`; returns the reachable ids.
fn validate(
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

fn check_strings<'a>(widgets: impl Iterator<Item = &'a Widget>) -> Result<(), TreeError> {
    let bytes: usize = widgets.map(widget_bytes).sum();
    if bytes > MAX_STRING_BYTES {
        return Err(TreeError::TooLarge(format!(
            "{bytes} bytes of text (limit {MAX_STRING_BYTES})"
        )));
    }
    Ok(())
}

/// Bytes of plugin-supplied text in one widget.
pub fn widget_bytes(w: &Widget) -> usize {
    fn opt(s: &Option<String>) -> usize {
        s.as_ref().map_or(0, String::len)
    }
    match w {
        Widget::Stack(_) | Widget::Split(_) | Widget::Scroll | Widget::Divider | Widget::Spacer => {
            0
        }
        Widget::Tabs(t) => t.titles.iter().map(String::len).sum(),
        Widget::Text(t) => t.text.len(),
        Widget::Badge(b) => b.text.len(),
        Widget::Button(b) => b.label.len() + opt(&b.icon),
        Widget::Input(i) => i.value.len() + i.placeholder.len() + opt(&i.language),
        Widget::Select(s) => s.options.iter().map(String::len).sum(),
        Widget::Checkbox(c) => c.label.len(),
        Widget::KeyValues(kv) => kv.iter().map(|(k, v)| k.len() + v.len()).sum(),
        Widget::Table(t) => t.columns.iter().map(|c| c.title.len()).sum(),
        Widget::Tree(items) => items
            .iter()
            .map(|i| i.key.len() + i.label.len() + opt(&i.detail) + opt(&i.icon))
            .sum(),
        Widget::Code(c) => c.text.len() + opt(&c.language),
        Widget::Json(j) | Widget::Spinner(j) => j.len(),
        Widget::EmptyState((a, b)) => a.len() + b.len(),
    }
}

#[cfg(test)]
#[path = "ui_tree_tests.rs"]
mod tests;
