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
        /// Opens the Settings page.
        OpenSettings,
        /// Runs the project's detected run command in the terminal.
        RunProject,
        /// Opens a new terminal in the bottom panel.
        NewTerminal,
        /// Moves focus to the terminal in the bottom panel.
        FocusTerminal,
        /// Quits the application.
        Quit,
        /// Opens a new, empty window.
        NewWindow,
        /// Prompts for a folder and opens it in a new window.
        OpenFolderInNewWindow,
        /// Closes the window (asking about unsaved changes).
        CloseWindow,
        /// Shows the active file in the file tree.
        RevealActiveFile,
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

actions!(
    navigation,
    [
        /// Goes back to where the cursor was before the last jump.
        GoBack,
        /// Goes forward again after going back.
        GoForward,
        /// Lists the symbols of the active file.
        ShowDocumentSymbols,
        /// Searches the symbols of the whole workspace.
        ShowWorkspaceSymbols,
    ]
);

actions!(
    search,
    [
        /// Shows the Search view (find in the whole project).
        ShowProjectSearch,
        /// Shows the Search view with the replace field.
        ReplaceInFiles,
        /// Opens the find bar of the active editor.
        Find,
        /// Opens the find bar of the active editor with the replace field.
        Replace,
        /// Selects the next match of the find bar.
        FindNext,
        /// Selects the previous match of the find bar.
        FindPrevious,
        /// Shows the TODO view in the sidebar.
        ShowTodos,
        /// Shows the TODO list in the bottom panel.
        ShowTodoPanel,
        /// Scans the project for TODO comments again.
        RefreshTodos,
    ]
);

actions!(
    palette,
    [
        /// Opens (or closes) the command palette.
        ToggleCommandPalette,
    ]
);

/// Key context of the workspace root view.
pub const WORKSPACE_CONTEXT: &str = "Workspace";

/// Key context of the root view while a modal overlay (the first-run
/// dialog, the tour) is up: no `Workspace` binding matches then.
pub const MODAL_CONTEXT: &str = "WorkspaceModal";

/// Key context of a pane (tab strip + editor).
pub const PANE_CONTEXT: &str = "Pane";

/// Human-readable names of the application commands, for a command
/// palette and menus. Each entry builds the action to dispatch.
pub fn commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    let mut list: Vec<(&'static str, Box<dyn gpui::Action>)> = vec![
        ("File: Find File", Box::new(ToggleFinder)),
        ("File: Open Folder", Box::new(OpenFolder)),
        ("File: New Project", Box::new(NewProject)),
        ("File: Close Project", Box::new(CloseProject)),
        ("File: New Window", Box::new(NewWindow)),
        (
            "File: Open Folder in New Window",
            Box::new(OpenFolderInNewWindow),
        ),
        ("File: Close Window", Box::new(CloseWindow)),
        (
            "File: Reveal Active File in Explorer",
            Box::new(RevealActiveFile),
        ),
        (
            "Explorer: New File",
            Box::new(crate::sidebar::file_tree::NewFile),
        ),
        (
            "Explorer: New Folder",
            Box::new(crate::sidebar::file_tree::NewFolder),
        ),
        (
            "Explorer: Rename",
            Box::new(crate::sidebar::file_tree::Rename),
        ),
        (
            "Explorer: Delete",
            Box::new(crate::sidebar::file_tree::Delete),
        ),
        (
            "Explorer: Copy Path",
            Box::new(crate::sidebar::file_tree::CopyPath),
        ),
        (
            "Explorer: Copy Relative Path",
            Box::new(crate::sidebar::file_tree::CopyRelativePath),
        ),
        (
            "Explorer: Reveal in File Manager",
            Box::new(crate::sidebar::file_tree::RevealInFileManager),
        ),
        (
            "Explorer: Open in Terminal",
            Box::new(crate::sidebar::file_tree::OpenInTerminal),
        ),
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
        ("Editor: Show Hover", Box::new(arcode_editor::ShowHover)),
        ("Go to Definition", Box::new(arcode_editor::GoToDefinition)),
        (
            "Go to Type Definition",
            Box::new(arcode_editor::GoToTypeDefinition),
        ),
        (
            "Go to Implementation",
            Box::new(arcode_editor::GoToImplementation),
        ),
        (
            "Find All References",
            Box::new(arcode_editor::FindAllReferences),
        ),
        ("Go to Symbol in File", Box::new(ShowDocumentSymbols)),
        ("Go to Symbol in Workspace", Box::new(ShowWorkspaceSymbols)),
        ("Go Back", Box::new(GoBack)),
        ("Go Forward", Box::new(GoForward)),
        ("Git: Show Source Control", Box::new(ShowSourceControl)),
        ("Git: Toggle Blame", Box::new(ToggleBlame)),
        ("Git: Switch Branch", Box::new(SwitchBranch)),
        ("Git: Commit", Box::new(GitCommit)),
        ("Git: Open Changes", Box::new(OpenChanges)),
        ("Git: Next Conflict", Box::new(NextConflict)),
        ("Git: Previous Conflict", Box::new(PreviousConflict)),
        ("Git: Refresh", Box::new(RefreshGit)),
        ("Git: Initialize Repository", Box::new(InitRepository)),
        ("View: Command Palette", Box::new(ToggleCommandPalette)),
        ("Search: Find in Files", Box::new(ShowProjectSearch)),
        ("Search: Replace in Files", Box::new(ReplaceInFiles)),
        ("Search: Find", Box::new(Find)),
        ("Search: Replace", Box::new(Replace)),
        ("Search: Find Next", Box::new(FindNext)),
        ("Search: Find Previous", Box::new(FindPrevious)),
        ("TODO: Show TODOs", Box::new(ShowTodos)),
        ("TODO: Show TODO Panel", Box::new(ShowTodoPanel)),
        ("TODO: Rescan Project", Box::new(RefreshTodos)),
        ("Application: Quit", Box::new(Quit)),
    ];
    list.extend(crate::debugger::actions::commands());
    list.extend(crate::languages::actions::commands());
    list.extend(crate::prefs::commands());
    list
}

