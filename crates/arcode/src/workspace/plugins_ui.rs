//! Plugin output in the workspace: panels as tabs or sidebar views,
//! openers, toasts for notifications and crashes, status and task chips,
//! consent prompts, quick picks, editor writes and the plugin log tab.

use arcode_plugin_host::bindings::ui::Location;
use arcode_plugin_host::manifest::PanelLocation;
use arcode_plugin_host::{PluginOutput, QualifiedId};
use arcode_ui_kit::IconName;
use gpui::{AppContext, Context, Entity, Focusable, Window};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::pane::PageTab;
use crate::plugins::commands::icon_for;
use crate::plugins::status::RunningTask;
use crate::plugins::{PanelKey, PanelSlot, PanelView, PanelViewEvent, Placement};

impl Workspace {
    pub(super) fn on_plugin_output(
        &mut self,
        output: PluginOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.plugins_page_output(&output, cx);
        match output {
            PluginOutput::Notify {
                plugin, message, ..
            } => {
                let text = format!("{}: {message}", self.plugins.client.plugin_name(&plugin));
                self.plugins.log(text.clone());
                self.show_toast(text, cx);
            }
            PluginOutput::Status {
                plugin,
                text,
                command,
            } => self
                .plugins
                .status
                .update(cx, |s, cx| s.set_item(plugin, text, command, cx)),
            PluginOutput::QuickPick {
                title,
                items,
                reply,
                ..
            } => self.plugin_quick_pick(&title, items, reply, window, cx),
            PluginOutput::Consent { request, reply } => {
                self.plugin_consent(request, reply, window, cx)
            }
            PluginOutput::PanelOpened {
                plugin,
                panel,
                contribution,
                title,
                location,
            } => self.plugin_panel_opened(
                (plugin, panel),
                &contribution,
                title,
                location,
                window,
                cx,
            ),
            PluginOutput::PanelView {
                plugin,
                panel,
                snapshot,
                changes,
            } => {
                let view = self
                    .plugins
                    .panels
                    .get(&(plugin, panel))
                    .and_then(|s| s.view.upgrade());
                if let Some(view) = view {
                    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
                }
            }
            PluginOutput::PanelTitle {
                plugin,
                panel,
                title,
            } => {
                let key = self.plugins.page_key(&(plugin, panel));
                for pane in self.panes.clone() {
                    pane.update(cx, |p, cx| p.set_page_title(&key, title.clone().into(), cx));
                }
            }
            PluginOutput::PanelClosed { plugin, panel } => {
                self.plugin_close_panel(&(plugin, panel), window, cx)
            }
            PluginOutput::TaskStarted {
                plugin,
                call,
                title,
                cancel,
            } => {
                let task = RunningTask {
                    plugin,
                    call,
                    title: title.into(),
                    cancel,
                };
                self.plugins
                    .status
                    .update(cx, |s, cx| s.task_started(task, cx));
                self.plugin_install_esc(cx);
            }
            PluginOutput::TaskFinished { call, .. } => self
                .plugins
                .status
                .update(cx, |s, cx| s.task_finished(call, cx)),
            PluginOutput::Crashed {
                plugin,
                reason,
                disabled,
            } => {
                let name = self.plugins.client.plugin_name(&plugin);
                let text = if disabled {
                    format!("{name} stopped: {reason}. It is disabled for this session.")
                } else {
                    format!("{name} stopped: {reason}")
                };
                self.plugins.log(text.clone());
                self.show_toast(text, cx);
                let views: Vec<Entity<PanelView>> = self
                    .plugins
                    .panels
                    .iter()
                    .filter(|((p, _), _)| *p == plugin)
                    .filter_map(|(_, s)| s.view.upgrade())
                    .collect();
                for view in views {
                    view.update(cx, |v, cx| v.set_stopped(reason.clone(), disabled, cx));
                }
                self.plugins
                    .status
                    .update(cx, |s, cx| s.forget_plugin(&plugin, cx));
            }
            PluginOutput::Unloaded { plugin } => {
                let name = self.plugins.client.plugin_name(&plugin);
                self.plugins
                    .log(format!("{name}: unloaded after being idle"));
            }
            PluginOutput::NeedsApproval { plugin } => {
                let name = self.plugins.client.plugin_name(&plugin);
                let text = format!(
                    "{name} changed since you enabled it; it does not run until you enable it again."
                );
                self.plugins.log(text.clone());
                self.show_toast(text, cx);
            }
        }
    }

    /// A view for a plugin panel, remembered in the slot table.
    pub(super) fn plugin_new_view(
        &mut self,
        key: &PanelKey,
        placement: Placement,
        cx: &mut Context<Self>,
    ) -> Entity<PanelView> {
        let client = self.plugins.client.clone();
        let snapshot = client
            .manager()
            .and_then(|m| m.instance(&key.0).ok())
            .and_then(|h| h.panel_view(key.1));
        let (plugin, panel) = key.clone();
        let view = cx.new(|cx| PanelView::new(client, plugin, panel, snapshot, cx));
        let sub = cx.subscribe(&view, |ws, _, event: &PanelViewEvent, cx| match event {
            PanelViewEvent::Message(text) => {
                ws.plugins.log(text.clone());
                ws.show_toast(text.clone(), cx);
            }
        });
        self.plugins.panels.insert(
            key.clone(),
            PanelSlot {
                view: view.downgrade(),
                placement,
                sub,
            },
        );
        view
    }

    /// Activates the tab of a panel shown as a page. False if it is gone.
    pub(super) fn plugin_activate_page(
        &mut self,
        key: &PanelKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let page = self.plugins.page_key(key);
        for (ix, pane) in self.panes.clone().into_iter().enumerate() {
            if let Some(pos) = pane.read(cx).page_position(&page) {
                self.active_pane = ix;
                pane.update(cx, |p, cx| p.activate_page(pos, window, cx));
                return true;
            }
        }
        false
    }

