//! `arcode:plugin/ui`: panel bookkeeping plus [`PanelTree`] validation on
//! the plugin thread. Accepted views reach the app as
//! [`PluginOutput::PanelView`] (an immutable snapshot plus what changed);
//! refused ones change nothing and the plugin gets `invalid-tree` or
//! `too-large`.

use std::sync::Arc;

use crate::bindings::PanelId;
use crate::bindings::arcode::plugin::ui::{self as wit, Location, Patch, UiError, View};
use crate::cancel::lock;
use crate::output::PluginOutput;
use crate::state::HostState;
use crate::ui_tree::{Changes, PanelSnapshot, PanelTree, TreeError};

/// Longest panel title (bytes).
const MAX_TITLE_BYTES: usize = 256;

impl HostState {
    fn panel_is_open(&self, panel: PanelId) -> bool {
        lock(&self.ctx.shared.panels).contains_key(&panel)
    }

    /// Applies `f` to the panel's tree and forwards an accepted change to
    /// the app. A refused change is logged and leaves the tree as it was.
    fn update_tree(
        &mut self,
        panel: PanelId,
        f: impl FnOnce(&mut PanelTree) -> Result<(Arc<PanelSnapshot>, Changes), TreeError>,
    ) -> Result<(), UiError> {
        if !self.panel_is_open(panel) {
            return Err(UiError::NoSuchPanel);
        }
        let result = f(lock(&self.ctx.shared.trees).entry(panel).or_default());
        match result {
            Ok((snapshot, changes)) => {
                self.ctx.output(PluginOutput::PanelView {
                    plugin: self.ctx.manifest.id().clone(),
                    panel,
                    snapshot,
                    changes,
                });
                Ok(())
            }
            Err(e) => {
                self.ctx.shared.log.push(
                    crate::bindings::Level::Warn,
                    &format!("panel {panel}: view refused: {e}"),
                );
                Err(e.into())
            }
        }
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
        if !manifest
            .contributes
            .panels
            .iter()
            .any(|p| p.id == contribution)
        {
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
            title: super::cap(title, MAX_TITLE_BYTES),
            location,
        });
        Ok(Ok(panel))
    }

    fn render(&mut self, panel: PanelId, view: View) -> wasmtime::Result<Result<(), UiError>> {
        Ok(self.update_tree(panel, |tree| tree.render(view)))
    }

    fn apply_patches(
        &mut self,
        panel: PanelId,
        patches: Vec<Patch>,
    ) -> wasmtime::Result<Result<(), UiError>> {
        Ok(self.update_tree(panel, |tree| tree.apply(patches)))
    }

    fn set_title(&mut self, panel: PanelId, title: String) -> wasmtime::Result<()> {
        if self.panel_is_open(panel) {
            self.ctx.output(PluginOutput::PanelTitle {
                plugin: self.ctx.manifest.id().clone(),
                panel,
                title: super::cap(title, MAX_TITLE_BYTES),
            });
        }
        Ok(())
    }

    fn close_panel(&mut self, panel: PanelId) -> wasmtime::Result<()> {
        if self.ctx.shared.panel_closed(panel) {
            self.ctx.output(PluginOutput::PanelClosed {
                plugin: self.ctx.manifest.id().clone(),
                panel,
            });
        }
        Ok(())
    }
}
