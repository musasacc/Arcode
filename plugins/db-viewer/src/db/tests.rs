use std::time::{Duration, Instant};

use arcode_plugin::testing;
use rusqlite::Connection;

use super::*;

/// A project dir with `name` holding `t(id, v)` and `rows` rows.
fn project_with(name: &str, rows: i64) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let conn = Connection::open(dir.path().join(name)).expect("create");
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT)")
        .expect("schema");
    for i in 0..rows {
        conn.execute("INSERT INTO t(v) VALUES (?1)", [format!("v{i}")])
            .expect("insert");
    }
    dir
}

fn count(db: &Database) -> i64 {
    db.conn()
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .expect("count")
}

#[test]
fn opens_read_only_by_default_and_refuses_writes() {
    testing::reset();
    let dir = project_with("a.db", 3);
    let db = Database::open(dir.path(), "a.db", false).expect("open");
    assert_eq!((db.mode(), db.writable()), (OpenMode::ReadOnly, false));
    assert_eq!(count(&db), 3);
    let e = db
        .conn()
        .execute("INSERT INTO t(v) VALUES ('x')", [])
        .expect_err("read-only");
    assert!(db.error(&e).message().contains("readonly"), "{e}");
}

#[test]
fn writes_need_the_toggle() {
    testing::reset();
    let dir = project_with("a.db", 1);
    let db = Database::open(dir.path(), "a.db", true).expect("open");
    assert_eq!((db.mode(), db.writable()), (OpenMode::ReadWrite, true));
    db.conn()
        .execute("INSERT INTO t(v) VALUES ('x')", [])
        .expect("write");
    assert_eq!(count(&db), 2);
}

#[test]
fn a_wal_file_means_a_snapshot_without_its_changes() {
    testing::reset();
    let dir = project_with("w.db", 3);
    let writer = Connection::open(dir.path().join("w.db")).expect("writer");
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
             INSERT INTO t(v) VALUES ('only in the wal'), ('also');",
        )
        .expect("wal writes");
    assert!(dir.path().join("w.db-wal").is_file());
    let db = Database::open(dir.path(), "w.db", true).expect("open");
    assert_eq!((db.mode(), db.writable()), (OpenMode::Snapshot, false));
    assert_eq!(count(&db), 3, "uncheckpointed WAL rows are not shown");
    drop(writer);
}

#[test]
fn a_wal_header_without_a_wal_file_is_a_snapshot_too() {
    testing::reset();
    let dir = project_with("w.db", 2);
    {
        let conn = Connection::open(dir.path().join("w.db")).expect("conn");
        conn.execute_batch("PRAGMA journal_mode = WAL; INSERT INTO t(v) VALUES ('x');")
            .expect("wal");
    }
    assert!(
        !dir.path().join("w.db-wal").exists(),
        "checkpointed on close"
    );
    assert!(needs_snapshot(&dir.path().join("w.db")));
    let db = Database::open(dir.path(), "w.db", false).expect("open");
    assert_eq!(db.mode(), OpenMode::Snapshot);
    assert_eq!(count(&db), 3);
}

#[test]
fn bad_files_and_paths_are_messages() {
    testing::reset();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("junk.db"), vec![7u8; 4096]).expect("junk");
    let e = Database::open(dir.path(), "junk.db", false).expect_err("junk");
    assert!(e.message().contains("not a database"), "{e:?}");
    let e = Database::open(dir.path(), "missing.db", false).expect_err("missing");
    assert!(e.message().contains("file not found"), "{e:?}");
    for bad in ["../x.db", "/etc/passwd", "", "a/../../b.db"] {
        assert!(resolve(dir.path(), bad).is_err(), "{bad}");
    }
    assert!(resolve(dir.path(), "a/./b.db").is_ok());
}

const SLOW: &str = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n)
                    SELECT count(*) FROM n";

#[test]
fn cancel_interrupts_a_running_statement() {
    testing::reset();
    let dir = project_with("a.db", 1);
    let mut db = Database::open(dir.path(), "a.db", false).expect("open");
    testing::with(|h| h.cancelled = true);
    db.limit(Limit::UntilCancelled);
    let e = db
        .conn()
        .query_row(SLOW, [], |r| r.get::<_, i64>(0))
        .expect_err("cancelled");
    assert_eq!(db.error(&e), DbError::Cancelled);
    testing::with(|h| h.cancelled = false);
}

#[test]
fn deadlines_stop_interactive_statements() {
    testing::reset();
    let dir = project_with("a.db", 1);
    let mut db = Database::open(dir.path(), "a.db", false).expect("open");
    let limit = Duration::from_millis(30);
    db.limit(Limit::Deadline(limit));
    let started = Instant::now();
    let e = db
        .conn()
        .query_row(SLOW, [], |r| r.get::<_, i64>(0))
        .expect_err("deadline");
    assert_eq!(db.error(&e), DbError::TooSlow(limit));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(DbError::TooSlow(limit).message().contains("30 ms"));
}
