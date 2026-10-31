//! Text-area decorations: current line, selections, cursors, bracket pair,
//! IME composition and diagnostic underlines.

use std::cmp::Reverse;

use arcode_buffer::Point as BufPoint;
use arcode_ui_kit::ThemeColors;
use gpui::{Bounds, Hsla, Pixels, UnderlineStyle, fill, outline, point, px, size};

use crate::ext::CursorShape;
use crate::layout::LayoutSnapshot;
use crate::paint::{Decorations, row_band};
use crate::{DiagnosticSeverity, Editor};

/// Computes text-area decorations for the visible lines.
pub(crate) fn text_decorations(
    editor: &Editor,
    layout: &LayoutSnapshot,
    focused: bool,
    colors: &ThemeColors,
    d: &mut Decorations,
) {
    let visible = layout.visible_range();
    if visible.is_empty() {
        return;
    }
    let buffer = &editor.buffer;
    let vis_start = buffer.line_start(visible.start);
    let vis_end = buffer.line_start(visible.end);
    let in_view = |pos: usize| (vis_start..=vis_end).contains(&pos);

    // The paused line's band replaces the current-line band on its line.
    let paused = editor
        .ext
        .paused
        .as_ref()
        .map(|p| p.line)
        .filter(|l| visible.contains(l));
    if let Some(band) =
        paused.and_then(|l| row_band(layout, layout.line_top(l), colors.paused_line))
    {
        d.line_highlights.push(band);
    }
    if layout.style.highlight_current_line {
        let mut lines: Vec<usize> = paused.into_iter().collect();
        for s in editor
            .selections
            .iter()
            .filter(|s| s.is_empty() && in_view(s.head))
        {
            let line = buffer.char_to_line(s.head);
            if !lines.contains(&line) && visible.contains(&line) {
                lines.push(line);
                d.line_highlights.extend(row_band(
                    layout,
                    layout.line_top(line),
                    colors.line_highlight,
                ));
            }
        }
    }

    let selection = if focused {
        colors.selection
    } else {
        colors.selection_inactive
    };
    for s in editor.selections.iter() {
        if s.is_empty() || s.end() < vis_start || s.start() > vis_end {
            continue;
        }
        let quads = range_rects(
            editor,
            layout,
            buffer.char_to_point(s.start()),
            buffer.char_to_point(s.end()),
            true,
        );
        for b in quads {
            d.selections.push(fill(b, selection).corner_radii(px(3.)));
        }
    }

    if let Some(marked) = &editor.marked {
        for r in marked.iter().filter(|r| in_view(r.start)) {
            let rects = range_rects(
                editor,
                layout,
                buffer.char_to_point(r.start),
                buffer.char_to_point(r.end),
                false,
            );
            for b in rects {
                let style = UnderlineStyle {
                    thickness: px(1.),
                    color: Some(colors.text),
                    wavy: false,
                };
                d.underlines
                    .push((point(b.left(), b.bottom() - px(2.)), b.size.width, style));
            }
        }
    }

    bracket_pair(editor, layout, colors, d);
    diagnostic_underlines(editor, layout, colors, d);
    for b in editor.link_bounds(layout) {
        let style = UnderlineStyle {
            thickness: px(1.),
            color: Some(colors.accent),
            wavy: false,
        };
        d.underlines
            .push((point(b.left(), b.bottom() - px(3.)), b.size.width, style));
    }

    let shape = editor.ext.cursor_shape;
    // Block-style cursors stay visible (hollow) when unfocused, like Vim.
    let show = (focused && editor.cursor_visible) || (!focused && shape != CursorShape::Bar);
    if show {
        for head in editor.cursor_heads().into_iter().filter(|&h| in_view(h)) {
            let p = buffer.char_to_point(head);
            if !visible.contains(&p.line) {
                continue;
            }
            let shape = if focused { shape } else { CursorShape::Hollow };
            d.cursors
                .push(cursor_quad(editor, layout, p, shape, colors));
        }
    }
}

/// The quad for a cursor of `shape` at `p`.
fn cursor_quad(
    editor: &Editor,
    layout: &LayoutSnapshot,
    p: BufPoint,
    shape: CursorShape,
    colors: &ThemeColors,
) -> gpui::PaintQuad {
    let lh = layout.line_height;
    let top = layout.line_top(p.line);
    let x = layout.text_left() + layout.x_for_glyph(p.line, p.column);
    let width = if p.column < editor.buffer.line_len(p.line) {
        (layout.x_for(p.line, p.column + 1) - layout.x_for_glyph(p.line, p.column)).max(px(2.))
    } else {
        layout.char_width
    };
    match shape {
        CursorShape::Bar => {
            let x = layout.text_left() + layout.x_for(p.line, p.column);
            fill(
                Bounds::new(point(x - px(1.), top), size(px(2.), lh)),
                colors.cursor,
            )
        }
        CursorShape::Block => {
            let mut color = colors.cursor;
            color.a *= 0.55;
            fill(Bounds::new(point(x, top), size(width, lh)), color)
        }
        CursorShape::Underline => fill(
            Bounds::new(point(x, top + lh - px(2.)), size(width, px(2.))),
            colors.cursor,
        ),
        CursorShape::Hollow => outline(
            Bounds::new(point(x, top + px(1.)), size(width, lh - px(2.))),
            colors.cursor,
            gpui::BorderStyle::Solid,
        ),
    }
}

