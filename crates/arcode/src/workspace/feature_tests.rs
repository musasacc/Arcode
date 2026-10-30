//! GPUI tests of the command palette, the find bar in panes and the
//! search / TODO actions inside a real workspace.

use std::fs;

use gpui::TestAppContext;

use super::tests::{open, project_dir};
use crate::actions::{Find, ShowProjectSearch, ShowTodos, ToggleCommandPalette};
use crate::activity_bar::Activity;

#[gpui::test]
fn palette_lists_commands_for_the_focus_and_runs_them_there(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();

    // Nothing open: editor commands are not offered.
    cx.dispatch_action(ToggleCommandPalette);
    cx.run_until_parked();
    let labels = ws.read_with(cx, |ws, cx| {
        ws.palette()
            .expect("palette open")
            .read(cx)
            .visible_labels()
    });
    assert!(labels.iter().any(|l| l.as_ref() == "View: Toggle Sidebar"));
    assert!(!labels.iter().any(|l| l.as_ref() == "Editor: Undo"));
    cx.dispatch_action(ToggleCommandPalette);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.palette().is_none()));

    // With an editor focused its commands appear, and a command runs there.
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/main.rs"), window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_palette("toggle line comment", window, cx)
    });
    cx.run_until_parked();
    let palette = ws.read_with(cx, |ws, _| ws.palette().cloned().expect("palette"));
    let labels = palette.read_with(cx, |p, _| p.visible_labels());
    assert_eq!(labels[0].as_ref(), "Editor: Toggle Line Comment");
    palette.update(cx, |p, cx| p.confirm_index(0, cx));
    cx.run_until_parked();
    let text = ws.read_with(cx, |ws, cx| {
        ws.active_editor(cx)
            .expect("editor")
            .read(cx)
            .buffer()
            .text()
            .to_string()
    });
    assert!(text.starts_with("// fn main"), "{text}");
    let recent = cx.update(|_, cx| crate::app_state::state(cx).recent_commands.clone());
    assert_eq!(recent, ["Editor: Toggle Line Comment"]);
}

#[gpui::test]
fn find_opens_in_the_pane_and_search_actions_switch_views(cx: &mut TestAppContext) {
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
    cx.dispatch_action(Find);
    cx.run_until_parked();
    let pane = ws.read_with(cx, |ws, _| ws.active_pane().clone());
    assert!(pane.read_with(cx, |p, _| p.find_open()));

    cx.dispatch_action(ShowProjectSearch);
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.activity), Activity::Search);
    cx.dispatch_action(ShowTodos);
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.activity), Activity::Todo);
}
