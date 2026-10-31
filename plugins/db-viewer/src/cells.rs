//! SQLite values → grid cells, and the size rules that keep a row page
//! under the host's caps (§5: cells shown cut to 4 KiB, 4 MiB per page).

use arcode_plugin::ui::Cell;
use rusqlite::types::ValueRef;

/// Longest text cell the plugin sends (the host cuts at 4 KiB too).
pub const MAX_CELL_BYTES: usize = 4096;

/// What the host charges per cell on top of its text
/// (`arcode_plugin_host::rows::CELL_BYTES`).
pub const HOST_CELL_BYTES: usize = 32;

/// Bytes per page the plugin aims for, counted like the host does
/// ([`HOST_CELL_BYTES`] per cell plus text). The host refuses pages over
/// 4 MiB; the rest is headroom for ellipses.
pub const PAGE_BUDGET: usize = 3 * 1024 * 1024 + 512 * 1024;

/// The cell for one SQLite value. Text that is not UTF-8 is shown lossily;
/// blobs show only their size.
pub fn cell(value: ValueRef<'_>) -> Cell {
    match value {
        ValueRef::Null => Cell::Empty,
        ValueRef::Integer(i) => Cell::Int(i),
        ValueRef::Real(r) => Cell::Real(r),
        ValueRef::Text(bytes) => Cell::Text(cut(&String::from_utf8_lossy(bytes), MAX_CELL_BYTES)),
        ValueRef::Blob(bytes) => Cell::Blob(bytes.len() as u64),
    }
}

/// `text` cut to at most `max` bytes on a char boundary, with an ellipsis
/// when something was dropped.
pub fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn text_len(cell: &Cell) -> usize {
    match cell {
        Cell::Text(t) => t.len(),
        _ => 0,
    }
}

/// Cuts text cells further when a page costs more than [`PAGE_BUDGET`],
/// so a page of wide rows is shown shortened instead of being refused by
/// the host. Every text cell gets an equal share of what the cells' fixed
/// cost leaves; a share too small for any text leaves just "…". (A table
/// so wide that the fixed cost alone is over the host's cap, about 650
/// columns at 200 rows, still cannot be paged.)
pub fn fit_page(rows: &mut [Vec<Cell>]) {
    let cells: usize = rows.iter().map(Vec::len).sum();
    let fixed = cells.saturating_mul(HOST_CELL_BYTES);
    let text: usize = rows.iter().flatten().map(text_len).sum();
    if fixed.saturating_add(text) <= PAGE_BUDGET {
        return;
    }
    let text_cells = rows.iter().flatten().filter(|c| text_len(c) > 0).count();
    let per_cell = PAGE_BUDGET.saturating_sub(fixed) / text_cells.max(1);
    for c in rows.iter_mut().flatten() {
        if let Cell::Text(t) = c
            && t.len() > per_cell
        {
            *t = cut(t, per_cell);
        }
    }
}

/// A short, plain-text rendering of a cell (status lines, tests).
pub fn display(cell: &Cell) -> String {
    match cell {
        Cell::Empty => "NULL".to_owned(),
        Cell::Int(i) => i.to_string(),
        Cell::Real(r) => r.to_string(),
        Cell::Text(t) => t.clone(),
        Cell::Boolean(b) => b.to_string(),
        Cell::Blob(n) => format!("<{n} bytes>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_map_to_cells() {
        assert_eq!(cell(ValueRef::Null), Cell::Empty);
        assert_eq!(cell(ValueRef::Integer(-3)), Cell::Int(-3));
        assert_eq!(cell(ValueRef::Real(1.5)), Cell::Real(1.5));
        assert_eq!(cell(ValueRef::Text(b"hi")), Cell::Text("hi".into()));
        assert_eq!(
            cell(ValueRef::Text(b"a\xffb")),
            Cell::Text("a\u{fffd}b".into())
        );
        assert_eq!(cell(ValueRef::Blob(&[1, 2, 3])), Cell::Blob(3));
    }

    #[test]
    fn long_text_is_cut_on_a_char_boundary() {
        let long = "é".repeat(5000);
        let Cell::Text(t) = cell(ValueRef::Text(long.as_bytes())) else {
            panic!("text");
        };
        assert!(t.len() <= MAX_CELL_BYTES && t.ends_with('…'));
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut("abcdef", 4), "a…");
    }

    #[test]
    fn wide_pages_fit_the_budget() {
        let mut rows: Vec<Vec<Cell>> = (0..200)
            .map(|_| (0..8).map(|_| Cell::Text("x".repeat(4000))).collect())
            .collect();
        fit_page(&mut rows);
        let text: usize = rows.iter().flatten().map(text_len).sum();
        let fixed = 200 * 8 * HOST_CELL_BYTES;
        assert!(fixed + text <= PAGE_BUDGET, "{text}");
        assert!(
            text > PAGE_BUDGET - fixed - 200 * 8 * 4,
            "cut no further than needed"
        );
        let mut small = vec![vec![Cell::Text("keep".into()), Cell::Int(1)]];
        fit_page(&mut small);
        assert_eq!(small[0][0], Cell::Text("keep".into()));
    }

    #[test]
    fn display_is_plain_text() {
        assert_eq!(display(&Cell::Empty), "NULL");
        assert_eq!(display(&Cell::Blob(9)), "<9 bytes>");
    }
}
