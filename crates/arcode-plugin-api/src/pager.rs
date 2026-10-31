//! [`TablePager`]: a plugin-side row store served page by page.
//!
//! Big result sets never cross the boundary in full: the plugin keeps the
//! rows (up to a row and byte cap) and answers `fetch-rows` from here.

use crate::ui::{Cell, RowPage};

/// Rows of one result set, capped by count and approximate bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct TablePager {
    rows: Vec<Vec<Cell>>,
    bytes: usize,
    max_rows: usize,
    max_bytes: usize,
    truncated: bool,
}

impl TablePager {
    /// An empty store holding at most `max_rows` rows and about `max_bytes`.
    pub fn new(max_rows: usize, max_bytes: usize) -> Self {
        Self {
            rows: Vec::new(),
            bytes: 0,
            max_rows,
            max_bytes,
            truncated: false,
        }
    }

    /// Adds a row. Returns false (and marks the set truncated) once a cap is
    /// reached; callers should stop producing rows then.
    pub fn push(&mut self, row: Vec<Cell>) -> bool {
        let size = row_size(&row);
        if self.truncated || self.rows.len() >= self.max_rows || self.bytes + size > self.max_bytes
        {
            self.truncated = true;
            return false;
        }
        self.bytes += size;
        self.rows.push(row);
        true
    }

    /// Number of stored rows.
    pub fn len(&self) -> u64 {
        self.rows.len() as u64
    }

    /// True when no rows are stored.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// True when rows were dropped because of a cap.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Drops every row and the truncation flag.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.bytes = 0;
        self.truncated = false;
    }

    /// Rows `start..start + count`, clamped to what is stored.
    pub fn page(&self, start: u64, count: u32) -> RowPage {
        let len = self.rows.len();
        let from = usize::try_from(start).unwrap_or(usize::MAX).min(len);
        let to = from.saturating_add(count as usize).min(len);
        RowPage {
            start,
            rows: self.rows[from..to].to_vec(),
            at_end: to == len,
        }
    }
}

/// Approximate heap size of a row, used for the byte cap.
fn row_size(row: &[Cell]) -> usize {
    row.iter()
        .map(|cell| match cell {
            Cell::Text(s) => 16 + s.len(),
            _ => 16,
        })
        .sum::<usize>()
        + 24
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_row(i: i64) -> Vec<Cell> {
        vec![Cell::Int(i), Cell::Text(format!("row {i}"))]
    }

    #[test]
    fn pages_are_clamped_and_report_the_end() {
        let mut pager = TablePager::new(1000, 1 << 20);
        for i in 0..250 {
            assert!(pager.push(int_row(i)));
        }
        let first = pager.page(0, 200);
        assert_eq!(first.rows.len(), 200);
        assert!(!first.at_end);
        let last = pager.page(200, 200);
        assert_eq!(last.rows.len(), 50);
        assert_eq!(last.rows[0][0], Cell::Int(200));
        assert!(last.at_end);
        let past = pager.page(u64::MAX, 10);
        assert!(past.rows.is_empty() && past.at_end);
    }

    #[test]
    fn caps_truncate() {
        let mut by_rows = TablePager::new(3, 1 << 20);
        let accepted = (0..10).take_while(|&i| by_rows.push(int_row(i))).count();
        assert_eq!((accepted, by_rows.len(), by_rows.truncated()), (3, 3, true));

        let mut by_bytes = TablePager::new(1000, 200);
        while by_bytes.push(int_row(1)) {}
        assert!(by_bytes.truncated() && by_bytes.len() < 10);
        by_bytes.clear();
        assert!(by_bytes.is_empty() && !by_bytes.truncated());
    }
}
