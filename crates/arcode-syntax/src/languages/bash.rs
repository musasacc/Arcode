//! Shell Script, via `tree-sitter-bash`.

use crate::Language;

fn grammar() -> Option<tree_sitter::Language> {
    Some(tree_sitter_bash::LANGUAGE.into())
}

/// The Shell Script language.
pub(crate) static BASH: Language = Language::new("Shell Script", "shellscript")
    .with_extensions(&["sh", "bash", "zsh", "ksh", "command"])
    .with_file_names(&[
        ".bashrc",
        ".bash_profile",
        ".bash_login",
        ".bash_logout",
        ".profile",
        ".zshrc",
        ".zshenv",
        ".zprofile",
        ".zlogin",
        "PKGBUILD",
        "APKBUILD",
    ])
    .with_line_comment("#")
    .with_grammar(grammar)
    .with_highlights(include_str!("../../queries/bash/highlights.scm"))
    .with_outline(include_str!("../../queries/bash/outline.scm"));
