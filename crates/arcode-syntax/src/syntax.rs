//! [`Syntax`]: a parsed tree-sitter tree kept in sync with a rope.

use std::fmt;
use std::ops::Range;

use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Tree};

use crate::rope_text::chunk_from;
use crate::{InputEditData, Language};

/// A syntax tree for one document, updated incrementally.
///
/// Typical flow on every buffer change: call [`Syntax::edit`] for each edit
/// (in order), then [`Syntax::reparse`] once with the new text.
pub struct Syntax {
    language: &'static Language,
    parser: Parser,
    tree: Tree,
}

impl Syntax {
    /// Parses `text` with `language`'s grammar.
    ///
    /// Returns `None` for languages without a grammar (plain text) or if the
    /// grammar cannot be loaded.
    pub fn new(language: &'static Language, text: &Rope) -> Option<Syntax> {
        let grammar = language.grammar()?;
        let mut parser = Parser::new();
        parser.set_language(&grammar).ok()?;
        let tree = parse(&mut parser, text, None)?;
        Some(Syntax {
            language,
            parser,
            tree,
        })
    }

    /// The language this tree was parsed with.
    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// The current syntax tree.
    ///
    /// After [`Syntax::edit`] and before [`Syntax::reparse`] the tree's
    /// positions are adjusted but its structure is stale.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Records an edit on the tree so the next reparse can reuse unchanged
    /// subtrees. Call once per edit, in the order the edits were applied.
    pub fn edit(&mut self, edit: &InputEditData) {
        let edit: InputEdit = edit.into();
        self.tree.edit(&edit);
    }

    /// Incrementally reparses `text` (the text after all recorded edits).
    ///
    /// Reads the rope chunk by chunk; the text is never copied. On parser
    /// failure the old tree is kept.
    pub fn reparse(&mut self, text: &Rope) {
        if let Some(tree) = parse(&mut self.parser, text, Some(&self.tree)) {
            self.tree = tree;
        }
    }

    /// Like [`Syntax::reparse`], but also returns the byte ranges whose
    /// syntactic structure changed (e.g. to invalidate cached highlights).
    ///
    /// Computing the ranges costs extra time on large files; prefer
    /// [`Syntax::reparse`] when they are not needed.
    pub fn reparse_with_changed_ranges(&mut self, text: &Rope) -> Vec<Range<usize>> {
        let Some(tree) = parse(&mut self.parser, text, Some(&self.tree)) else {
            return Vec::new();
        };
        let changed = self
            .tree
            .changed_ranges(&tree)
            .map(|range| range.start_byte..range.end_byte)
            .collect();
        self.tree = tree;
        changed
    }

    /// Parses `text` from scratch, discarding the old tree.
    ///
    /// Use this when the whole document was replaced (e.g. reloaded from disk).
    pub fn reset(&mut self, text: &Rope) {
        if let Some(tree) = parse(&mut self.parser, text, None) {
            self.tree = tree;
        }
    }
}

impl fmt::Debug for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Syntax")
            .field("language", &self.language.name)
            .field("root", &self.tree.root_node())
            .finish_non_exhaustive()
    }
}

fn parse(parser: &mut Parser, text: &Rope, old_tree: Option<&Tree>) -> Option<Tree> {
    let mut read = |byte: usize, _position: tree_sitter::Point| chunk_from(text, byte);
    parser.parse_with_options(&mut read, old_tree, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{language_by_id, plain_text};

    #[test]
    fn plain_text_has_no_syntax() {
        assert!(Syntax::new(plain_text(), &Rope::from_str("hello")).is_none());
    }

    #[test]
    fn parses_rust() {
        let rust = language_by_id("rust").expect("rust");
        let syntax = Syntax::new(rust, &Rope::from_str("fn main() {}\n")).expect("parse");
        let root = syntax.tree().root_node();
        assert_eq!(root.kind(), "source_file");
        assert!(!root.has_error());
        assert_eq!(syntax.language().id, "rust");
    }

    #[test]
    fn empty_text_parses() {
        let json = language_by_id("json").expect("json");
        let syntax = Syntax::new(json, &Rope::new()).expect("parse");
        assert_eq!(syntax.tree().root_node().byte_range(), 0..0);
    }

    #[test]
    fn reset_reparses_from_scratch() {
        let rust = language_by_id("rust").expect("rust");
        let mut syntax = Syntax::new(rust, &Rope::from_str("fn a() {}")).expect("parse");
        syntax.reset(&Rope::from_str("struct S;"));
        let first = syntax.tree().root_node().child(0).map(|n| n.kind());
        assert_eq!(first, Some("struct_item"));
    }
}
