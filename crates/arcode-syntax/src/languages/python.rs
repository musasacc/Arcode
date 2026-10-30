//! Python, via `tree-sitter-python`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_python::LANGUAGE.into())
}

/// The Python language.
pub(crate) static PYTHON: Language = Language::new("Python", "python")
    .with_extensions(&["py", "pyi", "pyw"])
    .with_file_names(&["SConstruct", "SConscript"])
    .with_line_comment("#")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/python/highlights.scm"))
    .with_runnables(include_str!("../../queries/python/runnables.scm"))
    .with_outline(include_str!("../../queries/python/outline.scm"));
