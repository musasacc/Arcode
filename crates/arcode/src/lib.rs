//! Arcode: a fast, native IDE. This crate is the application shell
//! (workspace, title bar, sidebar, panes, panels, status bar, welcome
//! screen); `main.rs` only calls [`run`].

pub mod actions;
pub mod activity_bar;
pub mod app;
pub mod app_state;
pub mod bottom_panel;
pub mod commands;
pub mod debugger;
pub mod file_icons;
pub mod find_bar;
pub mod finder;
pub mod fuzzy;
pub mod git;
pub mod languages;
pub mod list_picker;
pub mod lsp_bridge;
pub mod navigation;
pub mod new_project;
pub mod pane;
pub mod path_prompt;
pub mod prefs;
pub mod problems;
pub mod project;
pub mod prompt;
pub mod refactor;
pub mod search_view;
pub mod search_widgets;
pub mod settings_ui;
pub mod sidebar;
pub mod status_bar;
pub mod terminal_panel;
#[cfg(test)]
pub(crate) mod test_support;
pub mod text_field;
pub mod text_prompt;
pub mod theme_sync;
pub mod title_bar;
pub mod todo;
pub mod vim_glue;
pub mod welcome;
pub mod windows;
pub mod workspace;

pub use app::run;
