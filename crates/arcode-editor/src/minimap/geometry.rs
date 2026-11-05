//! Minimap layout math: where display rows are drawn, the viewport slider,
//! and the editor scroll offset a click or drag asks for.
//!
//! The minimap draws every display row `pitch` pixels tall. When the whole
//! file (plus the half page the editor scrolls past the end) fits, row 0 is
//! at the top and the slider moves with the editor at `pitch / line height`
//! of its speed. A longer file scrolls the minimap too, proportionally: at
//! the top of the file the minimap shows its first rows, at the end its last
//! rows, and the slider travels the full height in between. Only the rows
//! in view are ever looked at, so the cost does not depend on the file size.

use std::ops::Range;

use gpui::{Bounds, Pixels, point, px, size};

/// Geometry of one minimap frame, in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MinimapGeometry {
    /// The minimap column.
    pub bounds: Bounds<Pixels>,
    /// Height of one display row.
    pub pitch: Pixels,
    /// Minimap content y shown at the top of `bounds` (the minimap's own
    /// scroll offset).
    pub offset: Pixels,
    /// The viewport slider.
    pub slider: Bounds<Pixels>,
    /// How far the slider moves per pixel of editor scroll.
    pub slider_per_scroll: f32,
    /// Display rows in the buffer.
    pub total_rows: usize,
}

/// Editor scroll state the minimap follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollState {
    pub scroll_y: Pixels,
    pub max_scroll_y: Pixels,
    pub viewport_h: Pixels,
    pub line_height: Pixels,
}

impl MinimapGeometry {
    /// Lays out a minimap of `total_rows` rows in `bounds` for `scroll`.
    pub fn new(
        bounds: Bounds<Pixels>,
        pitch: Pixels,
        total_rows: usize,
        scroll: ScrollState,
    ) -> Self {
        let lh = scroll.line_height.max(px(1.));
        let height = bounds.size.height.max(px(1.));
        let slider_h = (pitch * (scroll.viewport_h / lh)).clamp(pitch, height);
        // Rows the editor can scroll through, plus one viewport: what the
        // minimap would need to show it all.
        let content_h = pitch * (scroll.max_scroll_y / lh) + slider_h;
        let (offset, slider_per_scroll) =
            if content_h <= height || scroll.max_scroll_y <= Pixels::ZERO {
                (Pixels::ZERO, pitch / lh)
            } else {
                let frac = (scroll.scroll_y / scroll.max_scroll_y).clamp(0., 1.);
                // Whole pixels keep the blocks sharp.
                (
                    ((content_h - height) * frac).round(),
                    (height - slider_h) / scroll.max_scroll_y,
                )
            };
        let slider_top = bounds.top() + scroll.scroll_y * slider_per_scroll;
        let slider = Bounds::new(
            point(bounds.left(), slider_top),
            size(bounds.size.width, slider_h),
        );
        MinimapGeometry {
            bounds,
            pitch,
            offset,
            slider,
            slider_per_scroll,
            total_rows,
        }
    }

    /// Window y of the top of display row `row`.
    pub fn row_top(&self, row: usize) -> Pixels {
        self.bounds.top() + self.pitch * row as f32 - self.offset
    }

    /// The display row at window y `y`, clamped to the buffer.
    pub fn row_at(&self, y: Pixels) -> usize {
        let rel = (y - self.bounds.top() + self.offset).max(Pixels::ZERO);
        let row = (rel / self.pitch).floor() as usize;
        row.min(self.total_rows.saturating_sub(1))
    }

    /// Display rows that are at least partly inside the minimap.
    pub fn rows_in_view(&self) -> Range<usize> {
        let first = (self.offset / self.pitch).floor() as usize;
        let last = ((self.offset + self.bounds.size.height) / self.pitch).ceil() as usize;
        first.min(self.total_rows)..last.min(self.total_rows)
    }

    /// The editor scroll offset after dragging by `dy` from `start_scroll`.
    pub fn scroll_after_drag(
        &self,
        start_scroll: Pixels,
        dy: Pixels,
        max_scroll_y: Pixels,
    ) -> Pixels {
        if self.slider_per_scroll <= 0. {
            return start_scroll;
        }
        (start_scroll + dy / self.slider_per_scroll).clamp(Pixels::ZERO, max_scroll_y)
    }
}

