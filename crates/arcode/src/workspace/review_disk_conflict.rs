//! Review tests for the changed-on-disk flow (`disk_conflict.rs`,
//! `pane/disk_bar.rs`). Each reproduces a bug found in review and fails
//! until it is fixed.

use std::path::PathBuf;

use arcode_buffer::Point;
use arcode_editor::{DiskConflict, Editor};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::Workspace;
use super::tests::{open, project_dir};

/// `src/lib.rs` open with unsaved edits, then changed on disk.
fn conflicted(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workspace>,
    Entity<Editor>,
    &mut VisualTestContext,
) {
    let dir = project_dir();
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let lib = root.join("src/lib.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&lib, window, cx);
    });
    let editor = ws.read_with(cx, |ws, cx| ws.editor_for_path(&lib, cx).expect("open"));
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
    });
    std::fs::write(&lib, "theirs\n").expect("write");
    let paths = [lib.clone()];
    ws.update(cx, |ws, cx| ws.files_changed_on_disk(&paths, false, cx));
    cx.run_until_parked();
    editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), Some(DiskConflict::Changed))
    });
    (dir, lib, ws, editor, cx)
}

/// Closing the tab and answering "Save" hits the conflict; the user then
/// answers "Overwrite" in the second prompt. The file is written, but the
/// tab they asked to close stays open (the close flow returned on the
/// refused save and nothing resumes it).
#[gpui::test]
fn close_save_overwrite_closes_the_tab(cx: &mut TestAppContext) {
    let (_dir, lib, ws, editor, cx) = conflicted(cx);
    let pane = ws.read_with(cx, |ws, _| ws.active_pane().clone());
    let ix = pane.read_with(cx, |p, _| {
        p.items().iter().position(|e| e == &editor).expect("tab")
    });
    pane.update_in(cx, |p, window, cx| p.close(ix, window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asks before overwriting");
    cx.simulate_prompt_answer("Overwrite");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "xpub fn f() {}\n");
    let open = pane.read_with(cx, |p, _| p.items().contains(&editor));
    assert!(!open, "the tab the user closed must be closed after saving");
}

/// "Keep mine" exists only as a bar button. The bar does not wrap and
/// its buttons do not shrink, so in a narrow pane (three splits on a
/// 1400px window) the button is clipped off the pane's edge; unlike
/// Reload and Compare it has no command palette entry to fall back on.
#[test]
fn keep_mine_is_in_the_command_palette() {
    let commands: Vec<_> = crate::actions::commands()
        .into_iter()
        .chain(crate::actions::editor_commands())
        .collect();
    let names: Vec<&str> = commands.iter().map(|(_, action)| action.name()).collect();
    assert!(names.contains(&"file::ReloadFromDisk"), "{names:?}");
    assert!(
        names.contains(&"file::KeepMyChanges"),
        "Keep mine must be reachable without the bar"
    );
}
