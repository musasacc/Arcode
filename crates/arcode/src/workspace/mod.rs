//! The workspace: the root view of a window. Holds the open project, the
//! editor panes, the sidebar, the bottom panel and the status bar.

mod actions;
mod overlays;
mod panes;
mod render;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arcode_buffer::Buffer;
use arcode_editor::{Editor, EditorEvent};
use gpui::{
    AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, PromptLevel,
    SharedString, Subscription, Task, Window, px,
};

use crate::activity_bar::Activity;
use crate::app_state;
use crate::bottom_panel::{self, BottomPanel};
use crate::finder::Finder;
use crate::pane::Pane;
use crate::project::{self, Project};
use crate::sidebar::{self, file_tree::{FileTree, FileTreeEvent}};
use crate::status_bar::StatusBar;

/// Upper bound on files indexed for the finder.
const MAX_INDEXED_FILES: usize = 200_000;
/// How long a toast message stays visible.
const TOAST_DURATION: Duration = Duration::from_secs(4);

/// Events other parts of the app (e.g. LSP wiring) can subscribe to.
#[derive(Clone)]
pub enum WorkspaceEvent {
    /// A project was opened or closed.
    ProjectChanged,
    /// An editor was created for a file.
    EditorOpened(Entity<Editor>),
    /// The active editor changed (`None` when no editor is active).
    ActiveEditorChanged(Option<Entity<Editor>>),
}

/// What a drag on a divider resizes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Resize {
    Sidebar,
    BottomPanel,
}

