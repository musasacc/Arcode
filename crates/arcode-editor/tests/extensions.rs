//! Editor extension points through GPUI's test platform.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, ThemeRegistry};
use arcode_editor::{
    CodeLens, ColorKey, CursorShape, DotStyle, Editor, EditorEvent, EditorPopover,
    GutterDecoration, GutterLane, GutterMark, LensItem, LensPlacement, VirtualText,
};
use arcode_ui_kit::IconName;
use gpui::{
    Entity, Focusable, Modifiers, TestAppContext, VisualTestContext, div, point, prelude::*, px,
};

type Events = Rc<RefCell<Vec<EditorEvent>>>;

fn open_with<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    config: Config,
) -> (Entity<Editor>, &'a mut VisualTestContext, Events) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(config, cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        arcode_editor::init(cx);
    });
    let text = text.to_string();
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let mut buffer = Buffer::new(&text);
        buffer.set_path("test.rs");
        Editor::new(buffer, cx)
    });
    let events: Events = Rc::default();
    let sink = events.clone();
    cx.update(|window, cx| {
        window.focus(&editor.focus_handle(cx));
        cx.subscribe(&editor, move |_, e: &EditorEvent, _| {
            sink.borrow_mut().push(e.clone())
        })
        .detach();
    });
    cx.run_until_parked();
    (editor, cx, events)
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext, Events) {
    open_with(cx, text, Config::default())
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().to_string())
}

fn cursor(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Point {
    editor.read_with(cx, |e, _| e.cursor_position())
}

/// Line height with default settings (14px * 1.5).
const LH: f32 = 21.;

#[gpui::test]
fn gutter_clicks_emit_events(cx: &mut TestAppContext) {
    let (editor, cx, events) = open(cx, "fn a() {}\n\n#[test]\nfn t() {}\n");
    editor.update(cx, |e, cx| {
        e.set_gutter_lane_visible(GutterLane::Breakpoints, true, cx);
        let play = GutterDecoration {
            line: 3,
            lane: GutterLane::Runnables,
            mark: GutterMark::Icon {
                icon: IconName::Play,
                color: ColorKey::new("success"),
            },
            tag: 7,
        };
        let bp = GutterDecoration {
            line: 0,
            lane: GutterLane::Breakpoints,
            mark: GutterMark::Dot {
                color: ColorKey::new("error"),
                style: DotStyle::Conditional,
            },
            tag: 1,
        };
        e.set_gutter_decorations("run", vec![play], cx);
        e.set_gutter_decorations("debug", vec![bp], cx);
    });
    cx.run_until_parked();
    let row = |line: f32| px(LH * line + 10.);
    // Empty breakpoint slot.
    cx.simulate_click(point(px(8.), row(1.)), Modifiers::default());
    // Existing breakpoint with shift held.
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    cx.simulate_click(point(px(8.), row(0.)), shift);
    // Run button (second lane).
    cx.simulate_click(point(px(24.), row(3.)), Modifiers::default());
    // Gutter clicks do not move the cursor.
    assert_eq!(cursor(&editor, cx), Point::new(0, 0));
    // An empty run slot is not clickable: it selects the line as usual.
    cx.simulate_click(point(px(24.), row(1.)), Modifiers::default());
    let sel = editor.read_with(cx, |e, _| e.selections().primary().range());
    assert_eq!(sel, 10..11);
    let clicks: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|e| match e {
            EditorEvent::GutterClicked {
                line,
                lane,
                tag,
                modifiers,
            } => Some((*line, *lane, *tag, modifiers.shift)),
            _ => None,
        })
        .collect();
    assert_eq!(
        clicks,
        vec![
            (1, GutterLane::Breakpoints, None, false),
            (0, GutterLane::Breakpoints, Some(1), true),
            (3, GutterLane::Runnables, Some(7), false),
        ]
    );
    cx.simulate_keystrokes("ctrl-home");
    // Decorations follow inserted lines.
    cx.simulate_keystrokes("enter");
    let line = editor.read_with(cx, |e, _| e.gutter_decorations("run")[0].line);
    assert_eq!(line, 4);
}

