//! Review tests of the debugger wiring in the workspace: each one
//! reproduces a bug found in review (they fail until it is fixed).

use std::fs;

use gpui::TestAppContext;

use super::tests::{open, project_dir};
use crate::activity_bar::Activity;
use crate::debugger::actions::OpenLaunchConfig;
use crate::test_support::settle;

fn cargo_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write");
    (dir, root)
}

/// `detect_run_command` and `toggle_run_menu` keep their background task in
/// the same `RunState::task` slot. Opening the Run dropdown while the
/// default command is still being detected (right after the project opens)
/// drops the detection task, so the Run button never learns its command.
#[gpui::test]
fn opening_the_run_menu_early_keeps_the_default_run_command(cx: &mut TestAppContext) {
    let (_dir, root) = cargo_project();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.toggle_run_menu(window, cx);
    });
    settle(cx);
    let line = ws.read_with(cx, |ws, _| ws.run_command_line());
    assert!(
        line.as_deref().is_some_and(|l| l.starts_with("cargo run")),
        "the default run command was lost: {line:?}"
    );
}

/// "Debug: Open launch.toml" writes the template's "Debug program" entry,
/// but the configurations are only re-read when the file is saved from an
/// editor, so the Run and Debug view does not list the entry that was just
/// created (nor anything written to launch.toml outside Arcode).
#[gpui::test]
fn a_created_launch_file_is_listed_without_saving_it_again(cx: &mut TestAppContext) {
    let (_dir, root) = cargo_project();
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    settle(cx);
    cx.dispatch_action(OpenLaunchConfig);
    settle(cx);
    assert!(root.join(arcode_dap::LAUNCH_FILE).is_file());
    let names: Vec<String> = ws.read_with(cx, |ws, cx| {
        ws.debugger()
            .read(cx)
            .choices()
            .iter()
            .map(|c| c.name.clone())
            .collect()
    });
    assert!(
        names.iter().any(|n| n == "Debug program"),
        "the new launch.toml entry is not listed: {names:?}"
    );
}

/// Moving a line that has a breakpoint (Alt+Down / Alt+Up) is one
/// multi-line replacement; the editor drops gutter marks inside replaced
/// lines, and `follow_edits` then treats the breakpoint as "deleted with its
/// line" and removes it from the book (and from state.toml).
#[gpui::test]
fn moving_a_line_keeps_its_breakpoint(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    let a = 1;\n    let b = 2;\n    println!(\"{a}{b}\");\n}\n",
    )
    .expect("write");
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
    cx.dispatch_action(crate::debugger::actions::ToggleBreakpoint);
    cx.run_until_parked();
    let lines = |cx: &mut gpui::VisualTestContext| -> Vec<usize> {
        ws.read_with(cx, |ws, cx| {
            ws.debugger()
                .read(cx)
                .book()
                .all()
                .iter()
                .map(|b| b.line)
                .collect()
        })
    };
    assert_eq!(lines(cx), [1]);

    // "let a = 1;" moves below "let b = 2;": its breakpoint should follow.
    cx.dispatch_action(arcode_editor::MoveLineDown);
    cx.run_until_parked();
    let text = editor.read_with(cx, |e, _| e.buffer().text().to_string());
    assert!(text.contains("let b = 2;\n    let a = 1;"), "{text}");
    assert_eq!(lines(cx), [2], "the breakpoint was deleted by a line move");
}

/// The Run and Debug view is bound to secondary-shift-d in the Workspace
/// context, but the editor binds the same keys to DuplicateLine in its own
/// (deeper) context. With an editor focused, which is most of the time, the
/// documented shortcut duplicates the current line instead.
#[gpui::test]
fn secondary_shift_d_opens_run_and_debug_from_an_editor(cx: &mut TestAppContext) {
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
    let editor = ws.update(cx, |ws, cx| ws.active_editor(cx).expect("editor"));
    let before = editor.read_with(cx, |e, _| e.buffer().text().to_string());

    cx.simulate_keystrokes("secondary-shift-d");
    cx.run_until_parked();

    let after = editor.read_with(cx, |e, _| e.buffer().text().to_string());
    assert_eq!(after, before, "the shortcut duplicated the line instead");
    assert!(
        ws.read_with(cx, |ws, _| ws.activity == Activity::Debug
            && ws.sidebar_visible),
        "the Run and Debug view did not open"
    );
}
