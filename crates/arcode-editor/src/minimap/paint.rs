//! Painting the minimap: what lies behind the glyphs (its background, the
//! viewport slider, background highlights, selections, diagnostics), the
//! glyph tiles themselves, and the change marks at its left edge.

use std::ops::Range;

use arcode_buffer::Point as BufPoint;
use arcode_ui_kit::ThemeColors;
use gpui::{
    Bounds, ContentMask, Corners, Hsla, PaintQuad, Path, Pixels, Window, fill, point, px, size,
};

use super::MinimapLayout;
use crate::ext::{ChangeKind, GutterLane, GutterMark, HighlightRange};
use crate::layout::LayoutSnapshot;
use crate::mouse::DragState;
use crate::{DiagnosticSeverity, Editor};

/// Corner radius of the viewport band.
const SLIDER_RADIUS: Pixels = px(2.);
/// Opacity of error and warning marks.
const DIAGNOSTIC_OPACITY: f32 = 0.4;
/// Change marks: x from the minimap's left edge and width.
const MARK_X: Pixels = px(2.);
const MARK_W: Pixels = px(2.);
/// Deleted-lines triangle: width and height.
const WEDGE_W: Pixels = px(4.);
const WEDGE_H: Pixels = px(6.);

/// What the minimap paints besides its glyphs.
#[derive(Default)]
pub(crate) struct MinimapPaint {
    /// Behind the glyphs.
    pub under: Vec<PaintQuad>,
    /// Over the glyphs.
    pub over: Vec<PaintQuad>,
    /// Deleted-lines triangles.
    pub wedges: Vec<(Path<Pixels>, Hsla)>,
}

/// Where buffer lines of the minimap are drawn.
struct Rows<'a> {
    m: &'a MinimapLayout,
    blocks: &'a crate::ext::BlockMap,
    lines: Range<usize>,
    max_cols: usize,
}

impl Rows<'_> {
    fn top(&self, line: usize) -> Pixels {
        self.m.geometry.row_top(self.blocks.row_for_line(line))
    }

    /// The rows of `lines` (clipped to the view) from column `a` of the
    /// first to column `b` of the last; `None` for whole lines.
    fn span(
        &self,
        lines: Range<usize>,
        a: Option<usize>,
        b: Option<usize>,
    ) -> Option<Bounds<Pixels>> {
        let first = lines.start.max(self.lines.start);
        let last = lines.end.min(self.lines.end);
        if first >= last {
            return None;
        }
        let g = &self.m.geometry;
        let top = self.top(first);
        let bottom = self.top(last - 1) + g.pitch;
        let cw = self.m.col_width;
        let (left, right) = match (a, b) {
            (None, None) => (g.bounds.left(), g.bounds.right()),
            _ => {
                let x = |col: usize| self.m.content_left() + cw * col.min(self.max_cols) as f32;
                let left = x(a.unwrap_or(0));
                // Never thinner than two columns, so a one-char match shows.
                (left, x(b.unwrap_or(self.max_cols)).max(left + cw * 2.))
            }
        };
        Some(Bounds::new(
            point(left, top),
            size(right - left, bottom - top),
        ))
    }
}

/// Display column of `p` (tabs expanded).
fn display_col(editor: &Editor, p: BufPoint, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let mut col = 0;
    for c in editor.buffer.line(p.line).chars().take(p.column) {
        col += if c == '\t' {
            tab_size - col % tab_size
        } else {
            1
        };
    }
    col
}

/// Highlight colours are made for text behind glyphs; on the minimap they
/// cover a row of a few pixels, so they get more weight.
fn strengthen(c: Hsla) -> Hsla {
    Hsla {
        a: (c.a * 2.5).clamp(0.45, 0.9),
        ..c
    }
}