/// Editor commands for the palette (they run in the focused editor).
pub fn editor_commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    use arcode_editor as e;
    vec![
        ("Editor: Undo", Box::new(e::Undo)),
        ("Editor: Redo", Box::new(e::Redo)),
        ("Editor: Cut", Box::new(e::Cut)),
        ("Editor: Copy", Box::new(e::Copy)),
        ("Editor: Paste", Box::new(e::Paste)),
        ("Editor: Select All", Box::new(e::SelectAll)),
        (
            "Editor: Select Next Occurrence",
            Box::new(e::SelectNextOccurrence),
        ),
        (
            "Editor: Select All Occurrences",
            Box::new(e::SelectAllOccurrences),
        ),
        ("Editor: Cursor Undo", Box::new(e::UndoCursor)),
        ("Editor: Add Cursor Above", Box::new(e::AddCursorAbove)),
        ("Editor: Add Cursor Below", Box::new(e::AddCursorBelow)),
        (
            "Editor: Toggle Line Comment",
            Box::new(e::ToggleLineComment),
        ),
        ("Editor: Move Line Up", Box::new(e::MoveLineUp)),
        ("Editor: Move Line Down", Box::new(e::MoveLineDown)),
        ("Editor: Duplicate Line", Box::new(e::DuplicateLine)),
        ("Editor: Go to Start of File", Box::new(e::MoveDocStart)),
        ("Editor: Go to End of File", Box::new(e::MoveDocEnd)),
        ("File: Save", Box::new(e::Save)),
        ("Editor: Code Actions", Box::new(e::ShowCodeActions)),
        ("Editor: Refactor", Box::new(e::Refactor)),
        ("Editor: Rename Symbol", Box::new(e::RenameSymbol)),
        ("Editor: Format Document", Box::new(e::FormatDocument)),
        ("Editor: Format Selection", Box::new(e::FormatSelection)),
        ("Editor: Toggle Inlay Hints", Box::new(e::ToggleInlayHints)),
        ("Editor: Toggle Vim Mode", Box::new(e::ToggleVimMode)),
        ("Editor: Show Context Menu", Box::new(e::ShowContextMenu)),
    ]
}

/// Bindings of search, the find bar, the TODO list and the palette.
fn bind_search_keys(cx: &mut App) {
    let ws = Some(WORKSPACE_CONTEXT);
    let editor = Some(arcode_editor::KEY_CONTEXT);
    let pane = Some(PANE_CONTEXT);
    let replace = if cfg!(target_os = "macos") {
        "cmd-alt-f"
    } else {
        "ctrl-h"
    };
    cx.bind_keys([
        KeyBinding::new("secondary-shift-p", ToggleCommandPalette, ws),
        KeyBinding::new("f1", ToggleCommandPalette, ws),
        KeyBinding::new("secondary-shift-f", ShowProjectSearch, ws),
        KeyBinding::new("secondary-shift-h", ReplaceInFiles, ws),
        KeyBinding::new("secondary-f", Find, editor),
        KeyBinding::new("secondary-f", Find, pane),
        KeyBinding::new(replace, Replace, editor),
        KeyBinding::new(replace, Replace, pane),
        KeyBinding::new("f3", FindNext, pane),
        KeyBinding::new("shift-f3", FindPrevious, pane),
    ]);
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
        KeyBinding::new("secondary-shift-n", NewWindow, ws),
        KeyBinding::new("secondary-shift-w", CloseWindow, ws),
        KeyBinding::new("secondary-k secondary-r", RevealActiveFile, ws),
        KeyBinding::new("secondary-,", OpenSettings, ws),
        KeyBinding::new("secondary-shift-`", NewTerminal, ws),
        // Linux (and some layouts elsewhere) report shift-` as `~` without
        // the shift modifier.
        KeyBinding::new("secondary-~", NewTerminal, ws),
        KeyBinding::new("ctrl-`", FocusTerminal, ws),
        KeyBinding::new("secondary-f5", RunProject, ws),
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
    bind_navigation_keys(cx);
    bind_search_keys(cx);
    crate::debugger::actions::bind_keys(cx);
    crate::debugger::bp_dialog::bind_keys(cx);
    crate::debugger::panel::bind_keys(cx);
    crate::text_prompt::bind_keys(cx);
    crate::languages::actions::bind_keys(cx);
    crate::prefs::bind_keys(cx);
}

/// Navigation bindings. They live in the editor (and empty pane) contexts
/// so keys like Alt+Left and Ctrl+T still reach the terminal.
fn bind_navigation_keys(cx: &mut App) {
    let editor = Some(arcode_editor::KEY_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("secondary-shift-o", ShowDocumentSymbols, editor),
        KeyBinding::new("secondary-t", ShowWorkspaceSymbols, editor),
        KeyBinding::new("secondary-t", ShowWorkspaceSymbols, Some(PANE_CONTEXT)),
    ];
    if cfg!(target_os = "macos") {
        bindings.extend([
            KeyBinding::new("ctrl--", GoBack, editor),
            KeyBinding::new("ctrl-shift--", GoForward, editor),
            KeyBinding::new("ctrl-_", GoForward, editor),
        ]);
    } else {
        bindings.extend([
            KeyBinding::new("alt-left", GoBack, editor),
            KeyBinding::new("alt-right", GoForward, editor),
        ]);
    }
    cx.bind_keys(bindings);
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
