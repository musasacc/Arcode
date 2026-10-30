//! Flattening a recursive [`Element`] tree into the WIT node arena.
//!
//! WIT has no recursive types, so a view crosses the boundary as a flat list
//! of nodes whose children are ids. Ids come from each node's *key path*
//! (the keys from the root down to it) and are kept in an [`IdMap`] across
//! renders: the same key path always gets the same id while it stays in the
//! tree, and new paths get fresh ids, so the host never confuses an old
//! node with a new one.

use std::collections::{BTreeMap, HashMap};

use super::Element;
use super::{Node, NodeId, View};

/// Separates key-path segments; cannot appear in a valid UTF-8 key by accident.
const SEP: char = '\u{1f}';

/// Why a tree could not be flattened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlattenError {
    /// Two siblings share an explicit key. Carries the key.
    DuplicateKey(String),
    /// The panel used up the `u32` id space.
    OutOfIds,
}

impl std::fmt::Display for FlattenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateKey(key) => write!(f, "duplicate sibling key `{key}`"),
            Self::OutOfIds => f.write_str("panel ran out of node ids"),
        }
    }
}

impl std::error::Error for FlattenError {}

/// Stable key-path to node-id assignment for one panel.
#[derive(Debug, Clone, Default)]
pub struct IdMap {
    next: NodeId,
    by_path: HashMap<String, NodeId>,
}

impl IdMap {
    /// An empty map; the first id handed out is 1.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live key paths.
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    /// True when no path has an id.
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// The id of the node with explicit key `key` (anywhere in the tree).
    /// If several nodes share the key, which one is returned is unspecified.
    pub fn find_key(&self, key: &str) -> Option<NodeId> {
        let last = format!("k:{key}");
        self.by_path
            .iter()
            .find(|(path, _)| path.rsplit(SEP).next() == Some(last.as_str()))
            .map(|(_, &id)| id)
    }
}

/// A flattened view: the root id and every node by id.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlatView {
    /// Id of the root node.
    pub root: NodeId,
    /// Every node of the view, ordered by id.
    pub nodes: BTreeMap<NodeId, Node>,
}

impl FlatView {
    /// Converts to the WIT `view` record (nodes ordered by id).
    pub fn to_view(&self) -> View {
        View {
            root: self.root,
            nodes: self.nodes.values().cloned().collect(),
        }
    }

    /// Builds a flat view from a WIT `view` record.
    pub fn from_view(view: &View) -> Self {
        Self {
            root: view.root,
            nodes: view.nodes.iter().map(|n| (n.id, n.clone())).collect(),
        }
    }
}

/// Flattens `root`, reusing ids from `ids` for key paths seen before.
///
/// On success `ids` keeps only the paths present in this tree, so it does
/// not grow with churn. On error `ids` is left unchanged.
pub fn flatten(root: &Element, ids: &mut IdMap) -> Result<FlatView, FlattenError> {
    let mut walk = Walk {
        old: ids,
        next: ids.next,
        live: HashMap::new(),
        nodes: BTreeMap::new(),
    };
    let root_id = walk.visit(root, root_segment(root))?;
    let (next, live, nodes) = (walk.next, walk.live, walk.nodes);
    ids.next = next;
    ids.by_path = live;
    Ok(FlatView {
        root: root_id,
        nodes,
    })
}

fn root_segment(root: &Element) -> String {
    match &root.key {
        Some(key) => format!("k:{key}"),
        None => "i:0".to_owned(),
    }
}

struct Walk<'a> {
    old: &'a IdMap,
    next: NodeId,
    live: HashMap<String, NodeId>,
    nodes: BTreeMap<NodeId, Node>,
}

