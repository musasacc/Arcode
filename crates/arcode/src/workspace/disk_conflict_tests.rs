use std::path::{Path, PathBuf};

use arcode_buffer::Point;
use arcode_editor::{DiskConflict, Editor};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::super::Workspace;
use super::super::tests::{open, project_dir};
use crate::pane::{CompareWithDisk, KeepMyChanges, ReloadFromDisk};

/// A workspace on a fresh project with `src/lib.rs` open and edited
/// (unsaved), then changed on disk by "another program".
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
    changed(&ws, &lib, cx);
    (dir, lib, ws, editor, cx)
}

fn changed(ws: &Entity<Workspace>, path: &Path, cx: &mut VisualTestContext) {
    let paths = [path.to_path_buf()];
    ws.update(cx, |ws, cx| ws.files_changed_on_disk(&paths, false, cx));
    cx.run_until_parked();
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().to_string())
}

#[gpui::test]
fn a_dirty_editor_is_marked_and_shows_the_bar(cx: &mut TestAppContext) {
    let (_dir, _, _, editor, cx) = conflicted(cx);
    editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), Some(DiskConflict::Changed));
        assert_eq!(e.buffer().to_string(), "xpub fn f() {}\n", "edits kept");
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("disk-conflict-bar").is_some(),
        "the bar shows"
    );
    // Undone back to what was saved: the editor follows the file again.
    editor.update(cx, |e, cx| e.undo(cx));
    cx.run_until_parked();
    editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), None);
        assert_eq!(e.buffer().to_string(), "theirs\n");
    });
    // (No check that the bar is gone: GPUI 0.2.2 never forgets debug
    // bounds; the bar shows exactly while the editor is marked `Changed`.)
}

#[gpui::test]
fn reload_discards_the_edits_undoably(cx: &mut TestAppContext) {
    let (_dir, _, _, editor, cx) = conflicted(cx);
    cx.dispatch_action(ReloadFromDisk);
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "theirs\n");
    editor.read_with(cx, |e, _| {
        assert!(!e.is_dirty());
        assert_eq!(e.disk_conflict(), None);
    });
    editor.update(cx, |e, cx| e.undo(cx));
    assert_eq!(text(&editor, cx), "xpub fn f() {}\n");
}

#[gpui::test]
fn keep_mine_saves_without_asking(cx: &mut TestAppContext) {
    let (_dir, lib, _, editor, cx) = conflicted(cx);
    cx.dispatch_action(KeepMyChanges);
    cx.run_until_parked();
    editor.read_with(cx, |e, _| assert_eq!(e.disk_conflict(), None));
    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "xpub fn f() {}\n");
}

#[gpui::test]
fn compare_opens_a_diff_of_disk_and_buffer(cx: &mut TestAppContext) {
    let (_dir, _, ws, editor, cx) = conflicted(cx);
    cx.dispatch_action(CompareWithDisk);
    cx.run_until_parked();
    let diff = ws.read_with(cx, |ws, cx| ws.active_editor(cx).expect("diff tab"));
    assert_ne!(diff, editor);
    let (title, diff_text) = ws.read_with(cx, |ws, cx| {
        let title = ws.active_pane().read(cx).title_of(&diff, cx).to_string();
        (title, diff.read(cx).buffer().to_string())
    });
    assert_eq!(title, "lib.rs (On Disk ↔ Yours)");
    assert!(diff_text.contains("theirs") && diff_text.contains("xpub fn f() {}"));
    // The buffer keeps its edits and its conflict.
    editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), Some(DiskConflict::Changed))
    });
}

#[gpui::test]
fn saving_over_a_changed_file_asks_first(cx: &mut TestAppContext) {
    let (_dir, lib, _, editor, cx) = conflicted(cx);
    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asks before overwriting");
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "theirs\n");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "theirs\n");
    // Asked again, the answer is Overwrite.
    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Overwrite");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "xpub fn f() {}\n");
    editor.read_with(cx, |e, _| {
        assert!(!e.is_dirty());
        assert_eq!(e.disk_conflict(), None);
    });
}

#[gpui::test]
fn deleted_files_are_marked_and_keep_their_text(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let (main, lib) = (root.join("src/main.rs"), root.join("src/lib.rs"));
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main, window, cx);
        ws.open_file(&lib, window, cx);
    });
    let editor = |path: &Path, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editor_for_path(path, cx).expect("open"))
    };
    let (main_editor, lib_editor) = (editor(&main, cx), editor(&lib, cx));
    lib_editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
    });
    std::fs::remove_file(&main).unwrap();
    std::fs::remove_file(&lib).unwrap();
    let paths = [main.clone(), lib.clone()];
    ws.update(cx, |ws, cx| ws.files_changed_on_disk(&paths, false, cx));
    cx.run_until_parked();
    for (editor, text) in [
        (&main_editor, "fn main() {}\n"),
        (&lib_editor, "xpub fn f() {}\n"),
    ] {
        editor.read_with(cx, |e, _| {
            assert_eq!(e.disk_conflict(), Some(DiskConflict::Deleted));
            assert_eq!(e.buffer().to_string(), text);
        });
    }
    // The file comes back: the clean editor follows it, unmarked.
    std::fs::write(&main, "fn main() { 2 }\n").unwrap();
    changed(&ws, &main, cx);
    main_editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), None);
        assert_eq!(e.buffer().to_string(), "fn main() { 2 }\n");
    });
}

/// Closing the tab, "Save", then "Cancel" in the overwrite question: the
/// tab stays open, and a later save (answered "Overwrite") does not close
/// it either.
#[gpui::test]
fn close_save_cancel_keeps_the_tab(cx: &mut TestAppContext) {
    let (_dir, lib, ws, editor, cx) = conflicted(cx);
    let pane = ws.read_with(cx, |ws, _| ws.active_pane().clone());
    let ix = pane.read_with(cx, |p, _| {
        p.items().iter().position(|e| e == &editor).expect("tab")
    });
    pane.update_in(cx, |p, window, cx| p.close(ix, window, cx));
    cx.run_until_parked();
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "theirs\n");
    let open = |cx: &mut VisualTestContext| pane.read_with(cx, |p, _| p.items().contains(&editor));
    assert!(open(cx), "cancelled: the tab stays open");
    cx.simulate_keystrokes("secondary-s");
    cx.run_until_parked();
    cx.simulate_prompt_answer("Overwrite");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), "xpub fn f() {}\n");
    assert!(open(cx), "a plain save never closes the tab");
}

/// In a narrow pane the bar wraps: its message keeps room and every
/// button stays inside the bar (clickable).
#[gpui::test]
fn the_bar_wraps_in_a_narrow_pane(cx: &mut TestAppContext) {
    let (_dir, _, _, _, cx) = conflicted(cx);
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let bar = cx.debug_bounds("disk-conflict-bar").expect("bar");
    let message = cx.debug_bounds("disk-conflict-message").expect("message");
    let buttons = cx.debug_bounds("disk-conflict-buttons").expect("buttons");
    assert!(bar.size.width < gpui::px(600.), "narrow pane: {bar:?}");
    assert!(message.size.width >= gpui::px(160.), "{message:?}");
    assert!(
        buttons.left() >= bar.left() && buttons.right() <= bar.right(),
        "buttons {buttons:?} inside bar {bar:?}"
    );
    assert!(buttons.top() >= message.bottom(), "the buttons wrapped");
}
