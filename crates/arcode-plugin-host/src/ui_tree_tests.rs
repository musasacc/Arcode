use super::*;
use crate::bindings::ui::{
    Axis, Column, InputProps, SplitProps, StackProps, TableProps, TabsProps, TextProps, Tone,
};

fn node(id: NodeId, widget: Widget, children: &[NodeId]) -> Node {
    Node {
        id,
        widget,
        children: children.to_vec(),
        grow: false,
    }
}

fn stack() -> Widget {
    Widget::Stack(StackProps {
        axis: Axis::Vertical,
        gap: 0,
        padding: 0,
    })
}

fn text(t: &str) -> Widget {
    Widget::Text(TextProps {
        text: t.into(),
        tone: Tone::Normal,
        mono: false,
        bold: false,
        wrap: false,
    })
}

fn input(value: &str) -> Widget {
    Widget::Input(InputProps {
        value: value.into(),
        placeholder: String::new(),
        multiline: false,
        language: None,
        submit_on_enter: false,
        notify_changes: false,
    })
}

fn table(rows: Option<u64>, generation: u32) -> Widget {
    Widget::Table(TableProps {
        columns: vec![Column {
            title: "a".into(),
            width: None,
            numeric: false,
            sortable: true,
        }],
        row_count: rows,
        generation,
    })
}

fn view(root: NodeId, nodes: Vec<Node>) -> View {
    View { root, nodes }
}

/// root(1) -> [text(2), input(3), table(4)]
fn basic() -> PanelTree {
    let mut t = PanelTree::new();
    t.render(view(
        1,
        vec![
            node(1, stack(), &[2, 3, 4]),
            node(2, text("hello"), &[]),
            node(3, input("q"), &[]),
            node(4, table(Some(10), 1), &[]),
        ],
    ))
    .unwrap();
    t
}

