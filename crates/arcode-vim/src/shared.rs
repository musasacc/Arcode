//! State shared between the Vim instances of several editors, and the log
//! of edits Vim made.
//!
//! Vim keeps registers, macros and the last search per [`Vim`], but in an
//! editor with several files they belong to the whole session: a yank in
//! one file is pasted in another, `n` repeats a search started elsewhere.
//! An editor keeps one [`VimShared`] and swaps it into the [`Vim`] of the
//! focused buffer around each keystroke with [`Vim::swap_shared`] (an O(1)
//! swap, no copies).

use std::collections::HashMap;

use arcode_buffer::Edit;

use crate::key::Key;
use crate::registers::Registers;
use crate::types::SearchQuery;
use crate::vim::Vim;

/// Registers, macros and the last search, shared by every [`Vim`].
#[derive(Debug, Default)]
pub struct VimShared {
    registers: Registers,
    macros: HashMap<char, Vec<Key>>,
    search: Option<SearchQuery>,
    highlight: bool,
}

impl VimShared {
    /// Empty registers, no macros, no search.
    pub fn new() -> Self {
        VimShared::default()
    }

    /// The last search, if its matches should be highlighted (like
    /// [`Vim::search_query`] while this state is swapped out).
    pub fn search_query(&self) -> Option<&SearchQuery> {
        self.search.as_ref().filter(|_| self.highlight)
    }
}

impl Vim {
    /// Exchanges this Vim's registers, macros and last search (with its
    /// highlight flag) with `shared`. Call it before and after handing a
    /// key to this Vim so every editor sees the same registers.
    pub fn swap_shared(&mut self, shared: &mut VimShared) {
        std::mem::swap(&mut self.registers, &mut shared.registers);
        std::mem::swap(&mut self.macros, &mut shared.macros);
        std::mem::swap(&mut self.search, &mut shared.search);
        std::mem::swap(&mut self.highlight, &mut shared.highlight);
    }

    /// The edits Vim applied to the buffer since the last call, in
    /// application order (apply them one after another), including undo
    /// and redo. The editor forwards them to syntax, diagnostics and
    /// language servers.
    pub fn take_edits(&mut self) -> Vec<Edit> {
        std::mem::take(&mut self.edit_log)
    }
}

#[cfg(test)]
mod tests {
    use arcode_buffer::{Buffer, SelectionSet};

    use super::*;
    use crate::types::VimContext;

    #[test]
    fn registers_follow_the_shared_state() {
        let ctx = VimContext::default();
        let mut shared = VimShared::new();
        let (mut a, mut b) = (Buffer::new("one two"), Buffer::new("x"));
        let (mut sa, mut sb) = (SelectionSet::single(0), SelectionSet::single(0));
        let mut va = Vim::new();
        let mut vb = Vim::new();

        va.swap_shared(&mut shared);
        va.handle_keys("yw", &mut a, &mut sa, &ctx);
        va.swap_shared(&mut shared);

        vb.swap_shared(&mut shared);
        vb.handle_keys("P", &mut b, &mut sb, &ctx);
        vb.swap_shared(&mut shared);
        assert_eq!(b.to_string(), "one x");
        assert!(
            va.register('"').is_none(),
            "registers live in the shared state"
        );
    }

    #[test]
    fn edits_are_logged_in_order_with_undo() {
        let ctx = VimContext::default();
        let mut buffer = Buffer::new("abc");
        let mut sels = SelectionSet::single(0);
        let mut vim = Vim::new();
        vim.handle_keys("x", &mut buffer, &mut sels, &ctx);
        let edits = vim.take_edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range, 0..1);
        assert!(vim.take_edits().is_empty());
        vim.handle_keys("u", &mut buffer, &mut sels, &ctx);
        let undo = vim.take_edits();
        assert_eq!(undo.len(), 1);
        assert_eq!(undo[0].new_text, "a");
        assert_eq!(buffer.to_string(), "abc");
    }
}
