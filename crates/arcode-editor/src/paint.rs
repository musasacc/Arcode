//! Paint primitives computed in prepaint and drawn in paint.

use arcode_ui_kit::theme;
use gpui::{
    App, Bounds, ContentMask, Context, Hsla, PaintQuad, Pixels, Point, ShapedLine, SharedString,
    UnderlineStyle, Window, fill, point, px, size,
};

use crate::Editor;
use crate::diagnostics::line_severities;
use crate::layout::{LayoutSnapshot, MARKER_WIDTH};

/// Everything to draw besides the text itself, in paint order.
#[derive(Default)]
pub(crate) struct Decorations {
    pub background: Option<PaintQuad>,
    pub line_highlights: Vec<PaintQuad>,
    pub selections: Vec<PaintQuad>,
    pub brackets: Vec<PaintQuad>,
    pub underlines: Vec<(Point<Pixels>, Pixels, UnderlineStyle)>,
    pub cursors: Vec<PaintQuad>,
    pub gutter_labels: Vec<(Point<Pixels>, ShapedLine)>,
    pub gutter_markers: Vec<PaintQuad>,
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
    gutter(editor, layout, &colors, window, &mut d);
    scrollbar(editor, layout, &colors, &mut d);
    d
}

fn gutter(
    editor: &Editor,
    layout: &LayoutSnapshot,
    colors: &arcode_ui_kit::ThemeColors,
    window: &mut Window,
    d: &mut Decorations,
) {
    let visible = layout.visible_range();
    let active: Vec<usize> = editor
        .selections
        .iter()
        .map(|s| editor.buffer.char_to_line(s.head))
        .filter(|l| visible.contains(l))
        .collect();
    let left = layout.bounds.left();
    let severities = line_severities(&editor.diagnostics, visible.clone());
    for (line, severity) in visible.clone().zip(severities) {
        let top = layout.line_top(line);
        if let Some(sev) = severity {
            let dot = px(6.);
            let origin = point(
                left + (MARKER_WIDTH - dot) * 0.5,
                top + (layout.line_height - dot) * 0.5,
            );
            d.gutter_markers.push(
                fill(Bounds::new(origin, size(dot, dot)), sev.color(colors))
                    .corner_radii(dot * 0.5),
            );
        }
        if !layout.style.show_line_numbers {
            continue;
        }
        let color = if active.contains(&line) {
            colors.gutter_text_active
        } else {
            colors.gutter_text
        };
        let label = SharedString::from((line + 1).to_string());
        let shaped = Editor::shape_label(label, &layout.style, color, window);
        let x = left + layout.gutter_width - px(12.) - shaped.width;
        d.gutter_labels.push((point(x, top), shaped));
    }
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
        let frac = (layout.line_height * diag.range.start.line as f32) / content;
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
        let text_mask = Some(ContentMask {
            bounds: layout.text_bounds,
        });
        window.with_content_mask(text_mask, |window| {
            for q in self.selections.drain(..).chain(self.brackets.drain(..)) {
                window.paint_quad(q);
            }
            let left = layout.text_left();
            for line in &layout.lines {
                let origin = point(left, layout.line_top(line.line));
                if let Err(err) = line.shaped.paint(origin, layout.line_height, window, cx) {
                    log::debug!("failed to paint line {}: {err}", line.line);
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
