//! The checked-in fixture databases in `tests/fixtures/` (also used by the
//! host's end-to-end tests) and their generator.
//!
//! Regenerate with
//! `cargo test --manifest-path plugins/Cargo.toml -p db-viewer --test fixtures -- --ignored`.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// `sample.db`: ~64 KB with a rowid table, a `WITHOUT ROWID` table, a
/// table whose columns shadow `rowid`, and a view.
const SAMPLE: &str = r#"
PRAGMA page_size = 4096;
CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, score REAL);
CREATE TABLE orders(id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id),
                    total NUMERIC, note TEXT, receipt BLOB);
CREATE TABLE tags(name TEXT PRIMARY KEY, color TEXT) WITHOUT ROWID;
CREATE TABLE "odd ""name"""(rowid TEXT, value INTEGER);
CREATE VIEW big_spenders AS
  SELECT u.name, sum(o.total) AS spent FROM users u JOIN orders o ON o.user_id = u.id
  GROUP BY u.id HAVING spent > 1500 ORDER BY spent DESC;
WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500)
INSERT INTO users(id, name, email, score)
  SELECT i, 'user ' || i, CASE WHEN i % 10 = 0 THEN NULL ELSE 'u' || i || '@example.test' END,
         (i * 37 % 1000) / 10.0 FROM n;
WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
INSERT INTO orders(id, user_id, total, note, receipt)
  SELECT i, 1 + (i * 7 % 500), (i * 13 % 997) + 0.5,
         CASE WHEN i % 100 = 0 THEN 'gift' ELSE NULL END,
         CASE WHEN i % 250 = 0 THEN zeroblob(16) ELSE NULL END FROM n;
INSERT INTO tags VALUES ('red', '#f00'), ('green', '#0f0'), ('blue', '#00f');
INSERT INTO "odd ""name"""(rowid, value) VALUES ('a', 1), ('b', 2);
"#;

#[test]
#[ignore = "regenerates the checked-in fixtures"]
fn regenerate() {
    let dir = fixtures();
    std::fs::create_dir_all(&dir).expect("fixtures dir");
    for name in ["sample.db", "wal.db", "wal.db-wal", "wal.db-shm"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let sample = Connection::open(dir.join("sample.db")).expect("sample");
    sample.execute_batch(SAMPLE).expect("sample schema");
    sample.execute_batch("VACUUM").expect("vacuum");
    drop(sample);

    // wal.db: 3 checkpointed rows, 2 more only in wal.db-wal. The files are
    // copied while the writer is open, so the WAL is not checkpointed away.
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("wal.db");
    let writer = Connection::open(&path).expect("wal");
    writer
        .execute_batch(
            "PRAGMA page_size = 4096; PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
             CREATE TABLE events(id INTEGER PRIMARY KEY, msg TEXT);
             INSERT INTO events(msg) VALUES ('one'), ('two'), ('three');
             PRAGMA wal_checkpoint(TRUNCATE);
             INSERT INTO events(msg) VALUES ('four (wal only)'), ('five (wal only)');",
        )
        .expect("wal writes");
    std::fs::copy(&path, dir.join("wal.db")).expect("copy db");
    std::fs::copy(tmp.path().join("wal.db-wal"), dir.join("wal.db-wal")).expect("copy wal");
    drop(writer);
}

#[test]
fn sample_fixture_has_the_expected_shape() {
    let path = fixtures().join("sample.db");
    let size = std::fs::metadata(&path)
        .expect("sample.db is checked in")
        .len();
    assert!((32_768..=98_304).contains(&size), "about 64 KB: {size}");
    let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open");
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).expect(sql) };
    assert_eq!(count("SELECT count(*) FROM users"), 500);
    assert_eq!(count("SELECT count(*) FROM orders"), 1000);
    assert_eq!(count("SELECT count(*) FROM tags"), 3);
    assert_eq!(count("SELECT count(*) FROM big_spenders"), 4);
    assert_eq!(count("SELECT count(*) FROM \"odd \"\"name\"\"\""), 2);
}

#[test]
fn wal_fixture_keeps_rows_in_the_wal() {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in ["wal.db", "wal.db-wal"] {
        std::fs::copy(fixtures().join(name), dir.path().join(name)).expect(name);
    }
    let conn = Connection::open(dir.path().join("wal.db")).expect("open");
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .expect("count");
    assert_eq!(rows, 5, "a normal reader replays the WAL");
}
