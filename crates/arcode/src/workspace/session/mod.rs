//! Session restore: what a project looked like (tabs per pane, cursors,
//! scroll positions, sidebar and bottom panel, expanded folders, window
//! placement) is saved in `state.toml` (`AppState::sessions`) when the
//! project is switched away from, closed, its window closes or the app
//! quits, and restored when the project is opened again.
//!
//! Unsaved changes are never written: closing a window or quitting with
//! dirty editors asks first, and the file is restored from disk.

mod restore;
#[cfg(test)]
mod review_session;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arcode_buffer::Point;
use arcode_config::{FileSession, LayoutSession, PaneSession, ProjectSession, SavedSelection};
use arcode_editor::Editor;
use gpui::{App, Context, Entity, Window};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::app_state;
use crate::bottom_panel::BottomTab;

/// The name stored for a sidebar view.
pub(crate) fn activity_key(activity: Activity) -> &'static str {
    match activity {
        Activity::Files => "files",
        Activity::Search => "search",
        Activity::Git => "git",
        Activity::Debug => "debug",
        Activity::Plugins => "plugins",
        Activity::Todo => "todo",
    }
}

/// The sidebar view stored as `key` (unknown names are ignored).
pub(crate) fn activity_from_key(key: &str) -> Option<Activity> {
    Activity::ALL.into_iter().find(|a| activity_key(*a) == key)
}

/// The name stored for a bottom panel tab.
pub(crate) fn tab_key(tab: BottomTab) -> &'static str {
    match tab {
        BottomTab::Terminal => "terminal",
        BottomTab::Problems => "problems",
        BottomTab::Todo => "todo",
        BottomTab::Debug => "debug",
    }
}

/// The bottom panel tab stored as `key`.
pub(crate) fn tab_from_key(key: &str) -> Option<BottomTab> {
    BottomTab::ALL.into_iter().find(|t| tab_key(*t) == key)
}

/// Session bookkeeping kept by the workspace.
#[derive(Default)]
pub(crate) struct SessionState {
    /// Scroll line restored into editors that were not painted since.
    restored_scroll: std::collections::HashMap<gpui::EntityId, usize>,
    /// Set while a session is being restored (no saves meanwhile).
    restoring: bool,
    /// The pending delayed save.
    task: Option<gpui::Task<()>>,
    /// Cursor and scroll of project files that are not open (from the
    /// stored session, or remembered when their tab closed). Applied when
    /// such a file is opened again, and saved with the session.
    files: BTreeMap<String, FileSession>,
}

