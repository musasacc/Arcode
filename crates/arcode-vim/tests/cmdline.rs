//! Ex commands and the command line.

mod common;

use arcode_vim::{Mode, VimAction, VimCommand};
use common::{Session, check_table};

fn command(initial: &str, keys: &str) -> Option<VimAction> {
    let mut s = Session::new(initial);
    s.keys(keys);
    s.last_action().cloned()
}

fn cmd(c: VimCommand) -> Option<VimAction> {
    Some(VimAction::Command(c))
}

#[test]
fn file_commands() {
    assert_eq!(command("|a", ":w<cr>"), cmd(VimCommand::Save));
    assert_eq!(command("|a", ":write<cr>"), cmd(VimCommand::Save));
    assert_eq!(command("|a", ":q<cr>"), cmd(VimCommand::Quit));
    assert_eq!(command("|a", ":wq<cr>"), cmd(VimCommand::SaveQuit));
    assert_eq!(command("|a", ":x<cr>"), cmd(VimCommand::SaveQuit));
    assert_eq!(command("|a", ":q!<cr>"), cmd(VimCommand::ForceQuit));
    assert_eq!(command("|a", "ZZ"), cmd(VimCommand::SaveQuit));
    assert_eq!(command("|a", "ZQ"), cmd(VimCommand::ForceQuit));
    assert_eq!(command("|a", ":noh<cr>"), cmd(VimCommand::NoHighlight));
    assert_eq!(
        command("|a", ":set nu<cr>"),
        cmd(VimCommand::Unknown("set nu".into()))
    );
    assert_eq!(
        command("|a", ":  <cr>"),
        Some(VimAction::ModeChanged(Mode::Normal))
    );
}

#[test]
fn goto_line() {
    check_table(&[
        ("|a\nb\n  c", ":3<cr>", "a\nb\n  |c"),
        ("|a\nb\nc", ":$<cr>", "a\nb\n|c"),
        ("|a\nb\nc", ":99<cr>", "a\nb\n|c"),
        ("a\n|b\nc", ":.+1<cr>", "a\nb\n|c"),
        ("a\n|b\nc", ":-<cr>", "|a\nb\nc"),
        ("|a\nb\nc", "jmxgg:'x<cr>", "a\n|b\nc"),
    ]);
    assert_eq!(command("|a\nb", ":2<cr>"), cmd(VimCommand::GotoLine(2)));
}

#[test]
fn substitute() {
    check_table(&[
        ("|a a\na", ":s/a/b/<cr>", "|b a\na"),
        ("|a a\na", ":s/a/b/g<cr>", "|b b\na"),
        ("|a a\na", ":%s/a/b/g<cr>", "b b\n|b"),
        ("|a a\na", ":%s/a/b/<cr>", "b a\n|b"),
        ("|a\na\na", ":2,3s/a/b/<cr>", "a\nb\n|b"),
        ("|a\na\na", ":.,$s/a/b/<cr>", "b\nb\n|b"),
        ("|A a", ":s/a/x/gi<cr>", "|x x"),
        ("|A a", ":s/a\\c/x/g<cr>", "|x x"),
        ("|a/b", ":s/\\//-/<cr>", "|a-b"),
        ("|a/b", ":s#/#-#<cr>", "|a-b"),
        ("|ab", ":s/b/[&]/<cr>", "|a[b]"),
        ("|ab", ":s/b/\\&/<cr>", "|a&"),
        ("|a,b", ":s/,/\\r/<cr>", "a\n|b"),
        ("|a\nb", ":%s/\\n//<cr>", "|ab"),
        ("|a.b", ":s/./x/<cr>", "|axb"),
        ("|ab ab", "/b<cr>:s//x/g<cr>", "|ax ax"),
        ("|abc", ":s/b<cr>", "|ac"),
        ("|aaa", ":substitute/a/b/<cr>", "|baa"),
    ]);
    assert_eq!(
        command("|a a\na", ":%s/a/b/g<cr>"),
        cmd(VimCommand::Substitute {
            replacements: 3,
            lines: 2
        })
    );
    assert_eq!(
        command("|a", ":s/z/y/<cr>"),
        Some(VimAction::StatusMessage(
            "E486: Pattern not found: z".into()
        ))
    );
}

#[test]
fn editing_the_command_line() {
    let mut s = Session::new("|a");
    s.keys(":wx");
    assert_eq!(s.mode(), Mode::CommandLine);
    assert_eq!(s.vim.command_line().as_deref(), Some(":wx"));
    s.keys("<bs><cr>");
    assert_eq!(s.last_action(), cmd(VimCommand::Save).as_ref());
    assert_eq!(s.mode(), Mode::Normal);
    s.keys(":q<esc>");
    assert_eq!(s.mode(), Mode::Normal);
    assert_eq!(s.vim.command_line(), None);
    s.keys(":abc<c-u>w<cr>");
    assert_eq!(s.last_action(), cmd(VimCommand::Save).as_ref());
    s.keys(":<bs>");
    assert_eq!(s.mode(), Mode::Normal);
    s.keys("3:");
    assert_eq!(s.vim.command_line().as_deref(), Some(":.,.+2"));
    s.keys("<esc>v:");
    assert_eq!(s.vim.command_line().as_deref(), Some(":'<,'>"));
}

#[test]
fn pending_keys_and_modes() {
    let mut s = Session::new("|a");
    s.keys("2d");
    assert_eq!(s.mode(), Mode::OperatorPending);
    assert_eq!(s.vim.pending_keys(), "2d");
    s.keys("<esc>");
    assert_eq!(s.mode(), Mode::Normal);
    assert_eq!(s.vim.pending_keys(), "");
    assert_eq!(Mode::VisualLine.label(), "V-LINE");
}
