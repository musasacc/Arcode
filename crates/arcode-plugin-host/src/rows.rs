//! Table rows on the host side (§7 "Big tables never cross the boundary in
//! full"). Pure: no threads, no GPUI.
//!
//! - [`check_page`] validates a `fetch-rows` answer on the plugin thread:
//!   it must start where it was asked to, at most 4 MiB per page counting
//!   [`CELL_BYTES`] per cell plus its text, and text cells are cut to 4 KiB
//!   for display.
//! - [`RowCache`] is what a table widget keeps: an LRU of at most
//!   [`MAX_CACHED_PAGES`] pages of [`PAGE_ROWS`] rows, and the scheduling of
//!   fetches around the viewport (visible pages first, then one page of
//!   prefetch). Only one fetch per table is outstanding; when the viewport
//!   moves away from it a new one may be issued, and the mailbox coalesces
//!   the queued one away (latest range wins).

use std::collections::VecDeque;

use crate::bindings::ui::{Cell, RowPage};

/// Rows per page.
pub const PAGE_ROWS: u32 = 200;
/// Pages kept per table.
pub const MAX_CACHED_PAGES: usize = 6;
/// Bytes per page (§5 host caps): every cell costs [`CELL_BYTES`] plus its
/// text, so pages of many small cells are bounded too.
pub const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
/// What one cell costs against [`MAX_PAGE_BYTES`] before its text (at
/// least the host's in-memory size of a cell).
pub const CELL_BYTES: usize = 32;
const _: () = assert!(std::mem::size_of::<Cell>() <= CELL_BYTES);
/// Text shown per cell.
pub const MAX_CELL_BYTES: usize = 4 * 1024;

/// Checks a `fetch-rows` answer for a request of `count` rows at `start`.
/// Extra rows are dropped and long text cells are cut.
pub fn check_page(mut page: RowPage, start: u64, count: u32) -> Result<RowPage, String> {
    if page.start != start {
        return Err(format!(
            "fetch-rows answered rows from {} for a request at {start}",
            page.start
        ));
    }
    page.rows.truncate(count as usize);
    let bytes: usize = page
        .rows
        .iter()
        .flatten()
        .map(|c| match c {
            Cell::Text(t) => CELL_BYTES + t.len(),
            _ => CELL_BYTES,
        })
        .sum();
    if bytes > MAX_PAGE_BYTES {
        return Err(format!(
            "row page needs {bytes} bytes ({CELL_BYTES} per cell plus text; \
             limit {MAX_PAGE_BYTES})"
        ));
    }
    for cell in page.rows.iter_mut().flatten() {
        if let Cell::Text(t) = cell
            && t.len() > MAX_CELL_BYTES
        {
            let cut = crate::host_impl::cap(std::mem::take(t), MAX_CELL_BYTES - 3);
            *t = format!("{cut}…");
        }
    }
    Ok(page)
}

/// One `fetch-rows` call the app should dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    /// First row.
    pub start: u64,
    /// Rows wanted.
    pub count: u32,
    /// The cache generation it was issued for; answers to older
    /// generations are ignored.
    pub epoch: u64,
}

/// The rows of one table that the host keeps.
#[derive(Debug, Clone, Default)]
pub struct RowCache {
    row_count: Option<u64>,
    /// First row known to be past the end (from an `at-end` page).
    end: Option<u64>,
    epoch: u64,
    /// Most recently used first: (page index, rows).
    pages: VecDeque<(u64, Vec<Vec<Cell>>)>,
    in_flight: Option<u64>,
    viewport: (u64, u64),
}

impl RowCache {
    /// A cache for a table that declared `row_count` (`None`: unknown).
    pub fn new(row_count: Option<u64>) -> Self {
        Self {
            row_count,
            ..Self::default()
        }
    }

    /// The row count: declared, or learned from an `at-end` page.
    pub fn row_count(&self) -> Option<u64> {
        self.row_count.or(self.end)
    }

    /// Drops every page (new generation, `rows-changed`). Answers still in
    /// flight are ignored when they arrive.
    pub fn reset(&mut self, row_count: Option<u64>) {
        self.row_count = row_count;
        self.end = None;
        self.epoch += 1;
        self.pages.clear();
        self.in_flight = None;
    }

