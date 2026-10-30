//! Rust, via `tree-sitter-rust`.

use std::sync::OnceLock;

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_rust::LANGUAGE.into())
}

/// The Rust language.
pub(crate) static RUST: Language = Language {
    name: "Rust",
    id: "rust",
    extensions: &["rs"],
    file_names: &[],
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    ts_language: grammar,
    highlights_query: include_str!("../../queries/rust/highlights.scm"),
    query: OnceLock::new(),
};
