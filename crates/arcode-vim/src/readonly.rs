//! Read-only buffers (Vim's `nomodifiable`): motions, yanks, searches and
//! Visual mode work; every command that would change the text is refused
//! with [`READ_ONLY_MESSAGE`] before it touches the buffer.

use crate::normal::is_change;
use crate::parse::{Act, CmdKind};
use crate::types::{Mode, VimAction};
use crate::vim::Vim;

/// The message Vim returns for a refused change.
pub const READ_ONLY_MESSAGE: &str = "E21: Cannot make changes, the buffer is read-only";

impl Vim {
    /// Marks the buffer as read-only (or writable again). While read-only
    /// Vim never edits the buffer: changes, undo, redo, `.` and `:s` are
    /// refused with a [`VimAction::StatusMessage`] of
    /// [`READ_ONLY_MESSAGE`]. Turning it on in Insert or Replace mode
    /// returns to Normal mode without replaying a count.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
        if read_only && matches!(self.mode, Mode::Insert | Mode::Replace) {
            self.insert = None;
            self.dot = None;
            self.mode = Mode::Normal;
        }
    }

    /// Whether the buffer is read-only (see [`Vim::set_read_only`]).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// True if `kind` must be refused in a read-only buffer.
    pub(crate) fn refuses(&self, kind: &CmdKind) -> bool {
        self.read_only
            && (is_change(kind)
                || matches!(
                    kind,
                    CmdKind::Act(Act::Undo | Act::Redo | Act::Repeat | Act::BlockInsert(_))
                ))
    }

    /// The action for a refused change.
    pub(crate) fn refuse(&self) -> VimAction {
        VimAction::StatusMessage(READ_ONLY_MESSAGE.into())
    }
}

#[cfg(test)]
mod tests {
    use arcode_buffer::{Buffer, SelectionSet};

    use super::*;
    use crate::types::VimContext;

    fn run(text: &str, keys: &str) -> (String, Vec<VimAction>, Vim) {
        let ctx = VimContext::default();
        let mut buffer = Buffer::new(text);
        let mut sels = SelectionSet::single(0);
        let mut vim = Vim::new();
        vim.set_read_only(true);
        let actions = vim.handle_keys(keys, &mut buffer, &mut sels, &ctx);
        assert!(vim.take_edits().is_empty(), "no edits for {keys:?}");
        assert!(!buffer.can_undo(), "no history for {keys:?}");
        (buffer.to_string(), actions, vim)
    }

    #[test]
    fn changes_are_refused() {
        for keys in [
            "x",
            "dw",
            "dd",
            "3ix<esc>",
            "3o<esc>",
            "cwz<esc>",
            "rX",
            "~",
            "J",
            "p",
            "P",
            "<C-a>",
            "Rab<esc>",
            ">>",
            "gUw",
            "vd",
            "Vd",
            "<C-v>jIx<esc>",
            "vrx",
            "d/b<cr>",
            ":s/a/b/<cr>",
            ":%s/a/b/g<cr>",
            "u",
            "<C-r>",
            ".",
        ] {
            let (text, actions, vim) = run("abc\nabc", keys);
            assert_eq!(text, "abc\nabc", "{keys:?} changed the buffer");
            assert!(
                actions.contains(&VimAction::StatusMessage(READ_ONLY_MESSAGE.into())),
                "{keys:?}: {actions:?}"
            );
            assert!(
                !matches!(vim.mode(), Mode::Insert | Mode::Replace),
                "{keys:?} left Vim in {:?}",
                vim.mode()
            );
        }
    }

    #[test]
    fn motions_yanks_and_searches_still_work() {
        let ctx = VimContext::default();
        let mut buffer = Buffer::new("one two\nthree");
        let mut sels = SelectionSet::single(0);
        let mut vim = Vim::new();
        vim.set_read_only(true);
        let actions = vim.handle_keys("wywj/thr<cr>", &mut buffer, &mut sels, &ctx);
        assert!(!actions.contains(&VimAction::StatusMessage(READ_ONLY_MESSAGE.into())));
        assert_eq!(
            vim.register('"').map(|r| r.pieces.concat()),
            Some("two".into())
        );
        assert_eq!(sels.primary().head, 8);
        assert_eq!(buffer.to_string(), "one two\nthree");
    }

    #[test]
    fn macros_replay_without_changes() {
        let ctx = VimContext::default();
        let mut buffer = Buffer::new("xyz\nxyz");
        let mut sels = SelectionSet::single(0);
        let mut vim = Vim::new();
        vim.handle_keys("qaxjxq", &mut buffer, &mut sels, &ctx);
        vim.handle_keys("u", &mut buffer, &mut sels, &ctx);
        vim.take_edits();
        let before = buffer.to_string();
        vim.set_read_only(true);
        vim.handle_keys("gg@a", &mut buffer, &mut sels, &ctx);
        assert!(vim.take_edits().is_empty());
        assert_eq!(buffer.to_string(), before);
    }

    #[test]
    fn going_read_only_in_insert_mode_leaves_it() {
        let ctx = VimContext::default();
        let mut buffer = Buffer::new("abc");
        let mut sels = SelectionSet::single(0);
        let mut vim = Vim::new();
        vim.handle_keys("3i", &mut buffer, &mut sels, &ctx);
        assert_eq!(vim.mode(), Mode::Insert);
        vim.set_read_only(true);
        assert_eq!(vim.mode(), Mode::Normal);
        vim.handle_keys("<esc>", &mut buffer, &mut sels, &ctx);
        assert_eq!(buffer.to_string(), "abc");
    }
}
