//! Workspace action handlers.

use std::path::PathBuf;

use gpui::{AppContext, Context, Focusable, PathPromptOptions, PromptLevel, Window};

use super::{Resize, Workspace};
use crate::actions::*;
use crate::app_state;
use crate::bottom_panel::BottomTab;
use crate::finder::{Finder, FinderEvent};
use crate::project;

impl Workspace {
    /// Saves the window placement when the window closes, after asking
    /// about unsaved changes.
    pub(super) fn install_close_guard(window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let dirty = this
                .upgrade()
                .map(|ws| {
                    let ws = ws.read(cx);
                    ws.editors(cx).iter().filter(|e| e.read(cx).is_dirty()).count()
                })
                .unwrap_or(0);
            app_state::record_window(window, cx);
            if dirty == 0 {
                return true;
            }
            let answer = window.prompt(
                PromptLevel::Warning,
                &format!("{dirty} file(s) have unsaved changes. Close anyway?"),
                None,
                &["Discard and Close", "Cancel"],
                cx,
            );
            window
                .spawn(cx, async move |cx| {
                    if answer.await.ok() == Some(0) {
                        cx.update(|window, _| window.remove_window()).ok();
                    }
                })
                .detach();
            false
        });
    }

    pub(super) fn register_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(Self::toggle_finder))
            .on_action(cx.listener(Self::toggle_bottom_panel))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(|ws, _: &SplitRight, window, cx| ws.split_right(window, cx)))
            .on_action(cx.listener(|ws, _: &OpenFolder, window, cx| ws.prompt_open_folder(window, cx)))
            .on_action(cx.listener(|ws, _: &NewProject, window, cx| ws.prompt_new_project(window, cx)))
            .on_action(cx.listener(|ws, _: &CloseProject, window, cx| ws.close_project(window, cx)))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::run_project))
            .on_action(cx.listener(Self::focus_terminal))
            .on_action(cx.listener(Self::new_terminal))
    }

    fn toggle_finder(&mut self, _: &ToggleFinder, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        if self.finder.is_some() {
            self.close_finder(window, cx);
            return;
        }
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            self.show_toast("Open a folder to search its files", cx);
            return;
        };
        let files = self.project_files.clone();
        let loading = self.files_loading || files.is_empty();
        let finder = cx.new(|cx| Finder::new(root, files, loading, cx));
        let sub = cx.subscribe_in(&finder, window, |ws, _, event: &FinderEvent, window, cx| {
            match event {
                FinderEvent::Open(path) => {
                    let path = path.clone();
                    ws.finder = None;
                    ws.open_file(&path, window, cx);
                }
                FinderEvent::Dismissed => ws.close_finder(window, cx),
            }
        });
        self._subscriptions.push(sub);
        window.focus(&finder.focus_handle(cx));
        self.finder = Some(finder);
        if !self.files_loading {
            self.rescan_files(cx);
        }
        cx.notify();
    }

    pub(super) fn close_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finder.take().is_some() {
            self.active_pane().read(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn toggle_bottom_panel(&mut self, _: &ToggleBottomPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_visible = !self.bottom_panel_visible;
        if self.bottom_panel_visible {
            self.bottom_panel.read(cx).focus(window);
        } else if self.bottom_panel.focus_handle(cx).contains_focused(window, cx) {
            self.active_pane().read(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    /// Selects an activity; clicking the active one hides the sidebar.
    pub(super) fn select_activity(&mut self, activity: crate::activity_bar::Activity, cx: &mut Context<Self>) {
        if self.sidebar_visible && self.activity == activity {
            self.sidebar_visible = false;
        } else {
            self.activity = activity;
            self.sidebar_visible = true;
        }
        cx.notify();
    }

    fn prompt_folder(
        &mut self,
        prompt: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.project_menu_open = false;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let chosen = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(err)) => {
                    log::error!("folder picker failed: {err:#}");
                    this.update(cx, |ws, cx| {
                        ws.show_toast(format!("Could not open the folder picker: {err}"), cx)
                    })
                    .ok();
                    None
                }
            };
            if let Some(path) = chosen {
                this.update_in(cx, |ws, window, cx| then(ws, path, window, cx)).ok();
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn prompt_open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_folder("Open", window, cx, |ws, path, window, cx| {
            ws.open_project(path, window, cx)
        });
    }

    pub(crate) fn prompt_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_folder("Create Project Here", window, cx, |ws, parent, window, cx| {
            match project::create_project_dir(&parent) {
                Ok(dir) => ws.open_project(dir, window, cx),
                Err(err) => ws.show_toast(format!("Cannot create project: {err}"), cx),
            }
        });
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = arcode_config::paths::config_file() else {
            self.show_toast("No configuration directory is available", cx);
            return;
        };
        if let Err(err) = arcode_config::Config::write_default_if_missing(&path) {
            self.show_toast(format!("Cannot create {}: {err}", path.display()), cx);
            return;
        }
        self.open_file(&path, window, cx);
    }

    fn run_project(&mut self, _: &RunProject, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = &self.project else {
            self.show_toast("Open a project to run it", cx);
            return;
        };
        let Some(command) = project.run_command() else {
            self.show_toast("No run command detected for this project", cx);
            return;
        };
        self.bottom_panel_visible = true;
        self.bottom_panel
            .update(cx, |panel, cx| panel.run_command(command, cx));
        self.bottom_panel.read(cx).focus(window);
        cx.notify();
    }

    fn focus_terminal(&mut self, _: &FocusTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_visible = true;
        self.bottom_panel.update(cx, |panel, cx| {
            panel.set_active_tab(BottomTab::Terminal, cx);
            panel.focus(window);
        });
        cx.notify();
    }

    fn new_terminal(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_terminal(&FocusTerminal, window, cx);
    }

    /// Starts resizing the sidebar or bottom panel with the mouse.
    pub(super) fn start_resize(&mut self, what: Resize, cx: &mut Context<Self>) {
        self.resizing = Some(what);
        cx.notify();
    }
}

impl Workspace {
    /// Whether the project dropdown menu is open.
    pub fn project_menu_open(&self) -> bool {
        self.project_menu_open
    }

    /// Opens or closes the project dropdown menu.
    pub fn toggle_project_menu(&mut self, cx: &mut Context<Self>) {
        self.project_menu_open = !self.project_menu_open;
        cx.notify();
    }
}
