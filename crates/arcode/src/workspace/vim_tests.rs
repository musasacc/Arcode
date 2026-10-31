//! Vim mode in the workspace: the status bar entry and the ex commands
//! that need the workspace (`:e`, `:q`, `:q!`).

use std::fs;
use std::path::Path;

use arcode_config::Config;
use gpui::TestAppContext;

use super::tests::{open, project_dir};

#[gpui::test]
fn ex_commands_open_and_close_tabs(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    cx.update(|_, cx| {
        let mut config = Config::default();
        config.editor.vim_mode = true;
        arcode_ui_kit::set_settings(config, cx);
        crate::vim_glue::init(cx);
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&root.join("src/main.rs"), window, cx);
    });
    cx.run_until_parked();
    let title = |ws: &gpui::Entity<super::Workspace>, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.active_editor(cx).and_then(|e| {
                e.read(cx)
                    .path()
                    .and_then(Path::file_name)
                    .map(|n| n.to_owned())
            })
        })
    };
    let mode = ws.read_with(cx, |ws, cx| ws.status_bar().read(cx).vim_status().cloned());
    assert_eq!(mode.map(|s| s.mode), Some("NORMAL"));

    cx.simulate_keystrokes(": e space s r c / l i b . r s enter");
    assert_eq!(title(&ws, cx).as_deref(), Some("lib.rs".as_ref()));
    cx.simulate_keystrokes(": q enter");
    assert_eq!(title(&ws, cx).as_deref(), Some("main.rs".as_ref()));

    cx.simulate_keystrokes("x : q enter");
    assert_eq!(
        title(&ws, cx).as_deref(),
        Some("main.rs".as_ref()),
        "dirty: kept"
    );
    let status = ws.read_with(cx, |ws, cx| ws.status_bar().read(cx).vim_status().cloned());
    assert!(
        status
            .and_then(|s| s.message)
            .is_some_and(|m| m.starts_with("E37"))
    );
    cx.simulate_keystrokes(": q ! enter");
    assert_eq!(title(&ws, cx), None, "`:q!` closes without asking");
    assert_eq!(
        fs::read_to_string(root.join("src/main.rs")).expect("read"),
        "fn main() {}\n",
        "nothing was written"
    );
}
