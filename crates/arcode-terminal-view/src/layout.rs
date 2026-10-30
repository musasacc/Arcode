//! Pure layout helpers: grid size from pixel bounds, runs of equal cell
//! backgrounds, and text segments batched by style for shaping.

use std::ops::Range;

use arcode_terminal::{Cell, GridPoint, MIN_COLS, MIN_ROWS, Rgb};

/// How many whole cells fit into `width` x `height` pixels, as
/// `(cols, rows)`, clamped to the terminal's minimum size.
pub fn grid_size(width: f32, height: f32, cell_width: f32, cell_height: f32) -> (u16, u16) {
    let fit = |len: f32, cell: f32, min: u16| -> u16 {
        if cell.is_nan() || cell <= 0. || !len.is_finite() || len <= 0. {
            return min;
        }
        let n = (len / cell).floor();
        if n >= f32::from(u16::MAX) {
            u16::MAX
        } else {
            (n as u16).max(min)
        }
    };
    (
        fit(width, cell_width, MIN_COLS),
        fit(height, cell_height, MIN_ROWS),
    )
}

/// The cell under a position relative to the grid origin, clamped to the
/// grid. Positions left of / above the grid map to the first column / row.
pub fn grid_point(
    x: f32,
    y: f32,
    cell_width: f32,
    cell_height: f32,
    cols: usize,
    rows: usize,
) -> GridPoint {
    let index = |pos: f32, cell: f32, count: usize| -> usize {
        if cell.is_nan() || cell <= 0. || pos.is_nan() || pos <= 0. || count == 0 {
            return 0;
        }
        ((pos / cell).floor() as usize).min(count - 1)
    };
    GridPoint::new(index(y, cell_height, rows), index(x, cell_width, cols))
}

/// Groups consecutive cells with an equal `key` into runs of columns;
/// cells whose key is `None` are not part of any run.
pub fn runs_by<K: PartialEq>(
    cells: &[Cell],
    key: impl Fn(&Cell) -> Option<K>,
) -> Vec<(Range<usize>, K)> {
    let mut runs: Vec<(Range<usize>, K)> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        let Some(k) = key(cell) else { continue };
        match runs.last_mut() {
            Some((range, last)) if range.end == col && *last == k => range.end = col + 1,
            _ => runs.push((col..col + 1, k)),
        }
    }
    runs
}

/// Runs of cells whose background differs from the default background.
pub fn background_runs(cells: &[Cell], default_bg: Rgb) -> Vec<(Range<usize>, Rgb)> {
    runs_by(cells, |c| (c.bg != default_bg).then_some(c.bg))
}

/// Runs of selected cells.
pub fn selection_runs(cells: &[Cell]) -> Vec<Range<usize>> {
    runs_by(cells, |c| c.selected.then_some(()))
        .into_iter()
        .map(|(range, ())| range)
        .collect()
}

/// Text attributes that require a separate text run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellStyle {
    /// Text color.
    pub fg: Rgb,
    /// Bold weight.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strikeout: bool,
}

impl CellStyle {
    fn of(cell: &Cell, fg: Rgb) -> Self {
        Self {
            fg,
            bold: cell.bold,
            italic: cell.italic,
            underline: cell.underline,
            strikeout: cell.strikeout,
        }
    }
}

/// A run of `len` UTF-8 bytes sharing one style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleRun {
    /// Length in bytes.
    pub len: usize,
    /// Style of the run.
    pub style: CellStyle,
}

/// A piece of a row to shape as one line, drawn starting at column `col`.
///
/// Normal segments hold one char per cell, so glyphs can be snapped to the
/// cell grid. Double-width characters and cells with combining marks get a
/// segment of their own (`single` is set) and are placed by column only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSegment {
    /// First column.
    pub col: usize,
    /// The text, without trailing blanks.
    pub text: String,
    /// Style runs covering `text`.
    pub runs: Vec<StyleRun>,
    /// One cell (wide char or combining marks) that must not be snapped.
    pub single: bool,
}

