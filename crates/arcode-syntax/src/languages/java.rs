//! Java, via `tree-sitter-java`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_java::LANGUAGE.into())
}

/// The Java language.
pub(crate) static JAVA: Language = Language::new("Java", "java")
    .with_extensions(&["java"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/java/highlights.scm"))
    .with_outline(include_str!("../../queries/java/outline.scm"));
