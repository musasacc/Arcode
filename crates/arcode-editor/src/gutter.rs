//! Gutter geometry and painting: decoration lanes, the run / light bulb
//! column, line numbers and the change lane.
//!
//! Left to right: breakpoint lane (16, while shown), run / light bulb
//! column (16, always), line numbers (at least 30, one size under the code),
//! change lane (14, always reserved so text never shifts), then a 6 px pad.

use arcode_ui_kit::{IconName, ThemeColors};
use gpui::{Bounds, Hsla, Path, Pixels, SharedString, fill, point, px, size};

use crate::Editor;
use crate::diagnostics::line_severities;
use crate::ext::{ChangeKind, DotStyle, GutterLane, GutterMark};
use crate::layout::{LayoutSnapshot, MARKER_WIDTH};
use crate::paint::{Decorations, GutterIcon};

/// Width of the breakpoint lane and of the run / light bulb column.
pub(crate) const LANE_WIDTH: Pixels = px(16.);
/// Minimum width of the line numbers.
const NUMBERS_MIN: Pixels = px(30.);
/// Width of the change lane.
const CHANGE_LANE: Pixels = px(14.);
/// Space between the gutter and the text.
const TEXT_PAD: Pixels = px(6.);
/// Change bar: x inside the change lane, width, corner radius.
const BAR_X: Pixels = px(5.);
const BAR_W: Pixels = px(3.);
const BAR_RADIUS: Pixels = px(2.);
/// Deleted-lines triangle: x inside the change lane, width, height.
const WEDGE_X: Pixels = px(3.);
const WEDGE_W: Pixels = px(6.);
const WEDGE_H: Pixels = px(10.);

/// Horizontal layout of the gutter, offsets from the editor's left edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GutterGeometry {
    /// Active lanes: lane, left offset, width.
    pub lanes: Vec<(GutterLane, Pixels, Pixels)>,
    /// Left offset of the run / light bulb column (also the diagnostic
    /// dot column).
    pub marker_left: Pixels,
    /// Right edge of the line numbers.
    pub numbers_right: Pixels,
    /// Left offset of the change lane.
    pub changes_left: Pixels,
    /// Total gutter width.
    pub width: Pixels,
}

impl GutterGeometry {
    /// Computes the geometry for the active `lanes`; `digit_width` is the
    /// advance of a digit in the line-number font.
    pub fn new(lanes: &[GutterLane], numbers: bool, digit_width: Pixels, digits: usize) -> Self {
        let mut out = Vec::new();
        let mut x = Pixels::ZERO;
        if lanes.contains(&GutterLane::Breakpoints) {
            out.push((GutterLane::Breakpoints, x, LANE_WIDTH));
            x += LANE_WIDTH;
        }
        // Run buttons share the column of the light bulb and the dots.
        let marker_left = x;
        if lanes.contains(&GutterLane::Runnables) {
            out.push((GutterLane::Runnables, x, LANE_WIDTH));
        }
        x += MARKER_WIDTH;
        if numbers {
            x += NUMBERS_MIN.max((digit_width * digits as f32).ceil());
        }
        let numbers_right = x;
        let changes_left = x;
        if lanes.contains(&GutterLane::Changes) {
            out.push((GutterLane::Changes, x, CHANGE_LANE));
        }
        x += CHANGE_LANE + TEXT_PAD;
        GutterGeometry {
            lanes: out,
            marker_left,
            numbers_right,
            changes_left,
            width: x,
        }
    }

    /// The lane under gutter-relative x `rel_x`.
    pub fn lane_at(&self, rel_x: Pixels) -> Option<GutterLane> {
        self.lanes.iter().find_map(|&(lane, left, width)| {
            (rel_x >= left && rel_x < left + width).then_some(lane)
        })
    }

    fn lane(&self, lane: GutterLane) -> Option<(Pixels, Pixels)> {
        self.lanes
            .iter()
            .find(|(l, _, _)| *l == lane)
            .map(|&(_, left, width)| (left, width))
    }
}

/// The line-number font size: one pixel under the code (12 next to 13).
pub(crate) fn number_font_size(font_size: Pixels) -> Pixels {
    (font_size - px(1.)).round().max(px(8.))
}

