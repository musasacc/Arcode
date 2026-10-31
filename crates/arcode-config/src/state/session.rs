//! What a project looked like when it was last closed: tabs per pane,
//! cursors and scroll positions, panel sizes and the file tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::WindowState;

/// Most file states kept per project (cursors of files no longer open are
/// dropped first).
pub const MAX_FILE_STATES: usize = 200;
/// Most expanded folders remembered per project.
pub const MAX_EXPANDED_DIRS: usize = 500;

/// One selection, as `(line, column)` points (zero-based, column in chars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSelection {
    /// Where the selection started.
    pub anchor: (usize, usize),
    /// Where the cursor is.
    pub head: (usize, usize),
}

impl SavedSelection {
    /// A cursor without a selection.
    pub fn cursor(line: usize, column: usize) -> Self {
        Self {
            anchor: (line, column),
            head: (line, column),
        }
    }
}

/// Cursor and scroll position of one file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileSession {
    /// Every selection, the primary one first.
    pub selections: Vec<SavedSelection>,
    /// First visible buffer line.
    pub scroll_line: usize,
    /// When the file was last open (Unix seconds; 0 if unknown). When a
    /// session has too many file states, the oldest are dropped first.
    #[serde(skip_serializing_if = "is_zero")]
    pub seen: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// The tabs of one pane (split), left to right.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneSession {
    /// Files in tab order.
    pub files: Vec<PathBuf>,
    /// Index of the active tab in `files`.
    pub active: usize,
}

/// Sidebar and bottom panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSession {
    /// Whether the sidebar is shown.
    pub sidebar_visible: bool,
    /// Sidebar width in logical pixels.
    pub sidebar_width: f32,
    /// The sidebar view (`files`, `search`, `git`, `debug`, `plugins`,
    /// `todo`).
    pub sidebar_view: String,
    /// Whether the bottom panel is shown.
    pub panel_visible: bool,
    /// Bottom panel height in logical pixels.
    pub panel_height: f32,
    /// The bottom panel tab (`terminal`, `problems`, `todo`, `debug`).
    pub panel_tab: String,
}

impl Default for LayoutSession {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_width: 240.,
            sidebar_view: "files".to_owned(),
            panel_visible: false,
            panel_height: 240.,
            panel_tab: "terminal".to_owned(),
        }
    }
}

/// A breakpoint of a project (kept between runs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedBreakpoint {
    /// The file.
    pub path: PathBuf,
    /// Zero-based line.
    pub line: usize,
    /// Unchecked in the breakpoints list: kept but not sent to adapters.
    pub enabled: bool,
    /// Break only when this expression is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Break only after this many hits (adapter syntax, e.g. `5` or `>= 5`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Log this message instead of stopping (a log point).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

impl Default for SavedBreakpoint {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            line: 0,
            enabled: true,
            condition: None,
            hit_condition: None,
            log_message: None,
        }
    }
}

/// Most breakpoints kept per project.
pub const MAX_BREAKPOINTS: usize = 1000;

/// The saved state of one project.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSession {
    /// The panes (splits), left to right. Empty when nothing was open.
    pub panes: Vec<PaneSession>,
    /// Index of the focused pane.
    pub active_pane: usize,
    /// Cursor and scroll position per file path.
    pub files: BTreeMap<String, FileSession>,
    /// Sidebar and bottom panel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutSession>,
    /// Folders expanded in the file tree.
    pub expanded_dirs: Vec<PathBuf>,
    /// Window placement of the project's window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
    /// Breakpoints (kept even when sessions are not restored).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breakpoints: Vec<SavedBreakpoint>,
    /// The run command picked in the title bar's Run menu (a command line).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<String>,
    /// Open files, in tab order (sessions written before panes existed;
    /// read by [`ProjectSession::pane_layout`], never written).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub open_files: Vec<PathBuf>,
    /// The focused file (older sessions, see `open_files`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_file: Option<PathBuf>,
    /// Cursor `(line, column)` per file path (older sessions).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub cursors: BTreeMap<String, (usize, usize)>,
}

