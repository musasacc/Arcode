//! Public API helpers and Unicode handling.

mod common;

use arcode_buffer::{Buffer, Selection, SelectionSet};
use arcode_vim::{CursorShape, Key, Mode, Vim, VimAction};
use common::{Session, check_table, ctx};

#[test]
fn unicode_graphemes() {
    check_table(&[
        ("|éa", "l", "é|a"),
        ("|e\u{301}x", "l", "e\u{301}|x"),
        ("|e\u{301}x", "x", "|x"),
        ("|中文字", "$", "中文|字"),
        ("|😀 b", "w", "😀 |b"),
        ("|ab", "r😀", "|😀b"),
        ("|äb", "~", "Ä|b"),
    ]);
}

#[test]
fn force_normal_and_sync() {
    let ctx = ctx();
    let mut buffer = Buffer::new("abc");
    let mut sels = SelectionSet::single(0);
    let mut vim = Vim::new();
    vim.handle_keys("ix", &mut buffer, &mut sels, &ctx);
    assert_eq!(vim.mode(), Mode::Insert);
    vim.force_normal(&mut buffer, &mut sels, &ctx);
    assert_eq!(vim.mode(), Mode::Normal);
    assert_eq!(buffer.to_string(), "xabc");
    // A click past the line end is clamped onto the last char.
    sels = SelectionSet::single(4);
    assert_eq!(
        vim.sync_selections(&mut buffer, &mut sels, &ctx),
        Mode::Normal
    );
    assert_eq!(sels.primary().head, 3);
    // A drag selection switches to Visual mode.
    sels = SelectionSet::from_selection(Selection::new(0, 2));
    assert_eq!(
        vim.sync_selections(&mut buffer, &mut sels, &ctx),
        Mode::Visual
    );
    assert_eq!(vim.cursor_positions(&sels), vec![1]);
    // Collapsing it returns to Normal mode.
    sels = SelectionSet::single(1);
    assert_eq!(
        vim.sync_selections(&mut buffer, &mut sels, &ctx),
        Mode::Normal
    );
}

#[test]
fn marks_follow_edits() {
    let mut s = Session::new("ab\n|cd");
    s.keys("mak0Ix<cr><esc>");
    assert_eq!(s.buffer.to_string(), "x\nab\ncd");
    assert_eq!(s.vim.mark('a'), Some(5));
    s.keys("'a");
    assert_eq!(s.state(), "x\nab\n|cd");
    let edits = s.buffer.edit(vec![(0..0, "zz".to_string())]);
    s.vim.on_external_edits(&edits);
    assert_eq!(s.vim.mark('a'), Some(7));
}

#[test]
fn mode_shapes_and_labels() {
    assert_eq!(Mode::Normal.cursor_shape(), CursorShape::Block);
    assert_eq!(Mode::Insert.cursor_shape(), CursorShape::Bar);
    assert_eq!(Mode::Replace.cursor_shape(), CursorShape::Underline);
    assert!(Mode::VisualBlock.is_visual() && !Mode::Insert.is_visual());
    assert_eq!(Mode::Visual.label(), "VISUAL");
}

#[test]
fn key_construction() {
    let ctx = ctx();
    let mut buffer = Buffer::new("abc");
    let mut sels = SelectionSet::single(0);
    let mut vim = Vim::new();
    let shifted = Key {
        key: "a".into(),
        shift: true,
        ..Key::default()
    };
    assert_eq!(
        vim.handle_key(&shifted, &mut buffer, &mut sels, &ctx),
        VimAction::ModeChanged(Mode::Insert)
    );
    vim.handle_key(&Key::char('!'), &mut buffer, &mut sels, &ctx);
    vim.handle_key(&Key::ctrl("["), &mut buffer, &mut sels, &ctx);
    assert_eq!(buffer.to_string(), "abc!");
    assert_eq!(vim.mode(), Mode::Normal);
    assert_eq!(Key::parse_keys("<c-d>"), vec![Key::ctrl("d")]);
}

#[test]
fn empty_buffer_is_safe() {
    let keys = [
        "x",
        "dd",
        "dw",
        "p",
        "P",
        "J",
        "~",
        "u",
        "<c-r>",
        "w",
        "b",
        "e",
        "G",
        "gg",
        "$",
        "0",
        "%",
        "}",
        "{",
        "*",
        "n",
        "vd",
        "Vd",
        "<c-v>d",
        "ciwx<esc>",
        "di(",
        "dit",
        "dap",
        ">>",
        "<<",
        "=G",
        "gcc",
        "rx",
        "<c-a>",
        ":s/a/b/<cr>",
        "o<esc>",
        "A<bs><esc>",
        "R<bs><esc>",
    ];
    for k in keys {
        let mut s = Session::new("|");
        s.keys(k);
        assert_eq!(s.mode(), Mode::Normal, "{k}");
    }
}
