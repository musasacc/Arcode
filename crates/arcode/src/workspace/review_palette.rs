//! Review tests for the command palette in the workspace (failing tests
//! that reproduce bugs found in review).

use std::fs;

use gpui::{Focusable, TestAppContext};

use super::tests::{open, project_dir};
use crate::actions::{ToggleCommandPalette, ToggleFinder};

/// Ctrl+P while the palette is open opens the file finder *under* the
/// palette: both overlays are open, the palette (drawn last, with an
/// occluding backdrop) hides the finder, and focus is in the hidden finder,
/// so typed text goes to an invisible input. After Escape closes the
/// finder the palette stays open without focus and keystrokes reach the
/// editor behind it.
#[gpui::test]
fn ctrl_p_while_the_palette_is_open_does_not_stack_a_hidden_finder(cx: &mut TestAppContext) {
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
    cx.dispatch_action(ToggleCommandPalette);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.palette.is_some()));

    // Ctrl+P with focus in the palette input.
    cx.dispatch_action(ToggleFinder);
    cx.run_until_parked();
    let (palette_open, finder_open) =
        ws.read_with(cx, |ws, _| (ws.palette.is_some(), ws.finder.is_some()));
    assert!(
        !(palette_open && finder_open),
        "palette and finder are both open; the finder is hidden under the palette"
    );
    if let Some(palette) = ws.read_with(cx, |ws, _| ws.palette.clone()) {
        let focused = cx.update(|window, cx| palette.focus_handle(cx).is_focused(window));
        assert!(focused, "an open palette must keep the focus");
    }
}

/// Every palette or finder open used to push a subscription onto the
/// workspace for good; opening and closing them many times must not grow
/// the workspace's subscriptions.
#[gpui::test]
fn opening_overlays_repeatedly_does_not_pile_up_subscriptions(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    let toggle = |cx: &mut gpui::VisualTestContext| {
        cx.dispatch_action(ToggleCommandPalette);
        cx.run_until_parked();
        cx.dispatch_action(ToggleCommandPalette);
        cx.dispatch_action(ToggleFinder);
        cx.run_until_parked();
        cx.dispatch_action(ToggleFinder);
        cx.run_until_parked();
    };
    toggle(cx);
    let before = ws.read_with(cx, |ws, _| ws.subs.len());
    for _ in 0..20 {
        toggle(cx);
    }
    assert_eq!(ws.read_with(cx, |ws, _| ws.subs.len()), before);
}
