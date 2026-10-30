//! JavaScript and JavaScript React, via `tree-sitter-javascript`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_javascript::LANGUAGE.into())
}

/// The JavaScript language.
pub(crate) static JAVASCRIPT: Language = Language::new("JavaScript", "javascript")
    .with_extensions(&["js", "mjs", "cjs"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(concat!(
        include_str!("../../queries/javascript/highlights.scm"),
        include_str!("../../queries/javascript/highlights-js-only.scm"),
        include_str!("../../queries/jsx/highlights.scm")
    ))
    .with_runnables(include_str!("../../queries/javascript/runnables.scm"))
    .with_outline(include_str!("../../queries/javascript/outline.scm"));

/// The JavaScript React language.
pub(crate) static JAVASCRIPT_REACT: Language = Language::new("JavaScript React", "javascriptreact")
    .with_extensions(&["jsx"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(concat!(
        include_str!("../../queries/javascript/highlights.scm"),
        include_str!("../../queries/javascript/highlights-js-only.scm"),
        include_str!("../../queries/jsx/highlights.scm")
    ))
    .with_runnables(include_str!("../../queries/javascript/runnables.scm"))
    .with_outline(include_str!("../../queries/javascript/outline.scm"));
