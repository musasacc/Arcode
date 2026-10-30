//! Text selection in viewport ("snapshot") coordinates, backed by
//! alacritty's [`Selection`] so it follows content as the grid scrolls.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Term, viewport_to_point};

/// A cell position in snapshot coordinates (row 0 is the top visible row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct GridPoint {
    /// Viewport row.
    pub row: usize,
    /// Column.
    pub col: usize,
}

impl GridPoint {
    /// Creates a point.
    pub const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// How a selection grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SelectionKind {
    /// Character-wise (click and drag).
    #[default]
    Simple,
    /// Rectangular block.
    Block,
    /// Whole words (double click).
    Semantic,
    /// Whole lines (triple click).
    Lines,
}

impl From<SelectionKind> for SelectionType {
    fn from(kind: SelectionKind) -> Self {
        match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Block => SelectionType::Block,
            SelectionKind::Semantic => SelectionType::Semantic,
            SelectionKind::Lines => SelectionType::Lines,
        }
    }
}

/// The anchor a selection was started at, in grid (scroll-independent)
/// coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Anchor {
    point: Point,
    kind: SelectionKind,
}

/// Converts a viewport point into a grid point, clamped to the screen.
fn to_grid<T>(term: &Term<T>, p: GridPoint) -> Point {
    let row = p.row.min(term.screen_lines().saturating_sub(1));
    let col = p.col.min(term.columns().saturating_sub(1));
    let offset = term.grid().display_offset();
    viewport_to_point(offset, Point::new(row, Column(col)))
}

/// Starts a selection at `p`; returns its anchor.
pub(crate) fn start<T>(term: &mut Term<T>, p: GridPoint, kind: SelectionKind) -> Anchor {
    let point = to_grid(term, p);
    term.selection = Some(Selection::new(kind.into(), point, Side::Left));
    Anchor { point, kind }
}

/// Extends the selection from `anchor` to `p`, both cells inclusive.
pub(crate) fn update<T>(term: &mut Term<T>, anchor: Anchor, p: GridPoint) {
    let point = to_grid(term, p);
    let forward = match anchor.kind {
        SelectionKind::Block => point.column >= anchor.point.column,
        _ => point >= anchor.point,
    };
    let (start_side, end_side) = if forward {
        (Side::Left, Side::Right)
    } else {
        (Side::Right, Side::Left)
    };
    let mut selection = Selection::new(anchor.kind.into(), anchor.point, start_side);
    selection.update(point, end_side);
    term.selection = Some(selection);
}

/// The selected text, `None` when nothing (or only nothing) is selected.
pub(crate) fn text<T>(term: &Term<T>) -> Option<String> {
    term.selection_to_string().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    use crate::size::TermSize;

    fn term(text: &str) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(20, 4), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, text.as_bytes());
        term
    }

    #[test]
    fn simple_forward_and_backward() {
        let mut t = term("hello world\r\nsecond line");
        let a = start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
        assert_eq!(text(&t), None);
        update(&mut t, a, GridPoint::new(0, 4));
        assert_eq!(text(&t).as_deref(), Some("hello"));
        let a = start(&mut t, GridPoint::new(0, 10), SelectionKind::Simple);
        update(&mut t, a, GridPoint::new(0, 6));
        assert_eq!(text(&t).as_deref(), Some("world"));
        let a = start(&mut t, GridPoint::new(0, 6), SelectionKind::Simple);
        update(&mut t, a, GridPoint::new(1, 5));
        assert_eq!(text(&t).as_deref(), Some("world\nsecond"));
    }

    #[test]
    fn semantic_and_lines() {
        let mut t = term("alpha beta gamma\r\nnext");
        let a = start(&mut t, GridPoint::new(0, 7), SelectionKind::Semantic);
        update(&mut t, a, GridPoint::new(0, 7));
        assert_eq!(text(&t).as_deref(), Some("beta"));
        let a = start(&mut t, GridPoint::new(1, 1), SelectionKind::Lines);
        update(&mut t, a, GridPoint::new(1, 1));
        assert_eq!(text(&t).as_deref(), Some("next\n"));
    }

    #[test]
    fn block_selection() {
        let mut t = term("abcd\r\nefgh\r\nijkl");
        let a = start(&mut t, GridPoint::new(0, 2), SelectionKind::Block);
        update(&mut t, a, GridPoint::new(2, 1));
        assert_eq!(text(&t).as_deref(), Some("bc\nfg\njk"));
    }

    #[test]
    fn points_are_clamped() {
        let mut t = term("xy");
        let a = start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
        update(&mut t, a, GridPoint::new(99, 99));
        assert_eq!(text(&t).as_deref(), Some("xy\n\n\n"));
    }
}
