//! C++, via `tree-sitter-cpp`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_cpp::LANGUAGE.into())
}

/// The C++ language.
pub(crate) static CPP: Language = Language::new("C++", "cpp")
    .with_extensions(&[
        "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++", "ipp", "inl", "tpp",
    ])
    .with_line_comment("//")
    .with_block_comment("/*", "*/")
    .with_grammar(grammar)
    .with_highlights(concat!(
        include_str!("../../queries/c/highlights.scm"),
        include_str!("../../queries/cpp/highlights.scm")
    ))
    .with_runnables(concat!(
        include_str!("../../queries/c/runnables.scm"),
        include_str!("../../queries/cpp/runnables.scm")
    ))
    .with_outline(concat!(
        include_str!("../../queries/c/outline.scm"),
        include_str!("../../queries/cpp/outline.scm")
    ));