#[gpui::test]
fn code_lens_above_and_at_end(cx: &mut TestAppContext) {
    let (editor, cx, events) = open(cx, "a\n    b\nc\n");
    editor.update(cx, |e, cx| {
        let items = |a: u64, b: u64| {
            vec![
                LensItem {
                    id: a,
                    label: "Run".into(),
                },
                LensItem {
                    id: b,
                    label: "Debug".into(),
                },
            ]
        };
        e.set_code_lenses(
            "tests",
            vec![
                CodeLens {
                    line: 1,
                    placement: LensPlacement::Above,
                    items: items(1, 2),
                },
                CodeLens {
                    line: 2,
                    placement: LensPlacement::EndOfLine,
                    items: items(3, 4),
                },
            ],
            cx,
        );
    });
    cx.run_until_parked();
    // Line 1 now sits one row lower because of the lens row above it.
    let (p1, lh) = editor
        .read_with(cx, |e, _| e.position_for_point(Point::new(1, 4)))
        .expect("line 1 laid out");
    assert_eq!(p1.y, px(2. * LH));
    assert_eq!(lh, px(LH));
    // Click "Run" on the block row (aligned with the indentation).
    cx.simulate_click(
        point(p1.x + px(4.), p1.y - px(LH / 2.)),
        Modifiers::default(),
    );
    // Click "Debug" at the end of line 2 (after "c", a gap, "Run | ").
    let (p2, _) = editor
        .read_with(cx, |e, _| e.position_for_point(Point::new(2, 1)))
        .expect("line 2");
    let mut hit = false;
    for dx in (20..200).step_by(4) {
        cx.simulate_click(
            point(p2.x + px(dx as f32), p2.y + px(10.)),
            Modifiers::default(),
        );
        if events
            .borrow()
            .iter()
            .any(|e| matches!(e, EditorEvent::CodeLensClicked { id: 4, .. }))
        {
            hit = true;
            break;
        }
    }
    assert!(hit, "Debug lens not clickable");
    let first = events.borrow().iter().find_map(|e| match e {
        EditorEvent::CodeLensClicked { set, line, id } => Some((set.to_string(), *line, *id)),
        _ => None,
    });
    assert_eq!(first, Some(("tests".to_string(), 1, 1)));
    // Clicking the text of line 1 still places the cursor on line 1.
    cx.simulate_click(point(p1.x, p1.y + px(10.)), Modifiers::default());
    assert_eq!(cursor(&editor, cx), Point::new(1, 4));
}

#[gpui::test]
fn inlay_hints_are_skipped_by_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "let x = 1;\n");
    let x_of = |cx: &mut VisualTestContext, col| {
        editor
            .read_with(cx, |e, _| e.position_for_point(Point::new(0, col)))
            .expect("laid out")
            .0
            .x
    };
    let before = x_of(cx, 6) - x_of(cx, 5);
    let x5_before = x_of(cx, 5);
    editor.update(cx, |e, cx| {
        let hint = VirtualText::inline(Point::new(0, 5), ": i32");
        let blame = VirtualText::end_of_line(0, "you, 2 days ago");
        e.set_virtual_text("hints", vec![hint, blame], cx);
    });
    cx.run_until_parked();
    let after = x_of(cx, 6) - x_of(cx, 5);
    let x5_after = x_of(cx, 5);
    assert!(
        (after - before).abs() < px(0.01),
        "glyph after the hint keeps its width"
    );
    // The glyph at col 5 moved right by about the hint's width (5 chars).
    assert!(x5_after - x5_before >= before * 4., "hint not inserted");
    // Moving right from col 5 lands on col 6 (the hint is skipped).
    cx.simulate_keystrokes("end home right right right right right");
    assert_eq!(cursor(&editor, cx), Point::new(0, 5));
    cx.simulate_keystrokes("right");
    assert_eq!(cursor(&editor, cx), Point::new(0, 6));
    // Typing does not insert hint text into the buffer.
    cx.simulate_input("!");
    assert_eq!(text(&editor, cx), "let x != 1;\n");
    // The hint moved with the text after it.
    let pos = editor.read_with(cx, |e, _| e.virtual_text("hints")[0].position);
    assert_eq!(
        pos,
        arcode_editor::VirtualTextPosition::Inline(Point::new(0, 5))
    );
}

#[gpui::test]
fn key_interceptors_run_before_bindings(cx: &mut TestAppContext) {
    let (editor, cx, _) = open(cx, "abc\ndef\n");
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = seen.clone();
    let id = editor.update(cx, |e, cx| {
        e.add_key_interceptor(
            0,
            Rc::new(move |editor, ks, _window, cx| {
                if ks.key == "j" || ks.key == "down" {
                    sink.borrow_mut().push(ks.key.clone());
                    editor.update(cx, |e, cx| e.set_cursor_shape(CursorShape::Block, cx));
                    return true;
                }
                false
            }),
            cx,
        )
    });
    cx.simulate_keystrokes("j down");
    cx.simulate_input("k");
    assert_eq!(text(&editor, cx), "kabc\ndef\n");
    assert_eq!(*seen.borrow(), vec!["j".to_string(), "down".to_string()]);
    assert_eq!(cursor(&editor, cx), Point::new(0, 1));
    assert_eq!(
        editor.read_with(cx, |e, _| e.cursor_shape()),
        CursorShape::Block
    );
    // Popover handlers run before installed interceptors.
    let popup_keys: Rc<RefCell<u32>> = Rc::default();
    let count = popup_keys.clone();
    editor.update(cx, |e, cx| {
        let content = cx.new(|_| Popup).into();
        let popover = EditorPopover::new(Point::new(0, 1), content).on_key(move |_, ks, _, _| {
            *count.borrow_mut() += 1;
            ks.key == "down"
        });
        e.show_popover("completion", popover, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down");
    assert_eq!(*popup_keys.borrow(), 1);
    assert_eq!(seen.borrow().len(), 2, "popover consumed down first");
    editor.update(cx, |e, cx| {
        assert!(e.hide_popover("completion", cx));
        assert!(e.remove_key_interceptor(id));
    });
    cx.simulate_keystrokes("down");
    assert_eq!(cursor(&editor, cx), Point::new(1, 1));
}

struct Popup;

impl gpui::Render for Popup {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().w(px(80.)).h(px(40.)).child("items")
    }
}
