//! Native tests of the `databases` sidebar.

mod common;

use arcode_plugin::Plugin;
use arcode_plugin::testing;
use arcode_plugin::ui::{Event, EventKind, Widget};
use common::*;

const SIDEBAR: u32 = 2;

fn items() -> Vec<(String, u8, bool)> {
    testing::with(|h| {
        h.panels[&SIDEBAR]
            .view
            .nodes
            .values()
            .find_map(|n| match &n.widget {
                Widget::Tree(items) => Some(
                    items
                        .iter()
                        .map(|i| (i.label.clone(), i.depth, i.expanded))
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    })
}

fn tree_event(p: &mut db_viewer::DbViewer, kind: EventKind) {
    let node = testing::with(|h| {
        h.panels[&SIDEBAR]
            .view
            .nodes
            .values()
            .find(|n| matches!(n.widget, Widget::Tree(_)))
            .map(|n| n.id)
            .expect("tree node")
    });
    let event = Event {
        node,
        kind,
        values: vec![],
    };
    p.ui_event(SIDEBAR, event).unwrap();
}

#[test]
fn sidebar_lists_databases_and_opens_tables() {
    let dir = project();
    let mut p = plugin(dir.path());
    testing::with(|h| h.files = vec!["sample.db".into(), "data/nested.sqlite3".into()]);
    add_panel(SIDEBAR, "databases");
    p.open_view(SIDEBAR, "databases", "{}".to_owned().into())
        .unwrap();
    assert_eq!(
        items(),
        [
            ("data/nested.sqlite3".into(), 0, false),
            ("sample.db".into(), 0, false)
        ]
    );
    assert!(shows(SIDEBAR, "2 files"));
    tree_event(&mut p, EventKind::TreeToggle("d1".into()));
    let listed = items();
    assert_eq!(listed.len(), 7);
    assert_eq!(listed[1], ("sample.db".into(), 0, true));
    assert_eq!(listed[5], ("users".into(), 1, false));
    tree_event(&mut p, EventKind::TreeToggle("t1.3".into()));
    assert_eq!(items()[6], ("id".into(), 2, false));
    tree_event(&mut p, EventKind::TreeActivate("t1.3".into()));
    let opened = testing::with(|h| h.opened_files.clone());
    assert_eq!(opened, [("sample.db".to_owned(), None)]);
    // The editor routes the file to the opener; the view selects `users`.
    open(&mut p, 11, "sample.db");
    assert_eq!(
        db(&mut p, 11).selected_object().map(|o| o.name.clone()),
        Some("users".into())
    );
    tree_event(&mut p, EventKind::TreeToggle("d1".into()));
    assert_eq!(items().len(), 2, "collapsed");
}

#[test]
fn sidebar_shows_an_empty_state_and_errors() {
    let dir = project();
    let mut p = plugin(dir.path());
    add_panel(SIDEBAR, "databases");
    p.open_view(SIDEBAR, "databases", "{}".to_owned().into())
        .unwrap();
    assert!(shows(SIDEBAR, "No databases"));
    testing::with(|h| h.files = vec!["broken.db".into()]);
    std::fs::write(dir.path().join("broken.db"), b"not sqlite at all, really").unwrap();
    p.command("refresh", Default::default()).unwrap();
    tree_event(&mut p, EventKind::TreeToggle("d0".into()));
    let listed = items();
    assert_eq!(listed.len(), 2);
    assert!(listed[1].0.contains("not a database"), "{listed:?}");
}
