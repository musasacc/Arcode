//! The `[workspace]` section of `config.toml`: sessions, windows and the
//! file tree.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[workspace]`: what Arcode remembers between runs and how the file tree
/// behaves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSettings {
    /// Reopen a project's tabs, splits, cursors and panels when it is
    /// opened again.
    pub restore_session: bool,
    /// Reopen the projects (one window each) that were open when Arcode
    /// quit, when it starts without a path.
    pub reopen_projects: bool,
    /// Ask before deleting files from the file tree.
    pub confirm_delete: bool,
    /// Follow file-system changes in the file tree (the expanded folders
    /// are watched; nothing is polled).
    pub watch_files: bool,
    /// Folder offered for new projects. Unset: the parent of the current
    /// project, else the home folder.
    pub projects_folder: Option<String>,
    /// Show a banner when the project's packages (`node_modules`, a Python
    /// virtualenv, Go modules, ...) are not installed. Nothing is installed
    /// unless you click its button.
    pub package_banner: bool,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            restore_session: true,
            reopen_projects: true,
            confirm_delete: true,
            watch_files: true,
            projects_folder: None,
            package_banner: true,
        }
    }
}

impl_section!(WorkspaceSettings {
    restore_session,
    reopen_projects,
    confirm_delete,
    watch_files,
    projects_folder,
    package_banner,
});

impl WorkspaceSettings {
    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        if self
            .projects_folder
            .as_deref()
            .is_some_and(|f| f.trim().is_empty())
        {
            warnings.push("`workspace.projects_folder` is empty (ignored)".to_owned());
            self.projects_folder = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn workspace_section_parses_and_validates() {
        let (config, warnings) = Config::from_toml_str(
            "[workspace]\nrestore_session = false\nwatch_files = false\nprojects_folder = \" \"\n",
        );
        assert!(!config.workspace.restore_session);
        assert!(!config.workspace.watch_files);
        assert!(config.workspace.reopen_projects);
        assert!(config.workspace.confirm_delete);
        assert_eq!(config.workspace.projects_folder, None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let (config, _) = Config::from_toml_str("[workspace]\nprojects_folder = \"~/code\"\n");
        assert_eq!(config.workspace.projects_folder.as_deref(), Some("~/code"));
    }
}
