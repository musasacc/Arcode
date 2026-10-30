//! Persisted application state (recent projects, window placement) as a
//! GPUI global, saved through [`arcode_config::state`].

use std::path::Path;

use arcode_config::{AppState, RecentProject, WindowState};
use gpui::{App, Global, Window, WindowBounds};

/// Global wrapper around the loaded [`AppState`].
pub struct PersistedState(pub AppState);

impl Global for PersistedState {}

/// Loads the state from disk (never fails) and installs it as a global.
pub fn init(cx: &mut App) {
    let mut state = AppState::load();
    let removed = state.prune_missing();
    if removed > 0 {
        log::info!("dropped {removed} recent project(s) that no longer exist");
    }
    cx.set_global(PersistedState(state));
}

/// The persisted state.
pub fn state(cx: &App) -> &AppState {
    &cx.global::<PersistedState>().0
}

/// Recent projects, most recent first.
pub fn recent_projects(cx: &App) -> &[RecentProject] {
    &state(cx).recent_projects
}

/// Records that `path` was opened and saves the state.
pub fn touch_recent(path: &Path, cx: &mut App) {
    cx.global_mut::<PersistedState>().0.touch_recent(path);
    save(cx);
}

/// Removes `path` from the recent list and saves the state.
pub fn remove_recent(path: &Path, cx: &mut App) {
    if cx.global_mut::<PersistedState>().0.remove_recent(path) {
        save(cx);
    }
}

/// Remembers the placement of `window` (saved on the next [`save`]).
pub fn record_window(window: &Window, cx: &mut App) {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
    };
    cx.global_mut::<PersistedState>().0.window = Some(WindowState {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    });
}

/// Writes the state to disk, logging failures.
pub fn save(cx: &App) {
    if let Err(err) = state(cx).save() {
        log::warn!("could not save app state: {err:#}");
    }
}
