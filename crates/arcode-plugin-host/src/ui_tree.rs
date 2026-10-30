//! Panel trees on the host side (§7 "Pipeline", step 3). No GPUI.
//!
//! A [`PanelTree`] holds the current view of one panel. `ui.render` replaces
//! it and `ui.apply-patches` edits it; both run on the plugin thread, are
//! validated as a whole and applied atomically: on error nothing changes.
//! Validation rejects dangling ids, nodes with two parents, cycles, wrong
//! child counts, non-finite split ratios and cap violations (§5 host caps:
//! nodes, depth, and [`MAX_STRING_BYTES`] of text and list elements per
//! panel).
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

#[path = "ui_tree_check.rs"]
mod check;

pub use check::widget_bytes;
use check::{check_strings, too_many, validate};

/// Nodes per panel.
pub const MAX_NODES: usize = 20_000;
/// Levels below and including the root.
pub const MAX_DEPTH: usize = 64;
/// Bytes of plugin-supplied data ([`widget_bytes`]) per panel. §5 says "per
/// `render` or `apply-patches`"; the cap applies to the panel's whole tree
/// after every change (which bounds each call too), so re-sending new nodes
/// cannot make the host keep more.
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
        let mut changes = self.commit(view.root, nodes, HashSet::new())?;
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
        let mut resets: HashSet<NodeId> = HashSet::new();
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
                    resets.insert(id);
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
        resets: HashSet<NodeId>,
    ) -> Result<Changes, TreeError> {
        let reachable = validate(root, &nodes)?;
        nodes.retain(|id, _| reachable.contains(id));
        check_strings(nodes.values().map(|n| &n.widget))?;
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
        for id in resets {
            // The final node decides the count, so a `rows-changed` followed
            // by an `upsert` of the same table agrees with the snapshot.
            let Some(Widget::Table(props)) = nodes.get(&id).map(|n| &n.widget) else {
                continue;
            };
            let row_count = props.row_count;
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

#[cfg(test)]
#[path = "ui_tree_tests.rs"]
mod tests;