/// Computes the minimap's quads for `layout` into `out`.
pub(crate) fn minimap_decorations(
    editor: &Editor,
    layout: &LayoutSnapshot,
    focused: bool,
    colors: &ThemeColors,
    out: &mut MinimapPaint,
) {
    let Some(m) = &layout.minimap else {
        return;
    };
    let g = &m.geometry;
    let rows = Rows {
        m,
        blocks: &layout.blocks,
        lines: m.lines.clone(),
        max_cols: m.max_cols(),
    };
    // Its own background hides the current-line band behind it.
    out.under.push(fill(g.bounds, colors.background));
    // The viewport: a soft band behind the glyphs, a little stronger while
    // the mouse is on the minimap.
    let active = editor.minimap.hovered || matches!(editor.drag, Some(DragState::Minimap(_)));
    let slider = if active { colors.pill } else { colors.hover };
    out.under
        .push(fill(g.slider, slider).corner_radii(Corners::all(SLIDER_RADIUS)));
    if rows.lines.is_empty() {
        return;
    }
    let tab = layout.tab_size;
    let col = |p: BufPoint| display_col(editor, p, tab);

    // Each set is sorted by start. A highlight starting above the view
    // can still cover its first lines, but none starts further above them
    // than its set's longest highlight reaches, so the search starts there
    // and does not grow with the matches above the view.
    for (set, list) in &editor.ext.highlights {
        let reach = editor.ext.highlight_reach(set);
        let lookback = rows.lines.start.saturating_sub(reach);
        let from = list.partition_point(|h| match &h.range {
            HighlightRange::Lines(r) => r.start < lookback,
            HighlightRange::Text(r) => r.start.line < lookback,
        });
        for h in &list[from..] {
            let color = strengthen(h.color.resolve(colors));
            let rect = match &h.range {
                HighlightRange::Lines(r) if r.start >= rows.lines.end => break,
                HighlightRange::Text(r) if r.start.line >= rows.lines.end => break,
                HighlightRange::Lines(r) => rows.span(r.clone(), None, None),
                HighlightRange::Text(r) if r.start.line == r.end.line => rows.span(
                    r.start.line..r.start.line + 1,
                    Some(col(r.start)),
                    Some(col(r.end)),
                ),
                HighlightRange::Text(r) => {
                    rows.span(r.start.line..r.end.line + 1, Some(col(r.start)), None)
                }
            };
            out.under.extend(rect.map(|b| fill(b, color)));
        }
    }

    let selection = if focused {
        colors.selection
    } else {
        colors.selection_inactive
    };
    let buffer = &editor.buffer;
    for s in editor.selections.iter().filter(|s| !s.is_empty()) {
        let (a, b) = (s.start(), s.end());
        let (a, b) = (buffer.char_to_point(a), buffer.char_to_point(b));
        if a.line >= rows.lines.end || b.line < rows.lines.start {
            continue;
        }
        if a.line == b.line {
            let rect = rows.span(a.line..a.line + 1, Some(col(a)), Some(col(b)));
            out.under.extend(rect.map(|r| fill(r, selection)));
            continue;
        }
        let first = rows.span(a.line..a.line + 1, Some(col(a)), None);
        let middle = rows.span(a.line + 1..b.line, Some(0), None);
        let last = rows.span(b.line..b.line + 1, Some(0), Some(col(b)));
        for rect in [first, middle, last].into_iter().flatten() {
            out.under.push(fill(rect, selection));
        }
    }

    for severity in [DiagnosticSeverity::Warning, DiagnosticSeverity::Error] {
        let color = severity.color(colors).opacity(DIAGNOSTIC_OPACITY);
        for d in editor.diagnostics.iter().filter(|d| d.severity == severity) {
            let (a, b) = (d.range.start, d.range.end);
            if !rows.lines.contains(&a.line) {
                continue;
            }
            let end = (b.line == a.line).then(|| col(b));
            out.under.extend(
                rows.span(a.line..a.line + 1, Some(col(a)), end)
                    .map(|r| fill(r, color)),
            );
        }
    }

    change_marks(editor, &rows, colors, out);
}

/// Change bars and deleted-lines triangles at the minimap's left edge.
fn change_marks(editor: &Editor, rows: &Rows<'_>, colors: &ThemeColors, out: &mut MinimapPaint) {
    let g = &rows.m.geometry;
    let x = g.bounds.left() + MARK_X;
    // Consecutive lines of one kind become one bar.
    let mut bar: Option<(usize, usize, ChangeKind)> = None;
    let flush = |bar: Option<(usize, usize, ChangeKind)>, out: &mut MinimapPaint| {
        if let Some((first, last, kind)) = bar {
            let top = rows.top(first);
            let bottom = rows.top(last) + g.pitch;
            let color = kind.color_key().resolve(colors);
            out.over.push(fill(
                Bounds::new(point(x, top), size(MARK_W, bottom - top)),
                color,
            ));
        }
    };
    for mark in editor.ext.gutter_in(rows.lines.clone()) {
        let GutterMark::Change(kind) = mark.mark else {
            continue;
        };
        if mark.lane != GutterLane::Changes {
            continue;
        }
        if kind == ChangeKind::Deleted {
            let y = rows.top(mark.line) + g.pitch;
            let mut path = Path::new(point(x - px(1.), y - WEDGE_H * 0.5));
            path.line_to(point(x - px(1.) + WEDGE_W, y));
            path.line_to(point(x - px(1.), y + WEDGE_H * 0.5));
            out.wedges.push((path, kind.color_key().resolve(colors)));
            continue;
        }
        bar = match bar {
            Some((first, last, k)) if k == kind && mark.line == last + 1 => {
                Some((first, mark.line, k))
            }
            Some((_, last, k)) if k == kind && mark.line == last => bar,
            other => {
                flush(other, out);
                Some((mark.line, mark.line, kind))
            }
        };
    }
    flush(bar, out);
}

/// Paints the minimap of `m` with the quads computed for it.
pub(crate) fn paint_minimap(m: &MinimapLayout, paint: &mut MinimapPaint, window: &mut Window) {
    let g = &m.geometry;
    // One layer: its quads and sprites share one draw order and keep their
    // painting order.
    window.with_content_mask(Some(ContentMask { bounds: g.bounds }), |window| {
        window.paint_layer(g.bounds, |window| {
            for q in paint.under.drain(..) {
                window.paint_quad(q);
            }
            let left = m.content_left();
            for tile in &m.tiles {
                let bounds = Bounds::new(point(left, g.row_top(tile.first_row)), m.tile_size);
                let image = tile.image.clone();
                if let Err(err) = window.paint_image(bounds, Corners::default(), image, 0, false) {
                    log::debug!("failed to paint a minimap tile: {err}");
                }
            }
            for q in paint.over.drain(..) {
                window.paint_quad(q);
            }
            for (path, color) in paint.wedges.drain(..) {
                window.paint_path(path, color);
            }
        });
    });
}
