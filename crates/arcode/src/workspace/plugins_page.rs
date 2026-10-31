//! The Plugins page in the workspace (`plugins::page`): carrying out what
//! the user asks there (enable or disable a plugin, revoke a grant,
//! install from a folder, uninstall, restart, open its log), the master
//! network switch from the settings, and keeping the page current as
//! plugins start, crash and stop.
//!
//! Disabling a plugin (or uninstalling it) takes everything it put on
//! screen away at once: its panels and tabs, its sidebar entry, its
//! lenses, its status items and its palette commands. Its openers stop
//! applying (the manager skips disabled plugins).

use arcode_buffer::Buffer;
use arcode_editor::Editor;
use arcode_plugin_host::grants::Capability;
use arcode_plugin_host::{PluginId, PluginOutput, PluginSource};
use arcode_ui_kit::settings;
use gpui::{AppContext, Context, PromptLevel, Window};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::pane::TabLabel;
use crate::plugins::lenses::LENS_SET;
use crate::plugins::page::rows::enable_summary;
use crate::plugins::page::{PageTab, PluginsPage, PluginsPageEvent};

impl Workspace {
    /// Creates the Plugins page (around the Languages page), starts the
    /// consent prompts and follows the network setting.
    pub(super) fn plugins_page_init(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.plugins.client.clone();
        let languages = self.langs.view.clone();
        let page = cx.new(|cx| PluginsPage::new(languages, client, cx));
        let sub = cx.subscribe_in(&page, window, |ws, _, event, window, cx| {
            ws.plugins_page_event(event.clone(), window, cx)
        });
        self._subscriptions.push(sub);
        self._subscriptions.push(
            cx.observe_global::<arcode_ui_kit::Settings>(|ws, cx| ws.plugins_apply_settings(cx)),
        );
        self.plugins.page = Some(page);
        self.plugin_consent_loop(window, cx);
    }

    /// The sidebar content of the Plugins activity.
    pub(super) fn plugins_page_element(&self) -> gpui::AnyElement {
        use gpui::IntoElement;
        match &self.plugins.page {
            Some(page) => page.clone().into_any_element(),
            None => self.langs.view.clone().into_any_element(),
        }
    }

    /// Shows the Plugins page on `tab`.
    pub fn show_plugins_page(&mut self, tab: PageTab, cx: &mut Context<Self>) {
        self.activity = Activity::Plugins;
        self.sidebar_visible = true;
        if let Some(page) = &self.plugins.page {
            page.update(cx, |p, cx| p.set_tab(tab, cx));
        }
        cx.notify();
    }

    /// The Plugins page.
    pub fn plugins_page(&self) -> Option<&gpui::Entity<PluginsPage>> {
        self.plugins.page.as_ref()
    }

    /// Redraws the page if it is on screen.
    pub(super) fn plugins_page_refresh(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_visible
            && self.activity == Activity::Plugins
            && let Some(page) = &self.plugins.page
        {
            page.update(cx, |p, cx| p.refresh(cx));
        }
    }

    /// Applies `settings.plugins.network` to the manager.
    pub(super) fn plugins_apply_settings(&mut self, cx: &mut Context<Self>) {
        let network = settings(cx).plugins.network;
        self.plugins.client.set_network_enabled(network);
    }

    /// Keeps the page's run states current (called for every output).
    pub(super) fn plugins_page_output(&mut self, output: &PluginOutput, cx: &mut Context<Self>) {
        match output {
            PluginOutput::Crashed { plugin, .. } => {
                if let Some(page) = &self.plugins.page {
                    page.update(cx, |p, cx| p.set_crashed(plugin, true, cx));
                }
            }
            // Frequent, and nothing the page shows.
            PluginOutput::PanelView { .. }
            | PluginOutput::PanelTitle { .. }
            | PluginOutput::Status { .. } => return,
            _ => {}
        }
        self.plugins_page_refresh(cx);
    }

    fn plugins_page_event(
        &mut self,
        event: PluginsPageEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_plugins(window, cx) {
            return;
        }
        match event {
            PluginsPageEvent::SetEnabled(plugin, enabled) => {
                self.plugins_request_enabled(plugin, enabled, window, cx)
            }
            PluginsPageEvent::Revoke(plugin, capability) => {
                self.plugins_revoke(&plugin, &capability, window, cx)
            }
            PluginsPageEvent::Install => {
                self.prompt_folder("Install Plugin", window, cx, |ws, path, window, cx| {
                    ws.plugins_install_from(path, window, cx)
                })
            }
            PluginsPageEvent::Uninstall(plugin) => {
                self.plugins_confirm_uninstall(plugin, window, cx)
            }
            PluginsPageEvent::OpenLog(plugin) => self.plugins_show_log_of(&plugin, window, cx),
            PluginsPageEvent::Restart(plugin) => {
                if let Some(m) = self.plugins.client.manager() {
                    m.restart(&plugin).ok();
                }
                if let Some(page) = &self.plugins.page {
                    page.update(cx, |p, cx| p.set_crashed(&plugin, false, cx));
                }
            }
            PluginsPageEvent::Reload => self.reload_plugins(window, cx),
        }
        self.plugins_page_refresh(cx);
    }

