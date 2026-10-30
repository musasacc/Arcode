//! Paint primitives computed in prepaint and drawn in paint.

use arcode_ui_kit::theme;
use gpui::{
    App, Bounds, ContentMask, Context, Hsla, PaintQuad, Pixels, Point, ShapedLine, SharedString,
    TransformationMatrix, UnderlineStyle, Window, fill, point, px, size,
};

use crate::Editor;
use crate::layout::LayoutSnapshot;

/// An SVG icon in the gutter.
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
    /// Virtual text and lenses that are not part of a shaped line.
    pub extras: Vec<(Point<Pixels>, ShapedLine)>,
    pub cursors: Vec<PaintQuad>,
    pub gutter_labels: Vec<(Point<Pixels>, ShapedLine)>,
    pub gutter_markers: Vec<PaintQuad>,
    pub gutter_icons: Vec<GutterIcon>,
    pub scrollbar: Vec<PaintQuad>,
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
        background: Some(fill(layout.bounds, colors.background)),
        ..Default::default()
    };
    let focused = editor.focus_handle.is_focused(window);
    crate::decorate::text_decorations(editor, layout, focused, &colors, &mut d);
    crate::decorate_ext::ext_decorations(editor, layout, &colors, &mut d);
    crate::gutter::paint_gutter(editor, layout, &colors, window, &mut d);
    scrollbar(editor, layout, &colors, &mut d);
    d
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
    let content = layout.max_scroll_y + layout.text_bounds.size.height;
    let mut last_y = None;
    for diag in &editor.diagnostics {
        let row = layout.blocks.row_for_line(diag.range.start.line);
        let frac = (layout.line_height * row as f32) / content;
        let y = (bar.track.top() + bar.track.size.height * frac.clamp(0., 1.)).floor();
        if last_y == Some((y, diag.severity)) {
            continue;
        }
        last_y = Some((y, diag.severity));
        d.scrollbar.push(fill(
            Bounds::new(
                point(bar.track.left() + px(2.), y),
                size(bar.track.size.width - px(4.), px(2.)),
            ),
            diag.severity.color(colors),
        ));
    }
    let mut thumb_color: Hsla = colors.scrollbar_thumb;
    if editor.drag.is_some() {
        thumb_color.a = (thumb_color.a * 1.5).min(1.);
    }
    d.scrollbar
        .push(fill(bar.thumb, thumb_color).corner_radii(px(4.)));
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
            size(layout.gutter_width, layout.bounds.size.height),
        );
        window.with_content_mask(Some(ContentMask { bounds: gutter }), |window| {
            for q in self.gutter_markers.drain(..) {
                window.paint_quad(q);
            }
            for icon in self.gutter_icons.drain(..) {
                let transform = TransformationMatrix::unit();
                if let Err(err) =
                    window.paint_svg(icon.bounds, icon.path, transform, icon.color, cx)
                {
                    log::debug!("failed to paint gutter icon: {err}");
                }
            }
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