impl Walk<'_> {
    fn id_for(&mut self, path: &str) -> Result<NodeId, FlattenError> {
        if let Some(&id) = self.old.by_path.get(path) {
            return Ok(id);
        }
        self.next = self.next.checked_add(1).ok_or(FlattenError::OutOfIds)?;
        Ok(self.next)
    }

    fn visit(&mut self, element: &Element, path: String) -> Result<NodeId, FlattenError> {
        let id = self.id_for(&path)?;
        self.live.insert(path.clone(), id);
        let mut children = Vec::with_capacity(element.children.len());
        let mut seen_keys = std::collections::HashSet::new();
        for (index, child) in element.children.iter().enumerate() {
            let segment = match &child.key {
                Some(key) => {
                    if !seen_keys.insert(key.as_str()) {
                        return Err(FlattenError::DuplicateKey(key.clone()));
                    }
                    format!("k:{key}")
                }
                None => format!("i:{index}"),
            };
            children.push(self.visit(child, format!("{path}{SEP}{segment}"))?);
        }
        self.nodes.insert(
            id,
            Node {
                id,
                widget: element.widget.clone(),
                children,
                grow: element.grow,
            },
        );
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::builder::*;
    use super::*;

    #[test]
    fn flattens_depth_first_with_child_ids() {
        let mut ids = IdMap::new();
        let flat = flatten(
            &vstack([text("a"), hstack([text("b"), text("c")])]),
            &mut ids,
        )
        .unwrap();
        assert_eq!(flat.nodes.len(), 5);
        let root = &flat.nodes[&flat.root];
        assert_eq!(root.children.len(), 2);
        let row = &flat.nodes[&root.children[1]];
        assert_eq!(row.children.len(), 2);
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn same_tree_gets_same_ids() {
        let mut ids = IdMap::new();
        let tree = vstack([text("a").key("x"), text("b")]);
        let first = flatten(&tree, &mut ids).unwrap();
        let second = flatten(&tree, &mut ids).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn keyed_nodes_keep_ids_when_siblings_are_inserted() {
        let mut ids = IdMap::new();
        let before = flatten(&vstack([text("s").key("status")]), &mut ids).unwrap();
        let after = flatten(
            &vstack([text("new").key("banner"), text("s").key("status")]),
            &mut ids,
        )
        .unwrap();
        let status_before = before.nodes[&before.root].children[0];
        let status_after = after.nodes[&after.root].children[1];
        assert_eq!(status_before, status_after);
    }

    #[test]
    fn removed_paths_are_forgotten_and_new_ids_are_fresh() {
        let mut ids = IdMap::new();
        let a = flatten(&vstack([text("a"), text("b")]), &mut ids).unwrap();
        let b = flatten(&vstack([text("a")]), &mut ids).unwrap();
        assert_eq!(ids.len(), 2);
        let c = flatten(&vstack([text("a"), text("b")]), &mut ids).unwrap();
        let old_b = a.nodes[&a.root].children[1];
        let new_b = c.nodes[&c.root].children[1];
        assert_ne!(old_b, new_b, "a re-added node must not reuse a removed id");
        assert_eq!(b.root, c.root);
    }

    #[test]
    fn duplicate_sibling_keys_are_rejected_without_touching_ids() {
        let mut ids = IdMap::new();
        flatten(&vstack([text("a")]), &mut ids).unwrap();
        let err = flatten(&vstack([text("a").key("k"), text("b").key("k")]), &mut ids).unwrap_err();
        assert_eq!(err, FlattenError::DuplicateKey("k".into()));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn same_key_under_different_parents_is_fine() {
        let mut ids = IdMap::new();
        let flat = flatten(
            &vstack([vstack([text("a").key("k")]), vstack([text("b").key("k")])]),
            &mut ids,
        )
        .unwrap();
        assert_eq!(flat.nodes.len(), 5);
    }

    #[test]
    fn find_key_locates_nested_keyed_nodes() {
        let mut ids = IdMap::new();
        let flat = flatten(&vstack([hstack([text("t").key("table")])]), &mut ids).unwrap();
        let row = flat.nodes[&flat.root].children[0];
        assert_eq!(ids.find_key("table"), Some(flat.nodes[&row].children[0]));
        assert_eq!(ids.find_key("nope"), None);
    }

    #[test]
    fn view_round_trip() {
        let mut ids = IdMap::new();
        let flat = flatten(&vstack([text("a"), spacer()]), &mut ids).unwrap();
        assert_eq!(FlatView::from_view(&flat.to_view()), flat);
    }
}
