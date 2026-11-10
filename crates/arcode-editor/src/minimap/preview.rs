//! The minimap's hover preview: while the mouse rests on the minimap, the
//! code around the line under it floats left of the minimap, over the
//! text, at the editor's own font and size: up to [`PREVIEW_LINES`] lines
//! with their numbers, syntax coloured, the hovered line marked with the
//! current-line band and placed beside the mouse. It follows the mouse
//! row by row, stays inside the editor and the window, and goes away when
//! the mouse leaves the minimap or starts dragging it. It is painted only:
//! it takes no focus and no mouse events.

use std::ops::Range;

use arcode_ui_kit::{popover, theme};
use gpui::{
    AnyElement, App, Bounds, IntoElement, ParentElement, Pixels, SharedString, Size, StyledText,
    Window, div, point, prelude::*, px, size,
};

use crate::Editor;
use crate::display::build_display_line;
use crate::gutter::number_font_size;
use crate::layout::LayoutSnapshot;
use crate::mouse::DragState;

/// Most lines the preview shows.
pub(crate) const PREVIEW_LINES: usize = 15;
/// Gap between the preview and the minimap.
const GAP: Pixels = px(8.);
/// Distance kept from the editor's and the window's edges.
const MARGIN: Pixels = px(6.);
/// Widest the preview gets.
const MAX_WIDTH: Pixels = px(720.);
/// Narrower than this, no preview is shown.
const MIN_WIDTH: Pixels = px(160.);
/// Padding above and below the lines.
const PAD_Y: Pixels = px(6.);
/// The popover's border.
const BORDER: Pixels = px(1.);
/// Chars of a line laid out at most (the rest is past the right edge).
const MAX_CHARS: usize = 400;

/// Where the preview goes and what it shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreviewPlan {
    pub lines: Range<usize>,
    pub hovered: usize,
    pub bounds: Bounds<Pixels>,
}

/// The preview for the mouse at window y `y` over the minimap of
/// `layout`, in a window `viewport` large; `None` without a minimap or
/// room.
pub(crate) fn preview_plan(
    layout: &LayoutSnapshot,
    y: Pixels,
    line_count: usize,
    viewport: Size<Pixels>,
) -> Option<PreviewPlan> {
    let m = layout.minimap.as_ref()?;
    let g = &m.geometry;
    if line_count == 0 || !(g.bounds.top()..g.bounds.bottom()).contains(&y) {
        return None;
    }
    let row = g.row_at(y);
    let hovered = layout.blocks.row_kind(row, line_count).line();
    let lh = layout.line_height;
    let top_limit = layout.bounds.top().max(Pixels::ZERO) + MARGIN;
    let bottom_limit = layout.bounds.bottom().min(viewport.height) - MARGIN;
    let chrome = PAD_Y * 2. + BORDER * 2.;
    let room = bottom_limit - top_limit - chrome;
    let n = ((room / lh).floor().max(1.) as usize)
        .min(PREVIEW_LINES)
        .min(line_count);
    let first = hovered.saturating_sub(n / 2).min(line_count - n);
    let height = lh * n as f32 + chrome;
    let right = g.bounds.left() - GAP;
    let width = (right - layout.bounds.left() - MARGIN).min(MAX_WIDTH);
    if width < MIN_WIDTH {
        return None;
    }
    // The hovered line sits level with its row in the minimap.
    let row_mid = g.row_top(row) + g.pitch * 0.5;
    let wanted = row_mid - (BORDER + PAD_Y + lh * (hovered - first) as f32 + lh * 0.5);
    let top = wanted.min(bottom_limit - height).max(top_limit).round();
    Some(PreviewPlan {
        lines: first..first + n,
        hovered,
        bounds: Bounds::new(point(right - width, top), size(width, height)),
    })
}

/// Lays out the preview for the mouse over the minimap, if any, and draws
/// it above the editor; records its plan for tests. Must be called during
/// prepaint.
pub(crate) fn defer_minimap_preview(
    editor: &mut Editor,
    layout: &LayoutSnapshot,
    window: &mut Window,
    cx: &mut App,
) {
    let dragging = matches!(editor.drag, Some(DragState::Minimap(_)));
    // Windows reports the mouse leaving the window only as the window's
    // hover state (with a redraw), not as a mouse exit event.
    let in_window = !cfg!(target_os = "windows") || window.is_window_hovered();
    let plan = match editor.minimap.hover_y {
        Some(y) if editor.minimap.hovered && !dragging && in_window => preview_plan(
            layout,
            y,
            editor.buffer.line_count(),
            window.viewport_size(),
        ),
        _ => None,
    };
    editor.minimap.preview = plan.clone();
    let Some(plan) = plan else {
        return;
    };
    let origin = plan.bounds.origin;
    let mut element = preview_element(editor, layout, &plan, cx);
    element.layout_as_root(plan.bounds.size.map(Into::into), window, cx);
    window.defer_draw(element, origin, 1);
}

