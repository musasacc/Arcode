//! Gutter geometry and painting: decoration lanes, the diagnostic marker
//! column and line numbers.

use arcode_ui_kit::{IconName, ThemeColors};
use gpui::{Bounds, Hsla, Pixels, SharedString, fill, point, px, size};

use crate::Editor;
use crate::diagnostics::line_severities;
use crate::ext::{ChangeKind, DotStyle, GutterLane, GutterMark};
use crate::layout::{LayoutSnapshot, MARKER_WIDTH};
use crate::paint::{Decorations, GutterIcon};

/// Width of the breakpoint and runnable lanes.
pub(crate) const LANE_WIDTH: Pixels = px(16.);
/// Width of a change bar.
const CHANGE_BAR: Pixels = px(3.);

/// Horizontal layout of the gutter, offsets from the editor's left edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GutterGeometry {
    /// Active lanes: lane, left offset, width.
    pub lanes: Vec<(GutterLane, Pixels, Pixels)>,
    /// Left offset of the diagnostic marker column.
    pub marker_left: Pixels,
    /// Right edge of the line numbers.
    pub numbers_right: Pixels,
    /// Total gutter width.
    pub width: Pixels,
}

impl GutterGeometry {
    /// Computes the geometry for the active `lanes`.
    pub fn new(lanes: &[GutterLane], numbers: bool, char_width: Pixels, digits: usize) -> Self {
        let mut out = Vec::new();
        let mut x = Pixels::ZERO;
        for lane in [GutterLane::Breakpoints, GutterLane::Runnables] {
            if lanes.contains(&lane) {
                out.push((lane, x, LANE_WIDTH));
                x += LANE_WIDTH;
            }
        }
        let marker_left = x;
        x += MARKER_WIDTH;
        let numbers_right = if numbers {
            x += char_width * digits as f32 + px(6.);
            let right = x;
            x += px(12.);
            right
        } else {
            x += px(6.);
            x - px(6.)
        };
        if lanes.contains(&GutterLane::Changes) {
            out.push((GutterLane::Changes, numbers_right + px(3.), CHANGE_BAR));
            x += px(4.);
        }
        GutterGeometry {
            lanes: out,
            marker_left,
            numbers_right,
            width: x,
        }
    }

    /// The lane under gutter-relative x `rel_x` (change bars get a few
    /// extra pixels of hit area).
    pub fn lane_at(&self, rel_x: Pixels) -> Option<GutterLane> {
        self.lanes.iter().find_map(|&(lane, left, width)| {
            let slack = if lane == GutterLane::Changes {
                px(3.)
            } else {
                Pixels::ZERO
            };
            (rel_x >= left - slack && rel_x < left + width + slack).then_some(lane)
        })
    }

    fn lane(&self, lane: GutterLane) -> Option<(Pixels, Pixels)> {
        self.lanes
            .iter()
            .find(|(l, _, _)| *l == lane)
            .map(|&(_, left, width)| (left, width))
    }
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
    let bulb = editor
        .lang
        .actions
        .bulb
        .filter(|(l, _)| visible.contains(l));
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
    for (line, severity) in visible.clone().zip(severities) {
        let top = layout.line_top(line);
        if let Some(sev) = severity.filter(|_| bulb.is_none_or(|(l, _)| l != line)) {
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
        let color = if active.contains(&line) {
            colors.gutter_text_active
        } else {
            colors.gutter_text
        };
        let label = SharedString::from((line + 1).to_string());
        let shaped = Editor::shape_label(label, &layout.style, color, window);
        let x = left + g.numbers_right - shaped.width;
        d.gutter_labels.push((point(x, top), shaped));
    }
    for mark in editor.ext.gutter_in(visible) {
        let Some((lane_left, lane_w)) = g.lane(mark.lane) else {
            continue;
        };
        let x = left + lane_left;
        let top = layout.line_top(mark.line);
        match &mark.mark {
            GutterMark::Change(kind) => {
                let color = kind.color_key().resolve(colors);
                let bounds = match kind {
                    ChangeKind::Deleted => Bounds::new(
                        point(x - px(1.), top - px(2.)),
                        size(lane_w + px(2.), px(4.)),
                    ),
                    _ => Bounds::new(point(x, top), size(lane_w, lh)),
                };
                d.gutter_markers
                    .push(fill(bounds, color).corner_radii(px(1.)));
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

/// Where the light bulb of `line` is drawn: centered on the diagnostic
/// marker column (it replaces the marker on that line).
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
    let r = px(10.).min(lh - px(2.));
    let origin = point(x + (lane_w - r) * 0.5, top + (lh - r) * 0.5);
    let bounds = Bounds::new(origin, size(r, r));
    match style {
        DotStyle::Filled => d
            .gutter_markers
            .push(fill(bounds, color).corner_radii(r * 0.5)),
        DotStyle::Ring => d.gutter_markers.push(
            gpui::outline(bounds, color, gpui::BorderStyle::Solid)
                .border_widths(px(2.))
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_orders_lanes() {
        let all = GutterLane::ALL;
        let g = GutterGeometry::new(&all, true, px(8.), 3);
        assert_eq!(g.lanes[0], (GutterLane::Breakpoints, px(0.), LANE_WIDTH));
        assert_eq!(g.lanes[1], (GutterLane::Runnables, LANE_WIDTH, LANE_WIDTH));
        assert_eq!(g.marker_left, LANE_WIDTH * 2.);
        assert_eq!(g.numbers_right, LANE_WIDTH * 2. + MARKER_WIDTH + px(30.));
        assert_eq!(g.width, g.numbers_right + px(16.));
        assert_eq!(g.lane_at(px(3.)), Some(GutterLane::Breakpoints));
        assert_eq!(g.lane_at(px(20.)), Some(GutterLane::Runnables));
        assert_eq!(
            g.lane_at(g.numbers_right + px(1.)),
            Some(GutterLane::Changes)
        );
        assert_eq!(g.lane_at(g.marker_left + px(2.)), None);

        let plain = GutterGeometry::new(&[], true, px(8.), 2);
        assert!(plain.lanes.is_empty());
        assert_eq!(plain.width, MARKER_WIDTH + px(16.) + px(18.));
    }
}
