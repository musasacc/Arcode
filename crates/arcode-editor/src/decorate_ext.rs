//! Painting extension decorations: background highlights, end-of-line
//! virtual text, lens pills and code lenses.

use arcode_ui_kit::ThemeColors;
use gpui::{Bounds, Pixels, SharedString, fill, point, px, size};

use crate::Editor;
use crate::decorate::range_rects;
use crate::ext::HighlightRange;
use crate::inline_lens::{PILL_H, PILL_RADIUS};
use crate::layout::LayoutSnapshot;
use crate::layout_extras::ExtraItem;
use crate::paint::{Decorations, GutterIcon, row_band};

/// Adds extension decorations for the visible lines to `d`.
pub(crate) fn ext_decorations(
    editor: &Editor,
    layout: &LayoutSnapshot,
    colors: &ThemeColors,
    d: &mut Decorations,
) {
    let visible = layout.visible_range();
    if visible.is_empty() {
        return;
    }
    let lh = layout.line_height;
    // Each set is sorted by start line: stop at the first one below the view.
    let in_view = editor.ext.highlights.values().flat_map(|list| {
        list.iter().take_while(|h| match &h.range {
            HighlightRange::Lines(r) => r.start < visible.end,
            HighlightRange::Text(r) => r.start.line < visible.end,
        })
    });
    for h in in_view {
        let color = h.color.resolve(colors);
        match &h.range {
            HighlightRange::Lines(lines) => {
                let first = lines.start.max(visible.start);
                let last = lines.end.min(visible.end);
                for line in first..last {
                    d.ext_line_highlights
                        .extend(row_band(layout, layout.line_top(line), color));
                }
            }
            HighlightRange::Text(r) => {
                if r.end.line < visible.start || r.start.line >= visible.end {
                    continue;
                }
                for b in range_rects(editor, layout, r.start, r.end, false) {
                    d.text_highlights.push(fill(b, color).corner_radii(px(2.)));
                }
            }
        }
    }
    let left = layout.text_left();
    for v in &layout.lines {
        let top = layout.line_top(v.line);
        let pill_top = top + (lh - PILL_H) * 0.5;
        for pill in &v.pills {
            let bounds = Bounds::new(point(left + pill.x, pill_top), size(pill.width, PILL_H));
            d.lens_boxes.push(
                fill(bounds, pill.fill)
                    .border_widths(px(1.))
                    .border_color(pill.border)
                    .corner_radii(PILL_RADIUS),
            );
            if let Some((icon, color, s, x)) = pill.icon {
                d.lens_icons.push(GutterIcon {
                    bounds: Bounds::new(point(left + x, top + (lh - s) * 0.5), size(s, s)),
                    path: SharedString::new_static(icon.path()),
                    color,
                });
            }
        }
        for item in &v.eol {
            push_extra(d, item, left, top, lh);
        }
    }
    for row in &layout.block_rows {
        let top = layout.line_top(row.line) - lh;
        for item in &row.items {
            push_extra(d, item, left, top, lh);
        }
    }
}

/// Adds an extra text item (and its icon) on the row at `top`.
fn push_extra(d: &mut Decorations, item: &ExtraItem, left: Pixels, top: Pixels, lh: Pixels) {
    if let Some((icon, color, s, x)) = item.icon {
        d.lens_icons.push(GutterIcon {
            bounds: Bounds::new(point(left + x, top + (lh - s) * 0.5), size(s, s)),
            path: SharedString::new_static(icon.path()),
            color,
        });
    }
    d.extras
        .push((point(left + item.x, top), item.shaped.clone()));
}