    /// Shows a panel as a tab: in the active pane, or `beside` it (the pane
    /// to its right, split off if needed, keeping focus where it was).
    pub(super) fn plugin_add_page(
        &mut self,
        key: &PanelKey,
        title: String,
        icon: IconName,
        beside: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page_key = self.plugins.page_key(key);
        let view = self.plugin_new_view(key, Placement::Page(page_key.clone()), cx);
        let tab = PageTab {
            key: page_key,
            title: title.into(),
            icon,
            focus: view.focus_handle(cx),
            view: view.into(),
        };
        if !beside {
            self.active_pane()
                .clone()
                .update(cx, |p, cx| p.add_page(tab, window, cx));
            return;
        }
        let home = self.active_pane;
        if home + 1 >= self.panes.len() {
            self.split_right(window, cx);
        }
        let target = self.panes[(home + 1).min(self.panes.len() - 1)].clone();
        target.update(cx, |p, cx| p.add_page(tab, window, cx));
        self.active_pane = home.min(self.panes.len() - 1);
        self.active_pane().read(cx).focus(window, cx);
        self.active_editor_changed(cx);
        cx.notify();
    }

    pub(super) fn plugin_panel_opened(
        &mut self,
        key: PanelKey,
        contribution: &QualifiedId,
        title: String,
        location: Option<Location>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let manifest = self.plugins.client.manager().and_then(|m| {
            m.registry()
                .panel(contribution)
                .map(|(_, p)| (p.location, p.icon.clone()))
        });
        let (default, icon) = manifest.unwrap_or((PanelLocation::Editor, None));
        let sidebar_ix = self
            .plugins
            .contributions
            .sidebar
            .iter()
            .position(|s| &s.id == contribution);
        let place = match location {
            Some(Location::Sidebar) => PanelLocation::Sidebar,
            Some(Location::Bottom) => PanelLocation::Bottom,
            Some(Location::Editor) => PanelLocation::Editor,
            Some(Location::EditorBeside) => PanelLocation::EditorBeside,
            None => default,
        };
        if place == PanelLocation::Sidebar
            && let Some(ix) = sidebar_ix
        {
            if !self.plugins.sidebar_views.contains_key(contribution) {
                let view = self.plugin_new_view(&key, Placement::Sidebar(contribution.clone()), cx);
                self.plugins
                    .sidebar_views
                    .insert(contribution.clone(), view);
            }
            self.plugins.sidebar_active = Some(ix);
            self.activity = Activity::Extension;
            self.sidebar_visible = true;
            cx.notify();
            return;
        }
        let alive = self
            .plugins
            .panels
            .get(&key)
            .is_some_and(|s| s.view.upgrade().is_some());
        let beside = place == PanelLocation::EditorBeside;
        let home = self.active_pane;
        if alive && self.plugin_activate_page(&key, window, cx) {
            if beside && home != self.active_pane && home < self.panes.len() {
                // Shown again beside the editor: the editor keeps focus.
                self.active_pane = home;
                self.active_pane().read(cx).focus(window, cx);
                self.active_editor_changed(cx);
            }
            return;
        }
        // Bottom panels are shown as tabs too: the bottom panel has no
        // plugin tabs yet.
        self.plugin_add_page(&key, title, icon_for(icon.as_deref()), beside, window, cx);
    }

    pub(super) fn plugin_close_panel(
        &mut self,
        key: &PanelKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.plugins.panels.remove(key) else {
            return;
        };
        match slot.placement {
            Placement::Page(page) => self.plugin_close_pages(&page, window, cx),
            Placement::Sidebar(id) => {
                self.plugins.sidebar_views.remove(&id);
                cx.notify();
            }
        }
        self.plugins.opened_files.retain(|_, k| k != key);
    }

    /// Closes the tabs of `page`, and a split pane left empty by that (the
    /// plugin opened it beside the editor; the user did not).
    fn plugin_close_pages(&mut self, page: &str, window: &mut Window, cx: &mut Context<Self>) {
        for pane in self.panes.clone() {
            if let Some(ix) = pane.read(cx).page_position(page) {
                pane.update(cx, |p, cx| p.close_page(ix, window, cx));
                if !pane.read(cx).has_tabs()
                    && let Some(pane_ix) = self.panes.iter().position(|p| *p == pane)
                {
                    self.close_pane(pane_ix, window, cx);
                }
            }
        }
    }

    /// Every instance was stopped ("Plugins: Reload", a project switch) and
    /// panel ids start at 1 again: the shown panels are retired so none of
    /// them talks to a new panel with its id. With a `window` their tabs
    /// are closed; without one they stay with a "stopped" banner. New
    /// panels get new page keys (`Plugins::generation`).
    pub(super) fn plugin_retire_panels(
        &mut self,
        reason: &str,
        mut window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.plugins.generation += 1;
        let slots: Vec<_> = self.plugins.panels.drain().map(|(_, s)| s).collect();
        for slot in slots {
            if let Some(view) = slot.view.upgrade() {
                view.update(cx, |v, cx| v.retire(reason.to_owned(), cx));
            }
            if let (Placement::Page(page), Some(window)) = (&slot.placement, window.as_deref_mut())
            {
                self.plugin_close_pages(page, window, cx);
            }
        }
        self.plugins.opened_files.clear();
        self.plugins.sidebar_views.clear();
        if self.activity == Activity::Extension {
            self.plugins.sidebar_active = None;
            self.activity = Activity::Files;
        }
        cx.notify();
    }
}
