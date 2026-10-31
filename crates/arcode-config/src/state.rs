//! Persisted application state (`state.toml`): recent projects, window
//! geometry and per-project editor sessions.
//!
//! Unlike the configuration, this file is written by Arcode itself. It is
//! saved atomically so a crash never leaves a truncated file behind.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::fsutil::write_atomic;
use crate::paths;

mod session;

pub use session::{
    FileSession, LayoutSession, MAX_BREAKPOINTS, MAX_EXPANDED_DIRS, MAX_FILE_STATES, PaneSession,
    ProjectSession, SavedBreakpoint, SavedSelection,
};

/// Maximum number of entries kept in [`AppState::recent_projects`].
pub const MAX_RECENT_PROJECTS: usize = 20;
/// Maximum number of entries kept in [`AppState::recent_commands`].
pub const MAX_RECENT_COMMANDS: usize = 30;
/// Maximum number of project sessions kept (sessions of projects that are
/// neither recent nor open are dropped first).
pub const MAX_SESSIONS: usize = 40;

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
    /// Command palette entries run recently (action names), most recent
    /// first, at most [`MAX_RECENT_COMMANDS`].
    pub recent_commands: Vec<String>,
    /// Main window placement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
    /// Editor sessions keyed by project path (see [`AppState::session_key`]).
    pub sessions: BTreeMap<String, ProjectSession>,
    /// Projects (keyed like [`AppState::sessions`]) whose "packages are
    /// not installed" banner was turned off with "Don't ask again".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden_package_banners: Vec<String>,
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
    ///
    /// Paths that are not valid Unicode (possible on Unix) cannot be stored
    /// in TOML; such recent/open projects and session files are left out of
    /// the file (with a log warning) instead of failing the whole save. They
    /// stay in `self` for the rest of the run.
    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        Self::write_toml(path, &self.to_toml()?)
    }

    /// The text [`AppState::save_to`] writes. Serializing is cheap; with
    /// [`AppState::write_toml`] the (slow, fsynced) write can run on
    /// another thread.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string(&*self.persistable()).context("serializing app state")
    }

    /// Writes text from [`AppState::to_toml`] to `path` atomically.
    pub fn write_toml(path: &Path, text: &str) -> anyhow::Result<()> {
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

    /// Moves the command `name` to the front of [`AppState::recent_commands`]
    /// (adding it if needed), keeping at most [`MAX_RECENT_COMMANDS`].
    pub fn touch_recent_command(&mut self, name: &str) {
        self.recent_commands.retain(|c| c != name);
        self.recent_commands.insert(0, name.to_owned());
        self.recent_commands.truncate(MAX_RECENT_COMMANDS);
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

    /// `self` without the paths that TOML cannot represent (non-Unicode).
    /// Borrows when nothing needs to be dropped.
    pub fn persistable(&self) -> Cow<'_, Self> {
        if self.all_paths_unicode() {
            return Cow::Borrowed(self);
        }
        let mut state = self.clone();
        let keep = |path: &Path| {
            let ok = is_unicode(path);
            if !ok {
                log::warn!("not saving non-Unicode path {}", path.display());
            }
            ok
        };
        state.recent_projects.retain(|p| keep(&p.path));
        state.open_projects.retain(|p| keep(p));
        for session in state.sessions.values_mut() {
            session.retain_unicode(&keep);
        }
        Cow::Owned(state)
    }

    fn all_paths_unicode(&self) -> bool {
        self.recent_projects.iter().all(|p| is_unicode(&p.path))
            && self.open_projects.iter().all(|p| is_unicode(p))
            && self
                .sessions
                .values()
                .all(ProjectSession::all_paths_unicode)
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

    /// Stores the session of a project (compacted), then drops sessions
    /// beyond [`MAX_SESSIONS`] whose project is neither recent nor open.
    pub fn set_session(&mut self, project: &Path, mut session: ProjectSession) {
        session.compact();
        self.sessions.insert(Self::session_key(project), session);
        if self.sessions.len() <= MAX_SESSIONS {
            return;
        }
        let known: std::collections::HashSet<String> = self
            .recent_projects
            .iter()
            .map(|p| Self::session_key(&p.path))
            .chain(self.open_projects.iter().map(|p| Self::session_key(p)))
            .chain([Self::session_key(project)])
            .collect();
        let mut extra = self.sessions.len() - MAX_SESSIONS;
        self.sessions.retain(|key, _| {
            if extra > 0 && !known.contains(key) {
                extra -= 1;
                false
            } else {
                true
            }
        });
    }

    /// Whether the package banner is turned off for `project`.
    pub fn package_banner_hidden(&self, project: &Path) -> bool {
        let key = Self::session_key(project);
        self.hidden_package_banners.contains(&key)
    }

    /// Turns the package banner off for `project` ("Don't ask again").
    pub fn hide_package_banner(&mut self, project: &Path) {
        if !self.package_banner_hidden(project) {
            self.hidden_package_banners.push(Self::session_key(project));
        }
    }

    /// Records that `project` is open in a window (restored on start).
    pub fn add_open_project(&mut self, project: &Path) {
        if !self.open_projects.iter().any(|p| p == project) {
            self.open_projects.push(project.to_path_buf());
        }
    }

    /// Records that `project`'s window was closed or switched away.
    pub fn remove_open_project(&mut self, project: &Path) {
        self.open_projects.retain(|p| p != project);
    }
}

/// Display name of a project: its folder name, or the whole path for roots.
pub fn project_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn is_unicode(path: &Path) -> bool {
    path.to_str().is_some()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
