//! Persisted application state (`state.toml`): recent projects, window
//! geometry and per-project editor sessions.
//!
//! Unlike the configuration, this file is written by Arcode itself. It is
//! saved atomically so a crash never leaves a truncated file behind.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::fsutil::write_atomic;
use crate::paths;

/// Maximum number of entries kept in [`AppState::recent_projects`].
pub const MAX_RECENT_PROJECTS: usize = 20;

/// A project shown on the welcome screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    /// Project root folder.
    pub path: PathBuf,
    /// Display name (the folder name).
    pub name: String,
    /// Last time the project was opened, in seconds since the Unix epoch.
    pub last_opened: u64,
}

/// Main window placement, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Whether the window was maximized.
    pub maximized: bool,
}

/// Open editors of one project.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSession {
    /// Open files, in tab order.
    pub open_files: Vec<PathBuf>,
    /// The focused file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_file: Option<PathBuf>,
    /// Cursor `(line, column)` per file path (zero-based, column in chars).
    pub cursors: BTreeMap<String, (usize, usize)>,
}

/// Everything Arcode remembers between runs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// The first-run flow has been completed.
    pub first_run_complete: bool,
    /// Most recently opened first, at most [`MAX_RECENT_PROJECTS`].
    pub recent_projects: Vec<RecentProject>,
    /// Projects open when Arcode last exited (restored on start).
    pub open_projects: Vec<PathBuf>,
    /// Main window placement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
    /// Editor sessions keyed by project path (see [`AppState::session_key`]).
    pub sessions: BTreeMap<String, ProjectSession>,
}

impl AppState {
    /// Loads state from `path`. A missing file yields the default state.
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        toml::from_str(&source).with_context(|| format!("parsing {}", path.display()))
    }

    /// Loads [`paths::state_file`]; any problem is logged and the default
    /// state is returned, so a corrupt state file never blocks startup.
    pub fn load() -> Self {
        let Some(path) = paths::state_file() else {
            return Self::default();
        };
        Self::load_from(&path).unwrap_or_else(|err| {
            log::warn!("ignoring app state: {err:#}");
            Self::default()
        })
    }

    /// Saves to `path` atomically (temporary file + rename), creating
    /// parent directories as needed.
    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        let text = toml::to_string(self).context("serializing app state")?;
        write_atomic(path, text.as_bytes()).with_context(|| format!("writing {}", path.display()))
    }

    /// Saves to [`paths::state_file`].
    pub fn save(&self) -> anyhow::Result<()> {
        let path = paths::state_file().context("no data directory available")?;
        self.save_to(&path)
    }

    /// Records that `path` was opened now (see [`AppState::touch_recent_at`]).
    pub fn touch_recent(&mut self, path: &Path) {
        self.touch_recent_at(path, unix_now());
    }

    /// Moves `path` to the front of the recent list (adding it if needed)
    /// with the given timestamp, and caps the list at
    /// [`MAX_RECENT_PROJECTS`].
    pub fn touch_recent_at(&mut self, path: &Path, unix_secs: u64) {
        self.recent_projects.retain(|p| p.path != path);
        self.recent_projects.insert(
            0,
            RecentProject {
                path: path.to_path_buf(),
                name: project_name(path),
                last_opened: unix_secs,
            },
        );
        self.recent_projects.truncate(MAX_RECENT_PROJECTS);
    }

    /// Removes `path` from the recent list. Returns whether it was present.
    pub fn remove_recent(&mut self, path: &Path) -> bool {
        let before = self.recent_projects.len();
        self.recent_projects.retain(|p| p.path != path);
        self.recent_projects.len() != before
    }

    /// Drops recent and open projects whose folder no longer exists, and
    /// their sessions. Returns how many recent entries were removed.
    pub fn prune_missing(&mut self) -> usize {
        let before = self.recent_projects.len();
        self.recent_projects.retain(|p| p.path.exists());
        self.open_projects.retain(|p| p.exists());
        self.sessions.retain(|key, _| Path::new(key).exists());
        before - self.recent_projects.len()
    }

    /// The key used in [`AppState::sessions`] for a project path.
    pub fn session_key(project: &Path) -> String {
        project.to_string_lossy().into_owned()
    }

    /// The saved session of a project.
    pub fn session(&self, project: &Path) -> Option<&ProjectSession> {
        self.sessions.get(&Self::session_key(project))
    }

    /// The session of a project, created empty if missing.
    pub fn session_mut(&mut self, project: &Path) -> &mut ProjectSession {
        self.sessions.entry(Self::session_key(project)).or_default()
    }
}

/// Display name of a project: its folder name, or the whole path for roots.
pub fn project_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
