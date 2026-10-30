//! Markdown, via `tree-sitter-md (block grammar; inline markup is not parsed)`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_md::LANGUAGE.into())
}

/// The Markdown language.
pub(crate) static MARKDOWN: Language = Language::new("Markdown", "markdown")
    .with_extensions(&["md", "markdown", "mkd", "mdown"])
    .with_block_comment("<!--", "-->")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/markdown/highlights.scm"))
    .with_outline(include_str!("../../queries/markdown/outline.scm"));
