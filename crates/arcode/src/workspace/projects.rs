//! Opening, switching and closing the project shown in this window.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, Window};

use super::{Workspace, WorkspaceEvent};
use crate::activity_bar::Activity;
use crate::app_state;
use crate::project::Project;

impl Workspace {
    /// Opens `path` as the project, asking first if editors have unsaved
    /// changes.
    pub fn open_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, move |ws, window, cx| {
            ws.open_project_now(&path, window, cx)
        });
    }

    pub(super) fn open_project_now(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = match Project::open(path) {
            Ok(project) => project,
            Err(err) => {
                self.show_toast(format!("Cannot open {}: {err}", path.display()), cx);
                return;
            }
        };
        if self.project_root() == Some(project.root.as_path()) {
            return;
        }
        // Another window has it: bring that one forward instead.
        if let Some(handle) = crate::windows::window_for_project(&project.root, cx) {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .ok();
            return;
        }
        log::info!("opening project {}", project.root.display());
        self.leave_project(Some(window), cx);
        app_state::touch_recent(&project.root, cx);
        app_state::add_open_project(&project.root, cx);
        window.set_window_title(&super::window_title_for(Some(&project.name)));
        let tree = self.make_file_tree(project.root.clone(), project.name.clone(), window, cx);
        self.file_tree = Some(tree);
        let root = Some(project.root.clone());
        self.lsp
            .update(cx, |lsp, cx| lsp.set_project_root(root.clone(), cx));
        self.bottom_panel
            .update(cx, |panel, cx| panel.set_project_root(root.clone(), cx));
        self.git
            .store
            .update(cx, |git, cx| git.set_project_root(root.clone(), cx));
        self.features.set_root(root.clone(), cx);
        self.set_debug_project(root, cx);
        self.branch_picker = None;
        self.palette = None;
        let session = arcode_ui_kit::settings(cx)
            .workspace
            .restore_session
            .then(|| app_state::session(&project.root, cx))
            .flatten();
        self.project = Some(project);
        self.project_files = Arc::default();
        self.reset_panes(window, cx);
        self.activity = Activity::Files;
        self.sidebar_visible = true;
        self.bottom_panel_visible = false;
        self.project_menu_open = false;
        self.finder = None;
        self.rescan_files(cx);
        self.langs.banner = Default::default();
        self.langs.manual.clear();
        self.schedule_package_check(cx);
        if let Some(session) = session {
            self.seed_file_states(&session);
            self.restore_session(&session, window, cx);
        }
        self.refresh_status(cx);
        if self.active_editor(cx).is_none() {
            self.active_pane().read(cx).focus(window, cx);
        }
        cx.emit(WorkspaceEvent::ProjectChanged);
        cx.notify();
    }

    /// Saves the session of the open project (if any) before this window
    /// shows something else, and forgets it as an open project.
    pub(super) fn leave_project(&mut self, window: Option<&Window>, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            return;
        };
        self.save_session(window, cx);
        let mut open = app_state::open_projects(cx);
        open.retain(|p| *p != root);
        app_state::set_open_projects(open, cx);
        self.session = Default::default();
    }

    /// Closes the project and returns to the welcome screen.
    pub fn close_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, |ws, window, cx| {
            ws.leave_project(Some(window), cx);
            app_state::save(cx);
            ws.project = None;
            ws.file_tree = None;
            ws.project_files = Arc::default();
            ws.files_task = None;
            ws.finder = None;
            ws.project_menu_open = false;
            ws.reset_panes(window, cx);
            ws.lsp.update(cx, |lsp, cx| lsp.set_project_root(None, cx));
            ws.bottom_panel
                .update(cx, |panel, cx| panel.set_project_root(None, cx));
            ws.bottom_panel_visible = false;
            ws.git
                .store
                .update(cx, |git, cx| git.set_project_root(None, cx));
            ws.features.set_root(None, cx);
            ws.set_debug_project(None, cx);
            ws.branch_picker = None;
            ws.langs.banner = Default::default();
            ws.langs.picker = None;
            window.set_window_title("Arcode");
            window.focus(&ws.focus_handle);
            ws.refresh_status(cx);
            cx.emit(WorkspaceEvent::ProjectChanged);
            cx.notify();
        });
    }
}
