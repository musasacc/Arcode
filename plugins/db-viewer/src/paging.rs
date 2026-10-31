//! The paging SQL builder (pure): which `SELECT` answers `fetch-rows(start,
//! count)` for a table or view.
//!
//! Tables with a rowid page by **keyset on the rowid** (§10): pages already
//! seen leave [`Anchors`] (row index → rowid), and the next page seeks to
//! the nearest anchor, then skips at most the distance from it. A table
//! whose rowids are exactly `min..=min + count - 1` (no gaps) needs no
//! skipping at all. Views, `WITHOUT ROWID` tables and sorted grids fall back
//! to `LIMIT/OFFSET`.

use std::collections::BTreeMap;

use crate::sql::quote_ident;

/// Anchors kept per grid; beyond this the oldest (lowest) are dropped.
const MAX_ANCHORS: usize = 4096;

/// What a data grid shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Table or view name (unquoted).
    pub name: String,
    /// The rowid alias to page on (see [`crate::sql::rowid_alias`]); `None`
    /// for views and `WITHOUT ROWID` tables.
    pub rowid: Option<&'static str>,
    /// `ORDER BY` chosen with a `sort` event.
    pub sort: Option<Sort>,
}

/// A user-chosen sort column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    /// Column name (unquoted).
    pub column: String,
    /// Ascending or descending.
    pub ascending: bool,
}

/// What is known about the row set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shape {
    /// `count(*)`, when it finished in time.
    pub row_count: Option<u64>,
    /// `min(rowid)` when the rowids are dense (`max - min + 1 == count`).
    pub dense_min: Option<i64>,
}

/// Row index → rowid of rows already fetched in natural (rowid) order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchors(BTreeMap<u64, i64>);

impl Anchors {
    /// Remembers the first and last rowid of a page that started at `start`.
    pub fn record(&mut self, start: u64, rowids: &[i64]) {
        let (Some(first), Some(last)) = (rowids.first(), rowids.last()) else {
            return;
        };
        self.0.insert(start, *first);
        self.0.insert(start + rowids.len() as u64 - 1, *last);
        while self.0.len() > MAX_ANCHORS {
            self.0.pop_first();
        }
    }

    /// Forgets everything (new sort, refresh).
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Number of anchors.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when no page has been seen.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn at_or_below(&self, index: u64) -> Option<(u64, i64)> {
        self.0.range(..=index).next_back().map(|(k, v)| (*k, *v))
    }

    fn at_or_above(&self, index: u64) -> Option<(u64, i64)> {
        self.0.range(index..).next().map(|(k, v)| (*k, *v))
    }
}

/// How a page is reached; tests and the log use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Nothing to read (`start` past the known end, or `count == 0`).
    Empty,
    /// Dense rowids: seek straight to `min + start`.
    Dense,
    /// Skip `skip` rows from the start of the table.
    FromStart { skip: u64 },
    /// Seek to an anchor at or before `start`, then skip.
    AfterAnchor { skip: u64 },
    /// Seek to an anchor after the page, walk backwards, then skip.
    BeforeAnchor { skip: u64 },
    /// Walk backwards from the end of the table, then skip.
    FromEnd { skip: u64 },
    /// `LIMIT/OFFSET` without a rowid order (views, sorts, WITHOUT ROWID).
    Offset { skip: u64 },
}

/// One page query.
#[derive(Debug, Clone, PartialEq)]
pub struct PageQuery {
    /// The statement.
    pub sql: String,
    /// Its positional parameters (`?1`, `?2`, ...).
    pub params: Vec<i64>,
    /// The first result column is the rowid (not shown).
    pub leading_rowid: bool,
    /// Rows arrive in descending rowid order and must be reversed.
    pub reversed: bool,
    /// Rows the statement may return.
    pub count: u64,
    /// How the page is reached.
    pub strategy: Strategy,
}

