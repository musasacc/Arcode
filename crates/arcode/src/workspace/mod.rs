//! The workspace: the root view of a window. Holds the open project, the
//! editor panes, the sidebar, the bottom panel and the status bar.

mod actions;
mod anim;
pub(crate) mod close_guard;
mod debug;
#[cfg(test)]
mod debug_tests;
mod debuggee;
mod disk_conflict;
#[cfg(test)]
mod feature_tests;
mod features;
mod files;
#[cfg(test)]
mod files_tests;
mod frame;
#[cfg(test)]
mod frame_tests;
mod git;
#[cfg(test)]
mod git_tests;
mod lang_pickers;
mod languages;
#[cfg(test)]
mod languages_tests;
mod lsp_install;
#[cfg(test)]
mod lsp_install_tests;
#[cfg(test)]
mod lsp_tests;
mod new_project;
mod overlays;
pub use overlays::ToastLevel;
#[cfg(all(test, feature = "plugins"))]
mod consent_dialog_tests;
#[cfg(test)]
mod debug_regression_tests;
#[cfg(test)]
mod disk_conflict_regression_tests;
#[cfg(test)]
mod git_regression_tests;
#[cfg(test)]
mod lang_packs_tests;
mod menu_bar;
// macOS uses the native menu bar; the in-window one is not drawn there.
#[cfg(test)]
mod file_icon_tests;
#[cfg(all(test, not(target_os = "macos")))]
mod menu_bar_tests;
mod packages;
#[cfg(test)]
mod packages_tests;
mod packages_ui;
mod palette;
#[cfg(test)]
mod palette_tests;
mod panes;
#[cfg(feature = "plugins")]
mod plugins;
#[cfg(feature = "plugins")]
mod plugins_cmd;
#[cfg(feature = "plugins")]
mod plugins_consent;
#[cfg(all(test, feature = "plugins"))]
mod plugins_consent_tests;
#[cfg(feature = "plugins")]
mod plugins_install;
#[cfg(feature = "plugins")]
mod plugins_open;
#[cfg(feature = "plugins")]
mod plugins_page;
#[cfg(all(test, feature = "plugins"))]
mod plugins_page_tests;
#[cfg(all(test, feature = "plugins"))]
mod plugins_regression_tests;
#[cfg(feature = "plugins")]
mod plugins_req;
#[cfg(feature = "plugins")]
mod plugins_sync;
#[cfg(all(test, feature = "plugins"))]
mod plugins_sync_tests;
#[cfg(all(test, feature = "plugins"))]
mod plugins_tests;
#[cfg(feature = "plugins")]
mod plugins_ui;
mod prefs;
#[cfg(test)]
mod prefs_fix_tests;
mod prefs_pickers;
#[cfg(test)]
mod prefs_regression_tests;
#[cfg(test)]
mod prefs_tests;
#[cfg(test)]
mod presence_tests;
mod project_menu;
mod projects;
mod reload;
mod render;
mod run_file;
#[cfg(test)]
mod run_file_output_tests;
#[cfg(test)]
mod run_file_tests;
mod run_menu;
mod session;
mod snippets_notice;
mod status;
mod subs;
#[cfg(test)]
mod tab_menu_tests;
#[cfg(test)]
mod tests;
mod theme_editor;
#[cfg(test)]
mod theme_editor_close_tests;
#[cfg(test)]
mod theme_editor_tests;
mod theme_manage;
#[cfg(test)]
mod theme_manage_tests;
mod theme_remove;
mod tour;
#[cfg(test)]
mod vim_tests;
#[cfg(test)]
mod welcome_tests;
mod welcome_ui;

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
    /// The focused (framed) panel and the title bar's paused state.
    shell: frame::Shell,
    /// The command palette, while open.
    palette: Option<Entity<crate::commands::palette::CommandPalette>>,
    branch_picker: Option<Entity<BranchPicker>>,
    path_prompt: Option<Entity<PathPrompt>>,
    path_prompt_then: Option<FolderCallback>,
    project_menu_open: bool,
    /// The menus in the title bar.
    menu_bar: Entity<crate::menu_bar::MenuBar>,
    resizing: Option<Resize>,
    toast: Option<SharedString>,
    /// The status icon of `toast`.
    toast_level: ToastLevel,
    /// The toast's ghost button: its label and the action it dispatches.
    toast_action: Option<(SharedString, Box<dyn gpui::Action>)>,
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
    /// The Clone Repository dialog, while open.
    clone_dialog: Option<Entity<crate::clone_dialog::CloneDialog>>,
    /// The welcome screen (shown while no project is open).
    welcome: Entity<crate::welcome::Welcome>,
    /// Languages page, status bar pickers and the package banner.
    langs: crate::languages::LangParts,
    /// The Packages view and its store.
    pkgs: packages_ui::PackagesParts,
    /// Settings page, theme and font pickers, first run, tour, animations.
    prefs: crate::prefs::PrefsParts,
    /// WASM plugins: manager, panels, lenses, status entries.
    #[cfg(feature = "plugins")]
    plugins: crate::plugins::Plugins,
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
        subscriptions.extend(crate::presence::track(window, cx));
        subscriptions.push(Self::subscribe_refactor(&lsp, window, cx));
        let (features, feature_subs) = features::Features::new(&bottom_panel, window, cx);
        subscriptions.extend(feature_subs);
        let (debug, debug_subs) = debug::DebugParts::new(&bottom_panel, window, cx);
        subscriptions.extend(debug_subs);
        let shell = frame::Shell::new(&debug.debugger, &status_bar, &bottom_panel, cx);
        let (langs, lang_subs) = Self::new_lang_parts(cx);
        let (pkgs, pkg_subs) = packages_ui::PackagesParts::new(window, cx);
        subscriptions.extend(pkg_subs);
        subscriptions.extend(lang_subs);
        subscriptions.push(Self::watch_snippet_errors(cx));
        let focus_handle = cx.focus_handle();
        let welcome = {
            let weak = cx.weak_entity();
            cx.new(|cx| crate::welcome::Welcome::new(weak, cx))
        };
        let menu_bar = {
            let (weak, root) = (cx.weak_entity(), focus_handle.clone());
            cx.new(|cx| crate::menu_bar::MenuBar::new(weak, root, window, cx))
        };
        let workspace = Self {
            focus_handle,
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
            shell,
            palette: None,
            branch_picker: None,
            path_prompt: None,
            path_prompt_then: None,
            project_menu_open: false,
            menu_bar,
            resizing: None,
            toast: None,
            toast_level: ToastLevel::Info,
            toast_action: None,
            toast_task: None,
            toast_seq: 0,
            active_editor_sub: None,
            nav: Default::default(),
            subs: Default::default(),
            session: Default::default(),
            new_project: None,
            clone_dialog: None,
            welcome,
            langs,
            prefs: Default::default(),
            pkgs,
            #[cfg(feature = "plugins")]
            plugins: crate::plugins::Plugins::new(cx),
            _subscriptions: subscriptions,
        }
        .with_first_run(window, cx);
        #[cfg(feature = "plugins")]
        let workspace = workspace.with_plugins(window, cx);
        workspace
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

    /// Shows a transient message at the bottom right; its status icon
    /// follows the wording (see [`Self::show_toast_as`]).
    pub fn show_toast(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        let message = message.into();
        let level = overlays::toast_level(&message);
        self.show_toast_as(message, level, cx);
    }

    /// Shows a transient message with the status icon of `level`.
    pub fn show_toast_as(
        &mut self,
        message: impl Into<SharedString>,
        level: ToastLevel,
        cx: &mut Context<Self>,
    ) {
        self.show_toast_for(message.into(), level, None, TOAST_DURATION, cx);
    }

    /// Shows a transient message with a button (`label`) that dispatches
    /// `action` and closes the toast. It stays twice as long, so there is
    /// time to reach the button.
    pub fn show_toast_with_action(
        &mut self,
        message: impl Into<SharedString>,
        level: ToastLevel,
        label: impl Into<SharedString>,
        action: Box<dyn gpui::Action>,
        cx: &mut Context<Self>,
    ) {
        let button = Some((label.into(), action));
        self.show_toast_for(message.into(), level, button, TOAST_DURATION * 2, cx);
    }

    fn show_toast_for(
        &mut self,
        message: SharedString,
        level: ToastLevel,
        action: Option<(SharedString, Box<dyn gpui::Action>)>,
        duration: Duration,
        cx: &mut Context<Self>,
    ) {
        self.toast = Some(message);
        self.toast_level = level;
        self.toast_action = action;
        self.toast_seq += 1;
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
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
                #[cfg(feature = "plugins")]
                ws.plugins.client.index().set_files(files.clone());
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
        // Files a plugin opens (e.g. SQLite databases) get its view.
        #[cfg(feature = "plugins")]
        if self.open_with_plugin(&path, window, cx) {
            return;
        }
        if self.activate_open_file(&path, window, cx) {
            return;
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
        self.watch_theme_file(&editor, &path, cx);
        self.lsp
            .update(cx, |lsp, cx| lsp.register_editor(&editor, cx));
        self.git
            .editors
            .update(cx, |git, cx| git.register(&editor, cx));
        self.track_feature_editor(&editor, cx);
        self.track_disk_conflicts(&editor, window, cx);
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
