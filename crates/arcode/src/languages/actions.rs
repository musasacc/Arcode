//! Actions of the language, line ending and indentation pickers, the
//! Languages page and the package banner, with their palette names and
//! key bindings.

use gpui::{App, KeyBinding, actions};

use crate::actions::WORKSPACE_CONTEXT;

actions!(
    languages,
    [
        /// Picks the language of the active file.
        SelectLanguageMode,
        /// Picks the line ending (LF / CRLF) of the active file.
        ChangeLineEnding,
        /// Picks the indentation (spaces or tabs, size) of the active file.
        ChangeIndentation,
        /// Rewrites the active file's leading tabs as spaces.
        ConvertIndentationToSpaces,
        /// Rewrites the active file's leading spaces as tabs.
        ConvertIndentationToTabs,
        /// Guesses the active file's indentation from its content again.
        DetectIndentation,
        /// Shows the Languages page (language packs) in the sidebar.
        ShowLanguages,
        /// Runs the install command of the project's missing packages in a
        /// terminal.
        InstallPackages,
        /// Looks for missing packages again and shows the banner (even if
        /// it was dismissed).
        CheckPackages,
    ]
);

/// Palette entries.
pub fn commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    vec![
        ("Editor: Change Language Mode", Box::new(SelectLanguageMode)),
        (
            "Editor: Change End of Line Sequence",
            Box::new(ChangeLineEnding),
        ),
        ("Editor: Change Indentation", Box::new(ChangeIndentation)),
        (
            "Editor: Convert Indentation to Spaces",
            Box::new(ConvertIndentationToSpaces),
        ),
        (
            "Editor: Convert Indentation to Tabs",
            Box::new(ConvertIndentationToTabs),
        ),
        (
            "Editor: Detect Indentation from Content",
            Box::new(DetectIndentation),
        ),
        ("Languages: Show Language Packs", Box::new(ShowLanguages)),
        (
            "Packages: Install Missing Packages",
            Box::new(InstallPackages),
        ),
        (
            "Packages: Check for Missing Packages",
            Box::new(CheckPackages),
        ),
    ]
}

/// Ctrl+K M (Cmd+K M) changes the language mode, as in other editors.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "secondary-k m",
        SelectLanguageMode,
        Some(WORKSPACE_CONTEXT),
    )]);
}
