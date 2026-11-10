//! Run Current File puts a compiled program next to its source (or in the
//! cache when `run.output` says so, or when the name is taken), and a
//! gutter Run on a loose `main` places it again right before it runs.
//! Unix only: the commands are checked as POSIX shell lines (the Windows
//! names and commands are tested in `arcode-project`).

#![cfg(unix)]

use std::fs;

use arcode_config::Config;
use gpui::TestAppContext;

use super::run_file::FILE_RUN_TITLE;
use super::run_file_tests::{loose_folder, open_folder, toast};
use crate::debugger::actions::RunCurrentFile;
use crate::test_support::settle;

fn has(program: &str) -> bool {
    arcode_project::packs::find_executable(program).is_some()
}

fn ran(ws: &gpui::Entity<super::Workspace>, cx: &mut gpui::VisualTestContext) -> Vec<String> {
    ws.read_with(cx, |ws, _| {
        ws.debug
            .run
            .ran
            .iter()
            .map(|(_, line)| line.clone())
            .collect()
    })
}

#[gpui::test]
fn the_program_is_built_next_to_the_file(cx: &mut TestAppContext) {
    if !has("rustc") {
        return;
    }
    let (_dir, root) = loose_folder(&[("test.rs", "fn main() {}\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("test.rs"));
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    assert_eq!(
        ran(&ws, cx),
        ["rustc --edition 2024 -g test.rs -o test && ./test"]
    );
    assert_eq!(toast(&ws, cx).0, "");
}

#[gpui::test]
fn a_taken_name_sends_the_program_to_the_cache_and_says_so(cx: &mut TestAppContext) {
    if !has("rustc") {
        return;
    }
    let (_dir, root) = loose_folder(&[("test.rs", "fn main() {}\n")]);
    fs::create_dir(root.join("test")).unwrap();
    let (ws, cx) = open_folder(cx, &root, Some("test.rs"));
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    let lines = ran(&ws, cx);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(!lines[0].ends_with("&& ./test"), "{lines:?}");
    let (note, _) = toast(&ws, cx);
    assert_eq!(note, "Built test.rs in Arcode's cache: test is a folder");
    assert!(root.join("test").is_dir(), "the folder is left alone");
}

#[gpui::test]
fn the_cache_setting_keeps_programs_out_of_the_project(cx: &mut TestAppContext) {
    if !has("cc") {
        return;
    }
    let (_dir, root) = loose_folder(&[("hello.c", "int main(void) { return 0; }\n")]);
    let (ws, cx) = open_folder(cx, &root, Some("hello.c"));
    cx.update(|_, cx| {
        let (config, warnings) = Config::from_toml_str("[run]\noutput = \"cache\"\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        arcode_ui_kit::set_settings(config, cx);
    });
    cx.dispatch_action(RunCurrentFile);
    settle(cx);
    let lines = ran(&ws, cx);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let cache = crate::debugger::runnables::file_build_dir();
    assert!(lines[0].contains(&*cache.to_string_lossy()), "{lines:?}");
    assert_eq!(toast(&ws, cx).0, "");
}

#[gpui::test]
fn a_gutter_run_places_the_program_again(cx: &mut TestAppContext) {
    if !has("rustc") {
        return;
    }
    let (_dir, root) = loose_folder(&[("test.rs", "fn main() {}\n")]);
    let (ws, cx) = open_folder(cx, &root, None);
    // A command made before a folder took the program's name.
    let stale = crate::debugger::runnables::single_file_run(
        &root.join("test.rs"),
        "rust",
        Some(&root),
        arcode_config::RunOutput::NextToSource,
    )
    .unwrap()
    .command;
    fs::create_dir(root.join("test")).unwrap();
    ws.update_in(cx, |ws, window, cx| {
        ws.run_command_in_terminal("run test.rs", &stale, window, cx)
    });
    settle(cx);
    let lines = ran(&ws, cx);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(!lines[0].ends_with("./test"), "{lines:?}");
    assert!(toast(&ws, cx).0.contains("test is a folder"));
    let titles = ws.read_with(cx, |ws, _| ws.debug.run.ran[0].0.clone());
    assert_eq!(titles, FILE_RUN_TITLE);
}
