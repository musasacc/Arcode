//! Painting extension decorations: background highlights, end-of-line
//! virtual text and code lenses.

use arcode_ui_kit::ThemeColors;
use gpui::{Bounds, fill, point, px, size};

use crate::Editor;
use crate::decorate::range_rects;
use crate::ext::HighlightRange;
use crate::layout::LayoutSnapshot;
use crate::paint::Decorations;

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
                    let origin = point(layout.bounds.left(), layout.line_top(line));
                    d.ext_line_highlights.push(fill(
                        Bounds::new(origin, size(layout.bounds.size.width, lh)),
                        color,
                    ));
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
        for item in &v.eol {
            d.extras
                .push((point(left + item.x, top), item.shaped.clone()));
        }
    }
    for row in &layout.block_rows {
        let top = layout.line_top(row.line) - lh;
        for item in &row.items {
            d.extras
                .push((point(left + item.x, top), item.shaped.clone()));
        }
    }
}
