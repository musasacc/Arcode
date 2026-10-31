//! The workspace: the root view of a window. Holds the open project, the
//! editor panes, the sidebar, the bottom panel and the status bar.

mod actions;
mod anim;
pub(crate) mod close_guard;
mod debug;
#[cfg(test)]
mod debug_tests;
mod debuggee;
#[cfg(test)]
mod feature_tests;
mod features;
mod files;
#[cfg(test)]
mod files_tests;
mod git;
#[cfg(test)]
mod git_tests;
mod lang_pickers;
mod languages;
#[cfg(test)]
mod languages_tests;
#[cfg(test)]
mod lsp_tests;
mod new_project;
mod overlays;
mod packages;
mod palette;
mod panes;
mod prefs;
#[cfg(test)]
mod prefs_fix_tests;
mod prefs_pickers;
#[cfg(test)]
mod prefs_tests;
mod project_menu;
mod projects;
mod render;
#[cfg(test)]
mod review_debug;
#[cfg(test)]
mod review_git;
#[cfg(test)]
mod review_lang_packs;
#[cfg(test)]
mod review_palette;
#[cfg(test)]
mod review_prefs;
mod run_menu;
mod session;
mod status;
mod subs;
#[cfg(test)]
mod tests;
mod tour;
#[cfg(test)]
mod vim_tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arcode_buffer::Buffer;
use arcode_editor::{Editor, EditorEvent};
use gpui::{
    AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, SharedString,
    Subscription, Task, Window, px,
};

use crate::activity_bar::Activity;
use crate::bottom_panel::{self, BottomPanel};
use crate::finder::Finder;
use crate::git::branch_picker::BranchPicker;
use crate::lsp_bridge::LspBridge;
use crate::pane::Pane;
use crate::path_prompt::PathPrompt;
use crate::project::{self, Project};
use crate::sidebar::{self, file_tree::FileTree};
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

/// Continuation run with the folder the user picked.
type FolderCallback =
    Box<dyn FnOnce(&mut Workspace, PathBuf, &mut Window, &mut Context<Workspace>)>;

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
    lsp: Entity<LspBridge>,
    bottom_panel: Entity<BottomPanel>,
    bottom_panel_visible: bool,
    bottom_panel_height: Pixels,
    status_bar: Entity<StatusBar>,
    finder: Option<Entity<Finder>>,
    git: git::GitParts,
    /// Search view, TODO list.
    features: features::Features,
    /// Debugger, run buttons and the Run menu.
    debug: debug::DebugParts,
    /// The command palette, while open.
    palette: Option<Entity<crate::commands::palette::CommandPalette>>,
    branch_picker: Option<Entity<BranchPicker>>,
    path_prompt: Option<Entity<PathPrompt>>,
    path_prompt_then: Option<FolderCallback>,
    project_menu_open: bool,
    resizing: Option<Resize>,
    toast: Option<SharedString>,
    toast_task: Option<Task<()>>,
    /// Toasts shown so far: a new toast fades in even over the last one.
    toast_seq: usize,
    active_editor_sub: Option<Subscription>,
    /// Go to definition, symbol pickers and the back/forward history.
    pub(crate) nav: crate::navigation::Navigator,
    /// Overlay, pane and editor subscriptions (bounded).
    subs: subs::Subs,
    /// Session restore bookkeeping.
    session: session::SessionState,
    /// The New Project dialog, while open.
    new_project: Option<Entity<crate::new_project::NewProjectDialog>>,
    /// Languages page, status bar pickers and the package banner.
    langs: crate::languages::LangParts,
    /// Settings page, theme and font pickers, first run, tour, animations.
    prefs: crate::prefs::PrefsParts,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The window title for a project (or for no project).
pub(crate) fn window_title_for(project: Option<&str>) -> String {
    match project {
        Some(name) => format!("{name} — Arcode"),
        None => "Arcode".to_owned(),
    }
}

impl Workspace {
    /// The title of this workspace's window.
    pub fn window_title(&self) -> String {
        window_title_for(self.project.as_ref().map(|p| p.name.as_str()))
    }

