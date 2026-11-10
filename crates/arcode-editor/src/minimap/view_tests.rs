//! The minimap in a drawn editor: its glyphs use the syntax colours of
//! lines far below the text in view, and keep them while edits are
//! highlighted again; no minimap for a short file; the hover preview goes
//! when the mouse leaves the window.

use std::sync::Arc;

use arcode_buffer::Buffer;
use arcode_config::{Config, ThemeRegistry};
use gpui::{
    Entity, EntityInputHandler, Modifiers, MouseExitEvent, TestAppContext, VisualTestContext,
    point, px,
};

use crate::Editor;

fn open(
    cx: &mut TestAppContext,
    config: Config,
    lines: usize,
) -> (Entity<Editor>, &mut VisualTestContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(config, cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        crate::init(cx);
    });
    let text: String = (0..lines).map(|i| format!("fn f{i}() {{}}\n")).collect();
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let mut buffer = Buffer::new(&text);
        buffer.set_path("test.rs");
        let mut editor = Editor::new(buffer, cx);
        editor.set_minimap_allowed(true, cx);
        editor
    });
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    (editor, cx)
}

/// Colours (BGR, without alpha) of the lit pixels on the lower pixel row of buffer line
/// `line` in the minimap's tiles, left to right.
fn line_colors(layout: &crate::layout::LayoutSnapshot, line: usize) -> Vec<[u8; 3]> {
    let m = layout.minimap.as_ref().expect("minimap shown");
    let row = layout.blocks.row_for_line(line);
    let Some(tile) = m
        .tiles
        .iter()
        .find(|t| (t.first_row..t.first_row + super::tiles::TILE_ROWS).contains(&row))
    else {
        return Vec::new();
    };
    let size = tile.image.size(0);
    let (w, h) = (size.width.0 as usize, size.height.0 as usize);
    let pitch = h / super::tiles::TILE_ROWS;
    let y = (row - tile.first_row) * pitch + pitch / 2;
    let bytes = tile.image.as_bytes(0).expect("frame");
    let mut colors: Vec<[u8; 3]> = Vec::new();
    for x in 0..w {
        let i = (y * w + x) * 4;
        let bgr: [u8; 3] = bytes[i..i + 3].try_into().unwrap();
        if bytes[i + 3] > 0 && colors.last() != Some(&bgr) {
            colors.push(bgr);
        }
    }
    colors
}

/// The last line in the minimap's view, the last line of text in view and
/// the colours of that minimap line's glyphs.
fn last_minimap_line(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
) -> (usize, usize, Vec<[u8; 3]>) {
    editor.read_with(cx, |e, _| {
        let layout = e.layout.as_ref().expect("drawn");
        let m = layout.minimap.as_ref().expect("minimap shown");
        let lines = super::lines_for_rows(
            &layout.blocks,
            m.geometry.rows_in_view(),
            e.buffer.line_count(),
        );
        let last = lines.end - 1;
        (last, layout.visible_range().end, line_colors(layout, last))
    })
}

fn keyword(cx: &mut VisualTestContext) -> [u8; 3] {
    cx.update(|_, cx| {
        let color = arcode_ui_kit::theme(cx)
            .syntax("keyword")
            .color
            .expect("keyword colour");
        let [b, g, r, _] = super::raster::bgra(color, 1.);
        [b, g, r]
    })
}

fn assert_highlighted(cx: &mut TestAppContext, config: Config) {
    let (editor, cx) = open(cx, config, 5000);
    let (line, visible_end, colors) = last_minimap_line(&editor, cx);
    assert!(line > visible_end + 50, "{line} vs {visible_end}");
    // "fn f123() {}": the keyword's glyphs come first.
    assert_eq!(
        colors.first(),
        Some(&keyword(cx)),
        "line {line}: {colors:?}"
    );
}

/// Types a char at the top and checks every frame drawn while the edit is
/// highlighted again: the minimap's last line never loses its colours.
/// Then scrolls down in steps: highlights that arrive always cover the
/// minimap's lines.
fn assert_colours_survive_edits(cx: &mut TestAppContext, config: Config, lines: usize) {
    let (editor, cx) = open(cx, config, lines);
    let keyword = keyword(cx);
    // Every time new highlights arrive they must still cover the lines in
    // the minimap's view, or the next frame draws those lines without
    // colours until another job brings them back.
    let gaps = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = gaps.clone();
    cx.update(|_, cx| {
        cx.observe(&editor, move |editor, cx| {
            let e = editor.read(cx);
            let (Some(layout), Some(cache)) = (e.layout.as_ref(), e.highlights.as_ref()) else {
                return;
            };
            let Some(m) = layout.minimap.as_ref() else {
                return;
            };
            if cache.lines.start > m.lines.start || cache.lines.end < m.lines.end {
                seen.borrow_mut()
                    .push((cache.lines.clone(), m.lines.clone()));
            }
        })
        .detach()
    });
    for ch in ["x", "y", "\n"] {
        editor.update_in(cx, |e, window, cx| {
            e.replace_text_in_range(None, ch, window, cx)
        });
        for frame in 0..4 {
            cx.update(|window, cx| window.draw(cx).clear());
            let (line, visible_end, colors) = last_minimap_line(&editor, cx);
            assert!(line > visible_end + 50, "{line} vs {visible_end}");
            assert_eq!(
                colors.first(),
                Some(&keyword),
                "after {ch:?}, frame {frame}, line {line}: {colors:?}"
            );
            cx.run_until_parked();
        }
    }
    // Scrolling: new highlights join the ones already there.
    for step in 1..=8 {
        editor.update(cx, |e, cx| {
            e.scroll_to_line(step * 120, crate::ScrollAnchor::Top, cx)
        });
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear());
            cx.run_until_parked();
        }
    }
    let (line, _, colors) = last_minimap_line(&editor, cx);
    assert_eq!(colors.first(), Some(&keyword), "line {line}: {colors:?}");
    assert!(
        gaps.borrow().is_empty(),
        "highlights vs minimap lines: {gaps:?}"
    );
}

