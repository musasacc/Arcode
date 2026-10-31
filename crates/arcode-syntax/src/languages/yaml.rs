//! YAML, via `tree-sitter-yaml`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_yaml::LANGUAGE.into())
}

/// The YAML language.
pub(crate) static YAML: Language = Language::new("YAML", "yaml")
    .with_extensions(&["yaml", "yml"])
    .with_file_names(&[".clang-format", ".clang-tidy", ".clangd"])
    .with_line_comment("#")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/yaml/highlights.scm"));
