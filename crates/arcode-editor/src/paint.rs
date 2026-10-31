//! Paint primitives computed in prepaint and drawn in paint.

use arcode_ui_kit::theme;
use gpui::{
    App, Bounds, ContentMask, Context, Corners, Hsla, PaintQuad, Path, Pixels, Point, ShapedLine,
    SharedString, TransformationMatrix, UnderlineStyle, Window, fill, point, px, size,
};

use crate::Editor;
use crate::ext::GutterLane;
use crate::layout::LayoutSnapshot;

/// An SVG icon in the gutter or in a lens.
pub(crate) struct GutterIcon {
    pub bounds: Bounds<Pixels>,
    pub path: SharedString,
    pub color: Hsla,
}

/// Everything to draw besides the text itself, in paint order.
#[derive(Default)]
pub(crate) struct Decorations {
    pub background: Option<PaintQuad>,
    pub line_highlights: Vec<PaintQuad>,
    /// Background highlights from extensions (whole lines).
    pub ext_line_highlights: Vec<PaintQuad>,
    /// Background highlights from extensions (text ranges).
    pub text_highlights: Vec<PaintQuad>,
    pub selections: Vec<PaintQuad>,
    pub brackets: Vec<PaintQuad>,
    pub underlines: Vec<(Point<Pixels>, Pixels, UnderlineStyle)>,
    /// Lens pills (diagnostics, the paused line) behind their text.
    pub lens_boxes: Vec<PaintQuad>,
    /// Icons inside lens pills.
    pub lens_icons: Vec<GutterIcon>,
    /// Virtual text and lenses that are not part of a shaped line.
    pub extras: Vec<(Point<Pixels>, ShapedLine)>,
    pub cursors: Vec<PaintQuad>,
    pub gutter_labels: Vec<(Point<Pixels>, ShapedLine)>,
    pub gutter_markers: Vec<PaintQuad>,
    /// Filled shapes in the gutter (the deleted-lines triangle).
    pub gutter_paths: Vec<(Path<Pixels>, Hsla)>,
    pub gutter_icons: Vec<GutterIcon>,
    pub scrollbar: Vec<PaintQuad>,
}

/// Inner corner radius of a rounded panel (its radius minus the 1 px
/// border): the editor's bottom corners follow it.
pub(crate) fn panel_inner_radius() -> Pixels {
    arcode_ui_kit::metrics::PANEL_RADIUS - px(1.)
}
/// Row bands stay this far from the editor's left and right edges.
const BAND_INSET: Pixels = px(1.);

/// The band behind the display row at `top` (current line, paused line,
/// whole-line highlights): inset 1 px from the editor's sides, with rounded
/// bottom corners where it reaches the bottom edge, so it never squares off
/// a rounded panel's corners.
pub(crate) fn row_band(layout: &LayoutSnapshot, top: Pixels, color: Hsla) -> Option<PaintQuad> {
    let b = layout.bounds;
    let bottom = (top + layout.line_height).min(b.bottom());
    let top = top.max(b.top());
    if bottom <= top {
        return None;
    }
    let bounds = Bounds::new(
        point(b.left() + BAND_INSET, top),
        size(b.size.width - BAND_INSET * 2., bottom - top),
    );
    let quad = fill(bounds, color);
    // Distance above the bottom edge; a band ending within 5 px of it
    // would poke out of the panel's corner curve at 1 px inset.
    let gap = b.bottom() - bottom;
    if gap < px(5.) {
        let r = (panel_inner_radius() - gap).min((bottom - top) * 0.5);
        return Some(quad.corner_radii(Corners {
            top_left: Pixels::ZERO,
            top_right: Pixels::ZERO,
            bottom_right: r,
            bottom_left: r,
        }));
    }
    Some(quad)
}

