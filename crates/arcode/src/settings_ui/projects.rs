//! Workspace: sessions, reopening projects, the projects folder and the
//! file tree (`[workspace]`).

use arcode_ui_kit::settings;
use gpui::{Context, Div, div, prelude::*};

use super::{SettingsPage, group_title};

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let w = settings(cx).workspace.clone();
    div()
        .child(group_title("Projects", cx))
        .child(page.bool_row(
            "Restore sessions",
            "Reopen a project's tabs, splits, cursors and panels.",
            "workspace.restore_session",
            w.restore_session,
            cx,
        ))
        .child(page.bool_row(
            "Reopen projects at start",
            "When Arcode starts without a path, reopen the projects that were open at quit.",
            "workspace.reopen_projects",
            w.reopen_projects,
            cx,
        ))
        .child(page.text_row(
            "Projects folder",
            "Offered for new projects and clones. Empty: next to the current project.",
            "workspace.projects_folder",
            cx,
        ))
        .child(page.bool_row(
            "Package banner",
            "Say when a project's packages are not installed (nothing installs by itself).",
            "workspace.package_banner",
            w.package_banner,
            cx,
        ))
        .child(group_title("File tree", cx))
        .child(page.bool_row(
            "Confirm deletes",
            "Ask before deleting files in the file tree.",
            "workspace.confirm_delete",
            w.confirm_delete,
            cx,
        ))
        .child(page.bool_row(
            "Watch files",
            "Show changes made outside Arcode (no polling).",
            "workspace.watch_files",
            w.watch_files,
            cx,
        ))
}