/// Computes gutter decorations, markers and line numbers.
pub(crate) fn paint_gutter(
    editor: &Editor,
    layout: &LayoutSnapshot,
    colors: &ThemeColors,
    window: &mut gpui::Window,
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
    let lh = layout.line_height;
    let g = &layout.gutter;
    let severities = line_severities(&editor.diagnostics, visible.clone());
    let marks = editor.ext.gutter_in(visible.clone());
    let has_run = |line: usize| {
        marks
            .iter()
            .any(|m| m.line == line && m.lane == GutterLane::Runnables)
    };
    // A run button wins over the bulb on its line (Ctrl+. still opens the
    // actions there).
    let bulb = editor
        .lang
        .actions
        .bulb
        .filter(|(l, _)| visible.contains(l) && !has_run(*l));
    if let Some((line, quick)) = bulb {
        let s = px(13.).min(layout.line_height);
        let bounds = bulb_bounds(layout, line, s);
        d.gutter_icons.push(GutterIcon {
            bounds,
            path: SharedString::new_static(IconName::Lightbulb.path()),
            color: if quick {
                colors.warning
            } else {
                colors.text_muted
            },
        });
    }
    let number_size = number_font_size(layout.style.font_size);
    let paused = editor.ext.paused.as_ref().map(|p| p.line);
    for (ix, (line, severity)) in visible.clone().zip(severities).enumerate() {
        let top = layout.line_top(line);
        // A dot marks diagnostics that have no lens on their line (none
        // for the setting, no room for one, or the paused lens instead).
        let dot_shown =
            !layout.lines[ix].diag_lens && bulb.is_none_or(|(l, _)| l != line) && !has_run(line);
        if let Some(sev) = severity.filter(|_| dot_shown) {
            let dot = px(6.);
            let origin = point(
                left + g.marker_left + (MARKER_WIDTH - dot) * 0.5,
                top + (lh - dot) * 0.5,
            );
            d.gutter_markers.push(
                fill(Bounds::new(origin, size(dot, dot)), sev.color(colors))
                    .corner_radii(dot * 0.5),
            );
        }
        if !layout.style.show_line_numbers {
            continue;
        }
        let color = if active.contains(&line) || paused == Some(line) {
            colors.gutter_text_active
        } else {
            colors.gutter_text
        };
        let label = SharedString::from((line + 1).to_string());
        let shaped = Editor::shape_label(label, &layout.style, number_size, color, window);
        let x = left + g.numbers_right - shaped.width;
        d.gutter_labels.push((point(x, top), shaped));
    }
    for mark in marks {
        let Some((lane_left, lane_w)) = g.lane(mark.lane) else {
            continue;
        };
        let x = left + lane_left;
        let top = layout.line_top(mark.line);
        match &mark.mark {
            GutterMark::Change(kind) => {
                let color = kind.color_key().resolve(colors);
                match kind {
                    ChangeKind::Deleted => d.gutter_paths.push((wedge(x, top + lh), color)),
                    _ => {
                        let bar =
                            Bounds::new(point(x + BAR_X, top + px(1.)), size(BAR_W, lh - px(2.)));
                        d.gutter_markers
                            .push(fill(bar, color).corner_radii(BAR_RADIUS));
                    }
                }
            }
            GutterMark::Dot { color, style } => {
                let color = color.resolve(colors);
                dot(d, x, lane_w, top, lh, color, *style, colors.background);
            }
            GutterMark::Icon { icon, color } => {
                let s = px(12.).min(lh);
                let origin = point(x + (lane_w - s) * 0.5, top + (lh - s) * 0.5);
                d.gutter_icons.push(GutterIcon {
                    bounds: Bounds::new(origin, size(s, s)),
                    path: SharedString::new_static(icon.path()),
                    color: color.resolve(colors),
                });
            }
        }
    }
}

/// The deleted-lines triangle of the change lane at `lane_x`, pointing
/// right and centred on the line boundary at `y`.
fn wedge(lane_x: Pixels, y: Pixels) -> Path<Pixels> {
    let [a, b, c] = wedge_points(lane_x, y);
    let mut path = Path::new(a);
    path.line_to(b);
    path.line_to(c);
    path
}

/// The corners of [`wedge`]: top left, tip, bottom left.
fn wedge_points(lane_x: Pixels, y: Pixels) -> [gpui::Point<Pixels>; 3] {
    let x = lane_x + WEDGE_X;
    [
        point(x, y - WEDGE_H * 0.5),
        point(x + WEDGE_W, y),
        point(x, y + WEDGE_H * 0.5),
    ]
}

/// Where the light bulb of `line` is drawn: centered on the run / bulb
/// column.
pub(crate) fn bulb_bounds(layout: &LayoutSnapshot, line: usize, s: Pixels) -> Bounds<Pixels> {
    let left = layout.bounds.left() + layout.gutter.marker_left;
    let top = layout.line_top(line);
    Bounds::new(
        point(
            left + (MARKER_WIDTH - s) * 0.5,
            top + (layout.line_height - s) * 0.5,
        ),
        size(s, s),
    )
}

