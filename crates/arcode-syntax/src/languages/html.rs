//! HTML, via `tree-sitter-html`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_html::LANGUAGE.into())
}

/// The HTML language.
pub(crate) static HTML: Language = Language::new("HTML", "html")
    .with_extensions(&["html", "htm", "xhtml"])
    .with_block_comment("<!--", "-->")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/html/highlights.scm"));
