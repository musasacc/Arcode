//! CSS, via `tree-sitter-css`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_css::LANGUAGE.into())
}

/// The CSS language.
pub(crate) static CSS: Language = Language::new("CSS", "css")
    .with_extensions(&["css"])
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/css/highlights.scm"));
