//! Following a file that changed on disk: `Editor::apply_disk_reload`,
//! and the conflict when the buffer has unsaved edits.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use arcode_buffer::{Buffer, DiskStamp, LineEnding, Point};
use arcode_editor::{DiskConflict, Editor, EditorEvent};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};

#[gpui::test]
fn reload_replaces_text_keeps_cursor_and_stays_clean(cx: &mut TestAppContext) {
    let (ed, cx) = common::open(cx, "fn main() {\n    \"oops\"\n}\n");
    ed.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(2, 1)..Point::new(2, 1)], cx);
        let v = e.buffer().version();
        let edit = (Point::new(1, 4)..Point::new(1, 10), "4".to_string());
        assert!(e.apply_disk_reload(v, vec![edit], LineEnding::Lf, None, cx));
        assert_eq!(e.buffer().to_string(), "fn main() {\n    4\n}\n");
        assert_eq!(e.cursor_position(), Point::new(2, 1));
        assert!(!e.is_dirty());
        // Undo goes back to the old text, which differs from the file.
        e.undo(cx);
        assert_eq!(e.buffer().to_string(), "fn main() {\n    \"oops\"\n}\n");
        assert!(e.is_dirty());
    });
}

#[gpui::test]
fn reload_skips_dirty_or_changed_buffers(cx: &mut TestAppContext) {
    let (ed, cx) = common::open(cx, "abc\n");
    ed.update(cx, |e, cx| {
        let v = e.buffer().version();
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
        let edit = (Point::new(0, 0)..Point::new(0, 3), "def".to_string());
        assert!(!e.apply_disk_reload(v, vec![edit.clone()], LineEnding::Lf, None, cx));
        assert_eq!(e.buffer().to_string(), "xabc\n");
        // Clean again, but the version moved on: stale edits are refused.
        e.undo(cx);
        assert!(!e.is_dirty());
        assert!(!e.apply_disk_reload(v, vec![edit], LineEnding::Lf, None, cx));
        assert_eq!(e.buffer().to_string(), "abc\n");
    });
}

#[gpui::test]
fn reload_follows_line_ending(cx: &mut TestAppContext) {
    let (ed, cx) = common::open(cx, "a\nb\n");
    ed.update(cx, |e, cx| {
        let v = e.buffer().version();
        assert!(e.apply_disk_reload(v, Vec::new(), LineEnding::CrLf, None, cx));
        assert_eq!(e.buffer().line_ending(), LineEnding::CrLf);
        assert!(!e.is_dirty());
    });
}

/// An editor on a real file `a.txt` holding `text`, and the events it
/// emits.
fn open_file<'a>(
    cx: &'a mut TestAppContext,
    dir: &std::path::Path,
    text: &str,
) -> (
    Entity<Editor>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<EditorEvent>>>,
) {
    let path = dir.join("a.txt");
    std::fs::write(&path, text).unwrap();
    let (_, cx) = common::open(cx, "");
    let editor = cx.update(|_, cx| cx.new(|cx| Editor::new(Buffer::from_file(&path).unwrap(), cx)));
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&editor, move |_, event: &EditorEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach()
    });
    (editor, cx, events)
}

#[gpui::test]
fn save_refuses_to_overwrite_a_file_changed_on_disk(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let (editor, cx, events) = open_file(cx, dir.path(), "one\n");
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 3)..Point::new(0, 3), "!".into())], cx)
    });
    std::fs::write(&path, "theirs\n").unwrap();
    let saved = editor.update(cx, |e, cx| e.save(cx));
    assert!(saved.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");
    assert!(events.borrow().contains(&EditorEvent::SaveConflict));
    editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), Some(DiskConflict::Changed));
        assert!(e.is_dirty());
    });
    // Told to, it overwrites, and the conflict is gone.
    editor.update(cx, |e, cx| e.save_overwriting(cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one!\n");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.disk_conflict(), None);
        assert!(!e.is_dirty());
    });
    // Saving again with nothing changed on disk does not ask.
    events.borrow_mut().clear();
    editor.update(cx, |e, cx| e.save(cx)).unwrap();
    assert!(!events.borrow().contains(&EditorEvent::SaveConflict));
}

#[gpui::test]
fn same_content_or_a_deleted_file_saves_without_asking(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let (editor, cx, events) = open_file(cx, dir.path(), "one\n");
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "1".into())], cx)
    });
    // Rewritten with the same bytes (e.g. `git checkout` of the same
    // version): not a conflict.
    std::fs::write(&path, "one\n").unwrap();
    editor.update(cx, |e, cx| e.save(cx)).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1one\n");
    // Deleted: saving writes it again and clears the mark.
    std::fs::remove_file(&path).unwrap();
    editor.update(cx, |e, cx| {
        e.set_disk_conflict(Some(DiskConflict::Deleted), cx);
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "2".into())], cx);
        e.save(cx).unwrap();
        assert_eq!(e.disk_conflict(), None);
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "21one\n");
    assert!(!events.borrow().contains(&EditorEvent::SaveConflict));
}

#[gpui::test]
fn discard_and_reload_replaces_unsaved_edits_undoably(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let (editor, cx, events) = open_file(cx, dir.path(), "a\n");
    std::fs::write(&path, "b\n").unwrap();
    let stamp = DiskStamp::read(&path).ok();
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
        e.set_disk_conflict(Some(DiskConflict::Changed), cx);
        let v = e.buffer().version();
        let edit = (Point::new(0, 0)..Point::new(0, 2), "b".to_string());
        assert!(!e.apply_disk_reload(v, vec![edit.clone()], LineEnding::Lf, stamp, cx));
        assert!(e.discard_and_reload(v, vec![edit], LineEnding::Lf, stamp, cx));
        assert_eq!(e.buffer().to_string(), "b\n");
        assert!(!e.is_dirty());
        assert_eq!(e.disk_conflict(), None);
        e.undo(cx);
        assert_eq!(e.buffer().to_string(), "xa\n");
        // The buffer is now based on the disk version: saving the old
        // edits again is the user's informed choice.
        e.save(cx).unwrap();
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xa\n");
    assert!(events.borrow().contains(&EditorEvent::DiskConflictChanged));
    assert!(events.borrow().contains(&EditorEvent::Reloaded));
    assert!(!events.borrow().contains(&EditorEvent::SaveConflict));
}

#[gpui::test]
fn keep_mine_saves_over_the_new_version_without_asking(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let (editor, cx, events) = open_file(cx, dir.path(), "a\n");
    std::fs::write(&path, "b\n").unwrap();
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
        e.set_disk_conflict(Some(DiskConflict::Changed), cx);
        e.keep_mine(DiskStamp::read(&path).ok(), cx);
        assert_eq!(e.disk_conflict(), None);
        assert!(e.is_dirty());
        e.save(cx).unwrap();
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xa\n");
    assert!(!events.borrow().contains(&EditorEvent::SaveConflict));
}

#[gpui::test]
fn set_path_reports_the_new_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (editor, cx, events) = open_file(cx, dir.path(), "a\n");
    editor.update(cx, |e, cx| e.set_path(dir.path().join("b.rs"), cx));
    assert!(events.borrow().contains(&EditorEvent::PathChanged));
}
