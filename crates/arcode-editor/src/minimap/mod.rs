//! The minimap: the document scaled down at the right edge of the editor,
//! left of the scrollbar, shown only while the document is taller than the
//! editor (it can scroll down).
//!
//! Every char is drawn as a tiny glyph in its syntax colour (see
//! [`glyphs`]): one device pixel wide and two tall at 1x, sharper at
//! higher scales, so indentation, word gaps and line lengths stay visible.
//! The glyphs are rasterized into image tiles that are kept between frames
//! (see [`tiles`]). Behind them: the viewport slider, selections, search
//! matches and other background highlights; over them, change marks at the
//! left edge and diagnostics. Clicking a row centers it in the editor,
//! dragging scrolls, the wheel scrolls the editor, and hovering shows the
//! code around the row under the mouse at full size (see [`preview`]).
//!
//! Only the rows in the minimap's view are built and painted (see
//! [`geometry`]).
//!
//! The host decides which editors may show one
//! ([`Editor::set_minimap_allowed`]); `editor.minimap` turns it off
//! everywhere, and low-end mode always does.

mod geometry;
mod glyphs;
mod mouse;
mod paint;
mod preview;
mod raster;
mod tiles;

use std::ops::Range;
use std::sync::Arc;

use arcode_ui_kit::theme;
use gpui::{Bounds, Context, Pixels, RenderImage, Window, point, px, size};

pub(crate) use geometry::{MinimapGeometry, ScrollState};
use glyphs::Patterns;
pub(crate) use mouse::MinimapDrag;
pub(crate) use paint::{MinimapPaint, minimap_decorations, paint_minimap};
pub(crate) use preview::defer_minimap_preview;
use tiles::{PlacedTile, TileCache, TileSource};

use crate::Editor;
use crate::ext::BlockMap;
use crate::layout::SCROLLBAR_WIDTH;
use crate::style::EditorStyle;

/// Space left of the glyphs, where change marks are drawn.
pub(crate) const PAD_LEFT: Pixels = px(8.);
/// Space right of the glyphs.
pub(crate) const PAD_RIGHT: Pixels = px(4.);
/// The minimap is shown only while the text area left beside it is at
/// least this many minimap widths wide, so a narrow split keeps its room
/// for text.
const MIN_TEXT_WIDTHS: f32 = 3.;

/// Size of one char of the minimap in device pixels at `scale`: 1 x 2 at
/// 1x, rounded multiples above.
pub(crate) fn cell_for_scale(scale: f32) -> (usize, usize) {
    let scale = scale.max(0.5);
    let col = scale.round().max(1.) as usize;
    let pitch = (scale * 2.).round().max(2.) as usize;
    (col, pitch)
}

/// Per-editor minimap state.
#[derive(Default)]
pub(crate) struct MinimapState {
    /// Whether the host lets this editor show a minimap (`None`: not
    /// decided, no minimap).
    pub allowed: Option<bool>,
    /// The mouse is over the minimap (the slider is drawn brighter and the
    /// preview shows).
    pub hovered: bool,
    /// Window y of the mouse while it is over the minimap.
    pub hover_y: Option<Pixels>,
    /// The window's hover state seen at the last frame (Windows only).
    window_hovered: bool,
    /// The hover preview drawn in the last frame.
    preview: Option<preview::PreviewPlan>,
    /// The tiles of the minimap's view, kept between frames.
    tiles: TileCache,
    /// Glyph patterns for the current scale.
    patterns: Option<Arc<Patterns>>,
    /// Retired tiles, dropped from the atlas at the next layout (the frame
    /// on screen may still show them).
    retired: Vec<Arc<RenderImage>>,
    /// Whether the editor drops its images when it is released.
    release_hooked: bool,
}

impl MinimapState {
    /// Every image held, for dropping them from the atlas.
    fn take_images(&mut self) -> Vec<Arc<RenderImage>> {
        let mut images = std::mem::take(&mut self.retired);
        images.extend(self.tiles.take_all());
        images
    }
}

/// The minimap part of a layout pass.
pub(crate) struct MinimapLayout {
    pub geometry: MinimapGeometry,
    /// Buffer lines in the minimap's view.
    pub lines: Range<usize>,
    /// The image tiles covering the rows in view.
    pub tiles: Vec<PlacedTile>,
    /// Size of a whole tile.
    pub tile_size: gpui::Size<Pixels>,
    /// Width of one column.
    pub col_width: Pixels,
}

impl MinimapLayout {
    /// Left edge of column 0 of the glyphs.
    pub fn content_left(&self) -> Pixels {
        self.geometry.bounds.left() + PAD_LEFT
    }

    /// Columns that fit between the paddings.
    pub fn max_cols(&self) -> usize {
        max_cols(self.geometry.bounds.size.width, self.col_width)
    }
}

fn max_cols(width: Pixels, col_width: Pixels) -> usize {
    ((width - PAD_LEFT - PAD_RIGHT) / col_width).floor().max(0.) as usize
}

