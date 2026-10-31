//! Go, via `tree-sitter-go`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_go::LANGUAGE.into())
}

/// The Go language.
pub(crate) static GO: Language = Language::new("Go", "go")
    .with_extensions(&["go"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/go/highlights.scm"))
    .with_runnables(include_str!("../../queries/go/runnables.scm"))
    .with_outline(include_str!("../../queries/go/outline.scm"));