fn saved(p: Point) -> (usize, usize) {
    (p.line, p.column)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Workspace {
    /// The current session of the open project (`None` without one).
    /// Only files inside the project are kept (diff tabs and library
    /// sources opened by navigation are left out). Files that are no
    /// longer open keep their last cursor and scroll position.
    pub fn capture_session(&self, window: Option<&Window>, cx: &App) -> Option<ProjectSession> {
        let root = &self.project.as_ref()?.root;
        let mut panes = Vec::new();
        let mut files = self.session.files.clone();
        let mut active_pane = 0;
        let now = unix_now();
        for (pane_ix, pane) in self.panes.iter().enumerate() {
            let pane = pane.read(cx);
            let active_editor = pane.active_editor().cloned();
            let mut saved_pane = PaneSession::default();
            for editor in pane.items() {
                let Some((path, mut state)) = self.editor_state(editor, root, cx) else {
                    continue;
                };
                if Some(editor) == active_editor.as_ref() {
                    saved_pane.active = saved_pane.files.len();
                }
                state.seen = now;
                files.insert(ProjectSession::file_key(&path), state);
                saved_pane.files.push(path);
            }
            if !saved_pane.files.is_empty() {
                if pane_ix == self.active_pane {
                    active_pane = panes.len();
                }
                panes.push(saved_pane);
            }
        }
        let layout = LayoutSession {
            sidebar_visible: self.sidebar_visible,
            sidebar_width: f32::from(self.sidebar_width),
            sidebar_view: activity_key(self.activity).to_owned(),
            panel_visible: self.bottom_panel_visible,
            panel_height: f32::from(self.bottom_panel_height),
            panel_tab: tab_key(self.bottom_panel.read(cx).active_tab()).to_owned(),
        };
        let expanded_dirs = self
            .file_tree
            .as_ref()
            .map(|t| t.read(cx).expanded_dirs())
            .unwrap_or_default();
        Some(ProjectSession {
            panes,
            active_pane,
            files,
            layout: Some(layout),
            expanded_dirs,
            window: window.map(app_state::window_state),
            breakpoints: self.debug.debugger.read(cx).book().to_saved(),
            run_command: self.debug.run.custom.clone(),
            ..Default::default()
        })
    }

    /// The path (inside `root`) and the cursor and scroll state of
    /// `editor`.
    fn editor_state(
        &self,
        editor: &Entity<Editor>,
        root: &Path,
        cx: &App,
    ) -> Option<(PathBuf, FileSession)> {
        let e = editor.read(cx);
        let path = e.path().filter(|p| p.starts_with(root))?.to_path_buf();
        let buffer = e.buffer();
        let mut selections = e
            .selections()
            .iter()
            .map(|s| SavedSelection {
                anchor: saved(buffer.char_to_point(s.anchor)),
                head: saved(buffer.char_to_point(s.head)),
            })
            .collect::<Vec<_>>();
        // Primary selection first.
        let primary = e.selections().primary_index();
        if primary < selections.len() {
            selections.swap(0, primary);
        }
        let visible = e.visible_line_range();
        let scroll_line = if visible.is_empty() {
            // Never painted (a background tab restored earlier).
            self.session
                .restored_scroll
                .get(&editor.entity_id())
                .copied()
                .unwrap_or(0)
        } else {
            visible.start
        };
        Some((
            path,
            FileSession {
                selections,
                scroll_line,
                seen: unix_now(),
            },
        ))
    }

    /// Remembers where the cursor of `editor` was (its tab is closing), so
    /// the file reopens there.
    pub(crate) fn remember_closed_editor(&mut self, editor: &Entity<Editor>, cx: &App) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            return;
        };
        if let Some((path, state)) = self.editor_state(editor, &root, cx) {
            self.session
                .files
                .insert(ProjectSession::file_key(&path), state);
        }
        self.session.restored_scroll.remove(&editor.entity_id());
    }

    /// Puts the remembered cursor and scroll of a file just opened into
    /// `editor` (not while a whole session is being restored).
    pub(crate) fn apply_remembered_state(
        &mut self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) {
        if self.session.restoring {
            return;
        }
        let Some(path) = editor.read(cx).path().map(Path::to_path_buf) else {
            return;
        };
        if let Some(state) = self
            .session
            .files
            .get(&ProjectSession::file_key(&path))
            .cloned()
        {
            self.apply_file_state(editor, &state, cx);
        }
    }

    /// Starts remembering file states from a stored session (including
    /// sessions written before per-file states existed).
    pub(crate) fn seed_file_states(&mut self, session: &ProjectSession) {
        let mut files = session.files.clone();
        for key in session.cursors.keys() {
            if let Some(state) = session.file(Path::new(key)) {
                files.entry(key.clone()).or_insert(state);
            }
        }
        self.session.files = files;
    }

    /// Moves remembered states after `from` was renamed or moved to `to`.
    pub(crate) fn rename_file_states(&mut self, from: &Path, to: &Path) {
        let files = std::mem::take(&mut self.session.files);
        self.session.files = files
            .into_iter()
            .map(|(key, state)| {
                match arcode_project::files::renamed_path(Path::new(&key), from, to) {
                    Some(new) => (ProjectSession::file_key(&new), state),
                    None => (key, state),
                }
            })
            .collect();
    }

    /// Forgets remembered states of `path` and the files inside it.
    pub(crate) fn forget_file_states(&mut self, path: &Path) {
        self.session
            .files
            .retain(|key, _| !Path::new(key).starts_with(path));
    }

    /// Stores the open project's session in the app state and saves it
    /// (synchronously: used when a window closes or the app quits).
    pub fn save_session(&self, window: Option<&Window>, cx: &mut App) {
        if let Some((root, session)) = self.session_to_store(window, cx) {
            app_state::set_session(&root, session, cx);
        }
    }

    /// The session to store; without a window the stored placement of the
    /// project's window is kept.
    fn session_to_store(
        &self,
        window: Option<&Window>,
        cx: &App,
    ) -> Option<(PathBuf, ProjectSession)> {
        let root = self.project.as_ref()?.root.clone();
        let mut session = self.capture_session(window, cx)?;
        if session.window.is_none() {
            session.window = app_state::session(&root, cx).and_then(|s| s.window);
        }
        Some((root, session))
    }

    /// The root of the open project.
    pub(crate) fn project_root(&self) -> Option<&Path> {
        self.project.as_ref().map(|p| p.root.as_path())
    }

    /// Saves the session after a short delay (coalescing bursts of tab
    /// changes), so a crash loses little. Only runs on changes; nothing
    /// is polled.
    pub(crate) fn schedule_session_save(&mut self, cx: &mut Context<Self>) {
        if self.project.is_none() || self.session.restoring {
            return;
        }
        self.session.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            this.update(cx, |ws, cx| {
                // The file is written off the UI thread.
                if let Some((root, session)) = ws.session_to_store(None, cx) {
                    app_state::store_session(&root, session, cx);
                    app_state::save_in_background(cx);
                }
            })
            .ok();
        }));
    }
}
