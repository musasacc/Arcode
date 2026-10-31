//! Workspace action handlers.

use std::path::PathBuf;

use gpui::{AppContext, Context, Focusable, PathPromptOptions, Window};

use super::{FolderCallback, Resize, Workspace};
use crate::actions::*;
use crate::bottom_panel::BottomTab;
use crate::finder::{Finder, FinderEvent};
use crate::path_prompt::{PathPrompt, PathPromptEvent};

impl Workspace {
    /// Saves the session and window placement when the window closes,
    /// after asking about unsaved changes and running terminal commands.
    pub(super) fn install_close_guard(window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| match this.upgrade() {
            Some(ws) => super::close_guard::request_close(ws, window, cx),
            None => true,
        });
    }

    /// Closes this window like the window manager's close button would.
    fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        // Closing runs after this update, when the workspace can be read.
        window.defer(cx, move |window, cx| {
            if super::close_guard::request_close(this, window, cx) {
                window.remove_window();
            }
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
            .on_action(
                cx.listener(|ws, _: &OpenFolder, window, cx| ws.prompt_open_folder(window, cx)),
            )
            .on_action(
                cx.listener(|ws, _: &NewProject, window, cx| ws.prompt_new_project(window, cx)),
            )
            .on_action(cx.listener(|ws, _: &CloseProject, window, cx| ws.close_project(window, cx)))
            .on_action(cx.listener(Self::close_window))
            .on_action(cx.listener(|ws, _: &OpenFolderInNewWindow, window, cx| {
                ws.prompt_folder("Open Folder in New Window", window, cx, |_, path, _, cx| {
                    crate::windows::open_project_in_new_window(path, cx)
                })
            }))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::run_project))
            .on_action(cx.listener(Self::focus_terminal))
            .on_action(cx.listener(Self::new_terminal))
    }

    pub(super) fn toggle_finder(
        &mut self,
        _: &ToggleFinder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project_menu_open = false;
        if self.finder.is_some() {
            self.close_finder(window, cx);
            return;
        }
        // Ctrl+P in the palette switches it to the file finder; the two
        // overlays never stack (the palette would hide the finder).
        self.close_palette(window, cx);
        self.close_prefs_picker(true, window, cx);
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            self.show_toast("Open a folder to search its files", cx);
            return;
        };
        let files = self.project_files.clone();
        let loading = self.files_loading || files.is_empty();
        let finder = cx.new(|cx| Finder::new(root, files, loading, cx));
        let sub =
            cx.subscribe_in(
                &finder,
                window,
                |ws, _, event: &FinderEvent, window, cx| match event {
                    FinderEvent::Open(path) => {
                        let path = path.clone();
                        ws.finder = None;
                        ws.open_file(&path, window, cx);
                    }
                    FinderEvent::Dismissed => ws.close_finder(window, cx),
                    FinderEvent::SwitchToCommands(query) => {
                        let query = query.clone();
                        ws.close_finder(window, cx);
                        ws.open_palette(&query, window, cx);
                    }
                },
            );
        self.subs.set("finder", sub);
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

    fn toggle_bottom_panel(
        &mut self,
        _: &ToggleBottomPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bottom_panel_visible = !self.bottom_panel_visible;
        let opening = self.bottom_panel_visible;
        self.start_slide(super::anim::Slider::Panel, opening, cx);
        if self.bottom_panel_visible {
            self.bottom_panel
                .update(cx, |panel, cx| panel.focus(window, cx));
        } else if self
            .bottom_panel
            .focus_handle(cx)
            .contains_focused(window, cx)
        {
            self.active_pane().read(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        let opening = self.sidebar_visible;
        self.start_slide(super::anim::Slider::Sidebar, opening, cx);
        cx.notify();
    }

    /// Selects an activity; clicking the active one hides the sidebar.
    pub(super) fn select_activity(
        &mut self,
        activity: crate::activity_bar::Activity,
        cx: &mut Context<Self>,
    ) {
        let was_visible = self.sidebar_visible;
        if self.sidebar_visible && self.activity == activity {
            self.sidebar_visible = false;
        } else {
            self.activity = activity;
            self.sidebar_visible = true;
        }
        if was_visible != self.sidebar_visible {
            let opening = self.sidebar_visible;
            self.start_slide(super::anim::Slider::Sidebar, opening, cx);
        }
        if self.sidebar_visible && activity == crate::activity_bar::Activity::Plugins {
            self.langs.view.update(cx, |v, cx| v.shown(cx));
        }
        cx.notify();
    }

    pub(super) fn prompt_folder(
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
        cx.spawn_in(window, async move |this, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    this.update_in(cx, |ws, window, cx| then(ws, path, window, cx))
                        .ok();
                }
            }
            Ok(Ok(None)) | Err(_) => {}
            Ok(Err(err)) => {
                log::warn!("folder picker unavailable, asking for a path: {err:#}");
                this.update_in(cx, |ws, window, cx| {
                    ws.show_path_prompt(prompt, Box::new(then), window, cx)
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    /// Shows the in-app folder path dialog (fallback for the system picker).
    fn show_path_prompt(
        &mut self,
        title: &'static str,
        then: FolderCallback,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = self
            .project
            .as_ref()
            .and_then(|p| p.root.parent().map(|d| d.display().to_string()))
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_default();
        let initial = format!("{}/", initial.trim_end_matches('/'));
        let prompt = cx.new(|cx| PathPrompt::new(title, &initial, cx));
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |ws, _, event: &PathPromptEvent, window, cx| {
                ws.path_prompt = None;
                let then = ws.path_prompt_then.take();
                if let (PathPromptEvent::Confirmed(path), Some(then)) = (event, then) {
                    then(ws, path.clone(), window, cx);
                } else {
                    ws.active_pane().read(cx).focus(window, cx);
                }
                cx.notify();
            },
        );
        self.subs.set("path_prompt", sub);
        window.focus(&prompt.focus_handle(cx));
        self.path_prompt = Some(prompt);
        self.path_prompt_then = Some(then);
        cx.notify();
    }

    pub(crate) fn prompt_open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_folder("Open Folder", window, cx, |ws, path, window, cx| {
            ws.open_project(path, window, cx)
        });
    }

    pub(crate) fn prompt_new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        self.show_new_project_dialog(window, cx);
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_page(window, cx);
    }

    fn run_project(&mut self, _: &RunProject, window: &mut Window, cx: &mut Context<Self>) {
        self.run_project_command(window, cx);
    }

    fn focus_terminal(&mut self, _: &FocusTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_visible = true;
        self.bottom_panel.update(cx, |panel, cx| {
            panel.set_active_tab(BottomTab::Terminal, cx);
            panel.focus(window, cx);
        });
        cx.notify();
    }

    fn new_terminal(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_visible = true;
        self.bottom_panel
            .update(cx, |panel, cx| panel.new_terminal(window, cx));
        cx.notify();
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
