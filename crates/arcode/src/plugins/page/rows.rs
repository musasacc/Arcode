//! What the Plugins page lists, read from the manager: one [`PluginRow`]
//! per discovered plugin with its state, what its manifest may ask for and
//! the grants the user gave. Reading costs nothing: no instance thread is
//! started and no engine is created.

use std::collections::HashSet;
use std::path::PathBuf;

use arcode_plugin_host::consent::quote;
use arcode_plugin_host::grants::{Capability, GrantScope, GrantState};
use arcode_plugin_host::manifest::{Capabilities, EditorAccess, WorkspaceAccess};
use arcode_plugin_host::{PluginId, PluginManager, PluginSource};

/// A plugin's run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Enabled, not running (it starts on demand).
    Inactive,
    /// Its instance thread is alive.
    Active,
    /// It trapped; disabled for the session after repeated crashes, or
    /// restarted on its next call.
    Crashed,
    /// Turned off by the user (or an installed plugin not approved yet).
    Disabled,
}

impl Status {
    /// The label shown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Active => "active",
            Self::Crashed => "crashed",
            Self::Disabled => "disabled",
        }
    }
}

/// A stored grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRow {
    /// What it is about (the key of a revoke).
    pub capability: Capability,
    /// Allowed or denied.
    pub state: GrantState,
    /// Project (or session) or global.
    pub scope: GrantScope,
}

impl GrantRow {
    /// E.g. "Allowed: connect to example.com (this project)".
    pub fn label(&self) -> String {
        let state = match self.state {
            GrantState::Allow => "Allowed",
            GrantState::Deny => "Denied",
        };
        let scope = match self.scope {
            GrantScope::Global => "always",
            GrantScope::Project | GrantScope::Once => "this project",
        };
        format!("{state}: {} ({scope})", self.capability)
    }
}

/// One plugin as the page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginRow {
    /// Validated id.
    pub id: PluginId,
    /// Display name (plugin text).
    pub name: String,
    /// Version (plugin text).
    pub version: String,
    /// One-line description (plugin text), if any.
    pub description: Option<String>,
    /// Where it was found.
    pub source: PluginSource,
    /// Run state.
    pub status: Status,
    /// Whether it may run.
    pub enabled: bool,
    /// What its manifest lets it ask for (host wording).
    pub capabilities: Vec<String>,
    /// The grants the user gave it.
    pub grants: Vec<GrantRow>,
    /// Its folder.
    pub dir: PathBuf,
}

impl PluginRow {
    /// Only installed plugins can be uninstalled.
    pub fn can_uninstall(&self) -> bool {
        self.source == PluginSource::Installed
    }
}

/// The host wording of what a manifest lets a plugin ask for. Host
/// patterns are plugin text and shown as written.
pub fn capability_summary(caps: &Capabilities) -> Vec<String> {
    let mut out = Vec::new();
    match caps.workspace {
        WorkspaceAccess::None => {}
        WorkspaceAccess::Read => out.push("Read project files (asks first)".to_owned()),
        WorkspaceAccess::ReadWrite => {
            out.push("Read and change project files (asks first)".to_owned())
        }
    }
    match caps.editor {
        EditorAccess::None => {}
        EditorAccess::Context => out.push("Read the document a command is run on".to_owned()),
        EditorAccess::AllOpen => out.push("Read every open document (asks first)".to_owned()),
    }
    if caps.editor_write {
        out.push("Edit documents (asks first)".to_owned());
    }
    if let Some(net) = &caps.network {
        let hosts = if net.hosts.iter().any(|h| h == "*") {
            "any host".to_owned()
        } else {
            net.hosts.join(", ")
        };
        out.push(format!(
            "Send requests when you click, to {hosts} (asks per host)"
        ));
        if net.background {
            out.push("Use the network in the background (off unless you allow it)".to_owned());
        }
    }
    if out.is_empty() {
        out.push("No access to files, documents or the network".to_owned());
    }
    out
}

/// Every discovered plugin, sorted by name. `crashed` are the plugins
/// the app saw crash since they last ran fine.
pub fn rows(manager: &PluginManager, crashed: &HashSet<PluginId>) -> Vec<PluginRow> {
    let mut out: Vec<PluginRow> = manager
        .registry()
        .plugins()
        .map(|plugin| {
            let id = plugin.id().clone();
            let enabled = manager.is_enabled(&id);
            let status = match manager.instance(&id) {
                Err(_) => Status::Disabled,
                Ok(h) if h.is_disabled() => Status::Crashed,
                Ok(h) if crashed.contains(&id) && !h.thread_running() => Status::Crashed,
                Ok(h) if h.thread_running() => Status::Active,
                Ok(_) => Status::Inactive,
            };
            let info = &plugin.manifest.plugin;
            PluginRow {
                name: info.name.clone(),
                version: info.version.clone(),
                description: info.description.clone(),
                source: plugin.source,
                status,
                enabled,
                capabilities: capability_summary(&plugin.manifest.capabilities),
                grants: manager
                    .grants(&id)
                    .into_iter()
                    .map(|(capability, state, scope)| GrantRow {
                        capability,
                        state,
                        scope,
                    })
                    .collect(),
                dir: plugin.dir.clone(),
                id,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// The text of the confirmation shown before an installed plugin is
/// enabled: what it may ask for, from its manifest (name and version are
/// plugin text, quoted and capped by the host).
pub fn enable_summary(row: &PluginRow) -> String {
    let mut text = format!(
        "{} {} ({}) can:\n",
        quote(&row.name),
        quote(&row.version).trim_matches(['\u{201c}', '\u{201d}']),
        row.id
    );
    for line in &row.capabilities {
        text.push_str("• ");
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("Only the plugin.wasm installed now is approved; a changed file does not run.");
    text
}
