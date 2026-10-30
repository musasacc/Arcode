//! Failing tests added by the adversarial review (one per verified issue).

mod common;

use arcode_vim::VimAction;
use common::{Session, assert_vim, ctx};

/// Ctrl-W on the command line truncates at a byte index that is not a
/// char boundary when the last non-word char is multi-byte: panic.
#[test]
fn cmdline_ctrl_w_after_multibyte_char_does_not_panic() {
    let mut s = Session::new("|a");
    s.keys(":a€b<c-w>");
    assert_eq!(s.vim.command_line().as_deref(), Some(":a€"));
}

/// `.` after a counted insert replays the internal count repetitions as
/// Normal-mode keys (`x` deletes, `dd` deletes lines).
#[test]
fn dot_after_counted_insert() {
    assert_vim("|b", "3ix<esc>.", "xxxx|xxb");
    assert_vim("|b", "2ix<esc>.", "xx|xxb");
    assert_vim("|b", "3ia<esc>.", "aaaa|aab");
    assert_vim("|a", "3odd<esc>.", "a\ndd\ndd\ndd\ndd\ndd\nd|d");
}

/// A buffer ending in `\n` gets a phantom empty last line that `G`/`j`
/// land on; `Gdd` then deletes the file's final newline instead of the
/// last line, and `Gp` pastes after a new empty line.
#[test]
fn trailing_newline_is_not_an_extra_line() {
    assert_vim("|a\nb\n", "G", "a\n|b\n");
    assert_vim("a\n|b\n", "j", "a\n|b\n");
    assert_vim("|a\nb\n", "Gdd", "|a\n");
    assert_vim("|a\nb\n", "yyGp", "a\nb\n|a\n");
}

/// `.` after a clipboard paste (`"+p`) does not repeat the paste: the
/// change is dropped because the buffer is only edited later by
/// `paste_clipboard`, so `.` repeats the previous change instead.
#[test]
fn dot_repeats_clipboard_paste() {
    let ctx = ctx();
    let mut s = Session::new("|a b c");
    s.keys("dw\"+p");
    assert_eq!(s.last_action(), Some(&VimAction::PasteRequest));
    s.vim.paste_clipboard("X", &mut s.buffer, &mut s.sels, &ctx);
    assert_eq!(s.state(), "b|X c");
    s.actions.clear();
    s.keys(".");
    assert_eq!(s.last_action(), Some(&VimAction::PasteRequest));
    assert_eq!(s.state(), "b|X c");
}

/// Word motions, `r`, `~` and `t` split grapheme clusters (base char +
/// combining mark), leaving the cursor inside a cluster or orphaned marks.
#[test]
fn grapheme_clusters_are_one_char() {
    assert_vim("|cafe\u{301} x", "w", "cafe\u{301} |x");
    assert_vim("|cafe\u{301} x", "dw", "|x");
    assert_vim("|e\u{301}a", "rx", "|xa");
    assert_vim("|e\u{301}a", "~", "E\u{301}|a");
    assert_vim("|ae\u{301}x", "tx", "a|e\u{301}x");
}

/// `?` from inside a match skips the match that starts before the cursor
/// (it only finds matches ending at or before the cursor).
#[test]
fn backward_search_from_inside_match() {
    assert_vim("xf|oo foo", "?foo<cr>", "x|foo foo");
}

/// Multi-cursor `J` with one cursor on the last line moves that cursor to
/// the buffer start.
#[test]
fn join_multicursor_keeps_last_line_cursor() {
    assert_vim("|a\nb\n|c", "J", "a| b\n|c");
}

/// `v$j` should keep selecting to the end of line (including the line
/// break) like Vim; the `$` is lost on the vertical move.
#[test]
fn visual_dollar_then_j_keeps_eol() {
    assert_vim("|ab\ncd\nef", "v$jd", "|ef");
}

/// The `.` register pops only one char for Ctrl-W/Ctrl-U deletes.
#[test]
fn dot_register_after_ctrl_w() {
    let mut s = Session::new("|");
    s.keys("ifoo bar<c-w>x<esc>");
    assert_eq!(s.reg('.').as_deref(), Some("foo x"));
}

/// A count on `R` is ignored (`3Rxy<Esc>` should overwrite with `xyxyxy`).
#[test]
fn replace_mode_count() {
    assert_vim("|abcd", "3Rxy<esc>", "xyxyx|y");
}
