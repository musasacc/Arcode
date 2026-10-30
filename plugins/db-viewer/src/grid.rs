//! The Data tab's grid: one table or view, paged through `fetch-rows`
//! (§10 "Data tab").
//!
//! - The row count comes from a `count` that stops after 200 ms; if it does,
//!   the count is unknown and the host pages until a page reports `at-end`.
//! - Pages use [`crate::paging`]: keyset on the rowid where the table has
//!   one, `LIMIT/OFFSET` otherwise. A `sort` re-queries with `ORDER BY` and
//!   bumps the table's `generation`.

use std::time::{Duration, Instant};

use arcode_plugin::host;
use arcode_plugin::ui::{Cell, Column, RowPage, column};

use crate::cells::{cell, fit_page};
use crate::db::{ColumnInfo, Database, DbError, Limit, Object};
use crate::paging::{Anchors, PageQuery, Shape, Sort, Source, page_query};
use crate::sql::{quote_ident, rowid_alias};

/// How long counting may take before the count is reported unknown (§10).
pub const COUNT_DEADLINE: Duration = Duration::from_millis(200);

/// How long one page may take. Well under the host's 1 s budget for
/// `fetch-rows`, so a slow page is an error message, never a trap.
pub const PAGE_DEADLINE: Duration = Duration::from_millis(700);

/// Databases of at most this many pages are counted with `count(*)`,
/// which SQLite answers in one step the progress handler cannot stop (a
/// b-tree walk). Measured in wasm at about 65 µs per 4 KiB page, so this
/// bounds that step to roughly 270 ms. Bigger databases use `count(1)`,
/// which stops at [`COUNT_DEADLINE`]; the user can then count on demand.
const FAST_COUNT_MAX_PAGES: i64 = 4096;

/// How [`DataGrid`] counts rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counting {
    /// While selecting a table: bounded by [`COUNT_DEADLINE`].
    Interactive,
    /// The user asked (a task): until done or cancelled.
    UntilCancelled,
}

/// The grid of one table or view.
#[derive(Debug, Clone, PartialEq)]
pub struct DataGrid {
    /// What is shown.
    pub object: Object,
    /// Visible columns (`SELECT *` order).
    pub columns: Vec<ColumnInfo>,
    /// Paging source (name, rowid alias, sort).
    pub source: Source,
    /// Row count and rowid density.
    pub shape: Shape,
    /// Rowids of pages seen so far.
    pub anchors: Anchors,
    /// The table's `generation`; bumped on sort and refresh.
    pub generation: u32,
}

impl DataGrid {
    /// Reads columns, counts rows (bounded) and checks rowid density.
    pub fn load(db: &mut Database, object: Object, generation: u32) -> Result<Self, DbError> {
        let all = db.columns(&object.name)?;
        let rowid = if object.has_rowid() {
            rowid_alias(all.iter().map(|c| c.name.as_str()))
                .filter(|alias| probe(db, &format!("SELECT {alias} FROM {}", ident(&object))))
        } else {
            None
        };
        let columns = all.into_iter().filter(|c| !c.hidden).collect();
        let source = Source {
            name: object.name.clone(),
            rowid,
            sort: None,
        };
        let mut grid = Self {
            object,
            columns,
            source,
            shape: Shape::default(),
            anchors: Anchors::default(),
            generation,
        };
        // An interactive count never fails the selection: unknown is fine.
        grid.shape = grid.measure(db, Counting::Interactive).unwrap_or_default();
        Ok(grid)
    }

    /// Counts rows however long it takes (the "Count" button, a
    /// cancellable task). Cached pages stay valid.
    pub fn count_now(&mut self, db: &mut Database) -> Result<(), DbError> {
        self.shape = self.measure(db, Counting::UntilCancelled)?;
        Ok(())
    }