/// Contrast the hovered line's band needs against the popover.
const BAND_CONTRAST: f32 = 1.1;

/// The band behind the hovered line: `pill`, the selected row of a list
/// on a raised surface, when it shows on the popover (`panel_raised`);
/// else whichever of `hover`, `selection_inactive`, `line_highlight` and
/// `selection` stands out most (older themes derive `pill` from
/// `panel_raised`). `line_highlight` alone is made for `panel` and nearly
/// vanishes on `panel_raised` in Ember.
fn band_color(ui: &arcode_ui_kit::UiTheme) -> gpui::Hsla {
    let u = &ui.source.ui;
    let on_popover = |c: arcode_config::Color| c.contrast_ratio(u.surface_raised);
    let color = if on_popover(u.pill) >= BAND_CONTRAST {
        u.pill
    } else {
        [u.hover, u.selection_inactive, u.line_highlight, u.selection]
            .into_iter()
            .max_by(|a, b| on_popover(*a).total_cmp(&on_popover(*b)))
            .unwrap_or(u.pill)
    };
    arcode_ui_kit::hsla(color)
}

fn preview_element(
    editor: &Editor,
    layout: &LayoutSnapshot,
    plan: &PreviewPlan,
    cx: &App,
) -> AnyElement {
    let ui = theme(cx);
    let colors = &ui.colors;
    let style = &layout.style;
    let lh = layout.line_height;
    let number_size = number_font_size(style.font_size);
    let digits = plan.lines.end.to_string().len().max(2);
    let number_w = layout.char_width * (number_size / style.font_size) * digits as f32 + px(20.);
    let highlights = editor.highlights.as_ref();
    let mut text = String::new();
    let rows = plan.lines.clone().map(|line| {
        text.clear();
        let slice = editor.buffer.line(line);
        let slice = slice.slice(..slice.len_chars().min(MAX_CHARS));
        for chunk in slice.chunks() {
            text.push_str(chunk);
        }
        let trimmed = text.trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}', '\u{85}']);
        let spans = highlights
            .and_then(|h| h.spans.get(line.checked_sub(h.lines.start)?))
            .map_or(&[][..], Vec::as_slice);
        let display = build_display_line(trimmed, spans, &[], style.tab_size, &style.font, ui);
        let current = line == plan.hovered;
        div()
            .flex()
            .flex_row()
            .h(lh)
            .when(current, |d| d.bg(band_color(ui)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .w(number_w)
                    .pr(px(12.))
                    .text_size(number_size)
                    .text_color(if current {
                        colors.gutter_text_active
                    } else {
                        colors.gutter_text
                    })
                    .child(SharedString::from((line + 1).to_string())),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(StyledText::new(display.text).with_runs(display.runs)),
            )
    });
    popover(cx)
        .w(plan.bounds.size.width)
        .h(plan.bounds.size.height)
        .px(px(0.))
        .py(PAD_Y)
        .overflow_hidden()
        .font_family(style.font.family.clone())
        .text_size(style.font_size)
        .line_height(lh)
        .text_color(colors.text)
        .children(rows.collect::<Vec<_>>())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::BlockMap;
    use crate::gutter::GutterGeometry;
    use crate::minimap::{MinimapLayout, ScrollState};

    /// An editor of `line_count` lines at window y 40..840 with 20 px
    /// lines, its minimap at x 800..896 with 2 px rows.
    fn snapshot(line_count: usize) -> LayoutSnapshot {
        let bounds = Bounds::new(point(px(100.), px(40.)), size(px(808.), px(800.)));
        let scroll = ScrollState {
            scroll_y: Pixels::ZERO,
            max_scroll_y: px(20.) * line_count as f32,
            viewport_h: px(796.),
            line_height: px(20.),
        };
        let geometry = Editor::minimap_geometry(bounds, px(96.), line_count, scroll, 1.);
        let minimap = MinimapLayout {
            geometry,
            lines: 0..line_count.min(398),
            tiles: Vec::new(),
            tile_size: size(px(84.), px(128.)),
            col_width: px(1.),
        };
        LayoutSnapshot {
            bounds,
            text_bounds: bounds,
            gutter_width: px(50.),
            gutter: GutterGeometry::new(&[], true, px(7.), 4),
            line_height: px(20.),
            char_width: px(8.),
            scroll_x: Pixels::ZERO,
            scroll_y: Pixels::ZERO,
            max_scroll_y: scroll.max_scroll_y,
            lines: Vec::new(),
            blocks: BlockMap::default(),
            block_rows: Vec::new(),
            lens_hits: Vec::new(),
            tab_size: 4,
            scrollbar: None,
            minimap: Some(minimap),
            style: test_style(),
        }
    }

    fn test_style() -> crate::style::EditorStyle {
        crate::style::EditorStyle {
            font: gpui::font("Mono"),
            font_size: px(14.),
            line_height: px(20.),
            tab_size: 4,
            highlight_current_line: true,
            show_line_numbers: true,
            cursor_blink: false,
            minimap_width: Some(px(96.)),
            fade_unused: true,
        }
    }

    // The hovered line's band must show on the popover in every built-in
    // theme (it used `line_highlight`, 1.04:1 on Ember's popover).
    #[test]
    fn the_hovered_band_shows_on_the_popover() {
        let registry = arcode_config::ThemeRegistry::builtin();
        for name in arcode_config::theme::BUILTIN_THEME_NAMES {
            let source = registry.get(name).expect("built in").clone();
            let ui = arcode_ui_kit::UiTheme::new(std::sync::Arc::new(source.clone()));
            let band = band_color(&ui);
            let ratio = [
                source.ui.pill,
                source.ui.hover,
                source.ui.selection_inactive,
            ]
            .into_iter()
            .chain([source.ui.line_highlight, source.ui.selection])
            .find(|c| arcode_ui_kit::hsla(*c) == band)
            .expect("a theme colour")
            .contrast_ratio(source.ui.surface_raised);
            assert!(ratio >= BAND_CONTRAST, "{name}: {ratio:.3}");
            if name == "Arcode Ember" || name == "Arcode Chalk" {
                assert_eq!(band, ui.colors.pill, "{name}");
            }
        }
    }

    const WINDOW: Size<Pixels> = Size {
        width: px(1000.),
        height: px(900.),
    };

    #[test]
    fn shows_the_lines_around_the_hovered_row_beside_it() {
        let layout = snapshot(1000);
        let g = layout.minimap.as_ref().unwrap().geometry;
        // Rows are 2 px: y 40 + 201 is row 100.
        let plan = preview_plan(&layout, px(241.), 1000, WINDOW).expect("plan");
        assert_eq!(plan.hovered, 100);
        assert_eq!(plan.lines, 93..108);
        // Left of the minimap, 8 px away, inside the editor.
        assert_eq!(plan.bounds.right(), g.bounds.left() - GAP);
        assert!(plan.bounds.left() >= layout.bounds.left());
        // The hovered line's middle is level with the mouse's row.
        let line_mid = plan.bounds.top() + BORDER + PAD_Y + px(20.) * 7. + px(10.);
        assert!((line_mid - g.row_top(100) - px(1.)).abs() <= px(0.5));
        assert_eq!(
            plan.bounds.size.height,
            px(20.) * 15. + PAD_Y * 2. + BORDER * 2.
        );
        // Off the minimap vertically: none.
        assert!(preview_plan(&layout, px(10.), 1000, WINDOW).is_none());
    }

    #[test]
    fn clamps_at_the_ends_of_the_file_and_window() {
        let layout = snapshot(1000);
        // The first row: lines 0..15, the preview pushed down to the top.
        let plan = preview_plan(&layout, px(40.5), 1000, WINDOW).expect("plan");
        assert_eq!((plan.hovered, plan.lines.clone()), (0, 0..15));
        assert_eq!(plan.bounds.top(), px(40.) + MARGIN);
        // The last rows of a short file: the last 15 lines, kept above
        // the bottom edge.
        let layout = snapshot(60);
        let plan = preview_plan(&layout, px(40.) + px(2.) * 59. + px(1.), 60, WINDOW);
        let plan = plan.expect("plan");
        assert_eq!((plan.hovered, plan.lines.clone()), (59, 45..60));
        // A window shorter than the editor keeps the preview inside it.
        let small = Size {
            width: px(1000.),
            height: px(200.),
        };
        let plan = preview_plan(&layout, px(120.), 60, small).expect("plan");
        assert!(plan.bounds.bottom() <= px(200.) - MARGIN);
        assert!(plan.lines.len() < PREVIEW_LINES);
        // Fewer lines than the preview holds: all of them.
        assert_eq!(
            preview_plan(&snapshot(5), px(41.), 5, WINDOW).map(|p| p.lines),
            Some(0..5)
        );
    }

    #[test]
    fn needs_room_left_of_the_minimap() {
        let mut layout = snapshot(1000);
        let m = layout.minimap.as_mut().unwrap();
        let narrow = Bounds::new(
            point(m.geometry.bounds.left() - px(100.), px(40.)),
            size(px(100.), px(800.)),
        );
        layout.bounds = narrow;
        assert!(preview_plan(&layout, px(241.), 1000, WINDOW).is_none());
    }
}
