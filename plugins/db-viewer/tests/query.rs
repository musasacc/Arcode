//! Native tests of the Query tab: results, inline errors, the read-only
//! default, the writes toggle, cancel, truncation and recent queries.

mod common;

use arcode_plugin::Plugin;
use arcode_plugin::testing;
use arcode_plugin::ui::{Cell, EventKind, Tone, Widget};
use common::*;

const PANEL: u32 = 9;

fn status(p: &mut db_viewer::DbViewer) -> (String, Tone) {
    text_of(p, PANEL, "status")
}

#[test]
fn a_query_fills_the_result_grid() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    run(
        &mut p,
        PANEL,
        "SELECT id, name FROM users WHERE id <= 3 ORDER BY id DESC",
    );
    let (line, tone) = status(&mut p);
    assert!(line.starts_with("3 rows · "), "{line}");
    assert_eq!(tone, Tone::Success);
    assert_eq!(testing::with(|h| h.tasks.clone()), ["Running query"]);
    let (rows, _, columns) = table(&mut p, PANEL, "result");
    assert_eq!((rows, columns), (Some(3), vec!["id".into(), "name".into()]));
    let result = node(&mut p, PANEL, "result");
    let page = p.fetch_rows(PANEL, result, 0, 200).unwrap();
    assert_eq!(page.rows[0], [Cell::Int(3), Cell::Text("user 3".into())]);
    assert!(page.at_end);
    assert_eq!(db(&mut p, PANEL).tab, 1);
}

#[test]
fn errors_are_shown_inline() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    run(&mut p, PANEL, "SELECT * FROM nope");
    let (line, tone) = status(&mut p);
    assert_eq!((line.as_str(), tone), ("no such table: nope", Tone::Danger));
    run(&mut p, PANEL, "   ");
    assert_eq!(status(&mut p).0, "Type a query first.");
}

#[test]
fn writes_are_refused_until_turned_on() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    for sql in [
        "DELETE FROM users",
        "SELECT 1; DROP TABLE tags",
        "VACUUM INTO '/tmp/copy.db'",
    ] {
        run(&mut p, PANEL, sql);
        let (line, tone) = status(&mut p);
        assert!(
            line.starts_with("attempt to write a readonly database"),
            "{sql}: {line}"
        );
        assert_eq!(tone, Tone::Danger);
    }
    run(&mut p, PANEL, "SELECT count(*) FROM users");
    let result = node(&mut p, PANEL, "result");
    let page = p.fetch_rows(PANEL, result, 0, 1).unwrap();
    assert_eq!(page.rows[0], [Cell::Int(500)], "nothing was deleted");
}

#[test]
fn turning_writes_on_allows_changes() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    send(&mut p, PANEL, "writes", EventKind::Toggle(true));
    assert!(db(&mut p, PANEL).writes);
    assert!(shows(PANEL, "writes on"));
    assert_eq!(text_of(&mut p, PANEL, "notice").1, Tone::Danger);
    run(
        &mut p,
        PANEL,
        "INSERT INTO tags VALUES ('pink', '#fcc'), ('grey', '#888')",
    );
    assert!(status(&mut p).0.starts_with("2 rows changed"));
    run(&mut p, PANEL, "CREATE TABLE notes(body TEXT)");
    let names: Vec<_> = tree(&mut p, PANEL, "schema")
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(
        names.contains(&"notes".to_owned()),
        "schema refreshed: {names:?}"
    );
    send(&mut p, PANEL, "writes", EventKind::Toggle(false));
    assert!(!db(&mut p, PANEL).writes && shows(PANEL, "read-only"));
    run(&mut p, PANEL, "DELETE FROM tags");
    assert!(status(&mut p).0.contains("readonly"));
    let conn = rusqlite::Connection::open(dir.path().join("sample.db")).unwrap();
    let tags: i64 = conn
        .query_row("SELECT count(*) FROM tags", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tags, 5, "the inserts reached the file, the delete did not");
}

#[test]
fn a_cancelled_query_says_so() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    testing::with(|h| h.cancelled = true);
    run(
        &mut p,
        PANEL,
        "SELECT a.user_id % 7, count(*) FROM orders a, orders b, orders c GROUP BY 1",
    );
    testing::with(|h| h.cancelled = false);
    assert_eq!(status(&mut p), ("Query cancelled".to_owned(), Tone::Danger));
}

#[test]
fn results_are_capped_by_max_result_rows() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    testing::with(|h| h.settings = r#"{"max-result-rows": 10}"#.into());
    run(&mut p, PANEL, "SELECT * FROM orders");
    assert_eq!(table(&mut p, PANEL, "result").0, Some(10));
    assert!(shows(PANEL, "Truncated: showing the first 10 rows"));
}

#[test]
fn recent_queries_refill_the_input() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    run(&mut p, PANEL, "SELECT 1");
    run(&mut p, PANEL, "SELECT 2");
    let options = match widget(PANEL, node(&mut p, PANEL, "recent")) {
        Widget::Select(s) => s.options,
        other => panic!("{other:?}"),
    };
    assert_eq!(options, ["SELECT 2", "SELECT 1"]);
    send(&mut p, PANEL, "recent", EventKind::Select(1));
    match widget(PANEL, node(&mut p, PANEL, "sql")) {
        Widget::Input(i) => assert_eq!(i.value, "SELECT 1"),
        other => panic!("{other:?}"),
    }
    // Another view of the project sees the same list (project-scope kv).
    open(&mut p, PANEL + 1, "wal.db");
    assert_eq!(p.db_view(PANEL + 1).unwrap().recent.len(), 2);
}

#[test]
fn new_query_shows_the_query_tab_or_opens_a_database() {
    let dir = project();
    let mut p = plugin(dir.path());
    testing::with(|h| {
        h.files = vec!["sample.db".into(), "data/nested.sqlite3".into()];
        h.quick_pick_answers = vec![Some(0)];
    });
    p.command("new-query", Default::default()).unwrap();
    let opened = testing::with(|h| h.opened_files.clone());
    assert_eq!(
        opened,
        [("data/nested.sqlite3".to_owned(), None)],
        "sorted list"
    );
    open(&mut p, PANEL, "data/nested.sqlite3");
    assert_eq!(db(&mut p, PANEL).tab, 1, "opened on the Query tab");
    send(&mut p, PANEL, "tabs", EventKind::Tab(0));
    p.command("new-query", Default::default()).unwrap();
    assert_eq!(db(&mut p, PANEL).tab, 1);
    p.view_closed(PANEL);
    testing::with(|h| h.files.clear());
    p.command("new-query", Default::default()).unwrap();
    assert!(testing::with(|h| h.notifications.len()) == 1);
}
