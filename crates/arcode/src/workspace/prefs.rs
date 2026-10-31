//! Preferences in the workspace: the Settings page tab, the first-run
//! dialog, low-end mode and the preferences overlays (the theme and font
//! pickers themselves are in `prefs_pickers.rs`).

use arcode_config::{Config, low_end_edits};
use arcode_ui_kit::{IconName, settings, theme};
use gpui::{
    AppContext, Context, Div, Focusable, InteractiveElement, MouseButton, Stateful, Window, div,
    prelude::*, px,
};

use super::Workspace;
use super::overlays::fade_in;
use crate::pane::PageTab;
use crate::prefs::first_run::{FirstRunDialog, FirstRunEvent};
use crate::prefs::pickers::{FontTarget, ThemeSlot};
use crate::prefs::store::write_and_report;
use crate::prefs::*;
use crate::settings_ui::{PAGE_KEY, SettingsEvent, SettingsPage};
use crate::theme_sync;
use crate::title_bar::TITLE_BAR_HEIGHT;

impl Workspace {
    pub(super) fn register_prefs_actions<E: InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(
            cx.listener(|ws, _: &OpenSettingsFile, window, cx| ws.open_settings_file(window, cx)),
        )
        .on_action(cx.listener(|ws, _: &SelectTheme, window, cx| {
            ws.open_theme_picker(ThemeSlot::Main, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &SelectEditorFont, window, cx| {
            ws.open_font_picker(FontTarget::Editor, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &SelectUiFont, window, cx| {
            ws.open_font_picker(FontTarget::Ui, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &SelectTerminalFont, window, cx| {
            ws.open_font_picker(FontTarget::Terminal, window, cx)
        }))
        .on_action(cx.listener(|ws, _: &ToggleLowEndMode, _, cx| {
            let on = !settings(cx).ui.low_end_mode;
            write_and_report(low_end_edits(on), cx.weak_entity(), cx);
            ws.show_toast(
                if on {
                    "Low-end mode on: no animations, inlay hints or inline blame"
                } else {
                    "Low-end mode off"
                },
                cx,
            );
        }))
        .on_action(cx.listener(|ws, _: &StartTour, window, cx| ws.start_tour(window, cx)))
        .on_action(
            cx.listener(|ws, _: &ShowWelcomeSetup, window, cx| ws.show_first_run(window, cx)),
        )
    }

    /// Shows the Settings page tab (activating it where it is open).
    pub fn open_settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        for (ix, pane) in self.panes.clone().into_iter().enumerate() {
            if let Some(page_ix) = pane.read(cx).page_position(PAGE_KEY) {
                self.active_pane = ix;
                pane.update(cx, |pane, cx| pane.activate_page(page_ix, window, cx));
                return;
            }
        }
        let page = match self.prefs.page.clone() {
            Some(page) => page,
            None => {
                let ws = cx.weak_entity();
                let page = cx.new(|cx| SettingsPage::new(ws, cx));
                let sub =
                    cx.subscribe_in(&page, window, |ws, _, event: &SettingsEvent, window, cx| {
                        ws.settings_event(event.clone(), window, cx)
                    });
                self.subs.set("settings_page", sub);
                self.prefs.page = Some(page.clone());
                page
            }
        };
        let tab = PageTab {
            key: PAGE_KEY,
            title: "Settings".into(),
            icon: IconName::Settings,
            view: page.clone().into(),
            focus: page.focus_handle(cx),
        };
        self.active_pane()
            .clone()
            .update(cx, |pane, cx| pane.add_page(tab, window, cx));
        cx.notify();
    }

    /// The Settings page, if it was opened.
    pub fn settings_page(&self) -> Option<&gpui::Entity<SettingsPage>> {
        self.prefs.page.as_ref()
    }

    /// Ends the Settings page's theme hover preview once the page is no
    /// longer shown (its card cannot report the pointer leaving then).
    pub(super) fn end_hidden_settings_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.prefs.page.clone() else {
            return;
        };
        if !page.read(cx).hover_previewing() {
            return;
        }
        let shown = self.panes.iter().any(|pane| {
            pane.read(cx)
                .active_page()
                .is_some_and(|p| p.key == PAGE_KEY)
        });
        if !shown {
            page.update(cx, |page, cx| page.end_hover_preview(window, cx));
        }
    }

    fn settings_event(
        &mut self,
        event: SettingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SettingsEvent::PickTheme(slot) => self.open_theme_picker(slot, window, cx),
            SettingsEvent::PickFont(target) => self.open_font_picker(target, window, cx),
            SettingsEvent::OpenFile => self.open_settings_file(window, cx),
            SettingsEvent::StartTour => self.start_tour(window, cx),
            SettingsEvent::ShowWelcome => self.show_first_run(window, cx),
        }
    }

    /// Opens `config.toml` in an editor tab, writing the commented
    /// template first when it does not exist.
    pub fn open_settings_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = crate::prefs::store::config_path(cx) else {
            self.show_toast("No configuration directory is available", cx);
            return;
        };
        let create = {
            let path = path.clone();
            cx.background_spawn(async move { Config::write_default_if_missing(&path) })
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = create.await;
            this.update_in(cx, |ws, window, cx| match result {
                Ok(_) => ws.open_file(&path, window, cx),
                Err(err) => ws.show_toast(format!("Cannot create {}: {err}", path.display()), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Whether a modal overlay (the first-run dialog or the tour) is up:
    /// workspace shortcuts are off meanwhile.
    pub fn modal_open(&self) -> bool {
        self.prefs.first_run.is_some() || self.prefs.tour.is_some()
    }

    /// Shows the first-run dialog (starting from the current settings).
    pub fn show_first_run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_prefs_picker(true, window, cx);
        self.close_palette(window, cx);
        self.close_finder(window, cx);
        self.project_menu_open = false;
        let dialog = cx.new(FirstRunDialog::new);
        let sub = cx.subscribe_in(
            &dialog,
            window,
            |ws, _, event: &FirstRunEvent, window, cx| match event {
                FirstRunEvent::Finished { tour } => {
                    ws.prefs.first_run = None;
                    theme_sync::apply(window.appearance(), cx);
                    ws.active_pane().read(cx).focus(window, cx);
                    if *tour {
                        ws.start_tour(window, cx);
                    }
                    cx.notify();
                }
                FirstRunEvent::Failed(message) => ws.show_toast(message.clone(), cx),
            },
        );
        self.subs.set("first_run", sub);
        window.focus(&dialog.focus_handle(cx));
        self.prefs.first_run = Some(dialog);
        cx.notify();
    }

    /// The first-run dialog, while shown.
    pub fn first_run_dialog(&self) -> Option<&gpui::Entity<FirstRunDialog>> {
        self.prefs.first_run.as_ref()
    }

    /// Adds the preferences picker and the first-run dialog to `root`.
    pub(super) fn render_prefs_overlays(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let mut root = root;
        if let Some((picker, _)) = &self.prefs.picker {
            let panel = div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(picker.clone());
            root = root.child(
                div()
                    .id("prefs-picker-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .flex()
                    .justify_center()
                    .pt(px(TITLE_BAR_HEIGHT + 8.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, _, window, cx| ws.close_prefs_picker(true, window, cx)),
                    )
                    .child(fade_in("prefs-picker-anim", panel, cx)),
            );
        }
        if let Some(dialog) = &self.prefs.first_run {
            let dim = if theme(cx).is_dark {
                c.background.opacity(0.72)
            } else {
                c.text.opacity(0.4)
            };
            root = root.child(
                div()
                    .id("first-run-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(dim)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(fade_in("first-run-anim", div().child(dialog.clone()), cx)),
            );
        }
        root
    }
}
