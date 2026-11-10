//! When the minimap shows: only while the document is taller than the
//! editor. It follows typing, pasting, deleting, window resizes and font
//! size changes at once, never flips between frames, and its tiles are
//! reused while nothing changes.

use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, ThemeRegistry};
use gpui::{Entity, TestAppContext, VisualTestContext, size};

use crate::Editor;

fn set_config(cx: &mut VisualTestContext, config: Config) {
    cx.update(|_, cx| arcode_ui_kit::set_settings(config, cx));
}

fn open(cx: &mut TestAppContext, text: String) -> (Entity<Editor>, &mut VisualTestContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let mut buffer = Buffer::new(&text);
        buffer.set_path("test.rs");
        let mut editor = Editor::new(buffer, cx);
        editor.set_minimap_allowed(true, cx);
        editor
    });
    draw(cx);
    (editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

fn shown(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |e, _| e.minimap_bounds().is_some())
}

/// Display rows that fit in the editor's view.
fn rows_that_fit(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |e, _| {
        let layout = e.layout.as_ref().expect("drawn");
        (layout.text_bounds.size.height / layout.line_height).floor() as usize
    })
}

/// `n` lines without a final newline (so `n` display rows).
fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("let x{i} = {i};"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn set_text(editor: &Entity<Editor>, cx: &mut VisualTestContext, text: &str) {
    editor.update(cx, |e, cx| {
        let last = e.line_count() - 1;
        let end = Point::new(last, e.buffer().line_len(last));
        e.apply_edits(vec![(Point::new(0, 0)..end, text.into())], cx);
    });
}

fn type_at_end(editor: &Entity<Editor>, cx: &mut VisualTestContext, text: &str) {
    editor.update(cx, |e, cx| {
        let last = e.line_count() - 1;
        let end = Point::new(last, e.buffer().line_len(last));
        e.apply_edits(vec![(end..end, text.into())], cx);
    });
}

#[gpui::test]
fn shown_only_while_the_document_overflows(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, lines(3));
    assert!(!shown(&editor, cx), "a short file gets the full width");
    let fit = rows_that_fit(&editor, cx);
    // Exactly as many rows as fit: still no minimap.
    set_text(&editor, cx, &lines(fit));
    draw(cx);
    assert!(!shown(&editor, cx));
    // One more line (typing Enter) overflows: it appears in that frame.
    type_at_end(&editor, cx, "\nmore");
    draw(cx);
    assert!(shown(&editor, cx), "appears as soon as the file overflows");
    // Deleting the line again hides it.
    set_text(&editor, cx, &lines(fit));
    draw(cx);
    assert!(!shown(&editor, cx), "disappears when the file fits again");
    // A big paste.
    type_at_end(&editor, cx, &format!("\n{}", lines(500)));
    draw(cx);
    assert!(shown(&editor, cx));
}

#[gpui::test]
fn the_decision_never_flips_between_frames(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, lines(3));
    let fit = rows_that_fit(&editor, cx);
    for n in [fit - 1, fit, fit + 1, fit + 2] {
        set_text(&editor, cx, &lines(n));
        draw(cx);
        let first = shown(&editor, cx);
        assert_eq!(first, n > fit, "{n} rows, {fit} fit");
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = count.clone();
        cx.update(|_, cx| {
            cx.observe(&editor, move |_, _| counter.set(counter.get() + 1))
                .detach()
        });
        for _ in 0..5 {
            draw(cx);
            assert_eq!(shown(&editor, cx), first, "{n} rows");
        }
        assert_eq!(count.get(), 0, "{n} rows: no redraw requested");
    }
}

#[gpui::test]
fn follows_window_resizes_and_font_size(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, lines(3));
    let fit = rows_that_fit(&editor, cx);
    set_text(&editor, cx, &lines(fit - 4));
    draw(cx);
    assert!(!shown(&editor, cx));
    // A shorter window: the same file overflows.
    let window = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(size(window.width, window.height * 0.5));
    draw(cx);
    assert!(shown(&editor, cx), "shorter window");
    cx.simulate_resize(window);
    draw(cx);
    assert!(!shown(&editor, cx), "back to the full height");
    // A bigger font makes the lines taller.
    let mut config = Config::default();
    config.editor.font_size *= 2.;
    set_config(cx, config);
    draw(cx);
    assert!(shown(&editor, cx), "bigger font");
    set_config(cx, Config::default());
    draw(cx);
    assert!(!shown(&editor, cx));
    // editor.minimap = false hides it even when the file overflows.
    set_text(&editor, cx, &lines(fit * 3));
    let mut config = Config::default();
    config.editor.minimap = false;
    set_config(cx, config);
    draw(cx);
    assert!(!shown(&editor, cx));
}

#[gpui::test]
fn tiles_are_reused_while_nothing_changes(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, lines(20_000));
    for _ in 0..3 {
        draw(cx);
    }
    let built = |editor: &Entity<Editor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |e, _| e.minimap.tiles.built)
    };
    let before = built(&editor, cx);
    assert!(before > 0);
    for _ in 0..5 {
        editor.update(cx, |_, cx| cx.notify());
        draw(cx);
    }
    assert_eq!(built(&editor, cx), before, "redraws reuse the tiles");
    // Scrolling a few lines at a time builds a new tile now and then.
    for step in 1..=30 {
        editor.update(cx, |e, cx| {
            e.scroll_to_line(step * 3, crate::ScrollAnchor::Top, cx)
        });
        draw(cx);
    }
    let after = built(&editor, cx);
    assert!(after - before <= 4, "{before} -> {after}");
    // Hiding the minimap retires every tile; their images are dropped
    // from the atlas with the next frame.
    editor.update(cx, |e, cx| e.set_minimap_allowed(false, cx));
    draw(cx);
    editor.read_with(cx, |e, _| assert_eq!(e.minimap.tiles.len(), 0));
    draw(cx);
    editor.read_with(cx, |e, _| assert!(e.minimap.retired.is_empty()));
}
