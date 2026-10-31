//! Optional Vim mode for the Arcode editor, as a UI-agnostic state machine
//! on top of [`arcode_buffer`].
//!
//! The editor view forwards every keystroke to [`Vim::handle_key`] together
//! with its [`Buffer`](arcode_buffer::Buffer), its
//! [`SelectionSet`](arcode_buffer::SelectionSet) and a [`VimContext`].
//! Vim edits the buffer and selections in place and returns a
//! [`VimAction`] for anything else (save, scroll, clipboard, status text).
//!
//! # Cursor mapping
//!
//! Buffer positions sit **between** chars; a Vim cursor sits **on** a char.
//! In Normal mode each selection is an empty cursor at `pos` and the block
//! cursor covers `[pos, pos + 1)`; Vim keeps `pos` on a char (never past the
//! last char of a non-empty line; at the line start of an empty line).
//! Visual selections are inclusive in Vim and are written to the selection
//! set as half-open ranges: charwise `[min, max + 1)`, linewise whole lines
//! (including the line break), blockwise one selection per line (multiple
//! cursors). Use [`Vim::cursor_positions`] to know where to draw the block
//! cursor in Visual modes. In Insert mode cursors are ordinary carets.
//!
//! As in Vim, a trailing line break ends the last line: the empty buffer
//! line after it is not a Vim line (`G`, `j`, `dd`, `p` never land on it).
//! Horizontal motions, `w`/`e`/`b`, `f`/`t`, `r` and `~` step over whole
//! grapheme clusters, so a cursor never sits inside one.
//!
//! # Insert mode
//!
//! Vim handles text insertion itself (typed chars, Enter with auto-indent,
//! Tab, Backspace, Delete, Ctrl-W, Ctrl-U, Ctrl-R {reg}) so that `.` and
//! counts can repeat it; these keys never return
//! [`VimAction::PassThrough`]. Other Ctrl/Alt keys pass through. For text
//! that does not come from keys (IME commits, menu paste), call
//! [`Vim::insert_text`].
//!
//! # Undo
//!
//! Every Normal-mode command is one undo step, and so is a whole Insert
//! session (`cw...<Esc>`, `3ihi<Esc>`, `.`): Vim's edits use
//! [`UndoGroup::Merge`](arcode_buffer::UndoGroup::Merge) and break the group
//! when the command completes. Arrow keys in Insert mode start a new step.
//!
//! # Read-only buffers
//!
//! [`Vim::set_read_only`] makes Vim refuse every change before it touches
//! the buffer (operators other than `y`, Insert/Replace mode, `p`, `x`,
//! `u`, `<C-r>`, `.`, `:s`, changes inside a replayed macro), returning a
//! [`VimAction::StatusMessage`] with [`READ_ONLY_MESSAGE`]. Motions,
//! yanks, searches and Visual mode keep working.
//!
//! # Registers and clipboard
//!
//! Unnamed, `"0`, `"1`-`"9`, `"-`, `"a`-`"z` (`"A`-`"Z` append), `"_`,
//! and read-only `".`, `":`, `"/`. Yanks/deletes into `"+`/`"*` return
//! [`VimAction::Yank`]; pastes from them return [`VimAction::PasteRequest`],
//! answered with [`Vim::paste_clipboard`].
//!
//! # Search and `:s`
//!
//! Searches and `:s` patterns are literal strings, not regular expressions
//! (`\c` ignores case). `:s` understands `\/` (the delimiter), `\n`, `\t`
//! and `\\` in the pattern, `&`/`\0` (the match), `\&`, `\n`/`\r` (line
//! break) and `\t` in the replacement, and the flags `g`, `i`, `I`.
//!
//! ```
//! use arcode_buffer::{Buffer, SelectionSet};
//! use arcode_vim::{Vim, VimContext};
//!
//! let mut buffer = Buffer::new("one two three");
//! let mut sels = SelectionSet::single(0);
//! let mut vim = Vim::new();
//! vim.handle_keys("dwwcwTWO<esc>", &mut buffer, &mut sels, &VimContext::default());
//! assert_eq!(buffer.to_string(), "two TWO");
//! ```

mod actions;
mod cmdline;
mod insert;
mod key;
mod line_ops;
mod motion;
mod normal;
mod ops;
mod parse;
mod put;
mod readonly;
mod registers;
mod repeat;
mod search;
mod shared;
mod subst;
mod text;
mod textobj;
mod types;
mod vim;
mod visual;

pub use arcode_buffer::ops::IndentUnit;
pub use key::Key;
pub use readonly::READ_ONLY_MESSAGE;
pub use shared::VimShared;
pub use types::{
    CursorShape, Mode, Register, RegisterKind, SearchQuery, VimAction, VimCommand, VimContext,
};
pub use vim::Vim;
