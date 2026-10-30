//! `.` repeat, undo grouping, registers and macros.

mod common;

use arcode_vim::{Register, RegisterKind, VimAction};
use common::{Session, check_table};

#[test]
fn dot_repeat() {
    check_table(&[
        ("|a b c", "dw.", "|c"),
        ("|a\nb\nc", "dd.", "|c"),
        ("|foo foo", "cwbar<esc>w.", "bar ba|r"),
        ("|x", "ia<esc>.", "|aax"),
        ("|a b c d", "dw2.", "|d"),
        ("|a b c d e", "2dw.", "|e"),
        ("|abc", "x..", "|"),
        ("|a\nb", "A;<esc>j.", "a;\nb|;"),
        ("|b", "2ia<esc>.", "aa|aab"),
        ("|abcdef", "vlx.", "|ef"),
        ("|a\nb\nc\nd", "Vjd.", "|"),
        ("|a\nb", ">>j.", "    a\n    |b"),
        ("|a", "ylp.", "aa|a"),
        ("|abcd", "rxl.", "x|xcd"),
        ("|abcd", "~.", "AB|cd"),
        ("|a\nb", "ox<esc>j.", "a\nx\nb\n|x"),
        ("|foo bar", "ciwX<esc>w.", "X |X"),
        ("|a\nb\nc\nd", "J.", "a b| c\nd"),
        ("|ab ab", "d/b<cr>.", "|b"),
        ("|1 1", "<c-a>w.", "2 |2"),
        ("|a b", "yw.", "|a b"),
        ("|abc", "lx.", "|a"),
        ("|a", ".", "|a"),
        ("|ab\ncd", "ix<right>y<esc>j.", "xayb\nc|yd"),
    ]);
}

#[test]
fn undo_grouping() {
    check_table(&[
        ("|a b c", "dwu", "|a b c"),
        ("|a b c", "dwdwuu", "|a b c"),
        ("|a b c", "dwdwu", "|b c"),
        ("|x", "iabc<esc>u", "|x"),
        ("|foo bar", "cwxyz<esc>u", "|foo bar"),
        ("|a b", "dwu<c-r>", "|b"),
        ("|a b c d", "d2w.u", "|c d"),
        ("|abcd", "3xu", "|abcd"),
        ("|a", "ob<cr>c<esc>u", "|a"),
        ("|", "ia<right>b<esc>u", "|a"),
        ("|abc", "xxx2u", "|bc"),
        ("|a a", ":s/a/b/g<cr>u", "|a a"),
        ("|b", "3ia<esc>u", "|b"),
        ("|a b", "u", "|a b"),
        ("|a", "yypu<c-r><c-r>", "a\n|a"),
    ]);
    let mut s = Session::new("|a");
    s.keys("u");
    assert_eq!(
        s.last_action(),
        Some(&VimAction::StatusMessage("Already at oldest change".into()))
    );
}

#[test]
fn registers() {
    check_table(&[
        ("|a b", "\"adwdw\"aP", "a| "),
        ("|a b", "yiwwdiw\"0p", "a |a"),
        ("|a\nb\n", "dddd\"2p", "\n|a"),
        ("|a b", "yiww\"_dwP", "a|a "),
        ("|ab cd", "\"ayiww\"Ayiw$\"ap", "ab cdabc|d"),
        ("|ab\ncd", "\"byyj\"Byy\"bP", "ab\n|ab\ncd\ncd"),
        ("|a b", "dw\"-P", "a| b"),
        ("|ab", "\"zp", "|ab"),
    ]);
    let mut s = Session::new("|ab cd");
    s.keys("dw");
    assert_eq!(s.reg('-').as_deref(), Some("ab "));
    assert_eq!(s.reg('"').as_deref(), Some("ab "));
    s.keys("dd");
    assert_eq!(s.reg('1').as_deref(), Some("cd\n"));
    s.keys(":noh<cr>");
    assert_eq!(s.reg(':').as_deref(), Some("noh"));
}

#[test]
fn clipboard_registers() {
    let mut s = Session::new("|ab");
    s.keys("\"+yy");
    assert_eq!(s.last_action(), Some(&VimAction::Yank("ab\n".into())));
    s.keys("\"*yl");
    assert_eq!(s.last_action(), Some(&VimAction::Yank("a".into())));
    s.keys("\"+p");
    assert_eq!(s.last_action(), Some(&VimAction::PasteRequest));
    let ctx = common::ctx();
    s.vim
        .paste_clipboard("XY", &mut s.buffer, &mut s.sels, &ctx);
    assert_eq!(s.state(), "aX|Yb");
    s.keys("u");
    assert_eq!(s.buffer.to_string(), "ab");
    s.keys("\"+P");
    s.vim
        .paste_clipboard("line\n", &mut s.buffer, &mut s.sels, &ctx);
    assert_eq!(s.state(), "|line\nab");
    s.keys("\"+dd");
    assert_eq!(s.last_action(), Some(&VimAction::Yank("line\n".into())));
    s.vim
        .set_register('a', Register::new("q", RegisterKind::Charwise));
    s.keys("\"ap");
    assert_eq!(s.buffer.to_string(), "aqb");
}

#[test]
fn macros() {
    check_table(&[
        ("|a\nb\nc", "qaA!<esc>jq2@a", "a!\nb!\nc|!"),
        ("|a\nb\nc", "qqA!<esc>jq@q@@", "a!\nb!\nc|!"),
        ("|1\n1", "qax<esc>q", "|\n1"),
        ("|a", "@z", "|a"),
    ]);
    let mut s = Session::new("|a");
    s.keys("qa");
    assert_eq!(s.vim.recording_macro(), Some('a'));
    s.keys("q");
    assert_eq!(s.vim.recording_macro(), None);
}
