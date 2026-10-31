//! JSON, via `tree-sitter-json`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_json::LANGUAGE.into())
}

/// The JSON language.
pub(crate) static JSON: Language = Language::new("JSON", "json")
    .with_extensions(&["json"])
    .with_file_names(&["flake.lock"])
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/json/highlights.scm"));
