//! TOML, via `tree-sitter-toml-ng`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_toml_ng::LANGUAGE.into())
}

/// The TOML language (also lock files written in TOML).
pub(crate) static TOML: Language = Language::new("TOML", "toml")
    .with_extensions(&["toml"])
    .with_file_names(&["Cargo.lock", "Pipfile", "poetry.lock", "uv.lock"])
    .with_line_comment("#")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/toml/highlights.scm"));
