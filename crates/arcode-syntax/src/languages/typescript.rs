//! TypeScript and TypeScript React, via `tree-sitter-typescript`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
}

fn grammar_tsx() -> Option<tree_sitter::Language> {
    Some(tree_sitter_typescript::LANGUAGE_TSX.into())
}

/// The TypeScript language.
pub(crate) static TYPESCRIPT: Language = Language::new("TypeScript", "typescript")
    .with_extensions(&["ts", "mts", "cts"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(concat!(
        include_str!("../../queries/javascript/highlights.scm"),
        include_str!("../../queries/typescript/highlights.scm")
    ))
    .with_runnables(include_str!("../../queries/javascript/runnables.scm"))
    .with_outline(concat!(
        include_str!("../../queries/javascript/outline.scm"),
        include_str!("../../queries/typescript/outline.scm")
    ));

/// The TypeScript React language.
pub(crate) static TSX: Language = Language::new("TypeScript React", "typescriptreact")
    .with_extensions(&["tsx"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar_tsx)
    .with_highlights(concat!(
        include_str!("../../queries/javascript/highlights.scm"),
        include_str!("../../queries/jsx/highlights.scm"),
        include_str!("../../queries/typescript/highlights.scm")
    ))
    .with_runnables(include_str!("../../queries/javascript/runnables.scm"))
    .with_outline(concat!(
        include_str!("../../queries/javascript/outline.scm"),
        include_str!("../../queries/typescript/outline.scm")
    ));
