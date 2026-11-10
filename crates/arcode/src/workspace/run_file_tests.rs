//! GPUI tests of Run / Debug Current File in a workspace: the Run button
//! and F5 fall back to the current file, and say why when nothing runs.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{TestAppContext, VisualTestContext};

use super::Workspace;
use super::run_file::FILE_RUN_TITLE;
use super::tests::open;
use crate::actions::RunProject;
use crate::debugger::actions::{
    DebugCurrentFile, EditRunCommand, OpenLaunchConfig, PickRunCommand, RunCurrentFile,
    StartOrContinue,
};
use crate::test_support::settle;

/// A folder of loose files (no project file), canonical.
pub(super) fn loose_folder(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(dir.path()).expect("canonical");
    for (name, text) in files {
        fs::write(root.join(name), text).expect("write");
    }
    (dir, root)
}

pub(super) fn open_folder<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    file: Option<&str>,
) -> (gpui::Entity<Workspace>, &'a mut VisualTestContext) {
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.to_path_buf(), window, cx)
    });
    if let Some(file) = file {
        let path = root.join(file);
        ws.update_in(cx, |ws, window, cx| ws.open_file(&path, window, cx));
    }
    settle(cx);
    (ws, cx)
}

pub(super) fn toast(
    ws: &gpui::Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> (String, Option<String>) {
    ws.read_with(cx, |ws, _| {
        (
            ws.toast.as_ref().map(|t| t.to_string()).unwrap_or_default(),
            ws.toast_action.as_ref().map(|(label, _)| label.to_string()),
        )
    })
}

#[gpui::test]
fn the_run_button_runs_a_loose_file_when_nothing_was_detected(cx: &mut TestAppContext) {
    if arcode_project::packs::find_executable("rustc").is_none() {
        return;
    }
    let (_dir, root) = loose_folder(&[("test.rs", "fn main() {\n    println!(\"hello\");\n}\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("test.rs"));
    assert_eq!(ws.read_with(cx, |ws, _| ws.run_command_line()), None);
    let shown = ws.read_with(cx, |ws, cx| ws.runnable_file_name(cx));
    assert_eq!(shown.as_deref(), Some("test.rs"));
    cx.dispatch_action(RunProject);
    settle(cx);
    let ran = ws.read_with(cx, |ws, _| ws.debug.run.ran.clone());
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].0, FILE_RUN_TITLE);
    assert!(
        ran[0].1.starts_with("rustc --edition 2024 -g test.rs -o "),
        "{ran:?}"
    );
    assert_eq!(toast(&ws, cx).0, "");
}

#[gpui::test]
fn a_detected_command_stays_the_default(cx: &mut TestAppContext) {
    let dir = super::tests::project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write");
    let (ws, cx) = open_folder(cx, &root, Some("src/main.rs"));
    let line = ws.read_with(cx, |ws, _| ws.run_command_line());
    assert!(line.is_some_and(|l| l.starts_with("cargo run")));
    // Run Current File is still there, and runs the file's package.
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    let ran = ws.read_with(cx, |ws, _| ws.debug.run.ran.clone());
    if arcode_project::packs::find_executable("cargo").is_some() {
        assert_eq!(ran.len(), 1, "{ran:?}");
        assert_eq!(ran[0].0, FILE_RUN_TITLE);
        assert_eq!(ran[0].1, "cargo run --package demo --bin demo");
    }
}

#[gpui::test]
fn a_folder_with_nothing_to_run_says_why(cx: &mut TestAppContext) {
    let (_dir, root) = loose_folder(&[("README.md", "# notes\n")]);
    let (ws, cx) = open_folder(cx, &root, None);

    // Nothing open: Run explains and offers to set a run command.
    cx.dispatch_action(RunProject);
    settle(cx);
    let (message, action) = toast(&ws, cx);
    assert!(
        message.starts_with("Nothing to run: open a file"),
        "{message}"
    );
    assert_eq!(action.as_deref(), Some("Set run command"));

    // F5 without a launch configuration explains too.
    cx.dispatch_action(StartOrContinue);
    settle(cx);
    let (message, action) = toast(&ws, cx);
    assert!(
        message.starts_with("Nothing to debug: open a file"),
        "{message}"
    );
    assert_eq!(action.as_deref(), Some("Create launch.toml"));

    // A Markdown file cannot run or be debugged.
    let readme = root.join("README.md");
    ws.update_in(cx, |ws, window, cx| ws.open_file(&readme, window, cx));
    settle(cx);
    assert_eq!(ws.read_with(cx, |ws, cx| ws.runnable_file_name(cx)), None);
    cx.dispatch_action(RunProject);
    settle(cx);
    let (message, _) = toast(&ws, cx);
    assert!(message.starts_with("Cannot run README.md: "), "{message}");
    cx.dispatch_action(DebugCurrentFile);
    settle(cx);
    let (message, _) = toast(&ws, cx);
    assert!(message.starts_with("Cannot debug README.md: "), "{message}");
    assert!(ws.read_with(cx, |ws, _| ws.debug.run.ran.is_empty()));
    assert!(!ws.read_with(cx, |ws, cx| ws.debugger().read(cx).is_active()));

    // The notice's button opens the run command dialog.
    cx.dispatch_action(EditRunCommand);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.debug.run.prompt.is_some()));
}

