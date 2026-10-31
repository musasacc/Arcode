//! GPUI tests of the debugger inside a workspace: F9, the breakpoint
//! dialog, breakpoints kept per project, the Run menu and the Debug views.

use std::fs;

use gpui::TestAppContext;

use super::tests::{open, project_dir};
use crate::activity_bar::Activity;
use crate::debugger::actions::{EditBreakpoint, PickRunCommand, ShowRunAndDebug, ToggleBreakpoint};
use crate::debugger::bp_dialog::Save;

#[gpui::test]
fn f9_toggles_and_breakpoints_are_kept_per_project(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("src/main.rs"), "fn main() {\n    let x = 1;\n}\n").expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/main.rs"), window, cx)
    });
    cx.run_until_parked();
    let editor = ws.update(cx, |ws, cx| ws.active_editor(cx).expect("editor"));
    let at = arcode_buffer::Point::new(1, 4);
    editor.update(cx, |e, cx| e.select_ranges(vec![at..at], cx));
    cx.dispatch_action(ToggleBreakpoint);
    cx.run_until_parked();
    let session = ws
        .update(cx, |ws, cx| ws.capture_session(None, cx))
        .expect("session");
    assert_eq!(session.breakpoints.len(), 1);
    assert_eq!(session.breakpoints[0].line, 1);

    // Closing and reopening the project brings it back.
    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, cx| ws.debugger().read(cx).book().is_empty()));
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    let lines: Vec<usize> = ws.read_with(cx, |ws, cx| {
        ws.debugger()
            .read(cx)
            .book()
            .all()
            .iter()
            .map(|b| b.line)
            .collect()
    });
    assert_eq!(lines, [1]);
}

#[gpui::test]
fn the_breakpoint_dialog_sets_a_condition(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/main.rs"), window, cx)
    });
    cx.run_until_parked();
    cx.dispatch_action(EditBreakpoint);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.debug.bp_dialog.is_some()));
    cx.simulate_input("i > 3");
    cx.dispatch_action(Save);
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert!(ws.debug.bp_dialog.is_none());
        let book = ws.debugger().read(cx).book();
        let bp = book.at(&root.join("src/main.rs"), 0).expect("breakpoint");
        assert_eq!(bp.condition.as_deref(), Some("i > 3"));
    });
}

#[gpui::test]
fn run_menu_lists_commands_and_debug_view_opens(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    crate::test_support::settle(cx);
    let line = ws.read_with(cx, |ws, _| ws.run_command_line());
    assert!(line.is_some_and(|l| l.starts_with("cargo run")));
    cx.dispatch_action(PickRunCommand);
    crate::test_support::settle(cx);
    let labels: Vec<String> = ws.read_with(cx, |ws, _| {
        ws.debug
            .run
            .menu
            .as_ref()
            .expect("menu open")
            .iter()
            .map(|c| c.command_line())
            .collect()
    });
    assert!(
        labels.iter().any(|l| l.starts_with("cargo test")),
        "{labels:?}"
    );
    cx.dispatch_action(PickRunCommand);
    assert!(ws.read_with(cx, |ws, _| ws.debug.run.menu.is_none()));

    cx.dispatch_action(ShowRunAndDebug);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.activity == Activity::Debug
        && ws.sidebar_visible));
}
