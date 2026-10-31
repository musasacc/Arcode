//! How table cells and columns are drawn: cell text, column widths and
//! the row-number column.

use arcode_plugin_host::bindings::ui::{Cell, Column};

/// Characters of a cell drawn (cells arrive cut to 4 KiB).
const MAX_CELL_CHARS: usize = 200;

/// Width of the row-number column for `rows` rows: room for the digits of
/// the last row number.
pub(super) fn index_width(rows: u64) -> f32 {
    let digits = rows.max(1).to_string().len() as f32;
    (digits * 8.5 + 20.).max(48.)
}

/// A cell as text: NULL, numbers, text (first line, capped), booleans and
/// blob sizes.
pub(super) fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Empty => "NULL".to_owned(),
        Cell::Int(v) => v.to_string(),
        Cell::Real(v) => v.to_string(),
        Cell::Text(t) => {
            let line: String = t
                .chars()
                .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                .take(MAX_CELL_CHARS)
                .collect();
            if t.chars().count() > MAX_CELL_CHARS {
                format!("{line}…")
            } else {
                line
            }
        }
        Cell::Boolean(b) => b.to_string(),
        Cell::Blob(n) => format!("<blob {n} bytes>"),
    }
}

/// Column width in pixels (`width` from the plugin, else by kind).
pub(super) fn column_width(column: &Column) -> f32 {
    column
        .width
        .map_or(if column.numeric { 110. } else { 180. }, |w| {
            (w as f32).clamp(40., 800.)
        })
}
