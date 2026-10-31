//! Installing plugins from a folder and uninstalling them, from the
//! Plugins page and the palette (`plugins_page.rs` has the rest).

use arcode_plugin_host::PluginId;
use arcode_plugin_host::install::{install_from_folder, precompile};
use gpui::{AppContext, Context, PromptLevel, Window};

use super::Workspace;
use crate::plugins::page::PageTab;

impl Workspace {
    /// Copies a plugin folder with the host's installer (validation,
    /// sha256 of the copied `plugin.wasm`, precompile) on a background
    /// thread. The new plugin is listed disabled.
    pub(crate) fn plugins_install_from(
        &mut self,
        src: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_plugins(window, cx) {
            return;
        }
        let Some((runtime, installed)) = self.plugins.client.install_target() else {
            return;
        };
        if let Some(page) = &self.plugins.page {
            page.update(cx, |p, cx| p.set_busy(Some("INSTALLING…".into()), cx));
        }
        let work = cx.background_spawn(async move {
            let report = install_from_folder(&src, &installed)?;
            precompile(&runtime, &report)?;
            Ok::<_, arcode_plugin_host::install::InstallError>(report)
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            this.update_in(cx, |ws, window, cx| {
                if let Some(page) = &ws.plugins.page {
                    page.update(cx, |p, cx| p.set_busy(None, cx));
                }
                let text = match result {
                    Ok(report) => {
                        ws.plugins.client.finish_install(&report);
                        ws.plugin_sync_contributions(cx);
                        ws.plugins_tell_other_windows(window, cx);
                        let name = ws.plugins.client.plugin_name(&report.id);
                        if ws
                            .plugins
                            .client
                            .manager()
                            .is_some_and(|m| m.is_enabled(&report.id))
                        {
                            format!("{name} updated")
                        } else {
                            format!("{name} installed; enable it to run it")
                        }
                    }
                    Err(e) => e.to_string(),
                };
                ws.plugins.log(text.clone());
                ws.show_toast(text, cx);
                ws.show_plugins_page(PageTab::Installed, cx);
                ws.plugins_page_refresh(cx);
            })
            .ok();
        });
        self.plugins.tasks.push(task);
    }

    pub(super) fn plugins_confirm_uninstall(
        &mut self,
        plugin: PluginId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.plugins.client.plugin_name(&plugin);
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Uninstall {name}?"),
            Some("Its folder and the permissions you gave it, in every project, are removed."),
            &["Uninstall", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |ws, window, cx| {
                    ws.plugins_uninstall(&plugin, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Removes an installed plugin and what it shows.
    pub(crate) fn plugins_uninstall(
        &mut self,
        plugin: &PluginId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.plugins.client.plugin_name(plugin);
        self.plugins_take_down(plugin, window, cx);
        let text = match self.plugins.client.uninstall(plugin) {
            Ok(()) => format!("{name} uninstalled"),
            Err(e) => e.to_string(),
        };
        self.plugin_sync_contributions(cx);
        self.plugins_tell_other_windows(window, cx);
        self.plugins.log(text.clone());
        self.show_toast(text, cx);
        self.plugins_page_refresh(cx);
    }
}
