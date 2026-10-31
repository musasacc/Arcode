//! Shows every editor extension point in a real window: gutter lanes
//! (breakpoints, run buttons, change bars), line and text highlights,
//! inlay hints, end-of-line virtual text, code lenses above and at the end
//! of lines, a keyboard-driven popover (Ctrl+Space) and a block cursor
//! toggle (Ctrl+K). Clicks are logged to stderr.
//!
//! ```text
//! cargo run -p arcode-editor --example extension_points
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Appearance, Config, ThemeRegistry};
use arcode_editor::{
    BackgroundHighlight, ChangeKind, CodeLens, ColorKey, CursorShape, DotStyle, Editor,
    EditorEvent, EditorPopover, GutterDecoration, GutterLane, GutterMark, LensItem, LensPlacement,
    VirtualText,
};
use arcode_ui_kit::{Assets, IconName, theme};
use gpui::{
    App, Application, Bounds, Context, Entity, Focusable, IntoElement, ParentElement, Render,
    Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};

const TEXT: &str = r#"use std::fmt;

fn add(a: i32, b: i32) -> i32 {
    let sum = a + b;
    sum
}

#[test]
fn adds_numbers() {
    assert_eq!(add(2, 2), 4);
}

<<<<<<< HEAD
let greeting = "hello";
=======
let greeting = "hi";
>>>>>>> feature

fn main() {
    let total = add(1, 2);
    println!("{total}");
}
"#;

/// A tiny completion-style list driven by the editor's key routing.
struct List {
    items: Vec<&'static str>,
    selected: usize,
}

impl Render for List {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        div()
            .id("demo-list")
            .occlude()
            .flex()
            .flex_col()
            .w(px(180.))
            .py_1()
            .bg(c.surface_raised)
            .border_1()
            .border_color(c.border)
            .rounded_md()
            .shadow_md()
            .text_size(px(13.))
            .children(self.items.iter().enumerate().map(|(i, item)| {
                let row = div().px_2().py(px(2.)).text_color(c.text).child(*item);
                if i == self.selected {
                    row.bg(c.selection)
                } else {
                    row
                }
            }))
    }
}

fn decorate(editor: &mut Editor, cx: &mut Context<Editor>) {
    let dot = |line, style| GutterDecoration {
        line,
        lane: GutterLane::Breakpoints,
        mark: GutterMark::Dot {
            color: ColorKey::new("error"),
            style,
        },
        tag: line as u64,
    };
    editor.set_gutter_decorations(
        "debugger",
        vec![
            dot(3, DotStyle::Filled),
            dot(19, DotStyle::Conditional),
            dot(20, DotStyle::Ring),
        ],
        cx,
    );
    let play = |line| GutterDecoration {
        line,
        lane: GutterLane::Runnables,
        mark: GutterMark::Icon {
            icon: IconName::Play,
            color: ColorKey::new("success"),
        },
        tag: line as u64,
    };
    editor.set_gutter_decorations("runnables", vec![play(7), play(18)], cx);
    let change = |line, kind| GutterDecoration {
        line,
        lane: GutterLane::Changes,
        mark: GutterMark::Change(kind),
        tag: 0,
    };
    editor.set_gutter_decorations(
        "git",
        vec![
            change(2, ChangeKind::Modified),
            change(3, ChangeKind::Added),
            change(4, ChangeKind::Added),
            change(9, ChangeKind::Deleted),
        ],
        cx,
    );
    editor.set_background_highlights(
        "conflict",
        vec![
            BackgroundHighlight::lines(12..14, ColorKey::new("diff_added").opacity(0.18)),
            BackgroundHighlight::lines(15..17, ColorKey::new("info").opacity(0.18)),
        ],
        cx,
    );
    editor.set_background_highlights(
        "debugger-line",
        vec![BackgroundHighlight::lines(
            19..20,
            ColorKey::new("warning").opacity(0.25),
        )],
        cx,
    );
    editor.set_background_highlights(
        "search",
        vec![
            BackgroundHighlight::text(
                Point::new(2, 3)..Point::new(2, 6),
                ColorKey::new("accent").opacity(0.3),
            ),
            BackgroundHighlight::text(
                Point::new(9, 15)..Point::new(9, 18),
                ColorKey::new("accent").opacity(0.3),
            ),
        ],
        cx,
    );
    editor.set_virtual_text(
        "hints",
        vec![
            VirtualText::inline(Point::new(3, 11), ": i32"),
            VirtualText::inline(Point::new(19, 13), ": i32"),
            VirtualText::inline(Point::new(19, 20), "a: "),
            VirtualText {
                italic: true,
                ..VirtualText::end_of_line(2, "You, 2 days ago · Add add()")
            },
        ],
        cx,
    );
    let item = |id, label: &str| LensItem {
        id,
        label: label.to_string().into(),
    };
    editor.set_code_lenses(
        "lenses",
        vec![
            CodeLens {
                line: 7,
                placement: LensPlacement::Above,
                items: vec![item(1, "Run"), item(2, "Debug")],
            },
            CodeLens {
                line: 12,
                placement: LensPlacement::Above,
                items: vec![
                    item(10, "Accept Current"),
                    item(11, "Accept Incoming"),
                    item(12, "Accept Both"),
                ],
            },
            CodeLens {
                line: 15,
                placement: LensPlacement::EndOfLine,
                items: vec![item(20, "(incoming change)")],
            },
        ],
        cx,
    );
}

