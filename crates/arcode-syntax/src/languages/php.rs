//! PHP, via `tree-sitter-php (PHP with embedded HTML)`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_php::LANGUAGE_PHP.into())
}

/// The PHP language.
pub(crate) static PHP: Language = Language::new("PHP", "php")
    .with_extensions(&["php", "phtml", "php3", "php4", "php5", "phps"])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/php/highlights.scm"));
