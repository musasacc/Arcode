//! Ruby, via `tree-sitter-ruby`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_ruby::LANGUAGE.into())
}

/// The Ruby language.
pub(crate) static RUBY: Language = Language::new("Ruby", "ruby")
    .with_extensions(&["rb", "rake", "gemspec", "ru", "rbw"])
    .with_file_names(&[
        "Gemfile",
        "Rakefile",
        "Guardfile",
        "Vagrantfile",
        "Podfile",
        "Brewfile",
        "Fastfile",
    ])
    .with_line_comment("#")
    .with_block_comment("=begin", "=end")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/ruby/highlights.scm"));
