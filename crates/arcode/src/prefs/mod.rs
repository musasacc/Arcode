//! Preferences: the Settings page, the theme and font pickers (with live
//! preview), the first-run dialog, the interactive tour and low-end mode.
//!
//! Every change made here is written to `config.toml` in place
//! ([`store::write_settings`]) and applied from the file, so the file stays
//! the single source of truth.

pub mod first_run;
mod first_run_choices;
pub mod frames;
pub mod pickers;
#[cfg(test)]
mod review_settings;
pub mod store;
pub mod tour;

use gpui::{App, Entity, KeyBinding, Task, actions};

use crate::list_picker::ListPicker;

actions!(
    prefs,
    [
        /// Opens `config.toml` in an editor tab.
        OpenSettingsFile,
        /// Picks the color theme (with a live preview).
        SelectTheme,
        /// Picks the editor font family.
        SelectEditorFont,
        /// Picks the UI font family.
        SelectUiFont,
        /// Picks the terminal font family.
        SelectTerminalFont,
        /// Turns low-end mode on or off.
        ToggleLowEndMode,
        /// Starts the interactive tour of the workspace.
        StartTour,
        /// Shows the first-run setup dialog again.
        ShowWelcomeSetup,
    ]
);

/// Palette entries.
pub fn commands() -> Vec<(&'static str, Box<dyn gpui::Action>)> {
    vec![
        (
            "Preferences: Open Settings File (config.toml)",
            Box::new(OpenSettingsFile),
        ),
        ("Preferences: Select Theme", Box::new(SelectTheme)),
        (
            "Preferences: Select Editor Font",
            Box::new(SelectEditorFont),
        ),
        ("Preferences: Select UI Font", Box::new(SelectUiFont)),
        (
            "Preferences: Select Terminal Font",
            Box::new(SelectTerminalFont),
        ),
        (
            "Preferences: Toggle Low-End Mode",
            Box::new(ToggleLowEndMode),
        ),
        ("Help: Start Tour", Box::new(StartTour)),
        ("Help: Show Welcome Setup", Box::new(ShowWelcomeSetup)),
    ]
}

/// Key bindings (workspace context).
pub fn bind_keys(cx: &mut App) {
    let ws = Some(crate::actions::WORKSPACE_CONTEXT);
    cx.bind_keys([KeyBinding::new("secondary-k secondary-t", SelectTheme, ws)]);
    tour::bind_keys(cx);
    first_run::bind_keys(cx);
}

/// What the preferences picker chooses.
#[derive(Clone, Debug, PartialEq)]
pub enum PrefsPick {
    /// `ui.theme`, `ui.dark_theme` or `ui.light_theme`.
    Theme(pickers::ThemeSlot, Vec<pickers::ThemeOption>),
    /// A font family setting.
    Font(pickers::FontTarget, Vec<String>),
}

/// The workspace's preferences state.
#[derive(Default)]
pub struct PrefsParts {
    /// The Settings page (kept while its tab is open).
    pub page: Option<Entity<crate::settings_ui::SettingsPage>>,
    /// The open theme or font picker.
    pub picker: Option<(Entity<ListPicker>, PrefsPick)>,
    /// Settings before a font preview, restored when the picker closes
    /// without a choice.
    pub preview_base: Option<arcode_config::Config>,
    /// Element bounds the tour points at.
    pub anchors: tour::Anchors,
    /// The running tour.
    pub tour: Option<tour::Tour>,
    /// Focus of the tour overlay while it runs.
    pub tour_focus: Option<gpui::FocusHandle>,
    /// The first-run dialog, while shown.
    pub first_run: Option<Entity<first_run::FirstRunDialog>>,
    /// A diagnostics-count refresh waiting for the low-end debounce.
    pub counts_task: Option<Task<()>>,
    /// Sidebar and bottom panel slide animations.
    pub anim: PanelAnim,
}

/// State of the sidebar and bottom panel slide animations. Each toggle
/// starts a new animation (a new element id); while one runs, a closing
/// panel stays drawn and the panel is clipped by an animated wrapper.
#[derive(Default)]
pub struct PanelAnim {
    /// The sidebar's animation.
    pub sidebar: Slide,
    /// The bottom panel's animation.
    pub panel: Slide,
}

/// One panel's slide.
#[derive(Default)]
pub struct Slide {
    /// Toggles so far (the animation id).
    pub generation: usize,
    /// Whether it slides open (else closed).
    pub opening: bool,
    /// Ends the animation (drops the wrapper) after its duration.
    pub task: Option<Task<()>>,
}

impl Slide {
    /// Whether an animation is running.
    pub fn running(&self) -> bool {
        self.task.is_some()
    }
}