/// SQLite takes `LIMIT/OFFSET` as signed 64-bit integers.
fn int(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The query for rows `start..start + count` of `source`.
pub fn page_query(
    source: &Source,
    anchors: &Anchors,
    shape: Shape,
    start: u64,
    count: u32,
) -> PageQuery {
    let table = quote_ident(&source.name);
    let mut count = u64::from(count);
    if let Some(n) = shape.row_count {
        count = count.min(n.saturating_sub(start));
    }
    let Some(rowid) = source.rowid else {
        // Views and WITHOUT ROWID tables: sortable too (ties keep SQLite's
        // order, which may differ between pages).
        let order = source.sort.as_ref().map_or_else(String::new, |sort| {
            let dir = if sort.ascending { "ASC" } else { "DESC" };
            format!(" ORDER BY {} {dir}", quote_ident(&sort.column))
        });
        return offset_query(format!("SELECT * FROM {table}{order}"), false, start, count);
    };
    if let Some(sort) = &source.sort {
        let dir = if sort.ascending { "ASC" } else { "DESC" };
        let sql = format!(
            "SELECT {rowid}, * FROM {table} ORDER BY {} {dir}, {rowid} {dir}",
            quote_ident(&sort.column)
        );
        return offset_query(sql, true, start, count);
    }
    let base = format!("SELECT {rowid}, * FROM {table}");
    if count == 0 {
        return query(format!("{base} LIMIT 0"), vec![], true, 0, Strategy::Empty);
    }
    if let Some(first) = shape.dense_min.and_then(|m| m.checked_add(int(start))) {
        let sql = format!("{base} WHERE {rowid} >= ?1 ORDER BY {rowid} LIMIT ?2");
        return query(sql, vec![first, int(count)], true, count, Strategy::Dense);
    }
    let last = start.saturating_add(count - 1);
    // Every way in, cheapest (fewest skipped rows) first.
    let mut options = vec![(start, Strategy::FromStart { skip: start })];
    if let Some((k, _)) = anchors.at_or_below(start) {
        options.push((start - k, Strategy::AfterAnchor { skip: start - k }));
    }
    if let Some((k, _)) = anchors.at_or_above(last) {
        options.push((k - last, Strategy::BeforeAnchor { skip: k - last }));
    }
    if let Some(n) = shape.row_count {
        let skip = n - 1 - last;
        options.push((skip, Strategy::FromEnd { skip }));
    }
    let strategy = options
        .into_iter()
        .min_by_key(|(skip, _)| *skip)
        .map_or(Strategy::FromStart { skip: start }, |(_, s)| s);
    let (sql, params, reversed) = match strategy {
        Strategy::AfterAnchor { skip } => {
            let (_, id) = anchors.at_or_below(start).unwrap_or((0, i64::MIN));
            let sql = format!("{base} WHERE {rowid} >= ?1 ORDER BY {rowid} LIMIT ?2 OFFSET ?3");
            (sql, vec![id, int(count), int(skip)], false)
        }
        Strategy::BeforeAnchor { skip } => {
            let (_, id) = anchors.at_or_above(last).unwrap_or((0, i64::MAX));
            let sql =
                format!("{base} WHERE {rowid} <= ?1 ORDER BY {rowid} DESC LIMIT ?2 OFFSET ?3");
            (sql, vec![id, int(count), int(skip)], true)
        }
        Strategy::FromEnd { skip } => {
            let sql = format!("{base} ORDER BY {rowid} DESC LIMIT ?1 OFFSET ?2");
            (sql, vec![int(count), int(skip)], true)
        }
        _ => {
            let sql = format!("{base} ORDER BY {rowid} LIMIT ?1 OFFSET ?2");
            (sql, vec![int(count), int(start)], false)
        }
    };
    PageQuery {
        sql,
        params,
        leading_rowid: true,
        reversed,
        count,
        strategy,
    }
}

fn offset_query(base: String, leading_rowid: bool, start: u64, count: u64) -> PageQuery {
    query(
        format!("{base} LIMIT ?1 OFFSET ?2"),
        vec![int(count), int(start)],
        leading_rowid,
        count,
        if count == 0 {
            Strategy::Empty
        } else {
            Strategy::Offset { skip: start }
        },
    )
}

fn query(
    sql: String,
    params: Vec<i64>,
    leading_rowid: bool,
    count: u64,
    strategy: Strategy,
) -> PageQuery {
    PageQuery {
        sql,
        params,
        leading_rowid,
        reversed: false,
        count,
        strategy,
    }
}

#[cfg(test)]
#[path = "paging_tests.rs"]
mod tests;
