//! Render-ready copies of the visible terminal grid.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode, point_to_viewport};
use alacritty_terminal::vte::ansi::CursorShape as TermCursorShape;

use crate::color::{ColorResolver, Palette, Rgb};

/// One grid cell with fully resolved colors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The character (a space for empty cells).
    pub ch: char,
    /// Zero-width characters (combining marks) drawn on top of `ch`.
    pub zerowidth: Vec<char>,
    /// Final foreground color.
    pub fg: Rgb,
    /// Final background color.
    pub bg: Rgb,
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Any kind of underline.
    pub underline: bool,
    /// Strikethrough.
    pub strikeout: bool,
    /// Dim text (color already darkened).
    pub dim: bool,
    /// Leading cell of a double-width character.
    pub wide: bool,
    /// Trailing half of a double-width character; skip when drawing.
    pub wide_spacer: bool,
    /// The cell is inside the current selection.
    pub selected: bool,
}

impl Cell {
    /// An empty cell in the palette's default colors.
    pub fn blank(palette: &Palette) -> Self {
        Self {
            ch: ' ',
            zerowidth: Vec::new(),
            fg: palette.foreground,
            bg: palette.background,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            dim: false,
            wide: false,
            wide_spacer: false,
            selected: false,
        }
    }
}

/// Cursor shape to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorShape {
    /// Filled block (also used for the hollow block style).
    #[default]
    Block,
    /// Vertical bar.
    Beam,
    /// Underscore.
    Underline,
}

/// Where and how to draw the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CursorState {
    /// Viewport row.
    pub row: usize,
    /// Column.
    pub col: usize,
    /// Shape.
    pub shape: CursorShape,
    /// Resolved cursor color.
    pub color: Rgb,
}

/// Terminal modes relevant to input handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TermModeFlags {
    /// Application cursor keys (DECCKM); pass to `encode_key`.
    pub app_cursor: bool,
    /// Bracketed paste is on.
    pub bracketed_paste: bool,
    /// The alternate screen is active (full-screen app).
    pub alt_screen: bool,
    /// The program wants mouse events.
    pub mouse_reporting: bool,
    /// Mouse events use SGR encoding.
    pub sgr_mouse: bool,
    /// Wheel scrolling in the alt screen should send arrow keys.
    pub alternate_scroll: bool,
}

impl From<TermMode> for TermModeFlags {
    fn from(mode: TermMode) -> Self {
        Self {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            mouse_reporting: mode.intersects(TermMode::MOUSE_MODE),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
        }
    }
}

/// A copy of the visible screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Width in cells.
    pub cols: usize,
    /// Height in cells.
    pub rows: usize,
    /// Row-major cells, `rows * cols` long.
    pub cells: Vec<Cell>,
    /// The cursor, `None` when hidden or scrolled out of view.
    pub cursor: Option<CursorState>,
    /// How many lines the view is scrolled back into history.
    pub display_offset: usize,
    /// Lines of scrollback history available.
    pub history_size: usize,
    /// Input-related terminal modes.
    pub mode: TermModeFlags,
}

