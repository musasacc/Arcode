//! The minimap: the document scaled down at the right edge of the editor,
//! left of the scrollbar.
//!
//! Every display row is [`ROW_PITCH`] tall and every column [`COL_WIDTH`]
//! wide; a line is drawn as blocks in its syntax colours (no glyphs), from
//! the same highlight spans the text uses. Behind the blocks: the viewport
//! slider, selections, search matches and other background highlights;
//! over them, change marks at the left edge and diagnostics. Clicking a
//! row centers it in the editor, dragging scrolls, the wheel scrolls the
//! editor.
//!
//! Only the rows in the minimap's view are built and painted (see
//! [`geometry`]), and their blocks are kept between frames (see [`rows`]).
//!
//! The host decides which editors may show one
//! ([`Editor::set_minimap_allowed`]); `editor.minimap` turns it off
//! everywhere, and low-end mode always does.

mod geometry;
mod mouse;
mod paint;
mod rows;

use std::ops::Range;
use std::rc::Rc;

use arcode_ui_kit::theme;
use gpui::{Bounds, Context, Pixels, point, px, size};

pub(crate) use geometry::{MinimapGeometry, ScrollState};
pub(crate) use mouse::MinimapDrag;
pub(crate) use paint::{MinimapPaint, minimap_decorations, paint_minimap};
use rows::{MinimapRows, RowsSource};

use crate::Editor;
use crate::ext::BlockMap;
use crate::layout::SCROLLBAR_WIDTH;
use crate::style::EditorStyle;

/// Height of one display row in the minimap.
pub(crate) const ROW_PITCH: Pixels = px(2.);
/// Width of one column.
pub(crate) const COL_WIDTH: Pixels = px(1.);
/// Space left of the blocks, where change marks are drawn.
pub(crate) const PAD_LEFT: Pixels = px(8.);
/// Space right of the blocks.
pub(crate) const PAD_RIGHT: Pixels = px(4.);
/// The minimap is shown only while the text area left beside it is at
/// least this many minimap widths wide, so a narrow split keeps its room
/// for text.
const MIN_TEXT_WIDTHS: f32 = 3.;

/// Per-editor minimap state.
#[derive(Default)]
pub(crate) struct MinimapState {
    /// Whether the host lets this editor show a minimap (`None`: not
    /// decided, no minimap).
    pub allowed: Option<bool>,
    /// The mouse is over the minimap (the slider is drawn brighter).
    pub hovered: bool,
    /// Blocks of the lines around the minimap's view, kept between frames.
    rows: Option<Rc<MinimapRows>>,
}

/// The minimap part of a layout pass.
pub(crate) struct MinimapLayout {
    pub geometry: MinimapGeometry,
    /// Buffer lines in the minimap's view.
    pub lines: Range<usize>,
    pub rows: Rc<MinimapRows>,
    pub line_count: usize,
}

impl MinimapLayout {
    /// Left edge of column 0 of the blocks.
    pub fn content_left(&self) -> Pixels {
        self.geometry.bounds.left() + PAD_LEFT
    }
}

/// Whether a minimap `minimap` wide leaves enough room for text in an
/// editor `width` wide with a gutter `gutter` wide.
fn fits(width: Pixels, gutter: Pixels, minimap: Pixels) -> bool {
    width - gutter - SCROLLBAR_WIDTH - minimap >= minimap * MIN_TEXT_WIDTHS
}

/// Buffer lines covered by display `rows`.
pub(crate) fn lines_for_rows(
    blocks: &BlockMap,
    rows: Range<usize>,
    line_count: usize,
) -> Range<usize> {
    if rows.is_empty() || line_count == 0 {
        return 0..0;
    }
    let first = blocks.row_kind(rows.start, line_count).line();
    let last = blocks.row_kind(rows.end - 1, line_count).line();
    first..(last + 1).min(line_count)
}

impl Editor {
    /// Lets this editor show a minimap (when `editor.minimap` is on and
    /// the editor is wide enough) or keeps it hidden. Editors have none
    /// until their host allows it; the workspace allows it for editors
    /// shown as tabs, except diffs.
    pub fn set_minimap_allowed(&mut self, allowed: bool, cx: &mut Context<Self>) {
        if self.minimap.allowed != Some(allowed) {
            self.minimap.allowed = Some(allowed);
            if !allowed {
                self.minimap.rows = None;
                self.minimap.hovered = false;
            }
            cx.notify();
        }
    }

