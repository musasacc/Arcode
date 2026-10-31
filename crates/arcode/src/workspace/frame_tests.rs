//! GPUI tests of the floating panels: the focus frame follows focus (and
//! stays put behind overlays), and the rail's panel toggles work.

use std::fs;

use gpui::{Modifiers, TestAppContext, VisualTestContext};

use super::frame::FramedPanel;
use super::tests::{open, project_dir};
use crate::actions::ToggleCommandPalette;

fn framed(ws: &gpui::Entity<super::Workspace>, cx: &mut VisualTestContext) -> FramedPanel {
    cx.update(|window, cx| window.draw(cx).clear());
    ws.read_with(cx, |ws, _| ws.shell.framed)
}

#[gpui::test]
fn the_frame_follows_focus_but_not_overlays(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&root.join("src/main.rs"), window, cx);
    });
    cx.run_until_parked();
    assert_eq!(framed(&ws, cx), FramedPanel::Editor);

    // Focus the bottom panel: the frame moves there.
    ws.update_in(cx, |ws, window, cx| {
        ws.bottom_panel_visible = true;
        cx.notify();
        let panel = ws.bottom_panel.clone();
        panel.update(cx, |p, cx| p.focus(window, cx));
    });
    cx.run_until_parked();
    // The first frame lays the panel out, the next one sees focus in it.
    framed(&ws, cx);
    assert_eq!(framed(&ws, cx), FramedPanel::Bottom);

    // The palette takes focus but not the frame.
    cx.dispatch_action(ToggleCommandPalette);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.palette().is_some()));
    assert_eq!(framed(&ws, cx), FramedPanel::Bottom);
    cx.dispatch_action(ToggleCommandPalette);
    cx.run_until_parked();

    // Hiding the framed panel hands the frame to the editor.
    ws.update(cx, |ws, cx| {
        ws.bottom_panel_visible = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(framed(&ws, cx), FramedPanel::Editor);
}

#[gpui::test]
fn rail_toggles_show_and_hide_the_panels(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    let before = ws.read_with(cx, |ws, _| (ws.sidebar_visible, ws.bottom_panel_visible));
    for selector in ["rail-toggle-sidebar", "rail-toggle-panel"] {
        cx.update(|window, cx| window.draw(cx).clear());
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    }
    let after = ws.read_with(cx, |ws, _| (ws.sidebar_visible, ws.bottom_panel_visible));
    assert_eq!(after, (!before.0, !before.1));
    // The Run button is in the title bar while a project is open.
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("run").is_some());
    assert!(cx.debug_bounds("continue").is_none(), "not paused");
}