#[gpui::test]
fn minimap_lines_below_the_text_are_highlighted(cx: &mut TestAppContext) {
    assert_highlighted(cx, Config::default());
}

#[gpui::test]
fn large_file_minimap_lines_are_highlighted_too(cx: &mut TestAppContext) {
    let mut config = Config::default();
    config.editor.large_file_lines = 1000;
    assert_highlighted(cx, config);
}

#[gpui::test]
fn large_file_minimap_keeps_colours_while_typing(cx: &mut TestAppContext) {
    let mut config = Config::default();
    config.editor.large_file_lines = 1000;
    assert_colours_survive_edits(cx, config, 5000);
}

#[gpui::test]
fn background_highlighting_keeps_minimap_colours_while_typing(cx: &mut TestAppContext) {
    // Above the size where parsing and highlighting run in the background.
    let lines = crate::syntax_sync::ASYNC_SYNTAX_BYTES / 12 + 1000;
    assert_colours_survive_edits(cx, Config::default(), lines);
}

#[gpui::test]
fn long_highlights_starting_far_above_the_view_are_drawn(cx: &mut TestAppContext) {
    use crate::ext::BackgroundHighlight;
    let (editor, cx) = open(cx, Config::default(), 5000);
    // One highlight from line 10 to 3000, the minimap's view near 2500.
    editor.update(cx, |e, cx| {
        let long = BackgroundHighlight::lines(10..3000, "warning".into());
        e.set_background_highlights("test", vec![long], cx);
        e.scroll_to_line(2500, crate::ScrollAnchor::Center, cx);
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    let under = |editor: &Entity<Editor>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let colors = arcode_ui_kit::theme(cx).colors.clone();
            editor.read_with(cx, |e, _| {
                let layout = e.layout.as_ref().expect("drawn");
                let m = layout.minimap.as_ref().expect("minimap shown");
                assert!(m.lines.start > 2000, "{:?}", m.lines);
                let mut out = super::MinimapPaint::default();
                super::minimap_decorations(e, layout, false, &colors, &mut out);
                out.under.len()
            })
        })
    };
    // Background and slider, then the highlight.
    assert_eq!(under(&editor, cx), 3);
    editor.update(cx, |e, cx| e.clear_decorations("test", cx));
    cx.update(|window, cx| window.draw(cx).clear());
    assert_eq!(under(&editor, cx), 2);
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

fn preview_lines(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<usize> {
    editor.read_with(cx, |e, _| e.minimap_preview().map(|(lines, _)| lines.len()))
}

#[gpui::test]
fn no_minimap_and_no_preview_for_a_short_file(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, Config::default(), 5);
    assert!(editor.read_with(cx, |e, _| e.minimap_bounds()).is_none());
    let window = cx.update(|window, _| window.viewport_size());
    let right = point(window.width - px(60.), px(100.));
    cx.simulate_mouse_move(right, None, Modifiers::none());
    draw(cx);
    assert!(preview_lines(&editor, cx).is_none());
}

// The mouse can leave the window straight from the minimap, with no move
// over the text first: the preview must not stay behind.
#[gpui::test]
fn leaving_the_window_from_the_minimap_hides_the_preview(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, Config::default(), 3000);
    let bounds = editor
        .read_with(cx, |e, _| e.minimap_bounds())
        .expect("shown");
    let at = point(bounds.left() + px(30.), bounds.top() + px(301.));
    cx.simulate_mouse_move(at, None, Modifiers::none());
    draw(cx);
    assert!(preview_lines(&editor, cx).is_some());
    cx.simulate_event(MouseExitEvent {
        position: point(bounds.right() + px(40.), at.y),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    draw(cx);
    assert!(preview_lines(&editor, cx).is_none(), "gone with the mouse");
    cx.simulate_mouse_move(at, None, Modifiers::none());
    draw(cx);
    assert!(preview_lines(&editor, cx).is_some(), "back with the mouse");
}
