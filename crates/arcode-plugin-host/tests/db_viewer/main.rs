//! End-to-end tests of the bundled DB viewer (docs/PLUGINS.md §13, Step
//! 11): the real `arcode.db-viewer` wasm under a headless `PluginManager`,
//! against the checked-in 64 KB fixture, a WAL fixture and a 1M-row
//! database the plugin generates itself.
//!
//! Needs the built plugin: run `scripts/build-plugins.sh db-viewer` first.

mod big;
mod harness;
mod sidebar;

use arcode_plugin_host::bindings::ui::{Cell, EventKind, Widget};
use arcode_plugin_host::consent::ConsentKind;
use arcode_plugin_host::{CallOutput, PluginOutput};

use harness::Viewer;

#[test]
fn sample_database_tree_rows_query_and_errors() {
    let v = Viewer::new(false);
    let db = v.open("sample.db");
    let asked = v.consent.asked();
    assert_eq!(asked.len(), 1, "one workspace prompt, before activation");
    assert!(matches!(
        asked[0].kind,
        ConsentKind::Workspace { write: true, .. }
    ));

    // Tree: tables, then views.
    let (_, items) = db.tree();
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        ["odd \"name\"", "orders", "tags", "users", "big_spenders"]
    );
    assert!(db.shows("read-only"));

    // Rows: select `users`, page the grid.
    db.activate(&v, "o3");
    let (data, props) = db.table(true).expect("data grid");
    assert_eq!(props.row_count, Some(500));
    let titles: Vec<_> = props.columns.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["id", "name", "email", "score"]);
    assert!(props.columns[0].numeric && !props.columns[1].numeric);
    let page = db.fetch(&v, data, 200, 200).expect("page");
    assert_eq!(page.rows.len(), 200);
    assert_eq!(page.rows[0][1], Cell::Text("user 201".into()));
    assert_eq!(page.rows[9][2], Cell::Empty, "NULL e-mail");
    let last = db.fetch(&v, data, 400, 200).expect("last page");
    assert!(last.at_end && last.rows.len() == 100);

    // Expanding a table lists its columns.
    let (tree, _) = db.tree();
    let r = db
        .event(&v, tree, EventKind::TreeToggle("o3".into()), vec![])
        .wait();
    assert_eq!(r, Ok(CallOutput::Done));
    let (_, items) = db.tree();
    assert!(
        items
            .iter()
            .any(|i| i.depth == 1 && i.label == "email" && i.detail.as_deref() == Some("TEXT"))
    );

    // Query: results, then errors inline.
    db.run_wait(
        &v,
        "SELECT name, score FROM users WHERE id IN (7, 8) ORDER BY id",
    );
    assert!(db.texts().iter().any(|t| t.starts_with("2 rows · ")));
    let (result, props) = db.table(false).expect("result grid");
    assert_eq!(props.row_count, Some(2));
    let rows = db.fetch(&v, result, 0, 200).expect("result page");
    assert_eq!(
        rows.rows[1],
        [Cell::Text("user 8".into()), Cell::Real(29.6)]
    );
    let started = v.wait_output(|o| {
        matches!(o, PluginOutput::TaskStarted { title, .. } if title == "Running query")
            .then_some(())
    });
    assert!(started.is_some(), "Run is a gesture and becomes a task");

    db.run_wait(&v, "SELECT * FROM nope");
    assert!(db.shows("no such table: nope"), "{:?}", db.texts());
    assert!(
        db.table(false).is_none(),
        "a failed query does not show the previous rows"
    );
    db.run_wait(&v, "DELETE FROM users");
    assert!(db.shows("attempt to write a readonly database"));
    db.run_wait(&v, "SELECT count(*) FROM users");
    let (result, _) = db.table(false).expect("result grid");
    assert_eq!(
        db.fetch(&v, result, 0, 1).expect("count").rows[0],
        [Cell::Int(500)]
    );
}

