//! JSON, via `tree-sitter-json`.

use std::sync::OnceLock;

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_json::LANGUAGE.into())
}

/// The JSON language.
pub(crate) static JSON: Language = Language {
    name: "JSON",
    id: "json",
    extensions: &["json"],
    file_names: &["flake.lock"],
    line_comment: None,
    block_comment: None,
    ts_language: grammar,
    highlights_query: include_str!("../../queries/json/highlights.scm"),
    query: OnceLock::new(),
};
