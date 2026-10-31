//! The Plugins page (Plugins activity): an "Installed" tab listing every
//! WASM plugin (bundled, installed, unpacked) next to the Languages tab
//! (`languages::view`, unchanged inside it).
//!
//! Each card shows name, version, description, where the plugin comes
//! from and its state (inactive / active / crashed / disabled), an enable
//! switch, what its manifest lets it ask for, the grants the user gave it
//! with a Revoke button each, and Open Log, Restart (after a crash) and
//! Uninstall (installed plugins only). "Install from Folder…" copies a
//! plugin folder with the host's installer. There is no marketplace and
//! nothing here uses the network.
//!
//! The page only reads the manager; every change is a
//! [`PluginsPageEvent`] the workspace carries out (it also has to close the
//! plugin's panels, lenses and palette commands) before it calls
//! [`PluginsPage::refresh`].

mod card;
pub mod rows;
#[cfg(test)]
mod tests;

use std::collections::HashSet;

use arcode_plugin_host::PluginId;
use arcode_plugin_host::grants::Capability;
use arcode_ui_kit::{IconName, badge, icon_button, pill_tab, section_header, spacer, theme};
use gpui::{Context, Entity, EventEmitter, IntoElement, SharedString, Window, div, prelude::*, px};

use super::PluginClient;
use crate::languages::view::LanguagesView;
use rows::PluginRow;

/// The page's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTab {
    /// WASM plugins.
    Installed,
    /// Language packs.
    Languages,
}

/// What the user asked for; the workspace does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsPageEvent {
    /// Turn a plugin on or off.
    SetEnabled(PluginId, bool),
    /// Forget a grant.
    Revoke(PluginId, Capability),
    /// "Install from Folder…".
    Install,
    /// Remove an installed plugin (after a confirmation).
    Uninstall(PluginId),
    /// Show the plugin's log and network log.
    OpenLog(PluginId),
    /// Run a plugin again after it was disabled for crashing.
    Restart(PluginId),
    /// Re-read every manifest.
    Reload,
}

/// The page view.
pub struct PluginsPage {
    tab: PageTab,
    languages: Entity<LanguagesView>,
    client: PluginClient,
    /// Plugins the app saw crash since they last ran.
    crashed: HashSet<PluginId>,
    /// Cards showing what the plugin may ask for.
    expanded: HashSet<PluginId>,
    /// A long operation in progress ("Installing…").
    busy: Option<SharedString>,
}

impl EventEmitter<PluginsPageEvent> for PluginsPage {}

impl PluginsPage {
    /// The page around the workspace's Languages page.
    pub fn new(
        languages: Entity<LanguagesView>,
        client: PluginClient,
        cx: &mut Context<Self>,
    ) -> Self {
        languages.update(cx, |v, cx| v.set_embedded(true, cx));
        Self {
            tab: PageTab::Installed,
            languages,
            client,
            crashed: HashSet::new(),
            expanded: HashSet::new(),
            busy: None,
        }
    }

    /// The shown tab.
    pub fn tab(&self) -> PageTab {
        self.tab
    }

    /// Shows `tab`; the Languages tab looks its servers up when shown.
    pub fn set_tab(&mut self, tab: PageTab, cx: &mut Context<Self>) {
        self.tab = tab;
        if tab == PageTab::Languages {
            self.languages.update(cx, |v, cx| v.shown(cx));
        }
        cx.notify();
    }

    /// The rows the Installed tab lists (empty before plugins load).
    pub fn rows(&self) -> Vec<PluginRow> {
        self.client
            .manager()
            .map(|m| rows::rows(&m, &self.crashed))
            .unwrap_or_default()
    }

    /// The manager changed (enable, grants, install, run state): redraw.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// Records a crash (or, `false`, a restart) of `plugin`.
    pub fn set_crashed(&mut self, plugin: &PluginId, crashed: bool, cx: &mut Context<Self>) {
        let changed = if crashed {
            self.crashed.insert(plugin.clone())
        } else {
            self.crashed.remove(plugin)
        };
        if changed {
            cx.notify();
        }
    }

    /// Shows (or, `None`, clears) a progress line.
    pub fn set_busy(&mut self, busy: Option<SharedString>, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    fn toggle_expanded(&mut self, id: &PluginId, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.clone());
        }
        cx.notify();
    }

    /// Installed / Languages as pill tabs (docs/DESIGN.md 6.6).
    fn render_tabs(&self, cx: &mut Context<Self>) -> gpui::Div {
        let tab = |id: &'static str, label: &'static str, which: PageTab| {
            pill_tab(id, label)
                .panel()
                .active(self.tab == which)
                .on_click(cx.listener(move |p, _, _, cx| p.set_tab(which, cx)))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(3.))
            .px(px(6.))
            .pb(px(6.))
            .child(tab(
                "plugins-tab-installed",
                "Installed",
                PageTab::Installed,
            ))
            .child(tab(
                "plugins-tab-languages",
                "Languages",
                PageTab::Languages,
            ))
    }

    fn render_installed(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let loaded = self.client.is_loaded();
        let rows = self.rows();
        let busy = self.busy.as_ref().map(|b| b.to_string());
        let toolbar = section_header("Installed", cx)
            .child(badge(rows.len().to_string()))
            .when_some(busy, |el, busy| {
                el.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(c.text_muted)
                        .child(busy),
                )
            })
            .child(spacer())
            .child(
                div()
                    .flex()
                    .gap_0p5()
                    .child(
                        icon_button("plugins-install", IconName::FolderOpen)
                            .size(px(24.))
                            .tooltip("Install from Folder…")
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(PluginsPageEvent::Install)),
                            ),
                    )
                    .child(
                        icon_button("plugins-reload", IconName::Refresh)
                            .size(px(24.))
                            .tooltip("Reload plugins")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PluginsPageEvent::Reload))),
                    ),
            );
        let cards: Vec<_> = rows
            .iter()
            .map(|row| {
                let expanded = self.expanded.contains(&row.id);
                card::render(row, expanded, cx).into_any_element()
            })
            .collect();
        let empty = (loaded && rows.is_empty()).then(|| {
            div()
                .px_3()
                .text_xs()
                .text_color(c.text_muted)
                .child("No plugins. Install one from a folder (plugin.toml + plugin.wasm).")
        });
        let loading = (!loaded).then(|| {
            div()
                .px_3()
                .text_xs()
                .text_color(c.text_muted)
                .child("Plugins are not loaded yet.")
        });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .id("plugins-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pb_3()
                    .children(loading)
                    .children(empty)
                    .children(cards),
            )
    }
}

impl Render for PluginsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.tab {
            PageTab::Installed => self.render_installed(cx).into_any_element(),
            PageTab::Languages => div()
                .flex_1()
                .min_h_0()
                .child(self.languages.clone())
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(crate::sidebar::header("Plugins", cx))
            .child(self.render_tabs(cx))
            .child(body)
    }
}
