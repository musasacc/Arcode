//! Native tests of the opener view's schema tree and Data tab, against the
//! checked-in fixtures, with the SDK's fake host.

mod common;

use arcode_plugin::Plugin;
use arcode_plugin::testing;
use arcode_plugin::ui::{Cell, EventKind, Tone};
use common::*;
use db_viewer::view::SNAPSHOT_NOTE;

const PANEL: u32 = 5;

#[test]
fn opener_lists_the_schema_and_selects_the_first_table() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    let labels: Vec<_> = tree(&mut p, PANEL, "schema")
        .into_iter()
        .map(|i| (i.label, i.detail.unwrap_or_default(), i.depth))
        .collect();
    assert_eq!(
        labels,
        [
            ("odd \"name\"".into(), "table".into(), 0),
            ("orders".into(), "table".into(), 0),
            ("tags".into(), "without rowid".into(), 0),
            ("users".into(), "table".into(), 0),
            ("big_spenders".into(), "view".into(), 0),
        ]
    );
    assert!(shows(PANEL, "read-only"));
    assert_eq!(
        testing::with(|h| h.panels[&PANEL].title.clone()),
        "sample.db"
    );
    // `odd "name"` has a column called rowid: paging uses another alias.
    assert_eq!(table(&mut p, PANEL, "data").0, Some(2));
    let data = node(&mut p, PANEL, "data");
    let page = p.fetch_rows(PANEL, data, 0, 200).unwrap();
    assert_eq!(page.rows[1], [Cell::Text("b".into()), Cell::Int(2)]);
    assert!(page.at_end);
}

#[test]
fn activating_a_table_pages_it() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    let (_, before, _) = table(&mut p, PANEL, "data");
    send(
        &mut p,
        PANEL,
        "schema",
        EventKind::TreeActivate("o3".into()),
    );
    let (rows, generation, columns) = table(&mut p, PANEL, "data");
    assert_eq!(rows, Some(500));
    assert_ne!(generation, before, "a new table drops cached pages");
    assert_eq!(columns, ["id", "name", "email", "score"]);
    assert!(shows(PANEL, "500 rows"));
    let data = node(&mut p, PANEL, "data");
    let first = p.fetch_rows(PANEL, data, 0, 200).unwrap();
    assert_eq!(first.rows.len(), 200);
    assert_eq!(
        first.rows[0],
        [
            Cell::Int(1),
            Cell::Text("user 1".into()),
            Cell::Text("u1@example.test".into()),
            Cell::Real(3.7)
        ]
    );
    assert!(!first.at_end);
    assert_eq!(first.rows[9][2], Cell::Empty, "NULL email");
    let last = p.fetch_rows(PANEL, data, 400, 200).unwrap();
    assert_eq!((last.rows.len(), last.at_end), (100, true));
    assert_eq!(last.rows[99][0], Cell::Int(500));
    let middle = p.fetch_rows(PANEL, data, 250, 3).unwrap();
    assert_eq!(middle.rows[0][0], Cell::Int(251));
    let blobs = {
        send(
            &mut p,
            PANEL,
            "schema",
            EventKind::TreeActivate("o1".into()),
        );
        let data = node(&mut p, PANEL, "data");
        p.fetch_rows(PANEL, data, 249, 1).unwrap()
    };
    assert_eq!(blobs.rows[0][4], Cell::Blob(16), "blobs show their size");
}

#[test]
fn expanding_a_table_lists_its_columns() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    send(&mut p, PANEL, "schema", EventKind::TreeToggle("o3".into()));
    let items = tree(&mut p, PANEL, "schema");
    let users = items.iter().position(|i| i.label == "users").unwrap();
    assert!(items[users].expanded);
    let columns: Vec<_> = items[users + 1..users + 5]
        .iter()
        .map(|i| {
            (
                i.label.as_str(),
                i.detail.clone().unwrap_or_default(),
                i.depth,
            )
        })
        .collect();
    assert_eq!(
        columns,
        [
            ("id", "INTEGER · PK".into(), 1),
            ("name", "TEXT · NOT NULL".into(), 1),
            ("email", "TEXT".into(), 1),
            ("score", "REAL".into(), 1),
        ]
    );
    send(&mut p, PANEL, "schema", EventKind::TreeToggle("o3".into()));
    assert_eq!(tree(&mut p, PANEL, "schema").len(), 5, "collapsed again");
}

#[test]
fn sorting_requeries_and_bumps_the_generation() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    send(
        &mut p,
        PANEL,
        "schema",
        EventKind::TreeActivate("o3".into()),
    );
    let (_, before, _) = table(&mut p, PANEL, "data");
    send(&mut p, PANEL, "data", EventKind::Sort((3, false)));
    let (rows, after, _) = table(&mut p, PANEL, "data");
    assert_eq!(rows, Some(500));
    assert_ne!(after, before);
    assert!(shows(PANEL, "sorted by score desc"));
    let data = node(&mut p, PANEL, "data");
    let page = p.fetch_rows(PANEL, data, 0, 2).unwrap();
    assert_eq!(page.rows[0][3], Cell::Real(99.9));
}

#[test]
fn views_and_without_rowid_tables_page_too() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    send(
        &mut p,
        PANEL,
        "schema",
        EventKind::TreeActivate("o4".into()),
    );
    assert_eq!(table(&mut p, PANEL, "data").0, Some(4));
    let data = node(&mut p, PANEL, "data");
    let spenders = p.fetch_rows(PANEL, data, 0, 200).unwrap();
    assert_eq!(spenders.rows[0][0], Cell::Text("user 111".into()));
    send(
        &mut p,
        PANEL,
        "schema",
        EventKind::TreeActivate("o2".into()),
    );
    let data = node(&mut p, PANEL, "data");
    let tags = p.fetch_rows(PANEL, data, 1, 200).unwrap();
    assert_eq!(tags.rows.len(), 2);
    assert!(tags.at_end);
}

#[test]
fn wal_databases_open_as_a_snapshot() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "wal.db");
    assert!(shows(PANEL, "snapshot"));
    assert!(shows(PANEL, SNAPSHOT_NOTE));
    assert_eq!(
        table(&mut p, PANEL, "data").0,
        Some(3),
        "WAL rows not shown"
    );
    send(&mut p, PANEL, "writes", EventKind::Toggle(true));
    let (notice, tone) = text_of(&mut p, PANEL, "notice");
    assert!(notice.contains("Writes are unavailable"), "{notice}");
    assert_eq!(tone, Tone::Warning);
    assert!(!db(&mut p, PANEL).writes);
}

#[test]
fn files_that_are_not_databases_show_the_error_in_the_panel() {
    let dir = project();
    std::fs::write(dir.path().join("junk.db"), vec![1u8; 2048]).unwrap();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "junk.db");
    assert!(shows(PANEL, "file is not a database"), "{:?}", texts(PANEL));
    open(&mut p, PANEL + 1, "gone.db");
    assert!(shows(PANEL + 1, "file not found"));
}

#[test]
fn refresh_rereads_the_schema() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    let before = table(&mut p, PANEL, "data").1;
    {
        let conn = rusqlite::Connection::open(dir.path().join("sample.db")).unwrap();
        conn.execute_batch("CREATE TABLE added(x)").unwrap();
    }
    p.command("refresh", Default::default()).unwrap();
    let names: Vec<_> = tree(&mut p, PANEL, "schema")
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(names.contains(&"added".to_owned()), "{names:?}");
    assert_ne!(table(&mut p, PANEL, "data").1, before, "new generation");
    assert_eq!(
        table(&mut p, PANEL, "data").0,
        Some(2),
        "kept the selection"
    );
}
