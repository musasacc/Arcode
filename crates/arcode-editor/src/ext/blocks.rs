//! Mapping between buffer lines and display rows when some lines have a
//! block row (an "above" code lens) drawn on top of them.

/// What occupies a display row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    /// The text of a buffer line.
    Text(usize),
    /// The block row drawn above a buffer line.
    Block(usize),
}

impl RowKind {
    /// The buffer line the row belongs to.
    pub fn line(self) -> usize {
        match self {
            RowKind::Text(l) | RowKind::Block(l) => l,
        }
    }
}

/// Sorted, unique buffer lines that have one block row above them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BlockMap {
    lines: Vec<usize>,
}

impl BlockMap {
    /// Builds the map from any list of lines; lines at or past
    /// `line_count` are ignored.
    pub fn new(mut lines: Vec<usize>, line_count: usize) -> Self {
        lines.retain(|&l| l < line_count);
        lines.sort_unstable();
        lines.dedup();
        BlockMap { lines }
    }

    /// Whether `line` has a block row above it.
    pub fn has_block(&self, line: usize) -> bool {
        self.lines.binary_search(&line).is_ok()
    }

    /// Display row of buffer line `line`'s text.
    pub fn row_for_line(&self, line: usize) -> usize {
        line + self.lines.partition_point(|&l| l <= line)
    }

    /// Total display rows for a buffer of `line_count` lines.
    pub fn total_rows(&self, line_count: usize) -> usize {
        line_count + self.lines.len()
    }

    /// What is shown on display `row` (clamped to the last line).
    pub fn row_kind(&self, row: usize, line_count: usize) -> RowKind {
        let last = line_count.saturating_sub(1);
        if self.lines.is_empty() {
            return RowKind::Text(row.min(last));
        }
        // Smallest line whose text row is at or after `row`.
        let (mut lo, mut hi) = (0usize, last);
        if self.row_for_line(last) < row {
            return RowKind::Text(last);
        }
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.row_for_line(mid) >= row {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        if self.row_for_line(lo) == row {
            RowKind::Text(lo)
        } else {
            RowKind::Block(lo)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_lines_round_trip() {
        // Blocks above lines 0 and 3 in a 5-line buffer:
        // row 0 block(0), 1 text 0, 2 text 1, 3 text 2, 4 block(3), 5 text 3, 6 text 4.
        let map = BlockMap::new(vec![3, 0, 3, 9], 5);
        assert_eq!(map.total_rows(5), 7);
        let kinds: Vec<RowKind> = (0..7).map(|r| map.row_kind(r, 5)).collect();
        assert_eq!(
            kinds,
            vec![
                RowKind::Block(0),
                RowKind::Text(0),
                RowKind::Text(1),
                RowKind::Text(2),
                RowKind::Block(3),
                RowKind::Text(3),
                RowKind::Text(4),
            ]
        );
        for line in 0..5 {
            assert_eq!(map.row_kind(map.row_for_line(line), 5), RowKind::Text(line));
        }
        assert_eq!(map.row_kind(99, 5), RowKind::Text(4));
        assert!(map.has_block(3) && !map.has_block(1));
    }

    #[test]
    fn empty_map_is_identity() {
        let map = BlockMap::default();
        assert_eq!(map.row_for_line(7), 7);
        assert_eq!(map.row_kind(3, 10), RowKind::Text(3));
        assert_eq!(map.row_kind(30, 10), RowKind::Text(9));
        assert_eq!(map.row_kind(0, 0), RowKind::Text(0));
    }
}
