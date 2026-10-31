//! GPUI tests of the debugger with real editors (no debug adapter needed).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use arcode_buffer::{Buffer, Point};
use arcode_editor::{Editor, EditorEvent, GutterLane};
use gpui::{AppContext, Entity, Modifiers, TestAppContext};

use super::editors::{BREAKPOINT_SET, RUN_SET};
use super::launch::{LaunchChoice, LaunchKind};
use super::{Debugger, DebuggerEvent};
use crate::test_support::{init, settle};

const MAIN: &str = "fn add(a: i32) -> i32 {\n    a + 1\n}\n\nfn main() {\n    let x = add(1);\n    println!(\"{x}\");\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {\n        assert_eq!(super::add(1), 2);\n    }\n}\n";

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/main.rs"), MAIN).expect("write");
    (dir, root)
}

type Events = Rc<RefCell<Vec<DebuggerEvent>>>;

fn setup(cx: &mut TestAppContext, root: &Path) -> (Entity<Debugger>, Entity<Editor>, Events) {
    init(cx);
    let debugger = cx.new(Debugger::new);
    let events: Events = Rc::default();
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&debugger, move |_, e: &DebuggerEvent, _| {
            sink.borrow_mut().push(e.clone())
        })
        .detach();
    });
    debugger.update(cx, |d, cx| d.set_project(Some(root.to_path_buf()), &[], cx));
    let path = root.join("src/main.rs");
    let editor = cx.new(|cx| Editor::new(Buffer::from_file(&path).expect("open"), cx));
    debugger.update(cx, |d, cx| d.register_editor(&editor, cx));
    settle(cx);
    (debugger, editor, events)
}

fn click(
    cx: &mut TestAppContext,
    editor: &Entity<Editor>,
    line: usize,
    lane: GutterLane,
    tag: Option<u64>,
) {
    editor.update(cx, |_, cx| {
        cx.emit(EditorEvent::GutterClicked {
            line,
            lane,
            tag,
            modifiers: Modifiers::default(),
        })
    });
    cx.run_until_parked();
}

fn dots(cx: &mut TestAppContext, editor: &Entity<Editor>) -> Vec<usize> {
    editor.read_with(cx, |e, _| {
        e.gutter_decorations(BREAKPOINT_SET)
            .iter()
            .map(|d| d.line)
            .collect()
    })
}

#[gpui::test]
fn gutter_clicks_toggle_breakpoints_that_follow_edits(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, editor, events) = setup(cx, &root);
    click(cx, &editor, 1, GutterLane::Breakpoints, None);
    click(cx, &editor, 5, GutterLane::Breakpoints, None);
    assert_eq!(dots(cx, &editor), [1, 5]);
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| matches!(e, DebuggerEvent::BreakpointsChanged))
    );

    // Two lines typed above: both breakpoints move down with their code.
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(0, 0)..Point::new(0, 0), "// a\n// b\n".into())],
            cx,
        )
    });
    cx.run_until_parked();
    let lines: Vec<usize> =
        debugger.read_with(cx, |d, _| d.book().all().iter().map(|b| b.line).collect());
    assert_eq!(lines, [3, 7]);

    // Deleting a breakpoint's line removes it.
    editor.update(cx, |e, cx| {
        e.apply_edits(
            vec![(Point::new(3, 0)..Point::new(4, 0), String::new())],
            cx,
        )
    });
    cx.run_until_parked();
    let lines: Vec<usize> =
        debugger.read_with(cx, |d, _| d.book().all().iter().map(|b| b.line).collect());
    assert_eq!(lines, [6]);

    // Clicking the dot again removes it.
    click(cx, &editor, 6, GutterLane::Breakpoints, None);
    assert!(dots(cx, &editor).is_empty());
    assert!(debugger.read_with(cx, |d, _| d.book().is_empty()));
}