    /// Creates an empty workspace (welcome screen) in `window`.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            crate::theme_sync::apply(window.appearance(), cx);
        });
        Self::install_close_guard(window, cx);
        let pane = cx.new(|cx| Pane::new(window, cx));
        let pane_sub = Self::subscribe_pane(&pane, window, cx);
        let lsp = cx.new(LspBridge::new);
        let bottom_panel = cx.new(|cx| BottomPanel::new(lsp.clone(), cx));
        let status_bar = cx.new(StatusBar::new);
        let workspace = cx.weak_entity();
        let lsp_status = cx.new(|cx| {
            crate::lsp_bridge::status_view::LspStatusView::new(lsp.clone(), workspace, cx)
        });
        status_bar.update(cx, |bar, cx| bar.set_lsp_view(lsp_status.into(), cx));
        let mut subscriptions = vec![appearance, pane_sub];
        subscriptions.extend(Self::subscribe_lsp(&lsp, &bottom_panel, window, cx));
        let (git, git_subs) = git::GitParts::new(window, cx);
        subscriptions.extend(git_subs);
        subscriptions.push(crate::navigation::Navigator::track(cx));
        subscriptions.push(Self::subscribe_refactor(&lsp, window, cx));
        let (features, feature_subs) = features::Features::new(&bottom_panel, window, cx);
        subscriptions.extend(feature_subs);
        let (debug, debug_subs) = debug::DebugParts::new(&bottom_panel, window, cx);
        subscriptions.extend(debug_subs);
        let (langs, lang_subs) = Self::new_lang_parts(cx);
        subscriptions.extend(lang_subs);
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
            lsp,
            bottom_panel,
            bottom_panel_visible: false,
            bottom_panel_height: px(bottom_panel::DEFAULT_HEIGHT),
            status_bar,
            finder: None,
            git,
            features,
            debug,
            palette: None,
            branch_picker: None,
            path_prompt: None,
            path_prompt_then: None,
            project_menu_open: false,
            resizing: None,
            toast: None,
            toast_task: None,
            toast_seq: 0,
            active_editor_sub: None,
            nav: Default::default(),
            subs: Default::default(),
            session: Default::default(),
            new_project: None,
            langs,
            prefs: Default::default(),
            _subscriptions: subscriptions,
        }
        .with_first_run(window, cx)
    }

    /// Shows the first-run dialog in the first window when Arcode started
    /// without a configuration file.
    fn with_first_run(self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        use crate::prefs::first_run::FirstRunPending;
        if cx.has_global::<FirstRunPending>() {
            cx.remove_global::<FirstRunPending>();
            cx.defer_in(window, |ws, window, cx| ws.show_first_run(window, cx));
        }
        self
    }

    /// The open project.
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// The status bar (LSP wiring updates its status and counts).
    pub fn status_bar(&self) -> &Entity<StatusBar> {
        &self.status_bar
    }

    /// The language server bridge (diagnostics and server status).
    pub fn lsp(&self) -> &Entity<LspBridge> {
        &self.lsp
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
        self.project.is_none() && self.panes.iter().all(|p| !p.read(cx).has_tabs())
    }

    /// Shows a transient message at the bottom right.
    pub fn show_toast(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(message.into());
        self.toast_seq += 1;
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

    /// Re-indexes the project's files in the background.
    fn rescan_files(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            return;
        };
        self.files_loading = true;
        let scan =
            cx.background_spawn(
                async move { Arc::new(project::list_files(&root, MAX_INDEXED_FILES)) },
            );
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
        self.prepare_editor_language(&editor, cx);
        let config =
            crate::prefs::store::config_path(cx).map(|p| std::fs::canonicalize(&p).unwrap_or(p));
        if config.as_deref() == Some(path.as_path()) {
            let sub = cx.subscribe(&editor, |_, _, event: &EditorEvent, cx| {
                if *event == EditorEvent::Saved {
                    crate::app::reload_settings(cx);
                }
            });
            self.subs.tie(editor.downgrade().into(), sub);
        }
        self.lsp
            .update(cx, |lsp, cx| lsp.register_editor(&editor, cx));
        self.git
            .editors
            .update(cx, |git, cx| git.register(&editor, cx));
        self.track_feature_editor(&editor, cx);
        self.debug
            .debugger
            .update(cx, |d, cx| d.register_editor(&editor, cx));
        self.apply_remembered_state(&editor, cx);
        cx.emit(WorkspaceEvent::EditorOpened(editor));
        self.schedule_session_save(cx);
        self.schedule_package_check(cx);
        cx.notify();
    }
}
