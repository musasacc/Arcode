use super::*;

/// Applies point edits the way the editor does, through a buffer.
fn applied(old: &str, new: &str) -> String {
    let edits = reload_edits(old, new);
    let mut buffer = Buffer::new(old);
    let raw = edits
        .into_iter()
        .map(|(r, t)| {
            (
                buffer.point_to_char(r.start)..buffer.point_to_char(r.end),
                t,
            )
        })
        .collect();
    buffer.edit(raw);
    buffer.to_string()
}

#[test]
fn edits_turn_old_into_new() {
    let cases = [
        ("fn main() {\n    \"oops\"\n}\n", "fn main() {\n    4\n}\n"),
        ("a\nb\nc\n", "a\nc\n"),
        ("a\nb", "a\nb\n"),
        ("a\nb\n", "a\nb"),
        ("", "x\ny\n"),
        ("x\ny\n", ""),
        ("a\nb", "a\nc\nd"),
        ("é\nü\n", "é\nñ\nü\n"),
        ("a\u{2028}b\n", "a\u{2028}c\n"),
        ("one\ntwo\nthree\nfour\n", "zero\none\ntwo\n3\nfour\nfive\n"),
    ];
    for (old, new) in cases {
        assert_eq!(applied(old, new), new, "{old:?} -> {new:?}");
    }
}

#[test]
fn blocks_are_separate_edits() {
    let old = "a\nb\nc\nd\ne\n";
    let new = "A\nb\nc\nd\nE\n";
    let edits = reload_edits(old, new);
    assert_eq!(
        edits,
        vec![
            (Point::new(0, 0)..Point::new(1, 0), "A\n".to_string()),
            (Point::new(4, 0)..Point::new(5, 0), "E\n".to_string()),
        ]
    );
}

#[test]
fn prefix_suffix_handles_multibyte() {
    assert_eq!(prefix_suffix_edit("aéb", "aüb"), (1..3, "ü".to_string()));
    assert_eq!(prefix_suffix_edit("aa", "aaa"), (2..2, "a".to_string()));
    assert_eq!(prefix_suffix_edit("same", "same"), (4..4, String::new()));
}

#[gpui::test]
fn clean_editors_follow_the_file_and_dirty_ones_keep_their_edits(cx: &mut gpui::TestAppContext) {
    use super::super::tests::{open, project_dir};
    let dir = project_dir();
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let (main, lib) = (root.join("src/main.rs"), root.join("src/lib.rs"));
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main, window, cx);
        ws.open_file(&lib, window, cx);
    });
    let editor = |path: &Path, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editor_for_path(path, cx).expect("open"))
    };
    let lib_editor = editor(&lib, cx);
    lib_editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
    });
    std::fs::write(&main, "fn main() {\n    4\n}\n").expect("write");
    std::fs::write(&lib, "changed\n").expect("write");
    ws.update(cx, |ws, cx| {
        ws.files_changed_on_disk(&[main.clone(), lib.clone()], false, cx)
    });
    cx.run_until_parked();
    let main_editor = editor(&main, cx);
    main_editor.read_with(cx, |e, _| {
        assert_eq!(e.buffer().to_string(), "fn main() {\n    4\n}\n");
        assert!(!e.is_dirty());
    });
    lib_editor.read_with(cx, |e, _| {
        assert_eq!(e.buffer().to_string(), "xpub fn f() {}\n");
        assert!(e.is_dirty());
    });
}

#[gpui::test]
fn a_reload_overtaken_by_a_change_tries_again(cx: &mut gpui::TestAppContext) {
    use super::super::tests::{open, project_dir};
    let dir = project_dir();
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    let main = root.join("src/main.rs");
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&main, window, cx);
    });
    let editor = ws.read_with(cx, |ws, cx| ws.editor_for_path(&main, cx).expect("open"));
    std::fs::write(&main, "fn main() { 1 }\n").expect("write");
    cx.update(|_, cx| reload_editor(&editor, cx));
    // Before the read finishes, the buffer changes and is clean again.
    editor.update(cx, |e, cx| {
        e.apply_edits(vec![(Point::new(0, 0)..Point::new(0, 0), "x".into())], cx);
        e.undo(cx);
        assert!(!e.is_dirty());
    });
    cx.run_until_parked();
    editor.read_with(cx, |e, _| {
        assert_eq!(e.buffer().to_string(), "fn main() { 1 }\n");
        assert!(!e.is_dirty());
    });
}