#[gpui::test]
fn right_click_asks_to_edit_and_options_change_the_mark(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, editor, events) = setup(cx, &root);
    editor.update(cx, |_, cx| {
        cx.emit(EditorEvent::GutterContextClicked {
            line: 5,
            lane: GutterLane::Breakpoints,
            tag: None,
        })
    });
    cx.run_until_parked();
    let path = root.join("src/main.rs");
    assert!(events.borrow().iter().any(|e| matches!(
        e,
        DebuggerEvent::EditBreakpoint { path: p, line: 5 } if *p == path
    )));
    debugger.update(cx, |d, cx| {
        d.set_breakpoint_options(&path, 5, Some("x > 1".into()), None, None, cx)
    });
    cx.run_until_parked();
    let style = editor.read_with(cx, |e, _| {
        e.gutter_decorations(BREAKPOINT_SET)[0].mark.clone()
    });
    assert!(
        matches!(
            style,
            arcode_editor::GutterMark::Dot {
                style: arcode_editor::DotStyle::Conditional,
                ..
            }
        ),
        "{style:?}"
    );
    // Unchecked breakpoints are drawn as rings and not sent.
    let id = debugger.read_with(cx, |d, _| d.book().all()[0].id);
    debugger.update(cx, |d, cx| d.set_breakpoint_enabled(id, false, cx));
    cx.run_until_parked();
    let style = editor.read_with(cx, |e, _| {
        e.gutter_decorations(BREAKPOINT_SET)[0].mark.clone()
    });
    assert!(matches!(
        style,
        arcode_editor::GutterMark::Dot {
            style: arcode_editor::DotStyle::Ring,
            ..
        }
    ));
    assert!(debugger.read_with(cx, |d, _| d.book().all_source_breakpoints().is_empty()));
}

#[gpui::test]
fn run_buttons_run_their_command_in_a_named_terminal(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, editor, events) = setup(cx, &root);
    let runs: Vec<usize> = editor.read_with(cx, |e, _| {
        e.gutter_decorations(RUN_SET)
            .iter()
            .map(|d| d.line)
            .collect()
    });
    assert_eq!(runs, [4, 10, 12], "main, mod tests and the test");

    // A click on the test's button opens the Run / Debug menu.
    click(cx, &editor, 12, GutterLane::Runnables, Some(2));
    assert!(editor.read_with(cx, |e, _| e.has_popover(super::editors::RUN_MENU)));

    let runnable = debugger.read_with(cx, |d, _| d.runnable(&editor, 2).expect("runnable"));
    let path = root.join("src/main.rs");
    debugger.update(cx, |d, cx| {
        d.run_runnable(&runnable, false, Some(&path), cx)
    });
    let events = events.borrow();
    let Some(DebuggerEvent::RunCommand { title, command }) = events
        .iter()
        .find(|e| matches!(e, DebuggerEvent::RunCommand { .. }))
    else {
        panic!("no run command");
    };
    assert_eq!(title, "test tests::adds");
    let line = command.command_line();
    assert!(line.starts_with("cargo test"), "{line}");
    assert!(line.contains("tests::adds --exact"), "{line}");
}

#[gpui::test]
fn a_missing_adapter_is_reported_with_what_to_install(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, _editor, events) = setup(cx, &root);
    cx.update(|cx| {
        let mut config = arcode_ui_kit::settings(cx).clone();
        config.debug.adapter_paths = arcode_dap::KnownAdapter::ALL
            .iter()
            .map(|a| (a.name().to_string(), "/no/such/adapter".to_string()))
            .collect();
        arcode_ui_kit::set_settings(config, cx);
    });
    let target = arcode_project::DebugTarget {
        language: "python",
        build: None,
        program: Some(root.join("tool.py")),
        module: None,
        args: Vec::new(),
        cwd: root.clone(),
        go_mode: None,
    };
    let choice = LaunchChoice {
        name: "tool".into(),
        kind: LaunchKind::Target(Box::new(target)),
    };
    debugger.update(cx, |d, cx| d.start(choice, None, cx));
    settle(cx);
    let message = events.borrow().iter().find_map(|e| match e {
        DebuggerEvent::Message(m) => Some(m.clone()),
        _ => None,
    });
    let message = message.expect("a message");
    assert!(message.contains("debugpy"), "{message}");
    assert!(message.contains("Nothing is downloaded"), "{message}");
    debugger.read_with(cx, |d, _| {
        assert!(!d.is_active());
        assert!(d.console().lines().any(|l| l.text.contains("debugpy")));
    });
}

#[gpui::test]
fn launch_configurations_are_detected(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (debugger, _editor, _events) = setup(cx, &root);
    let names: Vec<String> = debugger.read_with(cx, |d, _| {
        d.choices().iter().map(|c| c.name.clone()).collect()
    });
    assert!(
        names.iter().any(|n| n.starts_with("cargo run")),
        "{names:?}"
    );

    // Saving launch.toml adds its entries first.
    std::fs::create_dir_all(root.join(".arcode")).expect("mkdir");
    std::fs::write(
        root.join(arcode_dap::LAUNCH_FILE),
        "[[configuration]]\nname = \"Mine\"\nadapter = \"gdb\"\nprogram = \"target/debug/demo\"\n",
    )
    .expect("write");
    debugger.update(cx, |d, cx| d.reload_choices(cx));
    settle(cx);
    let first = debugger.read_with(cx, |d, _| d.choices()[0].name.clone());
    assert_eq!(first, "Mine");
}