impl Snapshot {
    /// The cell at `(row, col)`, if inside the grid.
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        self.cells.get(row * self.cols + col)
    }

    /// The cells of one row, `None` when `row` is outside the grid.
    pub fn row(&self, row: usize) -> Option<&[Cell]> {
        if row >= self.rows {
            return None;
        }
        let start = row.checked_mul(self.cols)?;
        let end = start.checked_add(self.cols)?;
        self.cells.get(start..end)
    }

    /// Iterates rows top to bottom.
    pub fn row_iter(&self) -> impl Iterator<Item = &[Cell]> {
        self.cells.chunks(self.cols.max(1))
    }

    /// One row as text, wide spacers skipped, trailing spaces trimmed.
    pub fn row_text(&self, row: usize) -> String {
        self.row(row).map(row_to_text).unwrap_or_default()
    }

    /// The whole screen as text, one line per row, trailing spaces trimmed.
    pub fn text(&self) -> String {
        self.row_iter()
            .map(row_to_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn row_to_text(cells: &[Cell]) -> String {
    let mut line = String::with_capacity(cells.len());
    for cell in cells.iter().filter(|c| !c.wide_spacer) {
        line.push(cell.ch);
        line.extend(cell.zerowidth.iter());
    }
    line.truncate(line.trim_end().len());
    line
}

/// Builds a [`Snapshot`] from the terminal state.
pub(crate) fn build<T: EventListener>(term: &Term<T>, palette: &Palette) -> Snapshot {
    let content = term.renderable_content();
    let rows = term.screen_lines();
    let cols = term.columns();
    let display_offset = content.display_offset;
    let resolver = ColorResolver::new(palette, Some(content.colors));
    let mut cells = vec![Cell::blank(palette); rows * cols];

    for indexed in content.display_iter {
        let Some(vp) = point_to_viewport(display_offset, indexed.point) else {
            continue;
        };
        if vp.line >= rows || vp.column.0 >= cols {
            continue;
        }
        let src = indexed.cell;
        let flags = src.flags;
        let (fg, bg) = resolver.cell_colors(src);
        let spacer = flags.contains(Flags::WIDE_CHAR_SPACER);
        let leading = flags.contains(Flags::LEADING_WIDE_CHAR_SPACER);
        cells[vp.line * cols + vp.column.0] = Cell {
            ch: if spacer || leading || src.c == '\t' {
                ' '
            } else {
                src.c
            },
            zerowidth: src.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
            fg,
            bg,
            bold: flags.contains(Flags::BOLD),
            italic: flags.contains(Flags::ITALIC),
            underline: flags.intersects(Flags::ALL_UNDERLINES),
            strikeout: flags.contains(Flags::STRIKEOUT),
            dim: flags.contains(Flags::DIM),
            wide: flags.contains(Flags::WIDE_CHAR),
            wide_spacer: spacer,
            selected: content.selection.is_some_and(|sel| {
                sel.contains(indexed.point)
                    || (flags.contains(Flags::WIDE_CHAR)
                        && sel.contains(Point::new(indexed.point.line, indexed.point.column + 1)))
            }),
        };
    }

    let cursor = cursor_state(
        content.cursor.shape,
        content.cursor.point,
        display_offset,
        rows,
        cols,
    )
    .map(|(row, col, shape)| CursorState {
        row,
        col,
        shape,
        color: resolver.cursor(),
    });

    Snapshot {
        cols,
        rows,
        cells,
        cursor,
        display_offset,
        history_size: term.history_size(),
        mode: content.mode.into(),
    }
}

/// Maps the terminal cursor into the viewport, `None` if hidden or not visible.
fn cursor_state(
    shape: TermCursorShape,
    point: Point,
    display_offset: usize,
    rows: usize,
    cols: usize,
) -> Option<(usize, usize, CursorShape)> {
    let shape = match shape {
        TermCursorShape::Hidden => return None,
        TermCursorShape::Block | TermCursorShape::HollowBlock => CursorShape::Block,
        TermCursorShape::Beam => CursorShape::Beam,
        TermCursorShape::Underline => CursorShape::Underline,
    };
    let vp = point_to_viewport(display_offset, point)?;
    (vp.line < rows && vp.column.0 < cols).then_some((vp.line, vp.column.0, shape))
}

/// Plain text of the visible screen.
pub(crate) fn text<T>(term: &Term<T>) -> String {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    let cols = term.columns();
    (0..term.screen_lines() as i32)
        .map(|row| {
            let row = &grid[Line(row - offset)];
            let mut line = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &row[Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                line.push(if cell.c == '\t' { ' ' } else { cell.c });
                line.extend(cell.zerowidth().unwrap_or_default());
            }
            line.truncate(line.trim_end().len());
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
