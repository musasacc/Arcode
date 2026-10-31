//! Persisted application state (recent projects, window placement,
//! project sessions) as a GPUI global, saved through [`arcode_config::state`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use arcode_config::{AppState, ProjectSession, RecentProject, WindowState};
use gpui::{App, AppContext, Global, Window, WindowBounds};

/// Global wrapper around the loaded [`AppState`] and where it is saved
/// (`None` keeps it in memory only, e.g. in tests).
pub struct PersistedState {
    /// The state.
    pub state: AppState,
    /// File the state is written to.
    pub path: Option<PathBuf>,
    /// Orders writes: the generation of the newest text handed out, and
    /// (locked while writing) the generation last written, so an older
    /// background write never lands after a newer one.
    writes: Arc<WriteOrder>,
}

#[derive(Default)]
struct WriteOrder {
    next: AtomicU64,
    written: Mutex<u64>,
}

impl WriteOrder {
    /// Writes `text` unless a newer generation was written already.
    fn write(&self, generation: u64, path: &Path, text: &str) {
        let Ok(mut written) = self.written.lock() else {
            return;
        };
        if generation <= *written {
            return;
        }
        match AppState::write_toml(path, text) {
            Ok(()) => *written = generation,
            Err(err) => log::warn!("could not save app state: {err:#}"),
        }
    }
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
    cx.set_global(PersistedState {
        state,
        path,
        writes: Arc::default(),
    });
}

/// Installs an in-memory state that is never written (tests).
pub fn init_in_memory(cx: &mut App) {
    cx.set_global(PersistedState {
        state: AppState::default(),
        path: None,
        writes: Arc::default(),
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
    save_in_background(cx);
}

/// Removes `path` from the recent list and saves the state.
pub fn remove_recent(path: &Path, cx: &mut App) {
    if cx.global_mut::<PersistedState>().state.remove_recent(path) {
        save_in_background(cx);
    }
}

/// Records that the palette command `label` was run and saves the state.
pub fn touch_recent_command(label: &str, cx: &mut App) {
    cx.global_mut::<PersistedState>()
        .state
        .touch_recent_command(label);
    save_in_background(cx);
}

/// The placement of `window` in logical pixels.
pub fn window_state(window: &Window) -> WindowState {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
    };
    WindowState {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    }
}

/// Remembers the placement of `window` as the default for new windows
/// (saved on the next [`save`]).
pub fn record_window(window: &Window, cx: &mut App) {
    cx.global_mut::<PersistedState>().state.window = Some(window_state(window));
}

/// The saved session of `project`.
pub fn session(project: &Path, cx: &App) -> Option<ProjectSession> {
    state(cx).session(project).cloned()
}

/// Stores the session of `project` and saves the state.
pub fn set_session(project: &Path, session: ProjectSession, cx: &mut App) {
    store_session(project, session, cx);
    save(cx);
}

/// Stores the session of `project` without saving the state.
pub fn store_session(project: &Path, session: ProjectSession, cx: &mut App) {
    cx.global_mut::<PersistedState>()
        .state
        .set_session(project, session);
}

/// Replaces the list of projects reopened at the next start.
pub fn set_open_projects(projects: Vec<PathBuf>, cx: &mut App) {
    cx.global_mut::<PersistedState>().state.open_projects = projects;
}

/// Records that `project` is open in a window, so it is reopened at the
/// next start even if Arcode does not quit normally, and saves the state
/// in the background.
pub fn add_open_project(project: &Path, cx: &mut App) {
    let state = &mut cx.global_mut::<PersistedState>().state;
    if !state.open_projects.iter().any(|p| p == project) {
        state.add_open_project(project);
        save_in_background(cx);
    }
}

/// Turns the package banner off for `project` and saves the state in the
/// background.
pub fn hide_package_banner(project: &Path, cx: &mut App) {
    cx.global_mut::<PersistedState>()
        .state
        .hide_package_banner(project);
    save_in_background(cx);
}

/// Projects open when Arcode last quit.
pub fn open_projects(cx: &App) -> Vec<PathBuf> {
    state(cx).open_projects.clone()
}

/// The state as TOML with its write generation, or `None` when the state
/// is kept in memory only.
fn snapshot(cx: &App) -> Option<(Arc<WriteOrder>, u64, PathBuf, String)> {
    let persisted = cx.global::<PersistedState>();
    let path = persisted.path.clone()?;
    let text = match persisted.state.to_toml() {
        Ok(text) => text,
        Err(err) => {
            log::warn!("could not save app state: {err:#}");
            return None;
        }
    };
    let writes = persisted.writes.clone();
    let generation = writes.next.fetch_add(1, Ordering::Relaxed) + 1;
    Some((writes, generation, path, text))
}

/// Writes the state to disk now, logging failures. For quitting and
/// closing windows, where the write must be done before going on.
pub fn save(cx: &App) {
    if let Some((writes, generation, path, text)) = snapshot(cx) {
        writes.write(generation, &path, &text);
    }
}

/// Serializes the state now and writes it on the background executor, so
/// the UI thread never waits for the disk (used on frequent changes).
pub fn save_in_background(cx: &App) {
    if let Some((writes, generation, path, text)) = snapshot(cx) {
        cx.background_spawn(async move { writes.write(generation, &path, &text) })
            .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_write_never_replaces_a_newer_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");
        let order = WriteOrder::default();
        order.write(2, &path, "new = 1\n");
        order.write(1, &path, "old = 1\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new = 1\n");
        order.write(3, &path, "newer = 1\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer = 1\n");
    }
}
