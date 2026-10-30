//! Grid and window size helpers.

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::Dimensions;

/// Smallest supported grid width.
pub const MIN_COLS: u16 = 2;
/// Smallest supported grid height.
pub const MIN_ROWS: u16 = 1;

/// Grid dimensions for creating and resizing a [`alacritty_terminal::Term`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TermSize {
    pub(crate) columns: usize,
    pub(crate) screen_lines: usize,
}

impl TermSize {
    pub(crate) fn new(columns: usize, screen_lines: usize) -> Self {
        Self {
            columns,
            screen_lines,
        }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// A clamped window size for the PTY plus matching grid dimensions.
pub(crate) fn window_size(
    cols: u16,
    rows: u16,
    cell_w: u16,
    cell_h: u16,
) -> (WindowSize, TermSize) {
    let cols = cols.max(MIN_COLS);
    let rows = rows.max(MIN_ROWS);
    let ws = WindowSize {
        num_lines: rows,
        num_cols: cols,
        cell_width: cell_w.max(1),
        cell_height: cell_h.max(1),
    };
    (ws, TermSize::new(usize::from(cols), usize::from(rows)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_to_minimum() {
        let (ws, ts) = window_size(0, 0, 0, 0);
        assert_eq!(
            (ws.num_cols, ws.num_lines, ws.cell_width, ws.cell_height),
            (2, 1, 1, 1)
        );
        assert_eq!(ts, TermSize::new(2, 1));
        let (ws, ts) = window_size(120, 40, 9, 18);
        assert_eq!((ws.num_cols, ws.num_lines), (120, 40));
        assert_eq!(
            (ts.columns(), ts.screen_lines(), ts.total_lines()),
            (120, 40, 40)
        );
    }
}
