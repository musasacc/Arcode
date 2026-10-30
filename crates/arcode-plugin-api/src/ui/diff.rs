//! Diffing two flattened views into WIT patches, inside the guest.
//!
//! Diffing costs plugin CPU, never UI-thread time (docs/PLUGINS.md §7).
//! The patch order is: upserts (ascending id), removes of the top-most
//! nodes that disappeared, then `set-root` if the root changed. The host
//! applies a patch list atomically, so intermediate states never matter.

use std::collections::BTreeSet;

use super::flatten::FlatView;
use super::{NodeId, Patch};

/// Patches that turn `prev` into `next`. With no previous view the caller
/// should send a full `render` instead; this returns an upsert per node.
pub fn diff(prev: Option<&FlatView>, next: &FlatView) -> Vec<Patch> {
    let empty = FlatView::default();
    let prev = prev.unwrap_or(&empty);
    let mut patches = Vec::new();
    for (id, node) in &next.nodes {
        if prev.nodes.get(id) != Some(node) {
            patches.push(Patch::Upsert(node.clone()));
        }
    }
    let removed: BTreeSet<NodeId> = prev
        .nodes
        .keys()
        .filter(|id| !next.nodes.contains_key(id))
        .copied()
        .collect();
    // A removed node whose parent was removed too goes away with that parent.
    let mut covered = BTreeSet::new();
    for id in &removed {
        if let Some(node) = prev.nodes.get(id) {
            covered.extend(
                node.children
                    .iter()
                    .copied()
                    .filter(|c| removed.contains(c)),
            );
        }
    }
    patches.extend(removed.difference(&covered).map(|&id| Patch::Remove(id)));
    if prev.root != next.root {
        patches.push(Patch::SetRoot(next.root));
    }
    patches
}

/// Applies `patches` to `view` with the semantics the host implements
/// (minus validation and caps, which live in the host's `PanelTree`).
/// Used by [`crate::testing`] and the SDK's own tests.
pub fn apply(view: &mut FlatView, patches: &[Patch]) {
    for patch in patches {
        match patch {
            Patch::Upsert(node) => {
                view.nodes.insert(node.id, node.clone());
            }
            Patch::Remove(id) => {
                view.nodes.remove(id);
            }
            Patch::SetRoot(id) => view.root = *id,
            Patch::RowsChanged(_) => {}
        }
    }
    // `remove` drops the node; its descendants go only if nothing reachable
    // references them any more (the host's `PanelTree` does the same).
    collect_garbage(view);
}

fn collect_garbage(view: &mut FlatView) {
    let mut reachable = BTreeSet::new();
    let mut stack = vec![view.root];
    while let Some(id) = stack.pop() {
        if reachable.insert(id)
            && let Some(node) = view.nodes.get(&id)
        {
            stack.extend(node.children.iter().copied());
        }
    }
    view.nodes.retain(|id, _| reachable.contains(id));
}

#[cfg(test)]
mod tests {
    use super::super::builder::*;
    use super::super::flatten::{IdMap, flatten};
    use super::*;
    use crate::ui::Tone;

    fn render_pair(a: &Element, b: &Element) -> (FlatView, FlatView, Vec<Patch>) {
        let mut ids = IdMap::new();
        let first = flatten(a, &mut ids).unwrap();
        let second = flatten(b, &mut ids).unwrap();
        let patches = diff(Some(&first), &second);
        (first, second, patches)
    }

    fn panel(status: &str, rows: usize) -> Element {
        vstack([
            hstack([badge(status, Tone::Success).key("status"), spacer()]).key("header"),
            vstack((0..rows).map(|i| text(format!("row {i}")).key(format!("r{i}")))).key("body"),
        ])
    }

    #[test]
    fn identical_trees_produce_no_patches() {
        let (_, _, patches) = render_pair(&panel("200", 3), &panel("200", 3));
        assert!(patches.is_empty());
    }

    #[test]
    fn a_status_change_is_one_upsert() {
        let (_, next, patches) = render_pair(&panel("200", 3), &panel("404", 3));
        assert_eq!(patches.len(), 1);
        let Patch::Upsert(node) = &patches[0] else {
            panic!("expected upsert, got {patches:?}")
        };
        assert_eq!(Some(node), next.nodes.get(&node.id));
    }

    #[test]
    fn removing_a_subtree_sends_one_remove_and_a_parent_upsert() {
        let a = vstack([
            text("keep").key("k"),
            vstack([text("x"), text("y")]).key("gone"),
        ]);
        let b = vstack([text("keep").key("k")]);
        let (prev, next, patches) = render_pair(&a, &b);
        let removes: Vec<_> = patches
            .iter()
            .filter(|p| matches!(p, Patch::Remove(_)))
            .collect();
        assert_eq!(removes.len(), 1, "{patches:?}");
        assert!(
            patches
                .iter()
                .any(|p| matches!(p, Patch::Upsert(n) if n.id == next.root))
        );
        let mut applied = prev.clone();
        apply(&mut applied, &patches);
        assert_eq!(applied, next);
    }

    #[test]
    fn root_change_sets_root() {
        let (prev, next, patches) = render_pair(&text("a").key("one"), &text("b").key("two"));
        assert_eq!(patches.last(), Some(&Patch::SetRoot(next.root)));
        let mut applied = prev;
        apply(&mut applied, &patches);
        assert_eq!(applied, next);
    }

    #[test]
    fn diff_from_nothing_upserts_everything() {
        let mut ids = IdMap::new();
        let flat = flatten(&panel("ok", 2), &mut ids).unwrap();
        let patches = diff(None, &flat);
        assert_eq!(
            patches
                .iter()
                .filter(|p| matches!(p, Patch::Upsert(_)))
                .count(),
            flat.nodes.len()
        );
    }

    #[test]
    fn apply_after_diff_equals_fresh_render_for_a_sequence() {
        let trees = [
            panel("a", 0),
            panel("b", 5),
            panel("b", 2),
            text("solo"),
            panel("c", 7),
            panel("c", 7),
        ];
        let mut ids = IdMap::new();
        let mut host_side = FlatView::default();
        let mut prev: Option<FlatView> = None;
        for tree in &trees {
            let next = flatten(tree, &mut ids).unwrap();
            apply(&mut host_side, &diff(prev.as_ref(), &next));
            assert_eq!(host_side, next);
            prev = Some(next);
        }
    }
}