#[gpui::test]
fn debugging_an_interpreted_file_says_it_is_not_supported(cx: &mut TestAppContext) {
    let (_dir, root) = loose_folder(&[("tool.rb", "puts 1\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("tool.rb"));
    cx.dispatch_action(StartOrContinue);
    settle(cx);
    let (message, action) = toast(&ws, cx);
    assert!(
        message.starts_with("Cannot debug tool.rb: there is no debugger for Ruby files"),
        "{message}"
    );
    assert_eq!(action.as_deref(), Some("Create launch.toml"));
}

#[gpui::test]
fn without_a_folder_there_is_no_button(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.dispatch_action(RunProject);
    settle(cx);
    let (message, action) = toast(&ws, cx);
    assert!(message.starts_with("Nothing to run"), "{message}");
    assert_eq!(action, None);
}

#[gpui::test]
fn the_run_menu_lists_the_current_file_when_nothing_was_detected(cx: &mut TestAppContext) {
    let (_dir, root) = loose_folder(&[("test.py", "print('hi')\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("test.py"));
    cx.dispatch_action(PickRunCommand);
    settle(cx);
    ws.read_with(cx, |ws, cx| {
        assert!(ws.debug.run.menu_loaded);
        assert_eq!(ws.debug.run.menu.as_ref().map(Vec::len), Some(0));
        assert_eq!(ws.runnable_file_name(cx).as_deref(), Some("test.py"));
    });
}

#[gpui::test]
fn a_launch_file_created_from_the_notice_keeps_f5_on_the_current_file(cx: &mut TestAppContext) {
    let (_dir, root) = loose_folder(&[("tool.rb", "puts 1\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("tool.rb"));
    cx.dispatch_action(StartOrContinue);
    settle(cx);
    assert_eq!(toast(&ws, cx).1.as_deref(), Some("Create launch.toml"));

    // The notice's button writes the starter file: examples only.
    cx.dispatch_action(OpenLaunchConfig);
    settle(cx);
    assert!(root.join(arcode_dap::LAUNCH_FILE).is_file());
    let choices = ws.read_with(cx, |ws, cx| ws.debugger().read(cx).choices().len());
    assert_eq!(choices, 0, "the starter file adds no configuration");

    // F5 on the file still debugs the file, not a placeholder program.
    let file = root.join("tool.rb");
    ws.update_in(cx, |ws, window, cx| ws.open_file(&file, window, cx));
    settle(cx);
    ws.update(cx, |ws, _| ws.toast = None);
    cx.dispatch_action(StartOrContinue);
    settle(cx);
    let (message, action) = toast(&ws, cx);
    assert!(message.starts_with("Cannot debug tool.rb: "), "{message}");
    assert_eq!(action.as_deref(), Some("Open launch.toml"));
    assert!(!ws.read_with(cx, |ws, cx| ws.debugger().read(cx).is_active()));
}

#[gpui::test]
fn a_header_says_it_does_not_run(cx: &mut TestAppContext) {
    let (_dir, root) = loose_folder(&[("util.h", "int helper(void);\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("util.h"));
    assert_eq!(ws.read_with(cx, |ws, cx| ws.runnable_file_name(cx)), None);
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    let (message, _) = toast(&ws, cx);
    assert!(
        message.starts_with("Cannot run util.h: a header does not run on its own"),
        "{message}"
    );
    cx.dispatch_action(DebugCurrentFile);
    settle(cx);
    let (message, _) = toast(&ws, cx);
    assert!(
        message.starts_with("Cannot debug util.h: a header"),
        "{message}"
    );
    assert!(ws.read_with(cx, |ws, _| ws.debug.run.ran.is_empty()));
}

#[gpui::test]
fn every_file_runs_in_the_same_terminal_tab(cx: &mut TestAppContext) {
    let tools = ["node", "bash"];
    if tools
        .iter()
        .any(|t| arcode_project::packs::find_executable(t).is_none())
    {
        return;
    }
    let (_dir, root) = loose_folder(&[("a.js", "console.log(1)\n"), ("b.sh", "echo 2\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("a.js"));
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    let b = root.join("b.sh");
    ws.update_in(cx, |ws, window, cx| ws.open_file(&b, window, cx));
    settle(cx);
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    let ran = ws.read_with(cx, |ws, _| ws.debug.run.ran.clone());
    let titles: Vec<&str> = ran.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(titles, [FILE_RUN_TITLE, FILE_RUN_TITLE], "{ran:?}");
}
