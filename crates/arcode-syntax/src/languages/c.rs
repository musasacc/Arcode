//! C, via `tree-sitter-c`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_c::LANGUAGE.into())
}

/// The C language.
pub(crate) static C: Language = Language::new("C", "c")
    .with_extensions(&["c", "h"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/c/highlights.scm"))
    .with_runnables(include_str!("../../queries/c/runnables.scm"))
    .with_outline(include_str!("../../queries/c/outline.scm"));