/// The root view.
pub struct Workspace {
    focus_handle: FocusHandle,
    project: Option<Project>,
    project_files: Arc<Vec<String>>,
    files_loading: bool,
    files_task: Option<Task<()>>,
    file_tree: Option<Entity<FileTree>>,
    panes: Vec<Entity<Pane>>,
    active_pane: usize,
    activity: Activity,
    sidebar_visible: bool,
    sidebar_width: Pixels,
    bottom_panel: Entity<BottomPanel>,
    bottom_panel_visible: bool,
    bottom_panel_height: Pixels,
    status_bar: Entity<StatusBar>,
    finder: Option<Entity<Finder>>,
    project_menu_open: bool,
    resizing: Option<Resize>,
    toast: Option<SharedString>,
    toast_task: Option<Task<()>>,
    active_editor_sub: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Workspace {
    /// Creates an empty workspace (welcome screen) in `window`.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            crate::theme_sync::apply(window.appearance(), cx);
        });
        Self::install_close_guard(window, cx);
        let pane = cx.new(|cx| Pane::new(window, cx));
        let pane_sub = Self::subscribe_pane(&pane, window, cx);
        let bottom_panel = cx.new(BottomPanel::new);
        let status_bar = cx.new(StatusBar::new);
        Self {
            focus_handle: cx.focus_handle(),
            project: None,
            project_files: Arc::default(),
            files_loading: false,
            files_task: None,
            file_tree: None,
            panes: vec![pane],
            active_pane: 0,
            activity: Activity::Files,
            sidebar_visible: true,
            sidebar_width: px(sidebar::DEFAULT_WIDTH),
            bottom_panel,
            bottom_panel_visible: false,
            bottom_panel_height: px(bottom_panel::DEFAULT_HEIGHT),
            status_bar,
            finder: None,
            project_menu_open: false,
            resizing: None,
            toast: None,
            toast_task: None,
            active_editor_sub: None,
            _subscriptions: vec![appearance, pane_sub],
        }
    }

    /// The open project.
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// The status bar (LSP wiring updates its status and counts).
    pub fn status_bar(&self) -> &Entity<StatusBar> {
        &self.status_bar
    }

    /// The bottom panel.
    pub fn bottom_panel(&self) -> &Entity<BottomPanel> {
        &self.bottom_panel
    }

    /// Every pane, left to right.
    pub fn panes(&self) -> &[Entity<Pane>] {
        &self.panes
    }

    /// The pane that receives newly opened files.
    pub fn active_pane(&self) -> &Entity<Pane> {
        &self.panes[self.active_pane.min(self.panes.len() - 1)]
    }

    /// The active editor of the active pane.
    pub fn active_editor(&self, cx: &gpui::App) -> Option<Entity<Editor>> {
        self.active_pane().read(cx).active_editor().cloned()
    }

    /// Every open editor across panes.
    pub fn editors(&self, cx: &gpui::App) -> Vec<Entity<Editor>> {
        self.panes
            .iter()
            .flat_map(|p| p.read(cx).items().to_vec())
            .collect()
    }

    /// Whether the welcome screen is shown (no project, no editors).
    pub fn shows_welcome(&self, cx: &gpui::App) -> bool {
        self.project.is_none() && self.panes.iter().all(|p| p.read(cx).items().is_empty())
    }

    /// Shows a transient message at the bottom right.
    pub fn show_toast(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(message.into());
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_DURATION).await;
            this.update(cx, |ws, cx| {
                ws.toast = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Opens `path` as the project, asking first if editors have unsaved
    /// changes.
    pub fn open_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, move |ws, window, cx| {
            ws.open_project_now(&path, window, cx)
        });
    }

    fn open_project_now(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let project = match Project::open(path) {
            Ok(project) => project,
            Err(err) => {
                self.show_toast(format!("Cannot open {}: {err}", path.display()), cx);
                return;
            }
        };
        log::info!("opening project {}", project.root.display());
        app_state::touch_recent(&project.root, cx);
        window.set_window_title(&format!("{} — Arcode", project.name));
        let tree = cx.new(|cx| FileTree::new(project.root.clone(), project.name.clone(), cx));
        let tree_sub = cx.subscribe_in(&tree, window, |ws, _, event: &FileTreeEvent, window, cx| {
            let FileTreeEvent::OpenFile(path) = event;
            ws.open_file(path, window, cx);
        });
        self._subscriptions.push(tree_sub);
        self.file_tree = Some(tree);
        self.project = Some(project);
        self.project_files = Arc::default();
        self.reset_panes(window, cx);
        self.activity = Activity::Files;
        self.sidebar_visible = true;
        self.project_menu_open = false;
        self.finder = None;
        self.rescan_files(cx);
        self.refresh_status(cx);
        cx.emit(WorkspaceEvent::ProjectChanged);
        cx.notify();
    }

    /// Closes the project and returns to the welcome screen.
    pub fn close_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, |ws, window, cx| {
            ws.project = None;
            ws.file_tree = None;
            ws.project_files = Arc::default();
            ws.files_task = None;
            ws.finder = None;
            ws.project_menu_open = false;
            ws.reset_panes(window, cx);
            window.set_window_title("Arcode");
            window.focus(&ws.focus_handle);
            ws.refresh_status(cx);
            cx.emit(WorkspaceEvent::ProjectChanged);
            cx.notify();
        });
    }

    /// Runs `then` right away, or after the user agreed to discard unsaved
    /// changes in open editors.
    fn confirm_discard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let dirty = self.editors(cx).iter().filter(|e| e.read(cx).is_dirty()).count();
        if dirty == 0 {
            then(self, window, cx);
            return;
        }
        let message = format!("{dirty} file(s) have unsaved changes. Discard them?");
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            None,
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |ws, window, cx| then(ws, window, cx)).ok();
            }
        })
        .detach();
    }

    /// Re-indexes the project's files in the background.
    fn rescan_files(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            return;
        };
        self.files_loading = true;
        let scan = cx.background_spawn(async move {
            Arc::new(project::list_files(&root, MAX_INDEXED_FILES))
        });
        self.files_task = Some(cx.spawn(async move |this, cx| {
            let files = scan.await;
            this.update(cx, |ws, cx| {
                ws.files_loading = false;
                ws.project_files = files.clone();
                if let Some(finder) = &ws.finder {
                    finder.update(cx, |f, cx| f.set_files(files, cx));
                }
            })
            .ok();
        }));
    }

    /// Opens `path` in the active pane, or activates it where it is open.
    pub fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        for (pane_ix, pane) in self.panes.clone().into_iter().enumerate() {
            if let Some(ix) = pane.read(cx).position_of(&path, cx) {
                self.active_pane = pane_ix;
                pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
                return;
            }
        }
        let buffer = match Buffer::from_file(&path) {
            Ok(buffer) => buffer,
            Err(err) => {
                log::warn!("cannot open {}: {err}", path.display());
                self.show_toast(format!("Cannot open {}: {err}", path.display()), cx);
                return;
            }
        };
        let editor = cx.new(|cx| Editor::new(buffer, cx));
        self.active_pane()
            .clone()
            .update(cx, |pane, cx| pane.add_editor(editor.clone(), window, cx));
        cx.emit(WorkspaceEvent::EditorOpened(editor));
        cx.notify();
    }

    /// Re-subscribes to the active editor and refreshes the status bar.
    fn active_editor_changed(&mut self, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx);
        self.active_editor_sub = editor.as_ref().map(|editor| {
            cx.subscribe(editor, |ws, _, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::SelectionsChanged | EditorEvent::Edited { .. }) {
                    ws.refresh_status(cx);
                }
            })
        });
        if let (Some(editor), Some(tree)) = (&editor, &self.file_tree)
            && let Some(path) = editor.read(cx).path().map(Path::to_path_buf)
        {
            tree.update(cx, |tree, cx| tree.reveal(&path, cx));
        }
        self.refresh_status(cx);
        cx.emit(WorkspaceEvent::ActiveEditorChanged(editor));
        cx.notify();
    }

    /// Pushes branch, language and cursor position to the status bar.
    pub fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let branch = self
            .project
            .as_ref()
            .and_then(Project::branch)
            .map(SharedString::from);
        let (language, cursor) = match self.active_editor(cx) {
            Some(editor) => {
                let e = editor.read(cx);
                let pos = e.cursor_position();
                (
                    Some(SharedString::new_static(e.language().name)),
                    Some((pos.line, pos.column)),
                )
            }
            None => (None, None),
        };
        self.status_bar.update(cx, |bar, cx| {
            bar.set_editor_info(branch, language, cursor, cx)
        });
    }
}
