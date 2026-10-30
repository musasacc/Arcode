//! Arcode: a fast, native IDE. This crate is the application shell
//! (workspace, title bar, sidebar, panes, panels, status bar, welcome
//! screen); `main.rs` only calls [`run`].

pub mod actions;
pub mod activity_bar;
pub mod app;
pub mod app_state;
pub mod bottom_panel;
pub mod file_icons;
pub mod finder;
pub mod fuzzy;
pub mod lsp_bridge;
pub mod pane;
pub mod path_prompt;
pub mod problems;
pub mod project;
pub mod prompt;
pub mod sidebar;
pub mod status_bar;
pub mod terminal_panel;
pub mod text_field;
pub mod theme_sync;
pub mod title_bar;
pub mod welcome;
pub mod workspace;

pub use app::run;