#[allow(clippy::too_many_arguments)]
fn dot(
    d: &mut Decorations,
    x: Pixels,
    lane_w: Pixels,
    top: Pixels,
    lh: Pixels,
    color: Hsla,
    style: DotStyle,
    background: Hsla,
) {
    let r = if style == DotStyle::PausedFrame {
        px(14.)
    } else {
        px(10.)
    }
    .min(lh - px(2.));
    let origin = point(x + (lane_w - r) * 0.5, top + (lh - r) * 0.5);
    let bounds = Bounds::new(origin, size(r, r));
    match style {
        DotStyle::Filled => d
            .gutter_markers
            .push(fill(bounds, color).corner_radii(r * 0.5)),
        DotStyle::Ring => d.gutter_markers.push(
            gpui::outline(bounds, color, gpui::BorderStyle::Solid)
                .border_widths(px(1.5))
                .corner_radii(r * 0.5),
        ),
        DotStyle::Conditional => {
            d.gutter_markers
                .push(fill(bounds, color).corner_radii(r * 0.5));
            // A short bar through the middle, like a "=" condition mark.
            let bar = Bounds::new(
                point(origin.x + px(2.), origin.y + r * 0.5 - px(1.)),
                size(r - px(4.), px(2.)),
            );
            d.gutter_markers.push(fill(bar, background));
        }
        DotStyle::PausedFrame => {
            d.gutter_markers
                .push(fill(bounds, color).corner_radii(r * 0.5));
            // The arrow is cut out of the dot in the background color.
            let s = (r - px(4.)).max(px(6.));
            d.gutter_icons.push(GutterIcon {
                bounds: Bounds::new(
                    point(origin.x + (r - s) * 0.5, origin.y + (r - s) * 0.5),
                    size(s, s),
                ),
                path: SharedString::new_static(IconName::ArrowRight.path()),
                color: background,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_orders_lanes() {
        let all = GutterLane::ALL;
        let g = GutterGeometry::new(&all, true, px(7.), 3);
        assert_eq!(g.lanes[0], (GutterLane::Breakpoints, px(0.), LANE_WIDTH));
        // Run buttons sit in the bulb column, right after the breakpoints.
        assert_eq!(g.lanes[1], (GutterLane::Runnables, LANE_WIDTH, LANE_WIDTH));
        assert_eq!(g.marker_left, LANE_WIDTH);
        // 16 + 16 + 30 (three digits fit the minimum) + 14 + 6 = 82.
        assert_eq!(g.numbers_right, px(62.));
        assert_eq!(g.lanes[2], (GutterLane::Changes, px(62.), CHANGE_LANE));
        assert_eq!(g.width, px(82.));
        assert_eq!(g.lane_at(px(3.)), Some(GutterLane::Breakpoints));
        assert_eq!(g.lane_at(px(20.)), Some(GutterLane::Runnables));
        assert_eq!(
            g.lane_at(g.numbers_right + px(6.)),
            Some(GutterLane::Changes)
        );
        assert_eq!(g.lane_at(g.numbers_right - px(2.)), None);

        // Long files widen the numbers; no lanes keep the bulb column and
        // the change lane's space, so text does not move when marks come.
        let wide = GutterGeometry::new(&[], true, px(7.2), 6);
        assert_eq!(wide.numbers_right, MARKER_WIDTH + px(44.));
        let plain = GutterGeometry::new(&[], true, px(7.), 2);
        assert!(plain.lanes.is_empty());
        assert_eq!(plain.width, MARKER_WIDTH + px(30.) + CHANGE_LANE + TEXT_PAD);
        let with_changes = GutterGeometry::new(&[GutterLane::Changes], true, px(7.), 2);
        assert_eq!(with_changes.width, plain.width);
        let no_numbers = GutterGeometry::new(&[], false, px(7.), 2);
        assert_eq!(no_numbers.width, MARKER_WIDTH + CHANGE_LANE + TEXT_PAD);
    }

    #[test]
    fn numbers_are_one_size_under_the_code() {
        assert_eq!(number_font_size(px(13.)), px(12.));
        assert_eq!(number_font_size(px(15.5)), px(15.));
        assert_eq!(number_font_size(px(6.)), px(8.));
    }

    #[test]
    fn deleted_wedge_points_right_on_the_line_boundary() {
        let [top, tip, bottom] = wedge_points(px(100.), px(40.));
        assert_eq!(top, point(px(103.), px(35.)));
        assert_eq!(tip, point(px(109.), px(40.)));
        assert_eq!(bottom, point(px(103.), px(45.)));
    }
}
