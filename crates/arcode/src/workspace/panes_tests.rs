use gpui::TestAppContext;

use super::super::tests::{open, project_dir};
use crate::actions::SplitRight;

#[gpui::test]
fn split_moves_the_active_tab_and_open_fills_an_empty_pane(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let (main, lib) = (root.join("src/main.rs"), root.join("src/lib.rs"));
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main, window, cx);
        ws.open_file(&lib, window, cx);
    });
    let paths = |ws: &super::Workspace, ix: usize, cx: &gpui::App| -> Vec<std::path::PathBuf> {
        ws.panes()[ix]
            .read(cx)
            .items()
            .iter()
            .filter_map(|e| e.read(cx).path().map(|p| p.to_path_buf()))
            .collect()
    };
    // Two tabs: lib.rs (active) moves into the new pane.
    cx.dispatch_action(SplitRight);
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.panes().len(), 2);
        assert_eq!(ws.active_pane, 1);
        assert_eq!(paths(ws, 0, cx), std::slice::from_ref(&main));
        assert_eq!(paths(ws, 1, cx), std::slice::from_ref(&lib));
    });
    // One tab: the new pane starts empty; opening main.rs (shown in the
    // first pane) brings it here.
    cx.dispatch_action(SplitRight);
    ws.update_in(cx, |ws, window, cx| {
        assert_eq!(ws.panes().len(), 3);
        assert!(paths(ws, 2, cx).is_empty());
        ws.open_file(&main, window, cx);
    });
    ws.update(cx, |ws, cx| {
        assert_eq!(ws.active_pane, 2);
        assert!(paths(ws, 0, cx).is_empty());
        assert_eq!(paths(ws, 2, cx), std::slice::from_ref(&main));
        assert_eq!(ws.editors(cx).len(), 2);
    });
}