/// The editor scroll offset that centers display row `row` in a viewport
/// `viewport_h` tall, clamped to `0..=max_scroll_y`.
pub(crate) fn scroll_centering_row(row: usize, scroll: ScrollState) -> Pixels {
    let lh = scroll.line_height;
    let top = lh * row as f32 - (scroll.viewport_h - lh) * 0.5;
    top.clamp(Pixels::ZERO, scroll.max_scroll_y.max(Pixels::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LH: Pixels = px(20.);
    const PITCH: Pixels = px(2.);

    fn bounds(h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(500.), px(40.)), size(px(96.), px(h)))
    }

    /// Scroll state of an editor with `rows` rows and a `viewport` tall view
    /// (it scrolls half a page past the end, as the editor does).
    fn state(rows: usize, viewport: f32, scroll_y: f32) -> ScrollState {
        let content = LH * rows as f32;
        let viewport_h = px(viewport);
        let max_scroll_y = if content <= viewport_h {
            Pixels::ZERO
        } else {
            content - viewport_h * 0.5
        };
        ScrollState {
            scroll_y: px(scroll_y).min(max_scroll_y),
            max_scroll_y,
            viewport_h,
            line_height: LH,
        }
    }

    #[test]
    fn short_file_maps_rows_one_to_one() {
        // 100 rows of 2 px fit in 400 px: no minimap scrolling.
        let s = state(100, 200., 300.);
        let g = MinimapGeometry::new(bounds(400.), PITCH, 100, s);
        assert_eq!(g.offset, Pixels::ZERO);
        assert_eq!(g.row_top(0), px(40.));
        assert_eq!(g.row_top(10), px(60.));
        assert_eq!(g.row_at(px(60.)), 10);
        assert_eq!(g.row_at(px(61.9)), 10);
        assert_eq!(g.row_at(px(0.)), 0, "above the minimap");
        assert_eq!(g.row_at(px(1000.)), 99, "below the last row");
        assert_eq!(g.rows_in_view(), 0..100);
        // A 200 px viewport is 10 rows: a 20 px slider at row 15 (300 px).
        assert_eq!(g.slider.size.height, px(20.));
        assert_eq!(g.slider.top(), g.row_top(15));
        assert_eq!(g.slider.size.width, px(96.));
    }

    #[test]
    fn long_file_scrolls_proportionally() {
        // 100 000 rows: far more than fits.
        let rows = 100_000;
        let top = state(rows, 800., 0.);
        let g = MinimapGeometry::new(bounds(800.), PITCH, rows, top);
        assert_eq!(g.offset, Pixels::ZERO);
        assert_eq!(g.slider.top(), px(40.));
        assert_eq!(g.rows_in_view(), 0..400);

        let end = state(rows, 800., f32::MAX);
        let g = MinimapGeometry::new(bounds(800.), PITCH, rows, end);
        // The slider sits at the bottom, the last rows are in view.
        assert!((g.slider.bottom() - g.bounds.bottom()).abs() < px(0.01));
        let view = g.rows_in_view();
        assert_eq!(view.end, rows);
        assert!(view.len() <= 401, "{view:?}");
        // The slider covers the rows the editor shows.
        let first_shown = (end.scroll_y / LH) as usize;
        let row = g.row_at(g.slider.top() + px(0.5));
        assert!(row.abs_diff(first_shown) <= 1, "{row} vs {first_shown}");

        let mid = state(rows, 800., 1_000_000.);
        let g = MinimapGeometry::new(bounds(800.), PITCH, rows, mid);
        let row = g.row_at(g.slider.top() + px(0.5));
        assert!(row.abs_diff(50_000) <= 1, "{row}");
        // Work stays proportional to the minimap's height.
        assert!(g.rows_in_view().len() <= 401);
    }

    #[test]
    fn slider_size_follows_the_viewport() {
        let g = MinimapGeometry::new(bounds(800.), PITCH, 5000, state(5000, 400., 0.));
        assert_eq!(g.slider.size.height, px(40.), "20 rows of 2 px");
        // Never thinner than a row, never taller than the minimap.
        let g = MinimapGeometry::new(bounds(30.), PITCH, 5000, state(5000, 4000., 0.));
        assert_eq!(g.slider.size.height, px(30.));
    }

    #[test]
    fn slider_drag_round_trips_to_the_scroll_offset() {
        for rows in [100, 1000, 100_000] {
            let s = state(rows, 600., 700.);
            let g = MinimapGeometry::new(bounds(600.), PITCH, rows, s);
            let back = g.scroll_after_drag(
                Pixels::ZERO,
                g.slider.top() - g.bounds.top(),
                s.max_scroll_y,
            );
            assert!((back - s.scroll_y).abs() < px(0.01), "{rows}: {back:?}");
            // Dragging by the slider's travel scrolls through the file.
            let again = g.scroll_after_drag(s.scroll_y, px(0.), s.max_scroll_y);
            assert_eq!(again, s.scroll_y);
            let down = g.scroll_after_drag(s.scroll_y, px(10_000.), s.max_scroll_y);
            assert_eq!(down, s.max_scroll_y);
            let up = g.scroll_after_drag(s.scroll_y, px(-10_000.), s.max_scroll_y);
            assert_eq!(up, Pixels::ZERO);
        }
        // Proportional: dragging 10 px in a long file moves 10 / travel
        // of the whole scroll range.
        let s = state(100_000, 800., 0.);
        let g = MinimapGeometry::new(bounds(800.), PITCH, 100_000, s);
        let travel = g.bounds.size.height - g.slider.size.height;
        let moved = g.scroll_after_drag(Pixels::ZERO, px(10.), s.max_scroll_y);
        let expected = s.max_scroll_y * (10. / f32::from(travel));
        assert!(
            (moved - expected).abs() < px(1.),
            "{moved:?} vs {expected:?}"
        );
    }

    #[test]
    fn click_centers_the_row() {
        let s = state(1000, 400., 0.);
        // Row 100 at 2000 px; centered in a 400 px view: 2000 - 190.
        assert_eq!(scroll_centering_row(100, s), px(1810.));
        assert_eq!(
            scroll_centering_row(2, s),
            Pixels::ZERO,
            "clamped at the top"
        );
        assert_eq!(scroll_centering_row(999, s), px(19_790.));
        assert_eq!(
            scroll_centering_row(5000, s),
            s.max_scroll_y,
            "and at the end"
        );
        // A click maps to the row under it, then to a scroll offset.
        let g = MinimapGeometry::new(bounds(400.), PITCH, 1000, s);
        let row = g.row_at(px(40.) + px(201.));
        assert_eq!(row, 100);
    }

    #[test]
    fn unscrollable_file_keeps_the_slider_at_the_top() {
        let s = state(5, 400., 0.);
        let g = MinimapGeometry::new(bounds(400.), PITCH, 5, s);
        assert_eq!(g.slider.top(), px(40.));
        assert_eq!(
            g.scroll_after_drag(Pixels::ZERO, px(300.), s.max_scroll_y),
            Pixels::ZERO
        );
        assert_eq!(g.rows_in_view(), 0..5);
    }
}