    /// Enables (installed plugins: after showing what they may ask for)
    /// or disables `plugin`.
    fn plugins_request_enabled(
        &mut self,
        plugin: PluginId,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self
            .plugins
            .page
            .as_ref()
            .and_then(|p| p.read(cx).rows().into_iter().find(|r| r.id == plugin));
        let Some(row) = row else {
            return;
        };
        if !enabled || row.source != PluginSource::Installed {
            self.plugins_set_enabled(&plugin, enabled, window, cx);
            return;
        }
        let summary = enable_summary(&row);
        let answer = window.prompt(
            PromptLevel::Info,
            "Enable this plugin?",
            Some(&summary),
            &["Enable", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |ws, window, cx| {
                    ws.plugins_set_enabled(&plugin, true, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Turns `plugin` on or off and updates everything it contributes.
    pub(crate) fn plugins_set_enabled(
        &mut self,
        plugin: &PluginId,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !enabled {
            self.plugins_take_down(plugin, window, cx);
        }
        self.plugins.client.set_enabled(plugin, enabled);
        if let Some(page) = &self.plugins.page {
            page.update(cx, |p, cx| p.set_crashed(plugin, false, cx));
        }
        self.plugin_sync_contributions(cx);
        self.plugins_tell_other_windows(window, cx);
        let name = self.plugins.client.plugin_name(plugin);
        let state = if enabled { "enabled" } else { "disabled" };
        self.plugins.log(format!("{name}: {state}"));
        self.show_toast(format!("{name} {state}"), cx);
        self.plugins_page_refresh(cx);
    }

    /// Removes what `plugin` shows: panels and tabs, its sidebar entry,
    /// lenses, status items.
    pub(super) fn plugins_take_down(
        &mut self,
        plugin: &PluginId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keys: Vec<_> = self
            .plugins
            .panels
            .keys()
            .filter(|(p, _)| p == plugin)
            .cloned()
            .collect();
        for key in keys {
            if let Some(view) = self.plugins.panels.get(&key).and_then(|s| s.view.upgrade()) {
                view.update(cx, |v, cx| v.retire("the plugin was disabled".into(), cx));
            }
            self.plugin_close_panel(&key, window, cx);
        }
        self.plugins
            .sidebar_views
            .retain(|id, _| id.plugin() != plugin.as_str());
        if self.activity == Activity::Extension {
            self.plugins.sidebar_active = None;
            self.activity = Activity::Plugins;
        }
        let editors: Vec<_> = self
            .plugins
            .editors
            .values()
            .filter_map(|t| t.editor.upgrade())
            .collect();
        for editor in editors {
            editor.update(cx, |e, cx| e.set_code_lenses(LENS_SET, Vec::new(), cx));
        }
        for tracked in self.plugins.editors.values_mut() {
            tracked.lens_version = None;
            tracked.lens_targets.clear();
            tracked.pending_click = None;
        }
        self.plugins
            .status
            .update(cx, |s, cx| s.forget_plugin(plugin, cx));
    }

    fn plugins_revoke(
        &mut self,
        plugin: &PluginId,
        capability: &Capability,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let removed = self
            .plugins
            .client
            .manager()
            .is_some_and(|m| m.revoke(plugin, capability));
        if removed {
            let name = self.plugins.client.plugin_name(plugin);
            let text = format!("{name}: revoked \u{201c}{capability}\u{201d}; it asks again");
            self.plugins.log(text.clone());
            self.show_toast(text, cx);
            self.plugins_tell_other_windows(window, cx);
        }
    }

    /// One plugin's log and network log in a read-only tab (replacing an
    /// open one of that plugin).
    fn plugins_show_log_of(
        &mut self,
        plugin: &PluginId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::plugins::log::{plugin_log_key, render_plugin};
        let text = {
            let manager = self.plugins.client.manager();
            render_plugin(manager.as_deref(), plugin)
        };
        let key = plugin_log_key(plugin);
        for pane in self.panes.clone() {
            if let Some(ix) = pane.read(cx).position_of_key(&key) {
                pane.update(cx, |p, cx| p.close(ix, window, cx));
            }
        }
        let editor = cx.new(|cx| {
            let mut e = Editor::new(Buffer::new(&text), cx);
            e.set_read_only(true, cx);
            e
        });
        let name = self.plugins.client.plugin_name(plugin);
        let label = TabLabel {
            title: format!("{name} Log").into(),
            key: key.into(),
            tooltip: "Plugin log and network log (reopen to refresh)".into(),
        };
        self.active_pane()
            .clone()
            .update(cx, |p, cx| p.add_titled_editor(editor, label, window, cx));
        cx.notify();
    }
}
