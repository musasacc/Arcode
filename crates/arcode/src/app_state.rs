//! Persisted application state (recent projects, window placement) as a
//! GPUI global, saved through [`arcode_config::state`].

use std::path::{Path, PathBuf};

use arcode_config::{AppState, RecentProject, WindowState};
use gpui::{App, Global, Window, WindowBounds};

/// Global wrapper around the loaded [`AppState`] and where it is saved
/// (`None` keeps it in memory only, e.g. in tests).
pub struct PersistedState {
    /// The state.
    pub state: AppState,
    /// File the state is written to.
    pub path: Option<PathBuf>,
}

impl Global for PersistedState {}

/// Loads the state from disk (never fails) and installs it as a global.
pub fn init(cx: &mut App) {
    let path = arcode_config::paths::state_file();
    let mut state = match &path {
        Some(path) => AppState::load_from(path).unwrap_or_else(|err| {
            log::warn!("ignoring app state: {err:#}");
            AppState::default()
        }),
        None => AppState::default(),
    };
    let removed = state.prune_missing();
    if removed > 0 {
        log::info!("dropped {removed} recent project(s) that no longer exist");
    }
    cx.set_global(PersistedState { state, path });
}

/// Installs an in-memory state that is never written (tests).
pub fn init_in_memory(cx: &mut App) {
    cx.set_global(PersistedState {
        state: AppState::default(),
        path: None,
    });
}

/// The persisted state.
pub fn state(cx: &App) -> &AppState {
    &cx.global::<PersistedState>().state
}

/// Recent projects, most recent first.
pub fn recent_projects(cx: &App) -> &[RecentProject] {
    &state(cx).recent_projects
}

/// Records that `path` was opened and saves the state.
pub fn touch_recent(path: &Path, cx: &mut App) {
    cx.global_mut::<PersistedState>().state.touch_recent(path);
    save(cx);
}

/// Removes `path` from the recent list and saves the state.
pub fn remove_recent(path: &Path, cx: &mut App) {
    if cx.global_mut::<PersistedState>().state.remove_recent(path) {
        save(cx);
    }
}

/// Records that the palette command `label` was run and saves the state.
pub fn touch_recent_command(label: &str, cx: &mut App) {
    cx.global_mut::<PersistedState>()
        .state
        .touch_recent_command(label);
    save(cx);
}

/// Remembers the placement of `window` (saved on the next [`save`]).
pub fn record_window(window: &Window, cx: &mut App) {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
    };
    cx.global_mut::<PersistedState>().state.window = Some(WindowState {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    });
}

/// Writes the state to disk, logging failures.
pub fn save(cx: &App) {
    let persisted = cx.global::<PersistedState>();
    if let Some(path) = &persisted.path
        && let Err(err) = persisted.state.save_to(path)
    {
        log::warn!("could not save app state: {err:#}");
    }
}