fn invalid_msg<T: std::fmt::Debug>(r: Result<T, TreeError>) -> String {
    match r {
        Err(TreeError::Invalid(m)) => m,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn render_builds_a_snapshot_and_reports_everything_dirty() {
    let mut t = PanelTree::new();
    let (snap, changes) = t
        .render(view(
            1,
            vec![node(1, stack(), &[2]), node(2, input("x"), &[])],
        ))
        .unwrap();
    assert!(changes.full);
    assert_eq!(changes.dirty, [1, 2]);
    assert_eq!(changes.inputs, [2]);
    assert_eq!(snap.revision, 1);
    assert_eq!(snap.children(1).count(), 1);
    assert_eq!(snap.root_node().map(|n| n.id), Some(1));
}

#[test]
fn dangling_ids_cycles_and_two_parents_are_rejected() {
    let mut t = PanelTree::new();
    let m = invalid_msg(t.render(view(1, vec![node(1, stack(), &[2])])));
    assert!(m.contains("missing node 2"), "{m}");
    let m = invalid_msg(t.render(view(9, vec![node(1, stack(), &[])])));
    assert!(m.contains("root 9"), "{m}");
    let cycle = vec![node(1, stack(), &[2]), node(2, stack(), &[1])];
    let m = invalid_msg(t.render(view(1, cycle)));
    assert!(m.contains("cycle"), "{m}");
    let self_loop = vec![node(1, stack(), &[1])];
    assert!(invalid_msg(t.render(view(1, self_loop))).contains("cycle"));
    let shared = vec![
        node(1, stack(), &[2, 3]),
        node(2, stack(), &[4]),
        node(3, stack(), &[4]),
        node(4, text("x"), &[]),
    ];
    assert!(invalid_msg(t.render(view(1, shared))).contains("two parents"));
    let dup = vec![node(1, stack(), &[]), node(1, stack(), &[])];
    assert!(invalid_msg(t.render(view(1, dup))).contains("twice"));
    assert_eq!(t.snapshot().revision, 0, "nothing was accepted");
}

#[test]
fn child_counts_follow_the_widget_schema() {
    let split = |ratio: f32| {
        Widget::Split(SplitProps {
            axis: Axis::Horizontal,
            ratio,
        })
    };
    let tabs = Widget::Tabs(TabsProps {
        titles: vec!["a".into(), "b".into()],
        active: 0,
    });
    let leaves = |n: u32| {
        (2..2 + n)
            .map(|i| node(i, text("x"), &[]))
            .collect::<Vec<_>>()
    };
    let mut t = PanelTree::new();
    let with = |w: Widget, n: u32| {
        let mut nodes = leaves(n);
        nodes.push(node(1, w, &(2..2 + n).collect::<Vec<_>>()));
        view(1, nodes)
    };
    assert!(t.render(with(split(0.5), 2)).is_ok());
    assert!(invalid_msg(t.render(with(split(0.5), 1))).contains("split"));
    assert!(invalid_msg(t.render(with(split(0.5), 3))).contains("split"));
    assert!(invalid_msg(t.render(with(split(f32::NAN), 2))).contains("ratio"));
    assert!(t.render(with(tabs.clone(), 2)).is_ok());
    assert!(invalid_msg(t.render(with(tabs, 3))).contains("tabs"));
    assert!(t.render(with(Widget::Scroll, 1)).is_ok());
    assert!(invalid_msg(t.render(with(Widget::Scroll, 0))).contains("scroll"));
    assert!(invalid_msg(t.render(with(text("leaf"), 1))).contains("leaf"));
    assert!(t.render(with(stack(), 0)).is_ok());
}

#[test]
fn caps_on_nodes_depth_and_strings() {
    let mut t = PanelTree::new();
    let wide = |n: u32| {
        let mut nodes: Vec<Node> = (2..=n).map(|i| node(i, text(""), &[])).collect();
        nodes.push(node(1, stack(), &(2..=n).collect::<Vec<_>>()));
        view(1, nodes)
    };
    let started = std::time::Instant::now();
    assert!(t.render(wide(MAX_NODES as u32)).is_ok());
    let rendered = started.elapsed();
    let started = std::time::Instant::now();
    assert!(
        t.apply(vec![Patch::Upsert(node(2, text("x"), &[]))])
            .is_ok()
    );
    let patched = started.elapsed();
    eprintln!("PanelTree, 20k nodes: render {rendered:?}, one-node patch {patched:?}");
    assert!(matches!(
        t.render(wide(MAX_NODES as u32 + 1)),
        Err(TreeError::TooLarge(_))
    ));
    let deep = |levels: u32| {
        let mut nodes: Vec<Node> = (1..levels).map(|i| node(i, stack(), &[i + 1])).collect();
        nodes.push(node(levels, text("bottom"), &[]));
        view(1, nodes)
    };
    assert!(t.render(deep(MAX_DEPTH as u32)).is_ok());
    assert!(invalid_msg(t.render(deep(MAX_DEPTH as u32 + 1))).contains("deeper"));
    let big = "x".repeat(MAX_STRING_BYTES / 2 + 1);
    let two = view(
        1,
        vec![
            node(1, stack(), &[2, 3]),
            node(2, text(&big), &[]),
            node(3, text(&big), &[]),
        ],
    );
    assert!(matches!(t.render(two), Err(TreeError::TooLarge(_))));
    let r = t.apply(vec![
        Patch::Upsert(node(2, text(&big), &[])),
        Patch::Upsert(node(3, text(&big), &[])),
    ]);
    assert!(matches!(r, Err(TreeError::TooLarge(_))));
    assert_eq!(
        UiError::from(TreeError::TooLarge(String::new())),
        UiError::TooLarge
    );
}

#[test]
fn patches_apply_atomically() {
    let mut t = basic();
    let before = t.snapshot();
    // The first patch is fine, the second dangles: nothing changes.
    let r = t.apply(vec![
        Patch::Upsert(node(2, text("changed"), &[])),
        Patch::Upsert(node(1, stack(), &[2, 3, 4, 99])),
    ]);
    assert!(invalid_msg(r).contains("missing node 99"));
    assert_eq!(t.snapshot(), before);
    assert!(invalid_msg(t.apply(vec![Patch::Remove(77)])).contains("unknown node 77"));
    assert!(invalid_msg(t.apply(vec![Patch::RowsChanged((2, None))])).contains("not a table"));
    assert!(invalid_msg(t.apply(vec![Patch::SetRoot(50)])).contains("root 50"));
    assert_eq!(t.snapshot(), before);
}

#[test]
fn a_one_node_patch_marks_one_node_dirty() {
    let mut t = basic();
    let before = t.snapshot();
    let (snap, changes) = t
        .apply(vec![Patch::Upsert(node(2, text("bye"), &[]))])
        .unwrap();
    assert!(!changes.full);
    assert_eq!(changes.dirty, [2]);
    assert!(changes.removed.is_empty() && changes.inputs.is_empty() && changes.tables.is_empty());
    assert_eq!(snap.revision, before.revision + 1);
    // Untouched nodes are shared with the previous snapshot.
    assert!(Arc::ptr_eq(&snap.nodes[&3], &before.nodes[&3]));
    // An identical upsert is not a change.
    let (_, changes) = t
        .apply(vec![Patch::Upsert(node(2, text("bye"), &[]))])
        .unwrap();
    assert!(changes.dirty.is_empty());
}

#[test]
fn remove_takes_unreferenced_descendants_along() {
    let mut t = PanelTree::new();
    t.render(view(
        1,
        vec![
            node(1, stack(), &[2, 5]),
            node(2, stack(), &[3, 4]),
            node(3, text("a"), &[]),
            node(4, text("b"), &[]),
            node(5, text("c"), &[]),
        ],
    ))
    .unwrap();
    // Remove 2 but move its child 4 under the root in the same batch.
    let (snap, changes) = t
        .apply(vec![
            Patch::Upsert(node(1, stack(), &[5, 4])),
            Patch::Remove(2),
        ])
        .unwrap();
    assert_eq!(changes.removed, [2, 3]);
    assert_eq!(changes.dirty, [1]);
    let mut ids: Vec<_> = snap.nodes.keys().copied().collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 4, 5]);
    // A detached upsert is dropped as unreachable.
    let (snap, _) = t
        .apply(vec![Patch::Upsert(node(9, text("orphan"), &[]))])
        .unwrap();
    assert!(snap.node(9).is_none());
}

