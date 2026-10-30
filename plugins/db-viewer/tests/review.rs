//! Adversarial review findings (failing on purpose until fixed): sorting
//! grids without a rowid, and pages of wide rows that still exceed the
//! host's 4 MiB page cap after `fit_page`.

mod common;

use arcode_plugin::Plugin;
use arcode_plugin::testing;
use arcode_plugin::ui::{Cell, EventKind};
use common::*;
use db_viewer::cells::fit_page;

const PANEL: u32 = 7;

/// Index of the schema tree item labelled `label` (`o<i>` key).
fn object_key(p: &mut db_viewer::DbViewer, label: &str) -> String {
    tree(p, PANEL, "schema")
        .into_iter()
        .find(|i| i.label == label && i.depth == 0)
        .map(|i| i.key)
        .unwrap_or_else(|| panic!("no object {label}"))
}

fn first_column(p: &mut db_viewer::DbViewer) -> Vec<Cell> {
    let data = node(p, PANEL, "data");
    let page = p.fetch_rows(PANEL, data, 0, 200).expect("fetch-rows");
    page.rows.into_iter().map(|r| r[0].clone()).collect()
}

/// A `WITHOUT ROWID` table offers sortable columns (`grid_columns` marks
/// every column sortable), but `page_query` ignores `source.sort` when
/// there is no rowid, so a "descending" sort still shows ascending rows.
#[test]
fn sorting_a_without_rowid_table_orders_the_rows() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    let key = object_key(&mut p, "tags");
    send(&mut p, PANEL, "schema", EventKind::TreeActivate(key));
    assert_eq!(table(&mut p, PANEL, "data").0, Some(3));
    testing::with(|h| h.gesture = true);
    send(&mut p, PANEL, "data", EventKind::Sort((0, false)));
    testing::with(|h| h.gesture = false);
    assert_eq!(
        first_column(&mut p),
        [
            Cell::Text("red".into()),
            Cell::Text("green".into()),
            Cell::Text("blue".into()),
        ],
        "tags sorted by name descending"
    );
}

/// Same for views: `big_spenders` sorted ascending by `spent` must start
/// with the smallest value (the view itself orders by `spent DESC`).
#[test]
fn sorting_a_view_orders_the_rows() {
    let dir = project();
    let mut p = plugin(dir.path());
    open(&mut p, PANEL, "sample.db");
    let key = object_key(&mut p, "big_spenders");
    send(&mut p, PANEL, "schema", EventKind::TreeActivate(key));
    testing::with(|h| h.gesture = true);
    send(&mut p, PANEL, "data", EventKind::Sort((1, true)));
    testing::with(|h| h.gesture = false);
    let data = node(&mut p, PANEL, "data");
    let page = p.fetch_rows(PANEL, data, 0, 200).expect("fetch-rows");
    let spent: Vec<Cell> = page.rows.into_iter().map(|r| r[1].clone()).collect();
    assert_eq!(
        spent,
        [
            Cell::Real(1504.0),
            Cell::Real(1506.0),
            Cell::Real(1512.0),
            Cell::Real(1514.0),
        ],
        "big_spenders sorted by spent ascending"
    );
}

/// `fit_page` never cuts a text cell below 64 bytes, so a 200-row page of
/// a wide table (400 text columns of 100 bytes) stays at ~5 MiB and the
/// host refuses it (`row page has ... bytes of text (limit 4 MiB)`): the
/// Data tab of such a table can never show a page.
#[test]
fn wide_pages_fit_the_host_page_cap() {
    const HOST_PAGE_CAP: usize = 4 * 1024 * 1024;
    let mut rows: Vec<Vec<Cell>> = (0..200)
        .map(|_| (0..400).map(|_| Cell::Text("x".repeat(100))).collect())
        .collect();
    fit_page(&mut rows);
    let total: usize = rows
        .iter()
        .flatten()
        .map(|c| match c {
            Cell::Text(t) => t.len(),
            _ => 0,
        })
        .sum();
    assert!(
        total <= HOST_PAGE_CAP,
        "{total} bytes of text in one page; the host refuses pages over {HOST_PAGE_CAP}"
    );
}
