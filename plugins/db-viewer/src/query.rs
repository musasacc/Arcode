//! Running the Query tab's SQL (§10 "Running a query").
//!
//! Every statement of the text runs in order. The last statement that
//! returns rows fills a [`TablePager`] (capped at `max-result-rows` and
//! 64 MiB, with a "truncated" note) that `fetch-rows` serves page by page.
//! Statements that change rows are summed into "N rows changed".
//!
//! With writes off, a statement that would write is refused with SQLite's
//! own read-only message before it runs (this also covers `VACUUM INTO`,
//! which could otherwise create a file through a read-write grant).

use std::time::{Duration, Instant};

use arcode_plugin::TablePager;
use arcode_plugin::ui::{Column, column};
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{Batch, Statement};

use crate::cells::cell;
use crate::db::{Database, DbError, Limit};

/// Bytes of cell data one result may keep (§10).
pub const MAX_RESULT_BYTES: usize = 64 * 1024 * 1024;

/// What one run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// Columns of the last result set; empty when no statement returned rows.
    pub columns: Vec<Column>,
    /// Its rows.
    pub rows: TablePager,
    /// Whether any statement returned rows.
    pub has_rows: bool,
    /// Rows changed by `INSERT`/`UPDATE`/`DELETE` statements.
    pub changed: u64,
    /// Whether any statement changed the database (schema included).
    pub wrote: bool,
    /// Statements run.
    pub statements: usize,
    /// Wall time.
    pub elapsed: Duration,
}

impl Outcome {
    /// The status line, e.g. `12 rows · 3 ms` or `2 rows changed · 1 ms`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.has_rows {
            parts.push(plural(self.rows.len(), "row"));
        }
        if self.wrote || !self.has_rows {
            parts.push(format!("{} changed", plural(self.changed, "row")));
        }
        if self.statements > 1 {
            parts.push(plural(self.statements as u64, "statement"));
        }
        parts.push(format!("{} ms", self.elapsed.as_millis()));
        parts.join(" · ")
    }

    /// The truncation note, when a cap dropped rows.
    pub fn truncation(&self) -> Option<String> {
        self.rows.truncated().then(|| {
            format!(
                "Truncated: showing the first {} (limit: max-result-rows or 64 MiB)",
                plural(self.rows.len(), "row")
            )
        })
    }
}

fn plural(n: u64, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Runs `sql` until it finishes or the user cancels.
pub fn run(db: &mut Database, sql: &str, max_rows: usize) -> Result<Outcome, DbError> {
    let started = Instant::now();
    db.limit(Limit::UntilCancelled);
    let mut out = Outcome {
        columns: Vec::new(),
        rows: TablePager::new(max_rows, MAX_RESULT_BYTES),
        has_rows: false,
        changed: 0,
        wrote: false,
        statements: 0,
        elapsed: Duration::ZERO,
    };
    let writable = db.writable();
    let result = run_all(db, sql, writable, max_rows, &mut out);
    out.elapsed = started.elapsed();
    result.map(|()| out).map_err(|e| match e {
        Failure::Sqlite(e) => db.error(&e),
        Failure::ReadOnly => DbError::Message(
            "attempt to write a readonly database (writes are off: tick \"Allow writes\")"
                .to_owned(),
        ),
    })
}

enum Failure {
    Sqlite(rusqlite::Error),
    ReadOnly,
}

impl From<rusqlite::Error> for Failure {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

fn run_all(
    db: &Database,
    sql: &str,
    writable: bool,
    max_rows: usize,
    out: &mut Outcome,
) -> Result<(), Failure> {
    let conn = db.conn();
    let mut batch = Batch::new(conn, sql);
    while let Some(mut stmt) = batch.next()? {
        if !writable && !stmt.readonly() {
            return Err(Failure::ReadOnly);
        }
        out.statements += 1;
        let before = conn.total_changes();
        if stmt.column_count() > 0 {
            out.columns = stmt.column_names().map(|c| column(c.to_owned())).collect();
            out.rows = TablePager::new(max_rows, MAX_RESULT_BYTES);
            out.has_rows = true;
            collect(&mut stmt, &mut out.rows)?;
        } else {
            stmt.raw_execute()?;
        }
        if !stmt.readonly() {
            out.wrote = true;
            out.changed += conn.total_changes().saturating_sub(before);
        }
    }
    Ok(())
}

/// Steps through every row, keeping what fits; stepping goes on after the
/// cap only until the statement is done, so writes with `RETURNING` finish.
fn collect(stmt: &mut Statement<'_>, pager: &mut TablePager) -> rusqlite::Result<()> {
    let width = stmt.column_count();
    let returning = !stmt.readonly();
    let mut rows = stmt.raw_query();
    while let Some(row) = rows.next()? {
        if pager.truncated() {
            if returning {
                continue;
            }
            break;
        }
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            cells.push(cell(row.get_ref(i)?));
        }
        pager.push(cells);
    }
    Ok(())
}