#[test]
fn set_root_replaces_the_tree() {
    let mut t = basic();
    let (snap, changes) = t
        .apply(vec![
            Patch::Upsert(node(10, text("solo"), &[])),
            Patch::SetRoot(10),
        ])
        .unwrap();
    assert_eq!(snap.root, 10);
    assert_eq!(snap.nodes.len(), 1);
    assert_eq!(changes.removed, [1, 2, 3, 4]);
}

#[test]
fn inputs_are_overwritten_only_when_their_value_changes() {
    let mut t = basic();
    let mut placeholder_only = input("q");
    if let Widget::Input(p) = &mut placeholder_only {
        p.placeholder = "type".into();
    }
    let (_, c) = t
        .apply(vec![Patch::Upsert(node(3, placeholder_only, &[]))])
        .unwrap();
    assert_eq!(
        (c.dirty.as_slice(), c.inputs.as_slice()),
        (&[3][..], &[][..])
    );
    let (_, c) = t
        .apply(vec![Patch::Upsert(node(3, input("new"), &[]))])
        .unwrap();
    assert_eq!(c.inputs, [3]);
    // A full render with the same value keeps the user's text too.
    let mut again = basic();
    let (_, c) = again
        .render(view(
            1,
            vec![
                node(1, stack(), &[2, 3, 4]),
                node(2, text("hello"), &[]),
                node(3, input("q"), &[]),
                node(4, table(Some(10), 1), &[]),
            ],
        ))
        .unwrap();
    assert!(c.full && c.dirty.is_empty() && c.inputs.is_empty());
}

#[test]
fn table_changes_reset_or_resize_the_row_cache() {
    let mut t = basic();
    let (_, c) = t
        .apply(vec![Patch::Upsert(node(4, table(Some(20), 1), &[]))])
        .unwrap();
    assert_eq!(
        c.tables,
        [TableChange {
            table: 4,
            row_count: Some(20),
            reset: false
        }]
    );
    let (_, c) = t
        .apply(vec![Patch::Upsert(node(4, table(Some(20), 2), &[]))])
        .unwrap();
    assert_eq!(
        c.tables,
        [TableChange {
            table: 4,
            row_count: Some(20),
            reset: true
        }]
    );
    // rows-changed keeps the count unless one is given, and always resets.
    let (snap, c) = t.apply(vec![Patch::RowsChanged((4, None))]).unwrap();
    assert_eq!(
        c.tables,
        [TableChange {
            table: 4,
            row_count: Some(20),
            reset: true
        }]
    );
    assert!(c.dirty.is_empty());
    assert_eq!(snap.node(4).map(|n| &n.widget), Some(&table(Some(20), 2)));
    let (snap, c) = t.apply(vec![Patch::RowsChanged((4, Some(5)))]).unwrap();
    assert_eq!(
        c.tables,
        [TableChange {
            table: 4,
            row_count: Some(5),
            reset: true
        }]
    );
    assert_eq!(c.dirty, [4]);
    assert_eq!(snap.node(4).map(|n| &n.widget), Some(&table(Some(5), 2)));
}
