//! TOML, via `tree-sitter-toml-ng`.

use std::sync::OnceLock;

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_toml_ng::LANGUAGE.into())
}

/// The TOML language (also lock files written in TOML).
pub(crate) static TOML: Language = Language {
    name: "TOML",
    id: "toml",
    extensions: &["toml"],
    file_names: &["Cargo.lock", "Pipfile", "poetry.lock", "uv.lock"],
    line_comment: Some("#"),
    block_comment: None,
    ts_language: grammar,
    highlights_query: include_str!("../../queries/toml/highlights.scm"),
    query: OnceLock::new(),
};
