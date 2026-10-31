//! The Packages activity: every manifest of the project (Cargo, npm,
//! pnpm, Yarn, Bun, uv, Poetry, Pipenv, pip, Go, Composer, Bundler) with
//! its declared dependencies, read offline from manifests and lockfiles
//! (`arcode_project::deps`). Add, Remove, Update and Update all run the
//! manager's own command in a terminal tab; "Check for updates" (the only
//! network use) runs only on request.

mod badges;
mod forms;
pub mod rows;
mod store;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod review_tests;
mod view;
mod view_actions;
mod view_rows;

use gpui::{App, actions};

pub use forms::AddForm;
pub use store::{CheckState, PackagesStore, PackagesStoreEvent, not_installed};
pub use view::{PackagesEvent, PackagesRequest, PackagesView};
pub use view_actions::DepCommand;

actions!(
    packages,
    [
        /// Shows the Packages view in the sidebar.
        ShowPackages,
        /// Opens the Add form for the manifest of the active file.
        AddPackage,
        /// Updates every package of the manifest of the active file.
        UpdateAllPackages,
        /// Looks for newer versions of every manifest's packages (uses the
        /// network).
        CheckPackageUpdates,
        /// Reads the project's manifests again.
        RefreshPackages,
    ]
);

/// Palette entries.
pub fn commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    vec![
        ("Packages: Show Packages", Box::new(ShowPackages)),
        ("Packages: Add Package", Box::new(AddPackage)),
        ("Packages: Update All", Box::new(UpdateAllPackages)),
        ("Packages: Check for Updates", Box::new(CheckPackageUpdates)),
        ("Packages: Refresh", Box::new(RefreshPackages)),
    ]
}

/// Key bindings of the Add form.
pub fn bind_keys(cx: &mut App) {
    forms::bind_keys(cx);
}
