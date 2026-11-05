//! The workspace side of the menu bar: what its items depend on (project,
//! repository, debug session, the active editor), the tap on Alt and
//! Alt+letter.

use gpui::{
    App, Context, Focusable, InteractiveElement, KeyDownEvent, ModifiersChangedEvent,
    MouseDownEvent, MouseUpEvent, Window,
};

use super::Workspace;
use crate::menu_bar::model::Facts;
use crate::menu_bar::{MenuFacts, ToggleMenuBar};

impl Workspace {
    /// The state the menu items are enabled by.
    pub(crate) fn menu_facts(&self, cx: &App) -> MenuFacts {
        let debugger = self.debug.debugger.read(cx);
        let paused = debugger
            .session()
            .is_some_and(|s| !s.is_terminated() && s.is_stopped());
        let repo = self.project.is_some() && self.git.store.read(cx).active_repo().is_some();
        let editor = self.active_editor(cx);
        let history = self.nav.history();
        MenuFacts {
            facts: Facts {
                project: self.project.is_some(),
                repo,
                debugging: debugger.is_active(),
                paused,
                editor: editor.is_some(),
                file: editor.as_ref().is_some_and(|e| e.read(cx).path().is_some()),
                back: history.can_go_back(),
                forward: history.can_go_forward(),
            },
            project_root: self.project_root().map(|p| p.to_path_buf()),
            editor_focus: editor.map(|e| e.focus_handle(cx)),
        }
    }

    /// The menu bar entity of this window.
    pub fn menu_bar(&self) -> &gpui::Entity<crate::menu_bar::MenuBar> {
        &self.menu_bar
    }

    /// Whether a dialog, picker or popup of the workspace is open: the menu
    /// bar stays out of the way of whatever has the keyboard there.
    pub(crate) fn overlay_open(&self) -> bool {
        #[cfg(feature = "plugins")]
        let plugin_pick = self.plugins.quick_pick.is_some();
        #[cfg(not(feature = "plugins"))]
        let plugin_pick = false;
        self.modal_open()
            || plugin_pick
            || self.palette.is_some()
            || self.finder.is_some()
            || self.branch_picker.is_some()
            || self.path_prompt.is_some()
            || self.project_menu_open
            || self.new_project.is_some()
            || self.list_picker().is_some()
            || self.prefs.picker.is_some()
            || self.nav.picker().is_some()
            || self.debug.run.menu.is_some()
            || self.debug.run.prompt.is_some()
            || self.debug.bp_dialog.is_some()
    }

    /// A tap on Alt (re)activates the bar; Alt+letter opens a menu when no
    /// view on the focus path used the key (the terminal keeps Alt+F and
    /// Alt+B for its shell). Neither acts while a dialog or picker is open.
    /// Alt used with the mouse (Alt+click, Alt+Shift+drag, a window manager
    /// moving the window on Alt+drag) is not a tap. On macOS the native
    /// menus follow the active window instead.
    pub(super) fn register_menu_bar<E: InteractiveElement>(
        &self,
        el: E,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> E {
        if cfg!(target_os = "macos") {
            if window.is_window_active() {
                crate::menu_bar::sync_native_menus(self.menu_facts(cx).facts, cx);
            }
            return el;
        }
        el.on_modifiers_changed(
            cx.listener(|ws, event: &ModifiersChangedEvent, window, cx| {
                let bounds = window.bounds();
                ws.menu_bar
                    .update(cx, |bar, _| bar.alt.modifiers(event.modifiers, bounds));
            }),
        )
        .capture_any_mouse_down(cx.listener(|ws, event: &MouseDownEvent, _, cx| {
            if event.modifiers.alt {
                ws.menu_bar
                    .update(cx, |bar, _| bar.alt.mouse(event.modifiers));
            }
        }))
        .capture_any_mouse_up(cx.listener(|ws, event: &MouseUpEvent, _, cx| {
            if event.modifiers.alt {
                ws.menu_bar
                    .update(cx, |bar, _| bar.alt.mouse(event.modifiers));
            }
        }))
        .on_action(cx.listener(|ws, _: &ToggleMenuBar, window, cx| {
            let bounds = window.bounds();
            let tap = ws.menu_bar.update(cx, |bar, _| bar.alt.take_tap(bounds));
            if !tap || ws.overlay_open() {
                return;
            }
            let facts = ws.menu_facts(cx);
            ws.menu_bar
                .update(cx, |bar, cx| bar.toggle(facts, window, cx));
        }))
        .on_key_down(cx.listener(|ws, event: &KeyDownEvent, window, cx| {
            let m = event.keystroke.modifiers;
            if !m.alt || m.control || m.platform || m.shift || m.function || ws.overlay_open() {
                return;
            }
            let mut chars = event.keystroke.key.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                return;
            };
            if !ch.is_ascii_alphabetic() {
                return;
            }
            let facts = ws.menu_facts(cx);
            let opened = ws.menu_bar.update(cx, |bar, cx| {
                bar.open_mnemonic(ch.to_ascii_lowercase(), facts, window, cx)
            });
            if opened {
                cx.stop_propagation();
            }
        }))
    }
}
