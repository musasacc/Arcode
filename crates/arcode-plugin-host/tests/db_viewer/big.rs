//! A 1M-row database inside wasm: the plugin itself generates it from
//! `plugins/db-viewer/tests/fixtures/big.sql` (Query tab, writes on), then
//! deep pages stay under a second each and a long `GROUP BY` stops on
//! cancel well before the host's hard-cancel grace period.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::ui::{Cell, EventKind, Widget};
use arcode_plugin_host::{CallOutput, PluginOutput};

use crate::harness::{Opened, Viewer, fixtures};
use crate::toggle_writes;

const ROWS: u64 = 1_000_000;

#[test]
fn a_million_rows_page_fast_and_a_group_by_cancels() {
    let v = Viewer::new(true);
    // An empty file is an empty database.
    std::fs::write(v.project.path().join("big.db"), b"").expect("big.db");
    let db = v.open("big.db");
    assert!(db.shows("No tables"));
    assert!(toggle_writes(&v, &db, true).starts_with("Writes are on"));
    let sql = std::fs::read_to_string(fixtures().join("big.sql")).expect("big.sql");
    let started = Instant::now();
    db.run_wait(&v, &sql);
    eprintln!("generated 2 x 1M rows in wasm in {:?}", started.elapsed());
    assert!(
        db.texts()
            .iter()
            .any(|t| t.starts_with("2000000 rows changed")),
        "{:?}",
        db.texts()
    );
    toggle_writes(&v, &db, false);
    let size = std::fs::metadata(v.project.path().join("big.db")).map_or(0, |m| m.len());
    eprintln!("big.db: {} MB", size / 1_000_000);

    for (key, table) in [("o0", "dense"), ("o1", "gaps")] {
        let started = Instant::now();
        db.activate(&v, key);
        eprintln!("{table}: selected in {:?}", started.elapsed());
        let (data, props) = db.table(true).expect("data grid");
        // Counting 1M rows through WASI takes longer than the 200 ms rule
        // (§10), so the count starts unknown and pages run until at-end.
        assert_eq!(props.row_count, None, "{table}");
        for start in [0, 500_000, 500_200, 250_000, 999_800] {
            page(&v, &db, data, table, start, false);
        }
        let past = db.fetch(&v, data, ROWS, 200).expect("past the end");
        assert!(past.rows.is_empty() && past.at_end);

        // "Count" counts as a cancellable task; then pages seek directly
        // (dense rowids) or walk back from the end.
        let button = db.find(|w| matches!(w, Widget::Button(b) if b.label == "Count"));
        let started = Instant::now();
        let r = db.event(&v, button, EventKind::Click, vec![]).wait();
        assert_eq!(r, Ok(CallOutput::Done));
        eprintln!("{table}: counted on request in {:?}", started.elapsed());
        let (data, props) = db.table(true).expect("data grid");
        assert_eq!(props.row_count, Some(ROWS), "{table}");
        for start in [750_000, 999_800, 400_000] {
            page(&v, &db, data, table, start, true);
        }
    }

    for line in db.handle.log().lines() {
        if line.text.contains("ms in all") {
            eprintln!("plugin log: {}", line.text);
        }
    }

    // About 10M grouped rows: seconds of work. Cancel it after 300 ms.
    let heavy = "SELECT g.k, count(*), max(g.label) FROM gaps AS g,
                 (SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4
                  UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7
                  UNION ALL SELECT 8 UNION ALL SELECT 9 UNION ALL SELECT 10)
                 GROUP BY g.k";
    let pending = db.run(&v, heavy);
    std::thread::sleep(Duration::from_millis(300));
    assert!(db.shows("Running query…"), "{:?}", db.texts());
    // Cancel the way the status bar's task chip does: with the token that
    // came with `TaskStarted` for this call.
    let chip = v.wait_output(|o| match o {
        PluginOutput::TaskStarted { call, cancel, .. } if *call == pending.id => {
            Some(cancel.clone())
        }
        _ => None,
    });
    let cancelled_at = Instant::now();
    chip.expect("the query is a task").cancel();
    assert_eq!(pending.wait(), Ok(CallOutput::Done), "cooperative, no trap");
    let latency = cancelled_at.elapsed();
    eprintln!("GROUP BY cancelled; the call returned {latency:?} after cancel");
    assert!(latency < Duration::from_millis(500), "{latency:?}");
    assert!(db.shows("Query cancelled"), "{:?}", db.texts());
    let stats = db.handle.stats();
    assert_eq!(
        stats.instantiations.load(Ordering::SeqCst),
        1,
        "never trapped or restarted"
    );
    eprintln!(
        "peak guest memory: {} MiB",
        stats.peak_memory.load(Ordering::SeqCst) >> 20
    );

    // Still alive and serving.
    db.run_wait(&v, "SELECT count(*) FROM dense WHERE k = 7");
    let (result, _) = db.table(false).expect("result grid");
    let page = db.fetch(&v, result, 0, 1).expect("count");
    assert_eq!(page.rows[0], [Cell::Int(1000)]);
}

/// Fetches 200 rows at `start` of `table` (`dense` or `gaps`), checks them
/// and that the page took under a second.
fn page(v: &Viewer, db: &Opened, data: u32, table: &str, start: u64, counted: bool) {
    let started = Instant::now();
    let page = db.fetch(v, data, start, 200).expect("page");
    let took = started.elapsed();
    eprintln!("{table}: rows {start}..+200 in {took:?} (count known: {counted})");
    assert!(took < Duration::from_secs(1), "{table} {start}: {took:?}");
    let first = i64::try_from(start + 1).unwrap_or(0);
    let id = if table == "gaps" { first * 3 } else { first };
    assert_eq!(page.rows[0][0], Cell::Int(id), "{table} {start}");
    assert_eq!(page.rows.len(), 200);
    // With an unknown count a full last page cannot know it is the last.
    assert_eq!(page.at_end, counted && start == 999_800);
}
