//! The 1M-row database (§13 Step 11), natively: deep paging stays under a
//! second per page for dense and gapped rowids, and a `GROUP BY` stops at
//! cancel. The host's end-to-end test repeats both inside wasm.
//!
//! The database is generated from `tests/fixtures/big.sql` once into
//! cargo's per-target temp dir and reused by later runs.

mod common;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use arcode_plugin::Plugin;
use arcode_plugin::testing;
use arcode_plugin::ui::{Cell, EventKind, Tone};
use common::*;

const ROWS: u64 = 1_000_000;
const PANEL: u32 = 3;

/// The directory holding `big.db`, generated on first use.
fn big_project() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("db-viewer-big");
        let done = dir.join("complete");
        if !done.is_file() {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("big dir");
            let started = Instant::now();
            let conn = rusqlite::Connection::open(dir.join("big.db")).expect("create");
            let sql = std::fs::read_to_string(fixtures().join("big.sql")).expect("big.sql");
            conn.execute_batch(&sql).expect("generate");
            drop(conn);
            std::fs::write(&done, b"").expect("marker");
            eprintln!("generated 2 x 1M rows in {:?}", started.elapsed());
        }
        dir
    })
}

/// Expected first id of row `index` in `table`.
fn id_at(table: &str, index: u64) -> i64 {
    let id = i64::try_from(index + 1).unwrap_or(i64::MAX);
    if table == "gaps" { id * 3 } else { id }
}

#[test]
fn deep_paging_stays_under_a_second_per_page() {
    let mut p = plugin(big_project());
    open(&mut p, PANEL, "big.db");
    for (key, table) in [("o0", "dense"), ("o1", "gaps")] {
        send(&mut p, PANEL, "schema", EventKind::TreeActivate(key.into()));
        assert_eq!(table_rows(&mut p), Some(ROWS), "{table}");
        let data = node(&mut p, PANEL, "data");
        for start in [0, 500_000, 999_800, 500_200, 250_000, 750_000] {
            let started = Instant::now();
            let page = p.fetch_rows(PANEL, data, start, 200).expect("page");
            let took = started.elapsed();
            eprintln!("{table}: rows {start}..+200 in {took:?}");
            assert!(took < Duration::from_secs(1), "{table} {start}: {took:?}");
            assert_eq!(page.rows.len(), 200);
            assert_eq!(page.rows[0][0], Cell::Int(id_at(table, start)));
            assert_eq!(
                page.rows[199][2],
                Cell::Text(format!("row {}", start + 200))
            );
            assert_eq!(page.at_end, start == 999_800);
        }
    }
}

fn table_rows(p: &mut db_viewer::DbViewer) -> Option<u64> {
    table(p, PANEL, "data").0
}

#[test]
fn cancel_stops_a_group_by() {
    let mut p = plugin(big_project());
    open(&mut p, PANEL, "big.db");
    let sql = "SELECT k, count(*), max(label) FROM gaps GROUP BY k ORDER BY 2 DESC, 1";
    let started = Instant::now();
    run(&mut p, PANEL, sql);
    let full = started.elapsed();
    let (line, tone) = text_of(&mut p, PANEL, "status");
    assert!(line.starts_with("1000 rows"), "{line}");
    assert_eq!(tone, Tone::Success);
    testing::with(|h| h.cancelled = true);
    let started = Instant::now();
    run(&mut p, PANEL, sql);
    let cancelled = started.elapsed();
    testing::with(|h| h.cancelled = false);
    eprintln!("GROUP BY over 1M rows: {full:?} in full, {cancelled:?} when cancelled");
    assert_eq!(
        text_of(&mut p, PANEL, "status"),
        ("Query cancelled".to_owned(), Tone::Danger)
    );
    assert!(cancelled < Duration::from_millis(100), "{cancelled:?}");
    let _ = p.command("refresh", Default::default());
}