fn install_keys(editor: &Entity<Editor>, cx: &mut App) {
    let list = cx.new(|_| List {
        items: vec!["add", "adds_numbers", "assert_eq!"],
        selected: 0,
    });
    let shown = Rc::new(RefCell::new(false));
    editor.update(cx, |e, cx| {
        let shown = shown.clone();
        e.add_key_interceptor(
            0,
            Rc::new(move |editor, ks, _window, cx| {
                let ctrl = ks.modifiers.control;
                if ctrl && ks.key == "space" {
                    let list = list.clone();
                    *shown.borrow_mut() = true;
                    editor.update(cx, |e, cx| {
                        let at = e.cursor_position();
                        let popover = EditorPopover::new(at, list.clone().into()).on_key(
                            move |editor, ks, _, cx| match ks.key.as_str() {
                                "up" | "down" => {
                                    let down = ks.key == "down";
                                    list.update(cx, |l, cx| {
                                        let n = l.items.len();
                                        l.selected = if down {
                                            (l.selected + 1) % n
                                        } else {
                                            (l.selected + n - 1) % n
                                        };
                                        cx.notify();
                                    });
                                    true
                                }
                                "escape" | "enter" => {
                                    let pick = list.read(cx).items[list.read(cx).selected];
                                    editor.update(cx, |e, cx| {
                                        if ks.key == "enter" {
                                            let at = e.cursor_position();
                                            e.apply_edits(vec![(at..at, pick.to_string())], cx);
                                        }
                                        e.hide_popover("completion", cx);
                                    });
                                    true
                                }
                                _ => false,
                            },
                        );
                        e.show_popover("completion", popover, cx);
                    });
                    return true;
                }
                if ctrl && ks.key == "k" {
                    editor.update(cx, |e, cx| {
                        let next = if e.cursor_shape() == CursorShape::Bar {
                            CursorShape::Block
                        } else {
                            CursorShape::Bar
                        };
                        e.set_cursor_shape(next, cx);
                    });
                    return true;
                }
                false
            }),
            cx,
        );
    });
}

fn main() {
    env_logger::init();
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        arcode_ui_kit::load_fonts(cx);
        arcode_ui_kit::set_settings(Config::default(), cx);
        let registry = ThemeRegistry::builtin();
        let appearance = if std::env::var("DEMO_DARK").is_ok() {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        arcode_ui_kit::set_theme(Arc::new(registry.fallback(appearance).clone()), cx);
        arcode_editor::init(cx);
        let bounds = Bounds::centered(None, size(px(1000.), px(640.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let editor = cx.new(|cx| {
                let mut buffer = Buffer::new(TEXT);
                buffer.set_path("demo.rs");
                let mut editor = Editor::new(buffer, cx);
                decorate(&mut editor, cx);
                editor.set_gutter_lane_visible(GutterLane::Breakpoints, true, cx);
                editor
            });
            cx.subscribe(&editor, |_, event, _| match event {
                EditorEvent::GutterClicked { .. } | EditorEvent::CodeLensClicked { .. } => {
                    eprintln!("event: {event:?}");
                }
                _ => {}
            })
            .detach();
            install_keys(&editor, cx);
            window.focus(&editor.focus_handle(cx));
            editor
        })
        .expect("open window");
        cx.activate(true);
    });
}