    /// The table kept its rows but now has `row_count` rows. The end
    /// learned from an `at-end` page is forgotten, rows past the new count
    /// are dropped, and a page that is now incomplete (the old partial last
    /// page of a table that grew) is fetched again.
    pub fn set_row_count(&mut self, row_count: Option<u64>) {
        self.row_count = row_count;
        self.end = None;
        let rows = u64::from(PAGE_ROWS);
        self.pages.retain_mut(|(p, page)| {
            let start = p.saturating_mul(rows);
            let want = row_count.map_or(rows, |n| n.saturating_sub(start).min(rows));
            let want = usize::try_from(want).unwrap_or(usize::MAX);
            page.truncate(want);
            want > 0 && page.len() == want
        });
    }

    /// The rows on screen, inclusive.
    pub fn set_viewport(&mut self, first: u64, last: u64) {
        self.viewport = (first.min(last), first.max(last));
    }

    /// Pages to have, most urgent first: the visible ones, then one ahead.
    fn wanted(&self) -> Vec<u64> {
        let rows = u64::from(PAGE_ROWS);
        let (first, last) = (self.viewport.0 / rows, self.viewport.1 / rows);
        let limit = self.row_count().map(|n| n.div_ceil(rows));
        (first..=last + 1)
            .filter(|p| limit.is_none_or(|l| *p < l))
            .collect()
    }

    /// The next fetch to dispatch, if any. Returns `None` while the
    /// outstanding fetch is still wanted or everything wanted is cached.
    pub fn next_fetch(&mut self) -> Option<PageRequest> {
        let wanted = self.wanted();
        if self.in_flight.is_some_and(|p| wanted.contains(&p)) {
            return None;
        }
        let page = wanted.into_iter().find(|p| !self.has_page(*p))?;
        self.in_flight = Some(page);
        Some(PageRequest {
            start: page * u64::from(PAGE_ROWS),
            count: PAGE_ROWS,
            epoch: self.epoch,
        })
    }

    /// Stores the answer to `request`. Returns false (and stores nothing)
    /// if the cache was reset since it was issued.
    pub fn page_arrived(&mut self, request: PageRequest, page: RowPage) -> bool {
        if request.epoch != self.epoch {
            return false;
        }
        let index = request.start / u64::from(PAGE_ROWS);
        if self.in_flight == Some(index) {
            self.in_flight = None;
        }
        if page.at_end {
            self.end = Some(request.start + page.rows.len() as u64);
        }
        self.pages.retain(|(p, _)| *p != index);
        self.pages.push_front((index, page.rows));
        self.pages.truncate(MAX_CACHED_PAGES);
        true
    }

    /// The fetch for `request` failed or was superseded.
    pub fn page_failed(&mut self, request: PageRequest) {
        let index = request.start / u64::from(PAGE_ROWS);
        if request.epoch == self.epoch && self.in_flight == Some(index) {
            self.in_flight = None;
        }
    }

    /// Row `index`, if its page is cached (marks the page recently used).
    pub fn row(&mut self, index: u64) -> Option<&[Cell]> {
        let rows = u64::from(PAGE_ROWS);
        let page = index / rows;
        let pos = self.pages.iter().position(|(p, _)| *p == page)?;
        if pos != 0 {
            let entry = self.pages.remove(pos)?;
            self.pages.push_front(entry);
        }
        let (_, page_rows) = self.pages.front()?;
        page_rows
            .get(usize::try_from(index % rows).ok()?)
            .map(Vec::as_slice)
    }

    fn has_page(&self, page: u64) -> bool {
        self.pages.iter().any(|(p, _)| *p == page)
    }

    /// Cached page indexes, most recently used first.
    pub fn cached_pages(&self) -> Vec<u64> {
        self.pages.iter().map(|(p, _)| *p).collect()
    }

    /// Rows held in memory.
    pub fn cached_rows(&self) -> usize {
        self.pages.iter().map(|(_, r)| r.len()).sum()
    }
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod tests;
