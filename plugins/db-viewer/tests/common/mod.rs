//! Helpers for the native plugin tests: a project copied from the
//! fixtures, a plugin rooted there, and readers for the fake host's views.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use arcode_plugin::Plugin;
use arcode_plugin::testing::{self, FakePanel};
use arcode_plugin::ui::{Event, EventKind, NodeId, PanelId, Tone, TreeItem, Widget};
use db_viewer::DbViewer;
use db_viewer::view::DbView;

/// `tests/fixtures`.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A temp project: `sample.db`, `wal.db` + `wal.db-wal`, and
/// `data/nested.sqlite3` (a copy of the sample).
pub fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in ["sample.db", "wal.db", "wal.db-wal"] {
        std::fs::copy(fixtures().join(name), dir.path().join(name)).expect(name);
    }
    std::fs::create_dir(dir.path().join("data")).expect("data dir");
    std::fs::copy(
        fixtures().join("sample.db"),
        dir.path().join("data/nested.sqlite3"),
    )
    .expect("nested");
    dir
}

/// A fresh fake host and a plugin rooted at `root`.
pub fn plugin(root: &Path) -> DbViewer {
    testing::reset();
    DbViewer::with_root(root)
}

/// Opens `path` with the `sqlite` opener in `panel`, as the host would.
pub fn open(p: &mut DbViewer, panel: PanelId, path: &str) {
    add_panel(panel, "sqlite");
    let args = serde_json::json!({ "path": path }).to_string();
    p.open_view(panel, "sqlite", args.into())
        .expect("open-view");
}

/// Registers a host-created panel with the fake host.
pub fn add_panel(panel: PanelId, contribution: &str) {
    testing::with(|h| {
        h.panels.insert(
            panel,
            FakePanel {
                contribution: contribution.into(),
                ..FakePanel::default()
            },
        )
    });
}

/// The database view in `panel`.
pub fn db(p: &mut DbViewer, panel: PanelId) -> &mut DbView {
    p.db_view(panel).expect("a database view")
}

/// Node id of `key` in the database view of `panel`.
pub fn node(p: &mut DbViewer, panel: PanelId, key: &str) -> NodeId {
    db(p, panel)
        .node(key)
        .unwrap_or_else(|| panic!("no node `{key}`"))
}

/// The widget of node `id` as the fake host holds it.
pub fn widget(panel: PanelId, id: NodeId) -> Widget {
    testing::with(|h| h.panels[&panel].view.nodes[&id].widget.clone())
}

/// Every plain string shown in `panel` (texts, badges, empty states).
pub fn texts(panel: PanelId) -> Vec<String> {
    testing::with(|h| {
        h.panels[&panel]
            .view
            .nodes
            .values()
            .filter_map(|n| match &n.widget {
                Widget::Text(t) => Some(t.text.clone()),
                Widget::Badge(b) => Some(b.text.clone()),
                Widget::EmptyState((title, hint)) => Some(format!("{title}: {hint}")),
                Widget::Spinner(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    })
}

/// The text and tone of the text node keyed `key`.
pub fn text_of(p: &mut DbViewer, panel: PanelId, key: &str) -> (String, Tone) {
    match widget(panel, node(p, panel, key)) {
        Widget::Text(t) => (t.text, t.tone),
        other => panic!("`{key}` is not text: {other:?}"),
    }
}

/// True when some string in `panel` contains `needle`.
pub fn shows(panel: PanelId, needle: &str) -> bool {
    texts(panel).iter().any(|t| t.contains(needle))
}

/// Items of the tree keyed `key`.
pub fn tree(p: &mut DbViewer, panel: PanelId, key: &str) -> Vec<TreeItem> {
    match widget(panel, node(p, panel, key)) {
        Widget::Tree(items) => items,
        other => panic!("`{key}` is not a tree: {other:?}"),
    }
}

/// `(row_count, generation)` of the table keyed `key`.
pub fn table(p: &mut DbViewer, panel: PanelId, key: &str) -> (Option<u64>, u32, Vec<String>) {
    match widget(panel, node(p, panel, key)) {
        Widget::Table(t) => (
            t.row_count,
            t.generation,
            t.columns.into_iter().map(|c| c.title).collect(),
        ),
        other => panic!("`{key}` is not a table: {other:?}"),
    }
}

/// Sends an event to the node keyed `key` of the database view.
pub fn send(p: &mut DbViewer, panel: PanelId, key: &str, kind: EventKind) {
    let event = Event {
        node: node(p, panel, key),
        kind,
        values: vec![],
    };
    p.ui_event(panel, event).expect("ui event");
}

/// Types `sql` and clicks Run, as a user gesture.
pub fn run(p: &mut DbViewer, panel: PanelId, sql: &str) {
    testing::with(|h| h.gesture = true);
    let event = Event {
        node: node(p, panel, "run"),
        kind: EventKind::Click,
        values: vec![(node(p, panel, "sql"), sql.to_owned())],
    };
    p.ui_event(panel, event).expect("run");
    testing::with(|h| h.gesture = false);
}
