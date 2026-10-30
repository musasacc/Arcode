//! The `databases` sidebar through the manager: `show_panel`, the
//! project-wide file search, lazy tree expansion, and opening a table
//! through the editor and the `sqlite` opener.

use arcode_plugin_host::bindings::ui::EventKind;
use arcode_plugin_host::memory_editor::Opened as EditorOpened;
use arcode_plugin_host::{CallOutput, QualifiedId};

use crate::harness::{Opened, Viewer, fixtures};

#[test]
fn sidebar_lists_project_databases_and_opens_a_table() {
    let v = Viewer::new(false);
    let root = v.project.path();
    std::fs::create_dir(root.join("nested")).expect("dir");
    std::fs::copy(
        fixtures().join("sample.db"),
        root.join("nested/copy.sqlite3"),
    )
    .expect("copy");
    std::fs::write(root.join("notes.txt"), "not a database").expect("txt");
    let panel = QualifiedId::parse("arcode.db-viewer/databases").expect("id");
    let call = v.manager.show_panel(&panel).expect("show_panel");
    assert_eq!(call.pending.wait(), Ok(CallOutput::Done));
    let side = Opened {
        handle: v.manager.instance(&call.plugin).expect("instance"),
        plugin: call.plugin,
        panel: call.panel,
    };
    let (tree, items) = side.tree();
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["nested/copy.sqlite3", "sample.db", "wal.db"]);
    assert!(side.shows("3 files"));

    let r = side
        .event(&v, tree, EventKind::TreeToggle("d1".into()), vec![])
        .wait();
    assert_eq!(r, Ok(CallOutput::Done));
    let (_, items) = side.tree();
    assert!(
        items
            .iter()
            .any(|i| i.depth == 1 && i.label == "users" && i.key == "t1.3"),
        "{items:?}"
    );

    let r = side
        .event(&v, tree, EventKind::TreeActivate("t1.3".into()), vec![])
        .wait();
    assert_eq!(r, Ok(CallOutput::Done));
    assert_eq!(
        v.editor.opened(),
        [EditorOpened::File {
            path: "sample.db".into(),
            at: None
        }]
    );
    // The app routes the file to the opener; the view selects `users`.
    let db = v.open("sample.db");
    let (_, props) = db.table(true).expect("data grid");
    assert_eq!(props.row_count, Some(500));
    assert!(db.shows("users"));
}
