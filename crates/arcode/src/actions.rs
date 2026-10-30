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

actions!(
    git,
    [
        /// Shows the Source Control view.
        ShowSourceControl,
        /// Shows or hides the blame column in editors.
        ToggleBlame,
        /// Opens the branch picker (switch or create a branch).
        SwitchBranch,
        /// Commits the staged changes with the message in the Source
        /// Control view.
        GitCommit,
        /// Opens a diff of the active file against `HEAD`.
        OpenChanges,
        /// Moves to the next merge conflict in the active editor.
        NextConflict,
        /// Moves to the previous merge conflict in the active editor.
        PreviousConflict,
        /// Re-reads the repository status.
        RefreshGit,
        /// Creates a git repository in the project folder.
        InitRepository,
    ]
);

/// Key context of the workspace root view.
pub const WORKSPACE_CONTEXT: &str = "Workspace";
/// Key context of a pane (tab strip + editor).
pub const PANE_CONTEXT: &str = "Pane";

/// Human-readable names of the application commands, for a command
/// palette and menus. Each entry builds the action to dispatch.
pub fn commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    vec![
        ("File: Find File", Box::new(ToggleFinder)),
        ("File: Open Folder", Box::new(OpenFolder)),
        ("File: New Project", Box::new(NewProject)),
        ("File: Close Project", Box::new(CloseProject)),
        ("Preferences: Open Settings", Box::new(OpenSettings)),
        ("View: Toggle Panel", Box::new(ToggleBottomPanel)),
        ("View: Toggle Sidebar", Box::new(ToggleSidebar)),
        ("View: Split Right", Box::new(SplitRight)),
        ("Run: Run Project", Box::new(RunProject)),
        ("Terminal: New Terminal", Box::new(NewTerminal)),
        ("Terminal: Focus Terminal", Box::new(FocusTerminal)),
        (
            "Editor: Trigger Suggest",
            Box::new(arcode_editor::ShowCompletions),
        ),
        (
            "Editor: Trigger Parameter Hints",
            Box::new(arcode_editor::ShowSignatureHelp),
        ),
        ("Git: Show Source Control", Box::new(ShowSourceControl)),
        ("Git: Toggle Blame", Box::new(ToggleBlame)),
        ("Git: Switch Branch", Box::new(SwitchBranch)),
        ("Git: Commit", Box::new(GitCommit)),
        ("Git: Open Changes", Box::new(OpenChanges)),
        ("Git: Next Conflict", Box::new(NextConflict)),
        ("Git: Previous Conflict", Box::new(PreviousConflict)),
        ("Git: Refresh", Box::new(RefreshGit)),
        ("Git: Initialize Repository", Box::new(InitRepository)),
    ]
}

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
        KeyBinding::new("secondary-shift-g", ShowSourceControl, ws),
        KeyBinding::new("secondary-alt-b", ToggleBlame, ws),
        KeyBinding::new("alt-]", NextConflict, ws),
        KeyBinding::new("alt-[", PreviousConflict, ws),
        KeyBinding::new(
            "secondary-enter",
            GitCommit,
            Some(crate::git::panel::CONTEXT),
        ),
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
            other => {
                let mut chars = other.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            }
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

    #[test]
    fn commands_have_unique_names() {
        let names: Vec<_> = commands().into_iter().map(|(n, _)| n).collect();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        assert!(names.contains(&"Git: Toggle Blame"));
    }
}