impl ProjectSession {
    /// The key of `path` in [`ProjectSession::files`].
    pub fn file_key(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// The panes to restore: [`ProjectSession::panes`], or one pane built
    /// from the fields of older sessions. Panes without files are dropped
    /// and active indices clamped.
    pub fn pane_layout(&self) -> Vec<PaneSession> {
        let mut panes: Vec<PaneSession> = if self.panes.is_empty() && !self.open_files.is_empty() {
            let active = self
                .active_file
                .as_ref()
                .and_then(|a| self.open_files.iter().position(|f| f == a))
                .unwrap_or(0);
            vec![PaneSession {
                files: self.open_files.clone(),
                active,
            }]
        } else {
            self.panes.clone()
        };
        panes.retain(|p| !p.files.is_empty());
        for pane in &mut panes {
            pane.active = pane.active.min(pane.files.len() - 1);
        }
        panes
    }

    /// The saved state of `path` (older sessions only had a cursor).
    pub fn file(&self, path: &Path) -> Option<FileSession> {
        let key = Self::file_key(path);
        if let Some(file) = self.files.get(&key) {
            return Some(file.clone());
        }
        self.cursors.get(&key).map(|&(line, column)| FileSession {
            selections: vec![SavedSelection::cursor(line, column)],
            scroll_line: line.saturating_sub(10),
            seen: 0,
        })
    }

    /// Keeps the session small: drops the fields of older sessions, file
    /// states beyond [`MAX_FILE_STATES`] (open files are kept, the files
    /// not seen for the longest time go first) and expanded folders beyond
    /// [`MAX_EXPANDED_DIRS`].
    pub fn compact(&mut self) {
        self.open_files.clear();
        self.active_file = None;
        self.cursors.clear();
        if self.files.len() > MAX_FILE_STATES {
            let open: std::collections::HashSet<String> = self
                .panes
                .iter()
                .flat_map(|p| p.files.iter().map(|f| Self::file_key(f)))
                .collect();
            let mut closed: Vec<(u64, String)> = self
                .files
                .iter()
                .filter(|(key, _)| !open.contains(*key))
                .map(|(key, f)| (f.seen, key.clone()))
                .collect();
            closed.sort();
            let extra = self.files.len() - MAX_FILE_STATES;
            for (_, key) in closed.into_iter().take(extra) {
                self.files.remove(&key);
            }
        }
        self.expanded_dirs.truncate(MAX_EXPANDED_DIRS);
        self.breakpoints.truncate(MAX_BREAKPOINTS);
    }

    /// Whether every stored path is valid Unicode (TOML cannot store
    /// others).
    pub(super) fn all_paths_unicode(&self) -> bool {
        let ok = |p: &Path| p.to_str().is_some();
        self.panes.iter().all(|p| p.files.iter().all(|f| ok(f)))
            && self.expanded_dirs.iter().all(|d| ok(d))
            && self.breakpoints.iter().all(|b| ok(&b.path))
            && self.open_files.iter().all(|f| ok(f))
            && self.active_file.as_deref().is_none_or(ok)
    }

    /// Drops the paths that are not valid Unicode.
    pub(super) fn retain_unicode(&mut self, keep: &dyn Fn(&Path) -> bool) {
        for pane in &mut self.panes {
            let active = pane.files.get(pane.active).cloned();
            pane.files.retain(|f| keep(f));
            pane.active = active
                .and_then(|a| pane.files.iter().position(|f| *f == a))
                .unwrap_or(0);
        }
        self.panes.retain(|p| !p.files.is_empty());
        self.expanded_dirs.retain(|d| keep(d));
        self.breakpoints.retain(|b| keep(&b.path));
        self.open_files.retain(|p| keep(p));
        if self.active_file.as_deref().is_some_and(|p| !keep(p)) {
            self.active_file = None;
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