/// Whether a document of `rows` display rows overflows a view
/// `viewport_h` tall, i.e. the editor can scroll down. The minimap is
/// shown only then. The answer depends on the height alone, never on the
/// width the minimap takes, so showing or hiding the minimap cannot change
/// it and the decision cannot flip back and forth between frames.
pub(crate) fn overflows(rows: usize, line_height: Pixels, viewport_h: Pixels) -> bool {
    line_height * rows as f32 > viewport_h
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
    /// Lets this editor show a minimap (when `editor.minimap` is on, the
    /// file is taller than the editor and the editor is wide enough) or
    /// keeps it hidden. Editors have none until their host allows it; the
    /// workspace allows it for editors shown as tabs, except diffs.
    pub fn set_minimap_allowed(&mut self, allowed: bool, cx: &mut Context<Self>) {
        if allowed && !self.minimap.release_hooked {
            self.minimap.release_hooked = true;
            cx.on_release(|editor: &mut Editor, cx| {
                for image in editor.minimap.take_images() {
                    cx.drop_image(image, None);
                }
            })
            .detach();
        }
        if self.minimap.allowed != Some(allowed) {
            self.minimap.allowed = Some(allowed);
            if !allowed {
                self.minimap.tiles.retire_all();
                self.minimap.hovered = false;
                self.minimap.hover_y = None;
                self.minimap.preview = None;
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

    /// The lines the minimap's hover preview showed in the last frame and
    /// its bounds (`None` while it is hidden).
    pub fn minimap_preview(&self) -> Option<(Range<usize>, Bounds<Pixels>)> {
        let plan = self.minimap.preview.as_ref()?;
        Some((plan.lines.clone(), plan.bounds))
    }

    /// Width the minimap takes in an editor `width` wide with a gutter
    /// `gutter_width` wide, for a document that `overflows` the editor
    /// (zero when none is shown).
    pub(crate) fn minimap_width(
        &self,
        width: Pixels,
        gutter_width: Pixels,
        overflows: bool,
        style: &EditorStyle,
    ) -> Pixels {
        match style.minimap_width {
            Some(w)
                if overflows
                    && self.minimap.allowed == Some(true)
                    && fits(width, gutter_width, w) =>
            {
                w
            }
            _ => Pixels::ZERO,
        }
    }

    /// Geometry of a minimap `width` wide at the right of `bounds` (left
    /// of the scrollbar), as tall as the viewport, at `scale`.
    pub(crate) fn minimap_geometry(
        bounds: Bounds<Pixels>,
        width: Pixels,
        total_rows: usize,
        scroll: ScrollState,
        scale: f32,
    ) -> MinimapGeometry {
        let column = Bounds::new(
            point(bounds.right() - SCROLLBAR_WIDTH - width, bounds.top()),
            size(width, scroll.viewport_h),
        );
        let (_, pitch) = cell_for_scale(scale);
        let pitch = px(pitch as f32 / scale.max(0.5));
        MinimapGeometry::new(column, pitch, total_rows, scroll, scale)
    }

    /// Drops the images retired in the previous frame from the atlas; with
    /// no minimap this frame, retires the ones still held.
    pub(crate) fn minimap_frame_start(
        &mut self,
        shown: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for image in std::mem::take(&mut self.minimap.retired) {
            cx.drop_image(image, Some(window));
        }
        if !shown {
            self.minimap.tiles.retire_all();
            self.minimap.retired = self.minimap.tiles.take_retired();
        }
    }

    /// The minimap's tiles for its view, built only when needed.
    pub(crate) fn minimap_layout(
        &mut self,
        geometry: MinimapGeometry,
        lines: Range<usize>,
        blocks: &BlockMap,
        style: &EditorStyle,
        scale: f32,
        cx: &Context<Self>,
    ) -> MinimapLayout {
        let scale = scale.max(0.5);
        let (col, pitch) = cell_for_scale(scale);
        let patterns = match &self.minimap.patterns {
            Some(p) if p.col == col && p.pitch == pitch => p.clone(),
            _ => {
                let p = Arc::new(Patterns::new(col, pitch));
                self.minimap.patterns = Some(p.clone());
                p
            }
        };
        let col_width = px(col as f32 / scale);
        let max_cols = max_cols(geometry.bounds.size.width, col_width);
        let line_count = self.buffer.line_count();
        let source = TileSource {
            buffer: &self.buffer,
            stamp: (
                self.buffer.version(),
                self.highlights.as_ref().map_or(0, |h| h.id),
            ),
            highlights: self
                .highlights
                .as_ref()
                .map(|h| (h.lines.clone(), h.spans.as_slice())),
            theme: theme(cx),
            tab_size: style.tab_size,
            max_cols,
            patterns: &patterns,
            blocks,
            line_count,
        };
        let built = self.minimap.tiles.built;
        let started = std::time::Instant::now();
        let tiles = self
            .minimap
            .tiles
            .tiles_for(&source, geometry.rows_in_view());
        if self.minimap.tiles.built > built {
            log::trace!(
                "minimap: {} tiles rasterized in {:?}",
                self.minimap.tiles.built - built,
                started.elapsed()
            );
        }
        self.minimap.retired = self.minimap.tiles.take_retired();
        MinimapLayout {
            geometry,
            lines,
            tiles,
            tile_size: size(
                col_width * max_cols as f32,
                geometry.pitch * tiles::TILE_ROWS as f32,
            ),
            col_width,
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
    fn shown_only_when_the_document_overflows() {
        let lh = px(20.);
        assert!(!overflows(0, lh, px(400.)));
        assert!(!overflows(20, lh, px(400.)), "exactly fits");
        assert!(overflows(21, lh, px(400.)));
        // A taller line (bigger font) overflows sooner.
        assert!(overflows(20, px(21.), px(400.)));
        // A shorter view (window resized) too.
        assert!(overflows(20, lh, px(399.)));
    }

    #[test]
    fn cells_follow_the_scale() {
        assert_eq!(cell_for_scale(1.), (1, 2));
        assert_eq!(cell_for_scale(1.25), (1, 3));
        assert_eq!(cell_for_scale(1.5), (2, 3));
        assert_eq!(cell_for_scale(2.), (2, 4));
        assert_eq!(cell_for_scale(3.), (3, 6));
        assert_eq!(cell_for_scale(0.), (1, 2));
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
#[cfg(test)]
mod visibility_tests;
