//! User configuration, themes and persisted application state for Arcode.
//!
//! This crate is UI-agnostic (no GPUI). It provides:
//!
//! - [`paths`]: where configuration, themes and state live on disk, with
//!   `ARCODE_CONFIG_DIR` / `ARCODE_DATA_DIR` overrides for tests and portable
//!   installs.
//! - [`config`]: the user configuration (`config.toml`). Loading never fails:
//!   invalid values and unknown keys become warnings and the defaults are kept.
//! - [`theme`]: colors, themes (TOML) and a registry of built-in and user themes.
//! - [`state`]: persisted app state (recent projects, window, sessions), saved
//!   atomically.

pub mod config;
pub mod paths;
pub mod state;
pub mod theme;

mod fsutil;
mod lenient;

pub use config::{
    CompletionSettings, Config, DebugSettings, EditorSettings, FilesSettings, FirstRunChoices,
    GitSettings, InlineDiagnostics, Keymap, LanguageSettings, LspFeatures, LspServerSettings,
    NamedServer, PluginSettings, SearchSettings, SettingEdit, SettingValue, TerminalSettings,
    ThemeChoice, TodoSettings, UiSettings, WorkspaceSettings, apply_edits, keymap_edits,
    low_end_edits,
};
pub use state::{
    AppState, FileSession, LayoutSession, PaneSession, ProjectSession, RecentProject,
    SavedBreakpoint, SavedSelection, WindowState,
};
pub use theme::{
    Appearance, Color, ColorParseError, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, SyntaxStyle,
    TerminalColors, Theme, ThemeError, ThemeRegistry, UiColors,
};
