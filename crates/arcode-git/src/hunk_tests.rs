use super::*;

fn revert_all(old: &str, new: &str) -> String {
    // Revert hunks from the last to the first so earlier line numbers hold.
    let mut text = new.to_owned();
    for hunk in diff_hunks(old, new).iter().rev() {
        text = revert_hunk(hunk, &text).apply(&text);
    }
    text
}

#[test]
fn hunks_carry_old_text_and_counts() {
    let hunks = diff_hunks("a\nb\nc\nd\n", "a\nB\nc\n");
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].kind, ChangeKind::Modified);
    assert_eq!((hunks[0].old_start, hunks[0].old_lines), (1, 1));
    assert_eq!((hunks[0].new_start, hunks[0].new_lines), (1, 1));
    assert_eq!(hunks[0].old_text, "b\n");
    assert_eq!(hunks[1].kind, ChangeKind::Deleted);
    assert_eq!(hunks[1].old_range(), 3..4);
    assert_eq!(hunks[1].new_range(), 3..3);
    assert_eq!(hunks[1].old_text, "d\n");
}

#[test]
fn added_hunk_has_empty_old_text() {
    let hunks = diff_hunks("", "x\ny");
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].kind, ChangeKind::Added);
    assert_eq!(hunks[0].old_text, "");
    assert_eq!(hunks[0].new_range(), 0..2);
}

#[test]
fn revert_restores_base() {
    let cases = [
        ("a\nb\nc\n", "a\nB\nc\n"),
        ("a\nb\nc\n", "a\nc\n"),
        ("a\nb\n", "x\na\nb\ny\n"),
        ("a\nb\nc\nd\ne\n", "A\nb\nc\nd\n"),
        ("", "new\n"),
        ("old\n", ""),
    ];
    for (old, new) in cases {
        assert_eq!(revert_all(old, new), old, "{old:?} <- {new:?}");
    }
}

#[test]
fn revert_after_unterminated_last_line_inserts_newline() {
    // The buffer lost its last line and its final newline.
    assert_eq!(revert_all("a\nb\n", "a"), "a\nb\n");
    let hunks = diff_hunks("a\nb\n", "a");
    let edit = revert_hunk(&hunks[0], "a");
    assert_eq!(edit.lines, 1..1);
    assert_eq!(edit.bytes, 1..1);
    assert_eq!(edit.text, "\nb\n");
}

#[test]
fn revert_follows_crlf_buffer() {
    let hunks = diff_hunks("a\nb\nc\n", "a\r\nB\r\nc\r\n");
    assert_eq!(hunks.len(), 1);
    let edit = revert_hunk(&hunks[0], "a\r\nB\r\nc\r\n");
    assert_eq!(edit.text, "b\r\n");
    assert_eq!(edit.bytes, 3..6);
    assert_eq!(edit.apply("a\r\nB\r\nc\r\n"), "a\r\nb\r\nc\r\n");
}

#[test]
fn apply_clamps_out_of_range() {
    let edit = HunkRevert {
        lines: 5..9,
        bytes: 50..90,
        text: "x\n".into(),
    };
    assert_eq!(edit.apply("ab"), "abx\n");
    assert_eq!(to_crlf("a\nb\r\n"), "a\r\nb\r\n");
}
