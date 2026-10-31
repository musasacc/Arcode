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

/// Converts a viewport point into a grid point, clamped to the screen.
fn to_grid<T>(term: &Term<T>, p: GridPoint) -> Point {
    let row = p.row.min(term.screen_lines().saturating_sub(1));
    let col = p.col.min(term.columns().saturating_sub(1));
    let offset = term.grid().display_offset();
    viewport_to_point(offset, Point::new(row, Column(col)))
}

/// Starts a selection at `p`, replacing any previous one.
pub(crate) fn start<T>(term: &mut Term<T>, p: GridPoint, kind: SelectionKind) {
    let point = to_grid(term, p);
    term.selection = Some(Selection::new(kind.into(), point, Side::Left));
}

/// Extends the current selection to `p`, both ends inclusive. Returns
/// `false` (and does nothing) when there is no selection.
///
/// The start is kept inside alacritty's selection, which the terminal
/// moves along whenever output scrolls the screen, so a drag that goes on
/// while output arrives stays anchored to the same text.
pub(crate) fn update<T>(term: &mut Term<T>, p: GridPoint) -> bool {
    let point = to_grid(term, p);
    let Some(selection) = term.selection.as_mut() else {
        return false;
    };
    selection.update(point, Side::Left);
    // Pick the sides so that both the start and the end cell are included.
    selection.include_all();
    true
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
        start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
        assert_eq!(text(&t), None);
        update(&mut t, GridPoint::new(0, 4));
        assert_eq!(text(&t).as_deref(), Some("hello"));
        start(&mut t, GridPoint::new(0, 10), SelectionKind::Simple);
        update(&mut t, GridPoint::new(0, 6));
        assert_eq!(text(&t).as_deref(), Some("world"));
        start(&mut t, GridPoint::new(0, 6), SelectionKind::Simple);
        update(&mut t, GridPoint::new(1, 5));
        assert_eq!(text(&t).as_deref(), Some("world\nsecond"));
    }

    #[test]
    fn semantic_and_lines() {
        let mut t = term("alpha beta gamma\r\nnext");
        start(&mut t, GridPoint::new(0, 7), SelectionKind::Semantic);
        update(&mut t, GridPoint::new(0, 7));
        assert_eq!(text(&t).as_deref(), Some("beta"));
        start(&mut t, GridPoint::new(1, 1), SelectionKind::Lines);
        update(&mut t, GridPoint::new(1, 1));
        assert_eq!(text(&t).as_deref(), Some("next\n"));
    }

    #[test]
    fn block_selection() {
        let mut t = term("abcd\r\nefgh\r\nijkl");
        start(&mut t, GridPoint::new(0, 2), SelectionKind::Block);
        update(&mut t, GridPoint::new(2, 1));
        assert_eq!(text(&t).as_deref(), Some("bc\nfg\njk"));
    }

    #[test]
    fn points_are_clamped() {
        let mut t = term("xy");
        start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
        update(&mut t, GridPoint::new(99, 99));
        assert_eq!(text(&t).as_deref(), Some("xy\n\n\n"));
    }

    #[test]
    fn drag_back_across_the_start() {
        let mut t = term("hello world");
        start(&mut t, GridPoint::new(0, 6), SelectionKind::Simple);
        update(&mut t, GridPoint::new(0, 10));
        assert_eq!(text(&t).as_deref(), Some("world"));
        update(&mut t, GridPoint::new(0, 0));
        assert_eq!(text(&t).as_deref(), Some("hello w"));
    }

    #[test]
    fn update_without_selection_does_nothing() {
        let mut t = term("abc");
        assert!(!update(&mut t, GridPoint::new(0, 1)));
        assert_eq!(text(&t), None);
        start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
        assert!(update(&mut t, GridPoint::new(0, 1)));
        assert_eq!(text(&t).as_deref(), Some("ab"));
    }

    #[test]
    fn start_follows_scrolling_output() {
        let mut t = term("anchor\r\n");
        start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
        // Five more lines scroll "anchor" off the 4-row screen.
        let mut parser: Processor = Processor::new();
        parser.advance(&mut t, b"l1\r\nl2\r\nl3\r\nl4\r\nl5");
        update(&mut t, GridPoint::new(3, 1));
        assert_eq!(text(&t).as_deref(), Some("anchor\nl1\nl2\nl3\nl4\nl5"));
    }

    #[test]
    fn points_use_the_scrolled_view() {
        let mut t = term("a\r\nb\r\nc\r\nd\r\ne\r\nf");
        t.scroll_display(alacritty_terminal::grid::Scroll::Delta(2));
        start(&mut t, GridPoint::new(0, 0), SelectionKind::Lines);
        update(&mut t, GridPoint::new(0, 0));
        assert_eq!(text(&t).as_deref(), Some("a\n"));
    }
}