impl TextSegment {
    fn new(col: usize) -> Self {
        Self {
            col,
            text: String::new(),
            runs: Vec::new(),
            single: false,
        }
    }

    /// Appends a cell. `None` marks a blank, which joins the previous run
    /// unless that run draws line decorations.
    fn push(&mut self, cell: &Cell, style: Option<CellStyle>) {
        let start = self.text.len();
        self.text.push(cell.ch);
        self.text.extend(cell.zerowidth.iter());
        let len = self.text.len() - start;
        let style = match (self.runs.last_mut(), style) {
            (Some(last), None) if !last.style.underline && !last.style.strikeout => {
                last.len += len;
                return;
            }
            // A blank after a decorated run: plain, in the segment's font.
            (Some(last), None) => CellStyle {
                underline: false,
                strikeout: false,
                ..last.style
            },
            (Some(last), Some(style)) if last.style == style => {
                last.len += len;
                return;
            }
            (_, style) => style.unwrap_or(CellStyle::of(cell, cell.fg)),
        };
        self.runs.push(StyleRun { len, style });
    }

    /// Bold and italic of the segment (every run shares them).
    fn font_key(&self) -> Option<(bool, bool)> {
        self.runs.first().map(|r| (r.style.bold, r.style.italic))
    }

    /// Drops trailing text after `keep` bytes, shortening the runs.
    fn truncate(&mut self, keep: usize) {
        self.text.truncate(keep);
        let mut total = 0;
        self.runs.retain_mut(|run| {
            if total >= keep {
                return false;
            }
            run.len = run.len.min(keep - total);
            total += run.len;
            true
        });
    }
}

/// A cell that draws nothing: a plain space without line decorations.
fn is_blank(cell: &Cell) -> bool {
    cell.ch == ' ' && cell.zerowidth.is_empty() && !cell.underline && !cell.strikeout
}

/// Splits one row into text segments with style runs; all runs of a
/// segment share bold/italic. Leading and trailing blanks are skipped and
/// wide-char spacers are ignored. `fg_override`
/// recolors the text of one column (the cell under a block cursor).
pub fn text_segments(cells: &[Cell], fg_override: Option<(usize, Rgb)>) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut current: Option<(TextSegment, usize)> = None; // (segment, bytes to keep)
    let flush = |current: &mut Option<(TextSegment, usize)>, out: &mut Vec<TextSegment>| {
        if let Some((mut seg, keep)) = current.take() {
            seg.truncate(keep);
            if !seg.text.is_empty() {
                out.push(seg);
            }
        }
    };
    let mut next_col = 0;
    for (col, cell) in cells.iter().enumerate() {
        if cell.wide_spacer {
            continue;
        }
        let fg = match fg_override {
            Some((c, color)) if c == col => color,
            _ => cell.fg,
        };
        let style = CellStyle::of(cell, fg);
        let blank = is_blank(cell);
        if cell.wide || !cell.zerowidth.is_empty() {
            flush(&mut current, &mut segments);
            let mut seg = TextSegment::new(col);
            seg.single = true;
            seg.push(cell, Some(style));
            segments.push(seg);
            next_col = col + 1;
            continue;
        }
        // GPUI merges runs that differ only by font into one font run, so
        // a bold/italic change starts a new segment (shaped separately).
        let font_changed = !blank
            && current
                .as_ref()
                .and_then(|(seg, _)| seg.font_key())
                .is_some_and(|key| key != (cell.bold, cell.italic));
        if current.is_some() && (next_col != col || font_changed) {
            flush(&mut current, &mut segments);
        }
        if blank && current.is_none() {
            continue;
        }
        let (seg, keep) = current.get_or_insert_with(|| (TextSegment::new(col), 0));
        seg.push(cell, (!blank).then_some(style));
        if !blank {
            *keep = seg.text.len();
        }
        next_col = col + 1;
    }
    flush(&mut current, &mut segments);
    segments
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
