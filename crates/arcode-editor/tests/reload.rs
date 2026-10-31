//! Following a file that changed on disk: `Editor::apply_disk_reload`.

mod common;

use arcode_buffer::{LineEnding, Point};
use gpui::TestAppContext;

#[gpui::test]
fn reload_replaces_text_keeps_cursor_and_stays_clean(cx: &mut TestAppContext) {
    let (ed, cx) = common::open(cx, "fn main() {\n    \"oops\"\n}\n");
    ed.update(cx, |e, cx| {
        e.select_ranges(vec![Point::new(2, 1)..Point::new(2, 1)], cx);
        let v = e.buffer().version();
        let edit = (Point::new(1, 4)..Point::new(1, 10), "4".to_string());
        assert!(e.apply_disk_reload(v, vec![edit], LineEnding::Lf, cx));
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
        assert!(!e.apply_disk_reload(v, vec![edit.clone()], LineEnding::Lf, cx));
        assert_eq!(e.buffer().to_string(), "xabc\n");
        // Clean again, but the version moved on: stale edits are refused.
        e.undo(cx);
        assert!(!e.is_dirty());
        assert!(!e.apply_disk_reload(v, vec![edit], LineEnding::Lf, cx));
        assert_eq!(e.buffer().to_string(), "abc\n");
    });
}

#[gpui::test]
fn reload_follows_line_ending(cx: &mut TestAppContext) {
    let (ed, cx) = common::open(cx, "a\nb\n");
    ed.update(cx, |e, cx| {
        let v = e.buffer().version();
        assert!(e.apply_disk_reload(v, Vec::new(), LineEnding::CrLf, cx));
        assert_eq!(e.buffer().line_ending(), LineEnding::CrLf);
        assert!(!e.is_dirty());
    });
}
