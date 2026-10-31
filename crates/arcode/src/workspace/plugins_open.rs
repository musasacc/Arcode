//! Where plugin views come from besides `ui.open-panel`: openers (a file
//! of a matching pattern opens in the plugin's view instead of the text
//! editor) and sidebar panels (activity-bar entries after the built-in
//! ones).

use std::path::Path;

use arcode_ui_kit::IconName;
use gpui::{Context, Window};

use super::Workspace;
use crate::activity_bar::Activity;
use crate::plugins::Placement;
use crate::plugins::commands::icon_for;

impl Workspace {
    /// Opens `path` with a plugin opener (e.g. a `.db` file in the DB
    /// viewer) instead of the text editor. False when no opener applies.
    pub(super) fn open_with_plugin(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rel) = self.plugin_relative(path) else {
            return false;
        };
        if path.is_dir() || !self.ensure_plugins(window, cx) {
            return false;
        }
        let opener = self
            .plugins
            .client
            .manager()
            .and_then(|m| m.opener_for(Path::new(&rel)));
        let Some(opener) = opener else {
            return false;
        };
        if let Some(key) = self.plugins.opened_files.get(path).cloned()
            && self.plugin_activate_page(&key, window, cx)
        {
            return true;
        }
        let call = match self
            .plugins
            .client
            .manager()
            .map(|m| m.open_with(&opener, &rel))
        {
            Some(Ok(call)) => call,
            Some(Err(e)) => {
                log::info!("opener {opener} for {rel}: {e}");
                return false;
            }
            None => return false,
        };
        let key = (call.plugin.clone(), call.panel);
        let title = path
            .file_name()
            .map_or_else(|| rel.clone(), |n| n.to_string_lossy().into_owned());
        self.plugin_add_page(&key, title, IconName::Rows, false, window, cx);
        self.plugins.opened_files.insert(path.to_path_buf(), key);
        let name = self.plugins.client.plugin_name(&call.plugin);
        cx.spawn(async move |this, cx| {
            let Ok(Err(error)) = call.pending.result.recv().await else {
                return;
            };
            this.update(cx, |ws, cx| ws.plugin_call_failed(&name, error, cx))
                .ok();
        })
        .detach();
        true
    }

    /// An activity-bar entry of a sidebar panel was clicked.
    pub(super) fn plugin_sidebar_clicked(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_visible = self.sidebar_visible;
        if self.sidebar_visible
            && self.activity == Activity::Extension
            && self.plugins.sidebar_active == Some(ix)
        {
            self.sidebar_visible = false;
        } else {
            self.activity = Activity::Extension;
            self.plugins.sidebar_active = Some(ix);
            self.sidebar_visible = true;
            self.plugin_ensure_sidebar_view(ix, window, cx);
        }
        if was_visible != self.sidebar_visible {
            let opening = self.sidebar_visible;
            self.start_slide(super::anim::Slider::Sidebar, opening, cx);
        }
        cx.notify();
    }

    pub(super) fn plugin_ensure_sidebar_view(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ensure_plugins(window, cx) {
            return;
        }
        let Some(entry) = self.plugins.contributions.sidebar.get(ix).cloned() else {
            return;
        };
        if self.plugins.sidebar_views.contains_key(&entry.id) {
            return;
        }
        let call = match self
            .plugins
            .client
            .manager()
            .map(|m| m.show_panel(&entry.id))
        {
            Some(Ok(call)) => call,
            Some(Err(e)) => {
                let text = format!("{}: {e}", entry.title);
                return self.show_toast_as(text, super::ToastLevel::Error, cx);
            }
            None => return,
        };
        let key = (call.plugin.clone(), call.panel);
        let view = self.plugin_new_view(&key, Placement::Sidebar(entry.id.clone()), cx);
        self.plugins.sidebar_views.insert(entry.id, view);
        let name = self.plugins.client.plugin_name(&call.plugin);
        cx.spawn(async move |this, cx| {
            let Ok(Err(error)) = call.pending.result.recv().await else {
                return;
            };
            this.update(cx, |ws, cx| ws.plugin_call_failed(&name, error, cx))
                .ok();
        })
        .detach();
    }

    /// The sidebar content while the activity is `Extension`.
    pub(super) fn render_plugin_sidebar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui::{IntoElement, ParentElement, Styled, div};
        let entry = self
            .plugins
            .sidebar_active
            .and_then(|ix| self.plugins.contributions.sidebar.get(ix));
        let view = entry.and_then(|e| self.plugins.sidebar_views.get(&e.id));
        let title = entry.map_or("Plugins", |e| e.title.as_str());
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(crate::sidebar::header(title, cx))
            .child(div().flex_1().min_h_0().children(view.cloned()))
            .into_any_element()
    }

    /// The activity-bar entries of plugin sidebar panels.
    pub(super) fn plugin_activities(&self) -> Vec<crate::activity_bar::ExtraActivity> {
        self.plugins
            .contributions
            .sidebar
            .iter()
            .enumerate()
            .map(|(ix, s)| crate::activity_bar::ExtraActivity {
                icon: icon_for(s.icon.as_deref()),
                label: s.title.clone().into(),
                selected: self.sidebar_visible
                    && self.activity == Activity::Extension
                    && self.plugins.sidebar_active == Some(ix),
            })
            .collect()
    }
}