/// Toggles "Allow writes" and returns the notice shown.
fn toggle_writes(v: &Viewer, db: &harness::Opened, on: bool) -> String {
    let checkbox = db.find(|w| matches!(w, Widget::Checkbox(c) if c.label == "Allow writes"));
    let r = db.event(v, checkbox, EventKind::Toggle(on), vec![]).wait();
    assert_eq!(r, Ok(CallOutput::Done));
    db.texts()
        .into_iter()
        .find(|t| t.starts_with("Writes are"))
        .unwrap_or_default()
}

#[test]
fn writes_need_the_toggle_and_a_write_grant() {
    // Read-only grant: the toggle cannot turn writes on.
    let v = Viewer::new(false);
    let db = v.open("sample.db");
    let notice = toggle_writes(&v, &db, true);
    assert!(notice.contains("shared read-only"), "{notice}");
    db.run_wait(&v, "INSERT INTO tags VALUES ('pink', '#fcc')");
    assert!(db.shows("readonly"), "{:?}", db.texts());

    // Read-write grant: writes only after the toggle.
    let v = Viewer::new(true);
    let db = v.open("sample.db");
    db.run_wait(&v, "INSERT INTO tags VALUES ('pink', '#fcc')");
    assert!(db.shows("writes are off"), "{:?}", db.texts());
    let notice = toggle_writes(&v, &db, true);
    assert!(notice.starts_with("Writes are on"), "{notice}");
    assert!(db.shows("writes on"));
    db.run_wait(
        &v,
        "INSERT INTO tags VALUES ('pink', '#fcc'), ('grey', '#888')",
    );
    assert!(
        db.texts().iter().any(|t| t.starts_with("2 rows changed")),
        "{:?}",
        db.texts()
    );
    assert!(
        !v.project.path().join("sample.db-journal").exists(),
        "the journal is gone"
    );
    toggle_writes(&v, &db, false);
    db.run_wait(&v, "SELECT count(*) FROM tags");
    let (result, _) = db.table(false).expect("result grid");
    assert_eq!(
        db.fetch(&v, result, 0, 1).expect("count").rows[0],
        [Cell::Int(5)]
    );
}

#[test]
fn wal_databases_are_shown_as_a_snapshot() {
    let v = Viewer::new(true);
    let db = v.open("wal.db");
    assert!(db.shows("snapshot; uncheckpointed WAL changes not shown"));
    let (data, props) = db.table(true).expect("data grid");
    assert_eq!(
        props.row_count,
        Some(3),
        "the 2 WAL-only rows are not shown"
    );
    let page = db.fetch(&v, data, 0, 200).expect("rows");
    assert_eq!(page.rows[2][1], Cell::Text("three".into()));
    let notice = toggle_writes(&v, &db, true);
    assert!(notice.contains("snapshot"), "{notice}");
    assert!(
        v.project.path().join("wal.db-wal").is_file(),
        "WAL untouched"
    );
}

#[test]
fn files_that_are_not_databases_report_inline() {
    let v = Viewer::new(false);
    std::fs::write(v.project.path().join("junk.sqlite"), vec![9u8; 1024]).unwrap();
    let db = v.open("junk.sqlite");
    assert!(db.shows("file is not a database"), "{:?}", db.texts());
}

/// Regression: two runtimes compiling the same component into one cache
/// dir at once used the same temp file name, so one rename failed.
#[test]
fn concurrent_compiles_of_one_component_both_succeed() {
    use std::sync::{Arc, Barrier};

    use arcode_plugin_host::{Runtime, RuntimeConfig};

    let dir = tempfile::tempdir().expect("cache dir");
    let wasm = Arc::new(harness::wasm_plugins().join("arcode.db-viewer/plugin.wasm"));
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (dir, wasm, barrier) = (dir.path().to_owned(), wasm.clone(), barrier.clone());
            std::thread::spawn(move || {
                let runtime = Runtime::new(RuntimeConfig::new(dir));
                let engine = runtime.engine().expect("engine");
                barrier.wait();
                runtime.cache().prepare(engine, &wasm).map(|_| ())
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().expect("thread"), Ok(()));
    }
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("cache")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "one entry, no temp files left: {entries:?}"
    );
}
