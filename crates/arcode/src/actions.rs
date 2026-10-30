//! Application actions and their default key bindings.
//!
//! `secondary` means Cmd on macOS and Ctrl elsewhere. Workspace bindings
//! live in the `"Workspace"` key context (the root view), pane bindings in
//! `"Pane"`, so they never fire inside unrelated views.

use gpui::{App, KeyBinding, actions};

actions!(
    arcode,
    [
        /// Opens (or closes) the fuzzy file finder.
        ToggleFinder,
        /// Shows or hides the bottom panel.
        ToggleBottomPanel,
        /// Shows or hides the sidebar.
        ToggleSidebar,
        /// Splits the active pane to the right.
        SplitRight,
        /// Closes the active tab, or the active pane when it is empty.
        CloseTab,
        /// Prompts for a folder and opens it as the project.
        OpenFolder,
        /// Prompts for a parent folder and creates a new project in it.
        NewProject,
        /// Closes the current project.
        CloseProject,
        /// Opens the configuration file in an editor tab.
        OpenSettings,
        /// Runs the project's detected run command in the terminal.
        RunProject,
        /// Opens a new terminal in the bottom panel.
        NewTerminal,
        /// Moves focus to the terminal in the bottom panel.
        FocusTerminal,
        /// Quits the application.
        Quit,
    ]
);

/// Key context of the workspace root view.
pub const WORKSPACE_CONTEXT: &str = "Workspace";
/// Key context of a pane (tab strip + editor).
pub const PANE_CONTEXT: &str = "Pane";

/// Registers the application key bindings.
pub fn bind_keys(cx: &mut App) {
    let ws = Some(WORKSPACE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-p", ToggleFinder, ws),
        KeyBinding::new("secondary-j", ToggleBottomPanel, ws),
        KeyBinding::new("secondary-b", ToggleSidebar, ws),
        KeyBinding::new("secondary-\\", SplitRight, ws),
        KeyBinding::new("secondary-o", OpenFolder, ws),
        KeyBinding::new("secondary-,", OpenSettings, ws),
        KeyBinding::new("secondary-shift-`", NewTerminal, ws),
        // Linux (and some layouts elsewhere) report shift-` as `~` without
        // the shift modifier.
        KeyBinding::new("secondary-~", NewTerminal, ws),
        KeyBinding::new("ctrl-`", FocusTerminal, ws),
        KeyBinding::new("f5", RunProject, ws),
        KeyBinding::new("secondary-w", CloseTab, Some(PANE_CONTEXT)),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
}

/// Human-readable form of a `secondary-` shortcut for the current platform,
/// e.g. `secondary-p` becomes `Ctrl+P` (or `Cmd+P` on macOS).
pub fn shortcut_label(keys: &str) -> String {
    keys.split('-')
        .map(|part| match part {
            "secondary" if cfg!(target_os = "macos") => "Cmd".to_string(),
            "secondary" | "ctrl" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" if cfg!(target_os = "macos") => "Option".to_string(),
            "alt" => "Alt".to_string(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_use_platform_modifier() {
        let expected = if cfg!(target_os = "macos") {
            "Cmd+P"
        } else {
            "Ctrl+P"
        };
        assert_eq!(shortcut_label("secondary-p"), expected);
        assert!(shortcut_label("secondary-shift-`").ends_with("Shift+`"));
    }
}
