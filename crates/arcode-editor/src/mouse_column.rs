//! Column (box) selection with Alt+Shift+drag or the middle mouse button.

use arcode_buffer::SelectionSet;
use gpui::{Context, Pixels, Point, ShapedLine};

use crate::Editor;
use crate::commands::multi_cursor::{column_selections, grid_hit};
use crate::editor::Autoscroll;
use crate::layout::LayoutSnapshot;
use crate::mouse::DragState;

/// The byte index of the glyph boundary closest to `x` (the line end
/// included). Unlike `ShapedLine::closest_index_for_x`, a point inside the
/// last glyph can map to its start.
fn closest_index(shaped: &ShapedLine, x: Pixels) -> usize {
    let glyphs = shaped.runs.iter().flat_map(|run| run.glyphs.iter());
    glyphs
        .map(|g| (g.index, g.position.x))
        .chain(std::iter::once((shaped.len, shaped.width)))
        .min_by(|a, b| {
            let (da, db) = ((a.1 - x).abs(), (b.1 - x).abs());
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(0, |(index, _)| index)
}

impl Editor {
    /// `(line, x from the text origin)` under window position `p`. Points
    /// past the end of a line count, so a box can start right of short
    /// lines.
    fn column_point(&self, layout: &LayoutSnapshot, p: Point<Pixels>) -> (usize, Pixels) {
        let line = layout.row_at(p.y, self.buffer.line_count()).line();
        (line, (p.x - layout.text_left()).max(Pixels::ZERO))
    }

    pub(crate) fn column_drag_start(
        &mut self,
        layout: &LayoutSnapshot,
        p: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let anchor = self.column_point(layout, p);
        self.buffer.break_undo_group();
        self.drag = Some(DragState::Column { anchor });
        self.column_drag_move(layout, anchor, p, cx);
    }

    pub(crate) fn column_drag_move(
        &mut self,
        layout: &LayoutSnapshot,
        anchor: (usize, Pixels),
        p: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let head = self.column_point(layout, p);
        let sels = self.column_selections_at(layout, anchor, head);
        if sels != self.selections {
            self.selections = sels;
            self.autoscroll = Some(Autoscroll::Fit);
            self.selections_changed(cx);
        }
    }

    fn column_selections_at(
        &self,
        layout: &LayoutSnapshot,
        anchor: (usize, Pixels),
        head: (usize, Pixels),
    ) -> SelectionSet {
        let buffer = &self.buffer;
        let grid = grid_hit(buffer, layout.tab_size, layout.char_width);
        // Laid-out lines map x through their shaped text (wide glyphs,
        // ligatures, inlay hints); others fall back to the grid.
        let hit = |line: usize, x: Pixels| match layout.visible(line) {
            Some(v) => {
                let len = buffer.line_len(line);
                let col = v.display.col_for_byte(closest_index(&v.shaped, x));
                (col.min(len), layout.x_for(line, len))
            }
            None => grid(line, x),
        };
        column_selections(buffer, anchor, head, layout.char_width, &hit)
    }
}
