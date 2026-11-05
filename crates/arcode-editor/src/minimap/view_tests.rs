//! The minimap in a drawn editor: its blocks use the syntax colours of
//! lines far below the text in view, and keep them while edits are
//! highlighted again.

use std::sync::Arc;

use arcode_buffer::Buffer;
use arcode_config::{Config, ThemeRegistry};
use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};

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

/// The last line in the minimap's view, the last line of text in view and
/// the colours of that minimap line's blocks.
fn last_minimap_line(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
) -> (usize, usize, Vec<gpui::Hsla>) {
    editor.read_with(cx, |e, _| {
        let layout = e.layout.as_ref().expect("drawn");
        let m = layout.minimap.as_ref().expect("minimap shown");
        let lines = super::lines_for_rows(&layout.blocks, m.geometry.rows_in_view(), m.line_count);
        let last = lines.end - 1;
        let colors = m.rows.line(last).iter().map(|s| s.color).collect();
        (last, layout.visible_range().end, colors)
    })
}

fn keyword(cx: &mut VisualTestContext) -> gpui::Hsla {
    cx.update(|_, cx| {
        arcode_ui_kit::theme(cx)
            .syntax("keyword")
            .color
            .expect("keyword colour")
            .opacity(super::rows::BLOCK_OPACITY)
    })
}

fn assert_highlighted(cx: &mut TestAppContext, config: Config) {
    let (editor, cx) = open(cx, config, 5000);
    let (line, visible_end, colors) = last_minimap_line(&editor, cx);
    assert!(line > visible_end + 50, "{line} vs {visible_end}");
    // "fn f123() {}": the keyword block comes first.
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