    /// Sorts by visible column `index` (a `sort` event).
    pub fn sort(&mut self, index: usize, ascending: bool) {
        if let Some(c) = self.columns.get(index) {
            self.source.sort = Some(Sort {
                column: c.name.clone(),
                ascending,
            });
            self.anchors.clear();
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Grid columns for the `table` widget.
    pub fn grid_columns(&self) -> Vec<Column> {
        self.columns
            .iter()
            .map(|c| Column {
                numeric: c.numeric(),
                sortable: true,
                ..column(c.name.clone())
            })
            .collect()
    }

    /// Rows `start..start + count`.
    pub fn fetch(&mut self, db: &mut Database, start: u64, count: u32) -> Result<RowPage, DbError> {
        let query = page_query(&self.source, &self.anchors, self.shape, start, count);
        db.limit(Limit::Deadline(PAGE_DEADLINE));
        let (rowids, mut rows) = run_page(db, &query).map_err(|e| db.error(&e))?;
        if query.leading_rowid && self.source.sort.is_none() {
            self.anchors.record(start, &rowids);
        }
        fit_page(&mut rows);
        let len = rows.len() as u64;
        let at_end =
            len < u64::from(count) || self.shape.row_count.is_some_and(|n| start + len >= n);
        Ok(RowPage {
            start,
            rows,
            at_end,
        })
    }

    /// Counts rows, then reads `min`/`max` of the rowid for the dense
    /// shortcut. Both costs go to the plugin log. An interactive count
    /// that runs out of time is `Ok` with an unknown count.
    fn measure(&self, db: &mut Database, counting: Counting) -> Result<Shape, DbError> {
        let table = ident(&self.object);
        let started = Instant::now();
        let small = db
            .conn()
            .query_row("PRAGMA page_count", [], |r| r.get::<_, i64>(0))
            .is_ok_and(|pages| pages <= FAST_COUNT_MAX_PAGES);
        let (expr, limit) = match counting {
            Counting::Interactive if small => ("count(*)", Limit::Deadline(COUNT_DEADLINE)),
            Counting::Interactive => ("count(1)", Limit::Deadline(COUNT_DEADLINE)),
            Counting::UntilCancelled => ("count(1)", Limit::UntilCancelled),
        };
        db.limit(limit);
        let counted = db
            .conn()
            .query_row(&format!("SELECT {expr} FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(|e| db.error(&e));
        let row_count = match counted {
            Ok(n) => u64::try_from(n).ok(),
            Err(e) if counting == Counting::UntilCancelled => return Err(e),
            Err(_) => None,
        };
        let count_ms = started.elapsed().as_millis();
        let dense_min = match (self.source.rowid, row_count) {
            (Some(alias), Some(n)) if n > 0 => {
                db.limit(Limit::Deadline(COUNT_DEADLINE));
                // Two scalar subqueries: SQLite's min/max optimization (one
                // b-tree seek each) only applies to a lone aggregate.
                let sql = format!(
                    "SELECT (SELECT min({alias}) FROM {table}), (SELECT max({alias}) FROM {table})"
                );
                db.conn()
                    .query_row(&sql, [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
                    .ok()
                    .filter(|(lo, hi)| {
                        hi.checked_sub(*lo)
                            .and_then(|span| u64::try_from(span).ok())
                            .is_some_and(|span| span + 1 == n)
                    })
                    .map(|(lo, _)| lo)
            }
            _ => None,
        };
        host::log(
            host::Level::Debug,
            &format!(
                "{}: {expr} = {row_count:?} in {count_ms} ms, dense: {}, {} ms in all",
                self.object.name,
                dense_min.is_some(),
                started.elapsed().as_millis()
            ),
        );
        Ok(Shape {
            row_count,
            dense_min,
        })
    }
}

fn ident(object: &Object) -> String {
    quote_ident(&object.name)
}

/// True when `sql` prepares (some virtual tables have no rowid).
fn probe(db: &Database, sql: &str) -> bool {
    db.conn().prepare(sql).is_ok()
}

/// Runs a page query; returns the rowids (when selected) and the cells.
fn run_page(db: &Database, q: &PageQuery) -> rusqlite::Result<(Vec<i64>, Vec<Vec<Cell>>)> {
    let mut stmt = db.conn().prepare(&q.sql)?;
    let width = stmt.column_count();
    let skip = usize::from(q.leading_rowid);
    let mut rows = stmt.query(rusqlite::params_from_iter(q.params.iter()))?;
    let mut ids = Vec::new();
    let mut cells = Vec::new();
    while let Some(row) = rows.next()? {
        if q.leading_rowid {
            ids.push(row.get::<_, i64>(0)?);
        }
        let mut out = Vec::with_capacity(width.saturating_sub(skip));
        for i in skip..width {
            out.push(cell(row.get_ref(i)?));
        }
        cells.push(out);
    }
    if q.reversed {
        ids.reverse();
        cells.reverse();
    }
    Ok((ids, cells))
}
