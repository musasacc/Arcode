//! Rust, via `tree-sitter-rust`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_rust::LANGUAGE.into())
}

/// The Rust language.
pub(crate) static RUST: Language = Language::new("Rust", "rust")
    .with_extensions(&["rs"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/rust/highlights.scm"))
    .with_runnables(include_str!("../../queries/rust/runnables.scm"))
    .with_outline(include_str!("../../queries/rust/outline.scm"));
