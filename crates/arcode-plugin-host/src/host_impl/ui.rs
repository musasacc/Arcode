//! `arcode:plugin/ui`: panel bookkeeping and size caps. Views and patches
//! are forwarded to the app as [`PluginOutput`]s; full `PanelTree`
//! validation (dangling ids, cycles, child counts, depth) is plan step 8.

use crate::bindings::PanelId;
use crate::bindings::arcode::plugin::ui::{self as wit, Location, Patch, UiError, View, Widget};
use crate::cancel::lock;
use crate::output::PluginOutput;
use crate::state::HostState;

/// Nodes per panel (§5 host caps).
pub const MAX_NODES: usize = 20_000;
/// String bytes per `render` / `apply-patches`.
pub const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;

fn widget_bytes(w: &Widget) -> usize {
    match w {
        Widget::Stack(_) | Widget::Split(_) | Widget::Scroll | Widget::Divider | Widget::Spacer => 0,
        Widget::Tabs(t) => t.titles.iter().map(String::len).sum(),
        Widget::Text(t) => t.text.len(),
        Widget::Badge(b) => b.text.len(),
        Widget::Button(b) => b.label.len() + b.icon.as_ref().map_or(0, String::len),
        Widget::Input(i) => {
            i.value.len() + i.placeholder.len() + i.language.as_ref().map_or(0, String::len)
        }
        Widget::Select(s) => s.options.iter().map(String::len).sum(),
        Widget::Checkbox(c) => c.label.len(),
        Widget::KeyValues(kv) => kv.iter().map(|(k, v)| k.len() + v.len()).sum(),
        Widget::Table(t) => t.columns.iter().map(|c| c.title.len()).sum(),
        Widget::Tree(items) => items
            .iter()
            .map(|i| {
                i.key.len()
                    + i.label.len()
                    + i.detail.as_ref().map_or(0, String::len)
                    + i.icon.as_ref().map_or(0, String::len)
            })
            .sum(),
        Widget::Code(c) => c.text.len() + c.language.as_ref().map_or(0, String::len),
        Widget::Json(j) | Widget::Spinner(j) => j.len(),
        Widget::EmptyState((a, b)) => a.len() + b.len(),
    }
}

impl HostState {
    fn panel_is_open(&self, panel: PanelId) -> bool {
        lock(&self.ctx.shared.panels).contains_key(&panel)
    }
}

impl wit::Host for HostState {
    fn open_panel(
        &mut self,
        contribution: String,
        title: String,
        location: Option<Location>,
    ) -> wasmtime::Result<Result<PanelId, UiError>> {
        let manifest = &self.ctx.manifest;
        if !manifest.contributes.panels.iter().any(|p| p.id == contribution) {
            return Ok(Err(UiError::UnknownContribution));
        }
        // "Opens (or focuses)": one panel per contribution.
        let existing = lock(&self.ctx.shared.panels)
            .iter()
            .find(|(_, p)| p.contribution == contribution)
            .map(|(id, _)| *id);
        let panel = match existing {
            Some(id) => id,
            None => {
                let id = self.ctx.shared.allocate_panel();
                self.ctx.shared.panel_opened(id, &contribution, "{}");
                id
            }
        };
        self.ctx.output(PluginOutput::PanelOpened {
            plugin: manifest.id().clone(),
            panel,
            contribution: manifest.id().qualify(&contribution),
            title: super::cap(title, 256),
            location,
        });
        Ok(Ok(panel))
    }

    fn render(&mut self, panel: PanelId, view: View) -> wasmtime::Result<Result<(), UiError>> {
        if !self.panel_is_open(panel) {
            return Ok(Err(UiError::NoSuchPanel));
        }
        let bytes: usize = view.nodes.iter().map(|n| widget_bytes(&n.widget)).sum();
        if view.nodes.len() > MAX_NODES || bytes > MAX_STRING_BYTES {
            return Ok(Err(UiError::TooLarge));
        }
        self.ctx.output(PluginOutput::Render {
            plugin: self.ctx.manifest.id().clone(),
            panel,
            view,
        });
        Ok(Ok(()))
    }

    fn apply_patches(&mut self, panel: PanelId, patches: Vec<Patch>) -> wasmtime::Result<Result<(), UiError>> {
        if !self.panel_is_open(panel) {
            return Ok(Err(UiError::NoSuchPanel));
        }
        let bytes: usize = patches
            .iter()
            .map(|p| match p {
                Patch::Upsert(n) => widget_bytes(&n.widget),
                _ => 0,
            })
            .sum();
        if patches.len() > MAX_NODES || bytes > MAX_STRING_BYTES {
            return Ok(Err(UiError::TooLarge));
        }
        self.ctx.output(PluginOutput::Patches {
            plugin: self.ctx.manifest.id().clone(),
            panel,
            patches,
        });
        Ok(Ok(()))
    }

    fn set_title(&mut self, panel: PanelId, title: String) -> wasmtime::Result<()> {
        if self.panel_is_open(panel) {
            self.ctx.output(PluginOutput::PanelTitle {
                plugin: self.ctx.manifest.id().clone(),
                panel,
                title: super::cap(title, 256),
            });
        }
        Ok(())
    }

    fn close_panel(&mut self, panel: PanelId) -> wasmtime::Result<()> {
        if lock(&self.ctx.shared.panels).remove(&panel).is_some() {
            self.ctx.output(PluginOutput::PanelClosed {
                plugin: self.ctx.manifest.id().clone(),
                panel,
            });
        }
        Ok(())
    }
}
