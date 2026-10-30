use rusqlite::Connection;

use super::*;

fn table(rowid: Option<&'static str>) -> Source {
    Source {
        name: "big \"t\"".into(),
        rowid,
        sort: None,
    }
}

fn known(n: u64) -> Shape {
    Shape {
        row_count: Some(n),
        dense_min: None,
    }
}

#[test]
fn views_and_without_rowid_tables_use_limit_offset() {
    let q = page_query(&table(None), &Anchors::default(), known(1000), 400, 200);
    assert_eq!(q.sql, "SELECT * FROM \"big \"\"t\"\"\" LIMIT ?1 OFFSET ?2");
    assert_eq!(q.params, [200, 400]);
    assert_eq!(q.strategy, Strategy::Offset { skip: 400 });
    assert!(!q.leading_rowid && !q.reversed);
}

#[test]
fn sorted_grids_order_by_the_column_then_rowid() {
    let mut source = table(Some("rowid"));
    source.sort = Some(Sort {
        column: "name".into(),
        ascending: false,
    });
    let q = page_query(&source, &Anchors::default(), Shape::default(), 0, 50);
    assert_eq!(
        q.sql,
        "SELECT rowid, * FROM \"big \"\"t\"\"\" ORDER BY \"name\" DESC, rowid DESC LIMIT ?1 OFFSET ?2"
    );
    assert!(q.leading_rowid);
}

#[test]
fn dense_rowids_seek_directly() {
    let shape = Shape {
        row_count: Some(1_000_000),
        dense_min: Some(1),
    };
    let q = page_query(
        &table(Some("rowid")),
        &Anchors::default(),
        shape,
        999_800,
        200,
    );
    assert_eq!(q.strategy, Strategy::Dense);
    assert_eq!(q.params, [999_801, 200]);
    assert!(q.sql.contains("WHERE rowid >= ?1 ORDER BY rowid LIMIT ?2"));
}

#[test]
fn the_cheapest_way_in_wins() {
    let src = table(Some("_rowid_"));
    let none = Anchors::default();
    // Nothing known: skip from the start.
    let q = page_query(&src, &none, Shape::default(), 5000, 200);
    assert_eq!(q.strategy, Strategy::FromStart { skip: 5000 });
    // Near the end of a counted table: walk back from the end.
    let q = page_query(&src, &none, known(10_000), 9_000, 200);
    assert_eq!(q.strategy, Strategy::FromEnd { skip: 800 });
    assert!(q.reversed && q.sql.contains("ORDER BY _rowid_ DESC LIMIT ?1 OFFSET ?2"));
    // Scrolling on from a seen page: seek to its last row.
    let mut anchors = Anchors::default();
    anchors.record(4800, &(10_000..10_200).collect::<Vec<_>>());
    let q = page_query(&src, &anchors, known(1_000_000), 5000, 200);
    assert_eq!(q.strategy, Strategy::AfterAnchor { skip: 1 });
    assert_eq!(q.params, [10_199, 200, 1]);
    // Scrolling back up from a seen page.
    let q = page_query(&src, &anchors, known(1_000_000), 4400, 200);
    assert_eq!(q.strategy, Strategy::BeforeAnchor { skip: 201 });
    assert_eq!(q.params, [10_000, 200, 201]);
}

#[test]
fn pages_past_the_known_end_are_empty_or_clamped() {
    let src = table(Some("rowid"));
    let q = page_query(&src, &Anchors::default(), known(100), 100, 200);
    assert_eq!((q.strategy, q.count), (Strategy::Empty, 0));
    let q = page_query(&src, &Anchors::default(), known(150), 100, 200);
    assert_eq!(q.count, 50);
    assert_eq!(q.strategy, Strategy::FromEnd { skip: 0 });
    let q = page_query(&src, &Anchors::default(), Shape::default(), u64::MAX, 200);
    assert_eq!(
        q.params[1],
        i64::MAX,
        "offset saturates instead of wrapping"
    );
}

#[test]
fn anchors_are_bounded() {
    let mut anchors = Anchors::default();
    for page in 0..5000u64 {
        let first = i64::try_from(page * 10).unwrap_or(0);
        anchors.record(page * 200, &[first, first + 5]);
    }
    assert_eq!(anchors.len(), MAX_ANCHORS);
    anchors.record(0, &[]);
    anchors.clear();
    assert!(anchors.is_empty());
}

/// Runs a [`PageQuery`] and returns the rowids, in display order.
fn run(conn: &Connection, q: &PageQuery) -> Vec<i64> {
    let mut stmt = conn.prepare(&q.sql).expect("prepare");
    let params = rusqlite::params_from_iter(q.params.iter());
    let mut ids: Vec<i64> = stmt
        .query_map(params, |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    if q.reversed {
        ids.reverse();
    }
    ids
}

/// Every strategy returns exactly what plain `LIMIT/OFFSET` would, for
/// tables with and without gaps, while anchors accumulate.
#[test]
fn every_strategy_agrees_with_limit_offset() {
    let conn = Connection::open_in_memory().expect("sqlite");
    conn.execute_batch(
        "CREATE TABLE \"big \"\"t\"\"\"(v);
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 3000)
         INSERT INTO \"big \"\"t\"\"\"(rowid, v) SELECT i * 3 + (i % 7), i FROM n;
         CREATE TABLE dense(v);
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 3000)
         INSERT INTO dense(rowid, v) SELECT i + 41, i FROM n;",
    )
    .expect("fixture");
    let mut seed = 0x9e37_79b9_u64;
    let mut next = |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    for (name, dense_min) in [("big \"t\"", None), ("dense", Some(42))] {
        let src = Source {
            name: name.into(),
            rowid: Some("rowid"),
            sort: None,
        };
        for shape in [
            Shape::default(),
            Shape {
                row_count: Some(3000),
                dense_min,
            },
        ] {
            let mut anchors = Anchors::default();
            for _ in 0..200 {
                let start = next(3100);
                let count = 1 + u32::try_from(next(250)).unwrap_or(0);
                let q = page_query(&src, &anchors, shape, start, count);
                let got = run(&conn, &q);
                let plain = format!(
                    "SELECT rowid FROM {} ORDER BY rowid LIMIT {count} OFFSET {start}",
                    quote_ident(name)
                );
                let want: Vec<i64> = conn
                    .prepare(&plain)
                    .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
                    .expect("plain");
                assert_eq!(got, want, "{name} {shape:?} {start}+{count} via {q:?}");
                anchors.record(start, &got);
            }
        }
    }
}