/// Computes the decorations for the current layout.
pub(crate) fn decorations(
    editor: &Editor,
    layout: &LayoutSnapshot,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Decorations {
    let colors = theme(cx).colors.clone();
    let mut d = Decorations {
        background: Some(
            fill(layout.bounds, colors.background).corner_radii(Corners {
                top_left: Pixels::ZERO,
                top_right: Pixels::ZERO,
                bottom_right: panel_inner_radius(),
                bottom_left: panel_inner_radius(),
            }),
        ),
        ..Default::default()
    };
    let focused = editor.focus_handle.is_focused(window);
    crate::decorate::text_decorations(editor, layout, focused, &colors, &mut d);
    crate::decorate_ext::ext_decorations(editor, layout, &colors, &mut d);
    crate::gutter::paint_gutter(editor, layout, &colors, window, &mut d);
    scrollbar(editor, layout, &colors, &mut d);
    d
}

/// Overview tick of the scrollbar: right offset from the track's right
/// edge, width, color.
fn tick(
    bar: &crate::layout::ScrollbarLayout,
    y: Pixels,
    right: Pixels,
    w: Pixels,
    color: Hsla,
) -> PaintQuad {
    let x = bar.track.right() - right - w;
    fill(Bounds::new(point(x, y), size(w, px(3.))), color).corner_radii(px(1.))
}

fn scrollbar(
    editor: &Editor,
    layout: &LayoutSnapshot,
    colors: &arcode_ui_kit::ThemeColors,
    d: &mut Decorations,
) {
    let Some(bar) = layout.scrollbar else {
        return;
    };
    let mut thumb_color: Hsla = colors.scrollbar_thumb;
    if editor.drag.is_some() {
        thumb_color = thumb_color.blend(colors.text_muted.opacity(0.25));
    }
    d.scrollbar
        .push(fill(bar.thumb, thumb_color).corner_radii(bar.thumb.size.width * 0.5));
    let content = layout.max_scroll_y + layout.text_bounds.size.height;
    let y_for = |line: usize| {
        let row = layout.blocks.row_for_line(line);
        let frac = (layout.line_height * row as f32) / content;
        (bar.track.top() + (bar.track.size.height - px(3.)) * frac.clamp(0., 1.)).floor()
    };
    // Breakpoints and the paused line: 6x3 in the accent.
    let marks = editor
        .ext
        .gutter
        .values()
        .flatten()
        .filter(|m| m.lane == GutterLane::Breakpoints)
        .map(|m| m.line)
        .chain(editor.ext.paused.as_ref().map(|p| p.line));
    let mut last = None;
    for line in marks {
        let y = y_for(line);
        if last != Some(y) {
            last = Some(y);
            d.scrollbar
                .push(tick(&bar, y, px(3.), px(6.), colors.accent));
        }
    }
    // Errors 8x3, warnings 4x3 at the right (half width); info and hints
    // have no tick. Errors are painted last so they stay on top.
    for (severity, width) in [
        (crate::DiagnosticSeverity::Warning, px(4.)),
        (crate::DiagnosticSeverity::Error, px(8.)),
    ] {
        let mut last = None;
        for diag in editor.diagnostics.iter().filter(|g| g.severity == severity) {
            let y = y_for(diag.range.start.line);
            if last != Some(y) {
                last = Some(y);
                d.scrollbar
                    .push(tick(&bar, y, px(2.), width, severity.color(colors)));
            }
        }
    }
}

impl Decorations {
    /// Draws the decorations and the visible text.
    pub(crate) fn paint(&mut self, layout: &LayoutSnapshot, window: &mut Window, cx: &mut App) {
        if let Some(bg) = self.background.take() {
            window.paint_quad(bg);
        }
        for q in self.line_highlights.drain(..) {
            window.paint_quad(q);
        }
        for q in self.ext_line_highlights.drain(..) {
            window.paint_quad(q);
        }
        let text_mask = Some(ContentMask {
            bounds: layout.text_bounds,
        });
        window.with_content_mask(text_mask, |window| {
            for q in self
                .text_highlights
                .drain(..)
                .chain(self.selections.drain(..))
                .chain(self.brackets.drain(..))
            {
                window.paint_quad(q);
            }
            let left = layout.text_left();
            for line in &layout.lines {
                let origin = point(left, layout.line_top(line.line));
                if let Err(err) = line.shaped.paint(origin, layout.line_height, window, cx) {
                    log::debug!("failed to paint line {}: {err}", line.line);
                }
            }
            for q in self.lens_boxes.drain(..) {
                window.paint_quad(q);
            }
            paint_icons(&mut self.lens_icons, window, cx);
            for (origin, shaped) in &self.extras {
                if let Err(err) = shaped.paint(*origin, layout.line_height, window, cx) {
                    log::debug!("failed to paint virtual text: {err}");
                }
            }
            for (origin, width, style) in self.underlines.drain(..) {
                window.paint_underline(origin, width, &style);
            }
            for q in self.cursors.drain(..) {
                window.paint_quad(q);
            }
        });
        let gutter = Bounds::new(
            layout.bounds.origin,
            size(layout.gutter_width, layout.text_bounds.size.height),
        );
        window.with_content_mask(Some(ContentMask { bounds: gutter }), |window| {
            for q in self.gutter_markers.drain(..) {
                window.paint_quad(q);
            }
            for (path, color) in self.gutter_paths.drain(..) {
                window.paint_path(path, color);
            }
            paint_icons(&mut self.gutter_icons, window, cx);
            for (origin, label) in self.gutter_labels.drain(..) {
                if let Err(err) = label.paint(origin, layout.line_height, window, cx) {
                    log::debug!("failed to paint gutter label: {err}");
                }
            }
        });
        for q in self.scrollbar.drain(..) {
            window.paint_quad(q);
        }
    }
}

fn paint_icons(icons: &mut Vec<GutterIcon>, window: &mut Window, cx: &mut App) {
    for icon in icons.drain(..) {
        let transform = TransformationMatrix::unit();
        if let Err(err) = window.paint_svg(icon.bounds, icon.path, transform, icon.color, cx) {
            log::debug!("failed to paint icon: {err}");
        }
    }
}
