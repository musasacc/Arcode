//! Painting extension decorations: background highlights, inlay chips,
//! end-of-line virtual text, lens pills and code lenses.

use std::ops::Range;

use arcode_ui_kit::ThemeColors;
use gpui::{Bounds, Hsla, Pixels, SharedString, fill, point, px, size};

use crate::Editor;
use crate::decorate::range_rects;
use crate::display::DisplayLine;
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
    let chip_h = (layout.style.font_size + CHIP_PAD_Y * 2.).min(lh);
    for v in &layout.lines {
        let top = layout.line_top(v.line);
        for (bytes, color) in inlay_chips(&v.display) {
            let x0 = v.shaped.x_for_index(bytes.start);
            let x1 = v.shaped.x_for_index(bytes.end);
            let bounds = Bounds::new(
                point(left + x0, top + (lh - chip_h) * 0.5),
                size(x1 - x0, chip_h),
            );
            d.text_highlights
                .push(fill(bounds, color).corner_radii(CHIP_RADIUS));
        }
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

/// Space above and below the text inside an inlay chip.
const CHIP_PAD_Y: Pixels = px(2.);
/// Corner radius of an inlay chip.
const CHIP_RADIUS: Pixels = px(4.);

/// The chips behind inline virtual text (inlay hints) that has a
/// background: display byte range (padding spaces left out, so the chip
/// hugs the label) and fill. The chips are painted as text highlights,
/// under the text and under selections.
pub(crate) fn inlay_chips(display: &DisplayLine) -> Vec<(Range<usize>, Hsla)> {
    let mut chips = Vec::new();
    for (_, range) in &display.inlays {
        let mut pos = 0;
        let background = display.runs.iter().find_map(|run| {
            let run_range = pos..pos + run.len;
            pos += run.len;
            run_range
                .contains(&range.start)
                .then_some(run.background_color)
                .flatten()
        });
        let Some(color) = background else {
            continue;
        };
        let text = &display.text[range.clone()];
        let start = range.start + (text.len() - text.trim_start().len());
        let end = range.end - (text.len() - text.trim_end().len());
        if start < end {
            chips.push((start..end, color));
        }
    }
    chips
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arcode_config::{Appearance, ThemeRegistry};
    use arcode_ui_kit::UiTheme;
    use gpui::{Font, hsla};

    use super::*;
    use crate::display::{InlayText, build_display_line};

    fn font() -> Font {
        gpui::font("JetBrains Mono")
    }

    fn line(inlays: &[InlayText<'_>]) -> DisplayLine {
        let registry = ThemeRegistry::builtin();
        let theme = UiTheme::new(Arc::new(registry.fallback(Appearance::Dark).clone()));
        build_display_line("let total = sum(values);", &[], inlays, 4, &font(), &theme)
    }

    #[test]
    fn inlay_hints_with_a_background_get_a_chip_around_their_label() {
        let chip = hsla(0.1, 0.1, 0.2, 1.);
        let muted = hsla(0.1, 0.1, 0.6, 1.);
        let hint = |col, text| InlayText {
            col,
            text,
            color: muted,
            background: Some(chip),
            italic: false,
        };
        // A type hint after `total` and a parameter hint with padding.
        let display = line(&[hint(9, ": u64"), hint(16, " values: ")]);
        assert_eq!(
            display.text.as_ref(),
            "let total: u64 = sum( values: values);"
        );
        let chips = inlay_chips(&display);
        let labels: Vec<&str> = chips
            .iter()
            .map(|(r, _)| &display.text[r.clone()])
            .collect();
        assert_eq!(labels, [": u64", "values:"]);
        assert!(chips.iter().all(|(_, c)| *c == chip));
    }

    #[test]
    fn virtual_text_without_a_background_has_no_chip() {
        let plain = InlayText {
            col: 9,
            text: ": u64",
            color: hsla(0., 0., 0.5, 1.),
            background: None,
            italic: true,
        };
        assert!(inlay_chips(&line(&[plain])).is_empty());
        assert!(inlay_chips(&line(&[])).is_empty());
    }
}