    /// What [`Editor::set_minimap_allowed`] last set (`None` if never).
    pub fn minimap_allowed(&self) -> Option<bool> {
        self.minimap.allowed
    }

    /// The minimap's bounds in the last drawn frame (`None` while hidden).
    pub fn minimap_bounds(&self) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        Some(layout.minimap.as_ref()?.geometry.bounds)
    }

    /// The minimap's viewport slider in the last drawn frame.
    pub fn minimap_slider_bounds(&self) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        Some(layout.minimap.as_ref()?.geometry.slider)
    }

    /// Width the minimap takes in an editor `width` wide with a gutter
    /// `gutter_width` wide (zero when none is shown).
    pub(crate) fn minimap_width(
        &self,
        width: Pixels,
        gutter_width: Pixels,
        style: &EditorStyle,
    ) -> Pixels {
        match style.minimap_width {
            Some(w) if self.minimap.allowed == Some(true) && fits(width, gutter_width, w) => w,
            _ => Pixels::ZERO,
        }
    }

    /// Geometry of a minimap `width` wide at the right of `bounds` (left
    /// of the scrollbar), `height` tall.
    pub(crate) fn minimap_geometry(
        bounds: Bounds<Pixels>,
        width: Pixels,
        total_rows: usize,
        scroll: ScrollState,
    ) -> MinimapGeometry {
        let column = Bounds::new(
            point(bounds.right() - SCROLLBAR_WIDTH - width, bounds.top()),
            size(width, scroll.viewport_h),
        );
        MinimapGeometry::new(column, ROW_PITCH, total_rows, scroll)
    }

    /// The minimap's blocks for buffer `lines`, rebuilt only when needed.
    pub(crate) fn minimap_layout(
        &mut self,
        geometry: MinimapGeometry,
        lines: Range<usize>,
        style: &EditorStyle,
        cx: &Context<Self>,
    ) -> MinimapLayout {
        let content = geometry.bounds.size.width - PAD_LEFT - PAD_RIGHT;
        let max_cols = (content / COL_WIDTH).floor().max(0.) as usize;
        let source = RowsSource {
            buffer: &self.buffer,
            version: self.buffer.version(),
            highlights_id: self.highlights.as_ref().map_or(0, |h| h.id),
            highlights: self
                .highlights
                .as_ref()
                .map(|h| (h.lines.clone(), h.spans.as_slice())),
            theme: theme(cx),
            tab_size: style.tab_size,
            max_cols,
        };
        let current = self
            .minimap
            .rows
            .as_ref()
            .filter(|rows| rows.is_current(&source, &lines))
            .cloned();
        let rows = match current {
            Some(rows) => rows,
            None => {
                let started = std::time::Instant::now();
                let rows = Rc::new(MinimapRows::build(&source, lines.clone()));
                log::trace!("minimap blocks built in {:?}", started.elapsed());
                self.minimap.rows = Some(rows.clone());
                rows
            }
        };
        MinimapLayout {
            geometry,
            lines,
            rows,
            line_count: self.buffer.line_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_only_beside_enough_room_for_text() {
        let (w, gutter) = (px(96.), px(60.));
        // Text area = width - gutter - scrollbar - minimap.
        let narrowest = gutter + SCROLLBAR_WIDTH + w + w * MIN_TEXT_WIDTHS;
        assert!(fits(narrowest, gutter, w));
        assert!(!fits(narrowest - px(1.), gutter, w));
        // A wider gutter takes from the text, so it needs a wider editor.
        assert!(!fits(narrowest, gutter + px(20.), w));
        // A 460 px editor with a 90 px gutter would leave 262 px, about
        // 30 columns, of text.
        assert!(!fits(px(460.), px(90.), w));
        assert!(fits(px(800.), gutter, w));
    }

    #[test]
    fn rows_map_to_lines_through_block_rows() {
        let none = BlockMap::default();
        assert_eq!(lines_for_rows(&none, 10..20, 100), 10..20);
        assert_eq!(lines_for_rows(&none, 90..120, 100), 90..100);
        assert_eq!(lines_for_rows(&none, 5..5, 100), 0..0);
        assert_eq!(lines_for_rows(&none, 0..10, 0), 0..0);
        // A lens row above line 3 shifts the lines below it by one row.
        let one = BlockMap::new(vec![3], 100);
        assert_eq!(lines_for_rows(&one, 0..10, 100), 0..9);
    }
}

#[cfg(test)]
mod view_tests;
