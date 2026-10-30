//! Regression tests around the review fixes (beyond the reviewer's own
//! cases in `review_tests.rs`), plus a deterministic key fuzz.

mod common;

use arcode_buffer::{Buffer, SelectionSet};
use arcode_vim::{Key, Vim, VimAction};
use common::{Session, check_table, ctx};

#[test]
fn trailing_newline_cases() {
    check_table(&[
        ("a\n|b\n", "dd", "|a\n"),
        ("|a\nb\n", "dG", "|"),
        ("|a\nb\n", "yyjp", "a\nb\n|a\n"),
        ("|a\nb\n", "yyGP", "a\n|a\nb\n"),
        ("|a\nb\n", "Gox<esc>", "a\nb\n|x\n"),
        ("|a\nb\n", "Gj", "a\n|b\n"),
        ("|a\nb\n", "Vjd", "|"),
        ("|a\nb\n", "jVd", "|a\n"),
        ("|a\nb\n", "100j", "a\n|b\n"),
        ("|a\nb\n", "}", "a\n|b\n"),
        ("|a\nb\n", "2dd", "|"),
        ("|a\nb\n", ":2<cr>", "a\n|b\n"),
        ("|a\nb\n", ":%s/b/c/<cr>", "a\n|c\n"),
        ("|\n", "dd", "|"),
        ("a\n\n|b\n", "dd", "a\n|\n"),
        ("|a\nb\n", "ylGp", "a\nb|a\n"),
    ]);
}

#[test]
fn search_and_star() {
    check_table(&[
        ("foo x fo|o", "#", "|foo x foo"),
        ("f|oo x foo", "*", "foo x |foo"),
        ("foo f|oo", "/foo<cr>N", "foo |foo"),
        ("|a foo b foo", "?foo<cr>", "a foo b |foo"),
        ("foo |foo", "?foo<cr>", "|foo foo"),
    ]);
}

#[test]
fn counted_insert_and_replace_repeat() {
    check_table(&[
        ("|b", "2ix<esc>j.", "xx|xxb"),
        ("|abcdef", "2Rxy<esc>0l.", "xxyx|yf"),
        ("|a", "2Ab<esc>.", "abbb|b"),
        ("|a", "3ix<esc>u", "|a"),
    ]);
}

#[test]
fn dot_register_contents() {
    let cases = [
        ("ifoo<c-u>bar<esc>", "bar"),
        ("i<tab>x<esc>", "    x"),
        ("iab<bs>c<esc>", "ac"),
        ("iab<del>c<esc>", "abc"),
    ];
    for (keys, want) in cases {
        let mut s = Session::new("|");
        s.keys(keys);
        assert_eq!(s.reg('.').as_deref(), Some(want), "keys {keys:?}");
    }
}

#[test]
fn join_and_visual_eol() {
    check_table(&[
        ("|a\nb\nc|d", "J", "a| b\nc|d"),
        ("|a\nb\n|c\nd", "J", "a| b\nc| d"),
        ("|ab\ncd\nef\ngh", "v$jjd", "|gh"),
        ("ab\n|cd\nef", "v$ky", "a|b\ncd\nef"),
        ("|ab\ncd\nef", "v$jy", "|ab\ncd\nef"),
    ]);
    let mut s = Session::new("|ab\ncd\nef");
    s.keys("v$jy");
    assert_eq!(s.reg('"').as_deref(), Some("ab\ncd\n"));
    let mut s = Session::new("ab\n|cd\nef");
    s.keys("v$ky");
    assert_eq!(s.reg('"').as_deref(), Some("\nc"));
}

#[test]
fn grapheme_operators() {
    check_table(&[
        ("|e\u{301}e\u{301}a", "2rx", "x|xa"),
        ("|e\u{301}a", "x", "|a"),
        ("|ae\u{301}x", "fx", "ae\u{301}|x"),
        ("ae\u{301}|x", "Ta", "a|e\u{301}x"),
        ("|cafe\u{301} x", "e", "caf|e\u{301} x"),
        ("|cafe\u{301} x", "cwy<esc>", "|y x"),
        ("cafe\u{301} |x", "b", "|cafe\u{301} x"),
        ("|e\u{301}", "3~", "|E\u{301}"),
    ]);
}

#[test]
fn clipboard_paste_dot_with_count() {
    let ctx = ctx();
    let mut s = Session::new("|a");
    s.keys("\"+2p");
    assert_eq!(s.last_action(), Some(&VimAction::PasteRequest));
    s.vim.paste_clipboard("X", &mut s.buffer, &mut s.sels, &ctx);
    assert_eq!(s.state(), "aX|X");
    s.keys(".");
    s.vim.paste_clipboard("Y", &mut s.buffer, &mut s.sels, &ctx);
    assert_eq!(s.state(), "aXXY|Y");
    s.keys("u");
    assert_eq!(s.buffer.to_string(), "aXX");
}

#[test]
fn cmdline_ctrl_w_multibyte() {
    let mut s = Session::new("|a");
    s.keys(":é→ü<c-w>");
    assert_eq!(s.vim.command_line().as_deref(), Some(":é→"));
    s.keys("<c-w>");
    assert_eq!(s.vim.command_line().as_deref(), Some(":é"));
    s.keys("<c-w>");
    assert_eq!(s.vim.command_line().as_deref(), Some(":"));
    s.keys("ab €€  <c-w>");
    assert_eq!(s.vim.command_line().as_deref(), Some(":ab "));
}

/// Random key sequences over multibyte text must never panic and must
/// leave valid selections.
#[test]
fn key_fuzz_does_not_panic() {
    const KEYS: &[&str] = &[
        "h", "j", "k", "l", "w", "b", "e", "W", "B", "E", "0", "$", "^", "x", "X", "d", "c", "y",
        "p", "P", "u", "<c-r>", ".", "i", "a", "o", "O", "A", "I", "<esc>", "<esc>", "v", "V",
        "<c-v>", "r", "~", "J", "f", "t", "F", "T", ";", ",", "%", "{", "}", "G", "g", "2", "3",
        "\"", "+", "a", "iw", "€", "é", "\u{301}", "→", " ", "<cr>", "<bs>", "<c-w>", "<c-u>", ":",
        "/", "?", "n", "N", "*", "#", "s", "S", "C", "D", "R", ">", "<", "=", "q", "@", "<tab>",
        "<del>", "<left>", "<right>", "<up>", "<down>", "\n", "(", ")", "\"",
    ];
    let texts = [
        "a€b →c\ncafe\u{301} (x)\n\n  ü\n",
        "",
        "\n",
        "x",
        "é\u{301}\u{301}\n\t[a] {b}\n",
    ];
    let ctx = ctx();
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..1500 {
        let text = texts[round % texts.len()];
        let mut buffer = Buffer::new(text);
        let mut sels = SelectionSet::single(0);
        let mut vim = Vim::new();
        for _ in 0..40 {
            let k = KEYS[(next() % KEYS.len() as u64) as usize];
            for key in Key::parse_keys(k) {
                if vim.handle_key(&key, &mut buffer, &mut sels, &ctx) == VimAction::PasteRequest {
                    vim.paste_clipboard("Z\u{301}\n", &mut buffer, &mut sels, &ctx);
                }
            }
            let len = buffer.len_chars();
            assert!(
                sels.iter().all(|s| s.anchor <= len && s.head <= len),
                "selection out of range"
            );
        }
    }
}
