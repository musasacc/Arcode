//! The calls the app makes into plugins. Each one stamps the user-gesture
//! flag as §4 prescribes, dispatches, and returns at once.

use std::path::Path;

use crate::bindings::editor::DocInfo;
use crate::bindings::ui::{Event, EventKind};
use crate::bindings::{DocId, NodeId, PanelId};
use crate::dispatch::Call;
use crate::instance::Pending;
use crate::manifest::{PluginId, QualifiedId};
use crate::rows::PageRequest;

use super::{ManagerError, PluginManager};

/// A panel being shown: its owner, its id and the `open-view` call.
#[derive(Debug)]
pub struct PanelCall {
    /// The plugin that draws it.
    pub plugin: PluginId,
    /// Host-allocated id; views arrive as `PluginOutput::PanelView`.
    pub panel: PanelId,
    /// The `open-view` call.
    pub pending: Pending,
}

/// One plugin's `provide-code-lenses` call for a document.
#[derive(Debug)]
pub struct LensCall {
    /// The plugin.
    pub plugin: PluginId,
    /// Resolves to `CallOutput::Lenses`.
    pub pending: Pending,
}

/// True for the UI events that carry a user gesture (§4): `click`,
/// `submit`, `select`, `tab`, `sort`, `row-activate`, `tree-activate`.
/// `change`, `toggle` and `tree-toggle` do not.
pub fn event_is_gesture(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Click
            | EventKind::Submit
            | EventKind::Select(_)
            | EventKind::Tab(_)
            | EventKind::Sort(_)
            | EventKind::RowActivate(_)
            | EventKind::TreeActivate(_)
    )
}

impl PluginManager {
    /// Runs a contributed command from the palette, a keybinding, a lens or
    /// the status item (always a user gesture). `about` is the document a
    /// lens or editor command is about, which `editor = "context"` may read.
    pub fn run_command(
        &self,
        command: &QualifiedId,
        args_json: &str,
        about: Option<DocId>,
    ) -> Result<Pending, ManagerError> {
        let plugin = self.owner(command)?;
        if self.registry.command(command).is_none() {
            return Err(ManagerError::UnknownContribution(command.to_string()));
        }
        let handle = self.instance(&plugin)?;
        let call = Call::Command {
            command: command.local().to_owned(),
            args: args_json.to_owned(),
        };
        Ok(match about {
            Some(doc) => handle.dispatch_about(call, true, doc),
            None => handle.dispatch(call, true),
        })
    }

    /// Asks every plugin that provides lenses for `doc`'s language. Call it
    /// for visible editors, 300 ms after the last edit (the app debounces);
    /// the mailbox keeps only the newest call per document.
    pub fn code_lenses(&self, doc: DocId) -> Vec<LensCall> {
        let Some(snap) = self.app.editor.snapshot(doc) else {
            return Vec::new();
        };
        let info = DocInfo {
            id: snap.id,
            path: snap.path.clone(),
            language: snap.language.clone(),
            version: snap.version,
            line_count: u32::try_from(snap.rope.len_lines()).unwrap_or(u32::MAX),
        };
        let plugins: Vec<PluginId> = self
            .registry
            .lens_providers(&snap.language)
            .into_iter()
            .map(|p| p.id().clone())
            .collect();
        plugins
            .into_iter()
            .filter_map(|plugin| {
                let handle = self.instance(&plugin).ok()?;
                let pending = handle.dispatch(Call::CodeLenses(info.clone()), false);
                Some(LensCall { plugin, pending })
            })
            .collect()
    }

    /// Shows a contributed panel (activity bar, "View: …").
    pub fn show_panel(&self, panel: &QualifiedId) -> Result<PanelCall, ManagerError> {
        let plugin = self.owner(panel)?;
        if self.registry.panel(panel).is_none() {
            return Err(ManagerError::UnknownContribution(panel.to_string()));
        }
        let handle = self.instance(&plugin)?;
        let (id, pending) = handle.open_view(panel.local(), "{}");
        Ok(PanelCall {
            plugin,
            panel: id,
            pending,
        })
    }

    /// The opener contribution for a project-relative path, if any (the
    /// file tree shows the plugin view instead of the binary notice).
    /// Openers of disabled plugins are skipped.
    pub fn opener_for(&self, relative_path: &Path) -> Option<QualifiedId> {
        self.registry
            .openers_for(relative_path)
            .find(|id| PluginId::parse(id.plugin()).is_ok_and(|p| self.is_enabled(&p)))
            .cloned()
    }

    /// Opens `relative_path` (with `/` separators) with `opener`, as
    /// `open-view(panel, opener, {"path": ...})`.
    pub fn open_with(
        &self,
        opener: &QualifiedId,
        relative_path: &str,
    ) -> Result<PanelCall, ManagerError> {
        let plugin = self.owner(opener)?;
        let known = self.registry.get(&plugin).is_some_and(|p| {
            p.manifest
                .contributes
                .openers
                .iter()
                .any(|o| o.id == opener.local())
        });
        if !known {
            return Err(ManagerError::UnknownContribution(opener.to_string()));
        }
        let handle = self.instance(&plugin)?;
        let args = serde_json::json!({ "path": relative_path }).to_string();
        let (id, pending) = handle.open_view(opener.local(), &args);
        Ok(PanelCall {
            plugin,
            panel: id,
            pending,
        })
    }

    /// Forwards a widget event; the gesture flag follows
    /// [`event_is_gesture`].
    pub fn ui_event(
        &self,
        plugin: &PluginId,
        panel: PanelId,
        event: Event,
    ) -> Result<Pending, ManagerError> {
        let gesture = event_is_gesture(&event.kind);
        let handle = self.instance(plugin)?;
        Ok(handle.dispatch(Call::UiEvent { panel, event }, gesture))
    }

    /// Fetches a page a table's [`crate::rows::RowCache`] asked for.
    pub fn fetch_rows(
        &self,
        plugin: &PluginId,
        panel: PanelId,
        table: NodeId,
        request: PageRequest,
    ) -> Result<Pending, ManagerError> {
        let handle = self.instance(plugin)?;
        let call = Call::FetchRows {
            panel,
            table,
            start: request.start,
            count: request.count,
        };
        Ok(handle.dispatch(call, false))
    }

    /// The user closed a panel.
    pub fn close_panel(&self, plugin: &PluginId, panel: PanelId) -> Result<Pending, ManagerError> {
        Ok(self.instance(plugin)?.close_view(panel))
    }
}