/// Per-line rectangles covering `start..end`, limited to visible lines.
/// With `newline`, lines that continue get a small extra width.
pub(crate) fn range_rects(
    editor: &Editor,
    layout: &LayoutSnapshot,
    start: BufPoint,
    end: BufPoint,
    newline: bool,
) -> Vec<Bounds<Pixels>> {
    let visible = layout.visible_range();
    let first = start.line.max(visible.start);
    let last = end.line.min(visible.end.saturating_sub(1));
    let mut out = Vec::new();
    for line in first..=last {
        if line < start.line || line > end.line {
            continue;
        }
        let a = if line == start.line { start.column } else { 0 };
        let b = if line == end.line {
            end.column
        } else {
            editor.buffer.line_len(line)
        };
        let x0 = layout.x_for_glyph(line, a);
        let mut x1 = layout.x_for(line, b);
        if newline && line < end.line {
            x1 += layout.char_width * 0.6;
        }
        if x1 <= x0 {
            continue;
        }
        let origin = point(layout.text_left() + x0, layout.line_top(line));
        out.push(Bounds::new(origin, size(x1 - x0, layout.line_height)));
    }
    out
}

fn bracket_pair(
    editor: &Editor,
    layout: &LayoutSnapshot,
    colors: &ThemeColors,
    d: &mut Decorations,
) {
    if !editor.selections.primary().is_empty() {
        return;
    }
    let buffer = &editor.buffer;
    let head = editor.selections.primary().head;
    for pos in [Some(head), head.checked_sub(1)].into_iter().flatten() {
        if !matches!(buffer.char_at(pos), Some('(' | ')' | '[' | ']' | '{' | '}')) {
            continue;
        }
        let byte = buffer.char_to_byte(pos);
        // Large files keep no tree: match brackets textually instead.
        let partner = match &editor.syntax {
            Some(syntax) => syntax.matching_bracket(buffer.text(), byte),
            None if editor.large_file => arcode_syntax::matching_bracket_plain(buffer.text(), byte),
            None => None,
        };
        let Some(partner) = partner else {
            continue;
        };
        let mut color: Hsla = colors.text_muted;
        color.a *= 0.8;
        for c in [pos, buffer.byte_to_char(partner)] {
            let p = buffer.char_to_point(c);
            if layout.visible(p.line).is_none() {
                continue;
            }
            let x0 = layout.x_for_glyph(p.line, p.column);
            let x1 = layout.x_for(p.line, p.column + 1);
            let origin = point(layout.text_left() + x0, layout.line_top(p.line) + px(1.));
            let bounds = Bounds::new(
                origin,
                size((x1 - x0).max(px(2.)), layout.line_height - px(2.)),
            );
            d.brackets
                .push(outline(bounds, color, gpui::BorderStyle::Solid).corner_radii(px(2.)));
        }
        return;
    }
}

/// The underline of a diagnostic: wavy for errors, straight for warnings
/// and info, none for hints (they keep the gutter dot and the lens).
fn diagnostic_underline(
    severity: DiagnosticSeverity,
    colors: &ThemeColors,
) -> Option<UnderlineStyle> {
    let wavy = match severity {
        DiagnosticSeverity::Error => true,
        DiagnosticSeverity::Warning | DiagnosticSeverity::Info => false,
        DiagnosticSeverity::Hint => return None,
    };
    Some(UnderlineStyle {
        thickness: px(1.),
        color: Some(severity.color(colors)),
        wavy,
    })
}

fn diagnostic_underlines(
    editor: &Editor,
    layout: &LayoutSnapshot,
    colors: &ThemeColors,
    d: &mut Decorations,
) {
    let visible = layout.visible_range();
    let mut diags: Vec<_> = editor
        .diagnostics
        .iter()
        .filter(|g| g.range.start.line < visible.end && g.range.end.line >= visible.start)
        .collect();
    diags.sort_by_key(|g| Reverse(g.severity));
    for g in diags {
        let Some(style) = diagnostic_underline(g.severity, colors) else {
            continue;
        };
        let (s, e) = (g.range.start, g.range.end);
        let first = s.line.max(visible.start);
        let last = e.line.min(visible.end - 1);
        for line in first..=last {
            let Some(v) = layout.visible(line) else {
                continue;
            };
            let len = editor.buffer.line_len(line);
            let a = if line == s.line { s.column.min(len) } else { 0 };
            let b = if line == e.line {
                e.column.min(len)
            } else {
                len
            };
            let (x0, width) = if s == e || (a == b && s.line == e.line) {
                let x0 = layout.x_for(line, a);
                let w = if a < len {
                    layout.x_for(line, a + 1) - x0
                } else {
                    layout.char_width
                };
                (x0, w)
            } else if a == b {
                continue;
            } else {
                let x0 = layout.x_for_glyph(line, a);
                (x0, layout.x_for(line, b) - x0)
            };
            let top = layout.line_top(line);
            let baseline = top
                + (layout.line_height - (v.shaped.ascent + v.shaped.descent)) * 0.5
                + v.shaped.ascent;
            let origin = point(layout.text_left() + x0, baseline + px(1.5));
            d.underlines.push((origin, width.max(px(2.)), style));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underlines_are_wavy_only_for_errors() {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.get("Arcode Ember").expect("Ember").clone();
        let colors = arcode_ui_kit::UiTheme::new(std::sync::Arc::new(theme)).colors;
        let style = |s| diagnostic_underline(s, &colors);
        let error = style(DiagnosticSeverity::Error).expect("error underline");
        assert!(error.wavy);
        assert_eq!(error.color, Some(colors.error));
        let warning = style(DiagnosticSeverity::Warning).expect("warning underline");
        assert!(!warning.wavy);
        assert_eq!(warning.color, Some(colors.warning));
        let info = style(DiagnosticSeverity::Info).expect("info underline");
        assert!(!info.wavy);
        assert_eq!(info.color, Some(colors.info));
        assert!(style(DiagnosticSeverity::Hint).is_none());
    }
}
