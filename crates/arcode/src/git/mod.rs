//! Git integration of the workspace: the repository store, editor
//! decorations (change bars, blame, conflicts), the source control view,
//! the branch picker and diff tabs. Library work lives in `arcode-git`;
//! everything here only arranges it and runs it off the main thread.

pub mod blame_text;
pub mod branch_picker;
pub mod diff_view;
pub mod editor;
pub mod hunk_popover;
pub mod panel;
#[cfg(test)]
mod review_blame_text;
#[cfg(test)]
mod review_watcher;
pub mod store;
pub mod watcher;

pub use editor::{GitEditors, GitEditorsEvent};
pub use store::{GitEvent, GitSnapshot, GitStore};
