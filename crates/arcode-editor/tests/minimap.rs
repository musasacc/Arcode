//! The minimap: when it is shown, clicking, dragging and the wheel, hover,
//! and that an idle editor with a minimap never asks for frames.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use arcode_config::Config;
use arcode_editor::Editor;
use common::*;
use gpui::{
    Entity, Modifiers, MouseButton, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};

/// `lines` lines of Rust.
fn rust_lines(lines: usize) -> String {
    (0..lines)
        .map(|i| format!("fn f{i}() -> u32 {{ let x = {i}; x * 2 }}\n"))
        .collect()
}

/// Opens `text` as `test.rs` with `config`, minimap allowed.
fn open_with_minimap<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    config: Config,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    let (editor, cx) = open_at(cx, text, "test.rs", config);
    editor.update(cx, |e, cx| {
        e.set_minimap_allowed(true, cx);
        let start = arcode_buffer::Point::new(0, 0);
        e.select_ranges(vec![start..start], cx);
    });
    cx.run_until_parked();
    (editor, cx)
}

fn minimap(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    editor.read_with(cx, |e, _| e.minimap_bounds())
}

fn first_visible(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |e, _| e.visible_line_range().start)
}

/// Redraws the window and lets background work finish.
fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

/// Counts the editor's notifications (each one is a redraw request).
fn notifications(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Rc<RefCell<usize>> {
    let count = Rc::new(RefCell::new(0));
    let counter = count.clone();
    cx.update(|_, cx| {
        cx.observe(editor, move |_, _| *counter.borrow_mut() += 1)
            .detach()
    });
    count
}

#[gpui::test]
fn shown_only_where_allowed_and_enabled(cx: &mut TestAppContext) {
    let (editor, cx) = open_at(cx, &rust_lines(50), "test.rs", config());
    assert_eq!(editor.read_with(cx, |e, _| e.minimap_allowed()), None);
    assert!(minimap(&editor, cx).is_none(), "editors start without one");

    editor.update(cx, |e, cx| e.set_minimap_allowed(true, cx));
    cx.run_until_parked();
    let bounds = minimap(&editor, cx).expect("shown once allowed");
    assert_eq!(bounds.size.width, px(96.), "editor.minimap_width");
    // The text ends where the minimap starts.
    let (origin, _) = editor
        .read_with(cx, |e, _| {
            e.position_for_point(arcode_buffer::Point::new(0, 0))
        })
        .expect("laid out");
    assert!(origin.x < bounds.left());

    // editor.minimap = false hides it, and so does low-end mode.
    for (minimap_on, low_end) in [(false, false), (true, true)] {
        let mut config = config();
        config.editor.minimap = minimap_on;
        config.ui.low_end_mode = low_end;
        cx.update(|_, cx| arcode_ui_kit::set_settings(config, cx));
        draw(cx);
        assert!(minimap(&editor, cx).is_none(), "{minimap_on} {low_end}");
    }
    let mut config = config();
    config.editor.minimap_width = 120;
    cx.update(|_, cx| arcode_ui_kit::set_settings(config, cx));
    draw(cx);
    assert_eq!(minimap(&editor, cx).expect("back").size.width, px(120.));

    editor.update(cx, |e, cx| e.set_minimap_allowed(false, cx));
    cx.run_until_parked();
    assert!(minimap(&editor, cx).is_none());
}

#[gpui::test]
fn click_centers_the_row_and_dragging_scrolls(cx: &mut TestAppContext) {
    let (editor, cx) = open_with_minimap(cx, &rust_lines(3000), config());
    let bounds = minimap(&editor, cx).expect("shown");
    assert_eq!(first_visible(&editor, cx), 0);
    // At the top of the file the minimap shows its first rows, 2 px each:
    // a click 300 px down is row 150, which the editor then centers.
    let at = point(bounds.left() + px(30.), bounds.top() + px(301.));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    let range = editor.read_with(cx, |e, _| e.visible_line_range());
    let middle = (range.start + range.end) / 2;
    assert!(middle.abs_diff(150) <= 2, "{range:?}");
    // The cursor stays where it was.
    let cursor = editor.read_with(cx, |e, _| e.cursor_position());
    assert_eq!(cursor.line, 0);

    // Dragging the slider down scrolls down, proportionally to the file.
    let slider = editor
        .read_with(cx, |e, _| e.minimap_slider_bounds())
        .expect("slider");
    let before = first_visible(&editor, cx);
    let from = point(slider.left() + px(10.), slider.top() + px(2.));
    let to = point(from.x, from.y + px(100.));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    let after = first_visible(&editor, cx);
    assert!(after > before + 100, "{before} -> {after}");
    let moved = editor
        .read_with(cx, |e, _| e.minimap_slider_bounds())
        .expect("slider");
    assert!(
        (moved.top() - (slider.top() + px(100.))).abs() < px(2.),
        "the slider follows the mouse: {slider:?} -> {moved:?}"
    );

    // Dragging far up stops at the top.
    let slider = moved;
    let from = point(slider.left() + px(10.), slider.top() + px(2.));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(from.x, px(-500.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        point(from.x, px(-500.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(first_visible(&editor, cx), 0);
}

#[gpui::test]
fn the_wheel_over_the_minimap_scrolls_the_editor(cx: &mut TestAppContext) {
    let (editor, cx) = open_with_minimap(cx, &rust_lines(500), config());
    let bounds = minimap(&editor, cx).expect("shown");
    cx.simulate_event(ScrollWheelEvent {
        position: point(bounds.left() + px(20.), bounds.top() + px(50.)),
        delta: ScrollDelta::Lines(point(0., -10.)),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(first_visible(&editor, cx), 10);
}

#[gpui::test]
fn hover_redraws_only_on_enter_and_leave(cx: &mut TestAppContext) {
    let (editor, cx) = open_with_minimap(cx, &rust_lines(500), config());
    let bounds = minimap(&editor, cx).expect("shown");
    let count = notifications(&editor, cx);
    let inside = |dy: f32| point(bounds.left() + px(20.), bounds.top() + px(dy));
    cx.simulate_mouse_move(inside(40.), None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*count.borrow(), 1, "entering redraws once");
    for dy in [60., 80., 120.] {
        cx.simulate_mouse_move(inside(dy), None, Modifiers::none());
        cx.run_until_parked();
    }
    assert_eq!(*count.borrow(), 1, "moving inside does not redraw");
    let (text, _) = editor
        .read_with(cx, |e, _| {
            e.position_for_point(arcode_buffer::Point::new(2, 3))
        })
        .expect("laid out");
    cx.simulate_mouse_move(text, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*count.borrow(), 2, "leaving redraws once");
}

/// An idle editor with a minimap requests no frames, also for a large
/// file whose highlights come from background jobs (the text's lines
/// first, then the minimap's): the jobs end and nothing asks again.
fn assert_idle_draws_nothing(cx: &mut TestAppContext, large: bool) {
    let mut config = config();
    if large {
        config.editor.large_file_lines = 1000;
    }
    let (editor, cx) = open_with_minimap(cx, &rust_lines(5000), config);
    assert_eq!(editor.read_with(cx, |e, _| e.is_large_file()), large);
    // Let the highlight jobs settle.
    for _ in 0..4 {
        draw(cx);
    }
    let count = notifications(&editor, cx);
    for _ in 0..5 {
        draw(cx);
    }
    assert_eq!(*count.borrow(), 0);
    // After a scroll, the same.
    editor.update(cx, |e, cx| {
        e.scroll_to_line(3000, arcode_editor::ScrollAnchor::Top, cx)
    });
    for _ in 0..4 {
        draw(cx);
    }
    let settled = *count.borrow();
    for _ in 0..5 {
        draw(cx);
    }
    assert_eq!(*count.borrow(), settled);
}

#[gpui::test]
fn idle_editor_with_minimap_draws_nothing(cx: &mut TestAppContext) {
    assert_idle_draws_nothing(cx, false);
}

#[gpui::test]
fn idle_large_file_with_minimap_draws_nothing(cx: &mut TestAppContext) {
    assert_idle_draws_nothing(cx, true);
}

/// Average and 95th percentile of `times`, in milliseconds.
fn stats(times: &mut [std::time::Duration]) -> (f64, f64) {
    times.sort();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.;
    let avg = times.iter().map(|d| ms(*d)).sum::<f64>() / times.len() as f64;
    (avg, ms(times[times.len() * 95 / 100]))
}

/// Times whole-window draws (layout, prepaint, paint) with the minimap on
/// and off: scrolling a 100 000-line file in steps of three lines and in
/// jumps, and typing in a 3 000-line file. Run with
/// `cargo test -p arcode-editor --test minimap -- --ignored --nocapture`.
#[gpui::test]
#[ignore]
fn frame_cost_with_and_without_minimap(cx: &mut TestAppContext) {
    use arcode_editor::ScrollAnchor;
    use std::time::Instant;
    let big = rust_lines(100_000);
    let medium = rust_lines(3000);
    let mut report = Vec::new();
    for on in [false, true] {
        let mut config = config();
        config.editor.minimap = on;
        let (editor, cx) = open_with_minimap(cx, &big, config.clone());
        for _ in 0..3 {
            draw(cx);
        }
        if let Some(b) = minimap(&editor, cx) {
            report.push(format!("minimap {}x{} px", b.size.width, b.size.height));
        }
        let mut step = Vec::new();
        let mut follow = Vec::new();
        for i in 0..300 {
            editor.update(cx, |e, cx| {
                e.scroll_to_line(20_000 + i * 3, ScrollAnchor::Top, cx)
            });
            let t = Instant::now();
            cx.update(|window, cx| window.draw(cx).clear());
            step.push(t.elapsed());
            cx.run_until_parked();
            let t = Instant::now();
            cx.update(|window, cx| window.draw(cx).clear());
            follow.push(t.elapsed());
        }
        let mut jump = Vec::new();
        for i in 0..100 {
            let line = (i * 7919) % 100_000;
            editor.update(cx, |e, cx| e.scroll_to_line(line, ScrollAnchor::Top, cx));
            let t = Instant::now();
            cx.update(|window, cx| window.draw(cx).clear());
            jump.push(t.elapsed());
            cx.run_until_parked();
        }
        let mut idle = Vec::new();
        for _ in 0..100 {
            editor.update(cx, |_, cx| cx.notify());
            let t = Instant::now();
            cx.update(|window, cx| window.draw(cx).clear());
            idle.push(t.elapsed());
        }
        let (editor, cx) = open_with_minimap(cx, &medium, config);
        editor.update(cx, |e, cx| {
            let at = arcode_buffer::Point::new(1500, 0);
            e.select_ranges(vec![at..at], cx)
        });
        for _ in 0..3 {
            draw(cx);
        }
        let mut typing = Vec::new();
        for _ in 0..100 {
            cx.simulate_input("x");
            let t = Instant::now();
            cx.update(|window, cx| window.draw(cx).clear());
            typing.push(t.elapsed());
            cx.run_until_parked();
        }
        let label = if on { "minimap on " } else { "minimap off" };
        for (what, times) in [
            ("100k lines, 3-line scroll", &mut step),
            ("100k lines, frame after highlight job", &mut follow),
            ("100k lines, jump", &mut jump),
            ("100k lines, redraw without change", &mut idle),
            ("3000 lines, typing", &mut typing),
        ] {
            let (avg, p95) = stats(times);
            report.push(format!(
                "{label} | {what}: avg {avg:.2} ms, p95 {p95:.2} ms"
            ));
        }
    }
    for line in report {
        eprintln!("{line}");
    }
}
