//! The grant state machine (§4). Pure: no I/O, no prompts.
//!
//! A grant is keyed by `(plugin, capability[, host])`, has a state
//! (`ask`/`allow`/`deny`) and a scope (`once`/`project`/`global`).
//! [`Grants::decide`] answers "may this plugin use this capability now?";
//! [`Grants::record`] stores the user's answer to a prompt. `once` answers
//! are never stored: they cover the request (or the static grant of one
//! instance) they were given for.
//!
//! Precedence, most specific first: session > project > global > default.
//! Undeclared capabilities are always denied, background network is denied
//! unless explicitly granted, and the master network switch denies every
//! network capability.

use std::collections::BTreeMap;
use std::fmt;

use crate::manifest::{Capabilities, EditorAccess, PluginId, WorkspaceAccess};

/// Something a plugin may be granted (the `ask` rows of §4's table).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// `/project` preopened read-only.
    WorkspaceRead,
    /// `/project` writable (only together with read).
    WorkspaceWrite,
    /// `editor = "all-open"`: any open document.
    EditorAllOpen,
    /// `editor.apply-edits`.
    EditorWrite,
    /// User-initiated requests to one host (lower-case, no port).
    Network {
        /// Host name or IP literal.
        host: String,
    },
    /// Requests outside user gestures.
    NetworkBackground,
}

impl Capability {
    /// Static capabilities become the WASI preopen; changing them recycles
    /// the instance. All others are checked on every call.
    pub fn is_static(&self) -> bool {
        matches!(self, Self::WorkspaceRead | Self::WorkspaceWrite)
    }

    /// True if `caps` (the manifest) declares this capability.
    pub fn is_declared(&self, caps: &Capabilities) -> bool {
        match self {
            Self::WorkspaceRead => caps.workspace >= WorkspaceAccess::Read,
            Self::WorkspaceWrite => caps.workspace == WorkspaceAccess::ReadWrite,
            Self::EditorAllOpen => caps.editor == EditorAccess::AllOpen,
            Self::EditorWrite => caps.editor_write,
            Self::Network { .. } => caps.network.is_some(),
            Self::NetworkBackground => caps.network.as_ref().is_some_and(|n| n.background),
        }
    }

    fn is_network(&self) -> bool {
        matches!(self, Self::Network { .. } | Self::NetworkBackground)
    }

    /// What applies when the user has not answered yet.
    fn default_decision(&self) -> Decision {
        match self {
            // Only via the explicit toggle in Plugins → Permissions.
            Self::NetworkBackground => Decision::Deny,
            _ => Decision::Ask,
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceRead => f.write_str("read project files"),
            Self::WorkspaceWrite => f.write_str("write project files"),
            Self::EditorAllOpen => f.write_str("read any open document"),
            Self::EditorWrite => f.write_str("edit documents"),
            Self::Network { host } => write!(f, "connect to {host}"),
            Self::NetworkBackground => f.write_str("use the network in the background"),
        }
    }
}

/// A stored answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrantState {
    /// Allowed.
    Allow,
    /// Denied; the user is not asked again in this scope.
    Deny,
}

/// How long an answer lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrantScope {
    /// This request (or this instance, for static grants); never stored.
    Once,
    /// This project (kept in the user's data dir, never in the project).
    Project,
    /// Every project.
    Global,
}

/// The outcome of [`Grants::decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Go ahead.
    Allow,
    /// Refuse without asking.
    Deny,
    /// Ask the user (through the `ConsentProvider`).
    Ask,
}

impl From<GrantState> for Decision {
    fn from(state: GrantState) -> Self {
        match state {
            GrantState::Allow => Self::Allow,
            GrantState::Deny => Self::Deny,
        }
    }
}

/// One scope's stored answers.
pub type GrantTable = BTreeMap<(PluginId, Capability), GrantState>;

/// All grants that apply right now: global, the open project's, and the
/// session's (project answers when no project is open).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    /// Every project (`plugin-grants.toml` in the config dir).
    pub global: GrantTable,
    /// The open project (`projects/<hash>/plugin-grants.toml` in the data dir).
    pub project: GrantTable,
    /// In memory only.
    pub session: GrantTable,
    /// Whether a project is open (project answers go to `session` if not).
    pub has_project: bool,
}

impl Grants {
    /// Decides `capability` for `plugin`, whose manifest declares `caps`.
    /// `network_enabled` is the `settings.plugins.network` master switch.
    pub fn decide(
        &self,
        plugin: &PluginId,
        capability: &Capability,
        caps: &Capabilities,
        network_enabled: bool,
    ) -> Decision {
        if !capability.is_declared(caps) || (capability.is_network() && !network_enabled) {
            return Decision::Deny;
        }
        let key = (plugin.clone(), capability.clone());
        [&self.session, &self.project, &self.global]
            .into_iter()
            .find_map(|table| table.get(&key))
            .map_or_else(|| capability.default_decision(), |s| Decision::from(*s))
    }

    /// Stores an answer. `Once` stores nothing. Returns the decision the
    /// answer stands for.
    pub fn record(
        &mut self,
        plugin: &PluginId,
        capability: &Capability,
        state: GrantState,
        scope: GrantScope,
    ) -> Decision {
        let key = (plugin.clone(), capability.clone());
        match scope {
            GrantScope::Once => {}
            GrantScope::Project if self.has_project => {
                self.session.remove(&key);
                self.project.insert(key, state);
            }
            GrantScope::Project => {
                self.session.insert(key, state);
            }
            GrantScope::Global => {
                // A global answer replaces narrower ones so it takes effect.
                self.session.remove(&key);
                self.project.remove(&key);
                self.global.insert(key, state);
            }
        }
        state.into()
    }

    /// Forgets `capability` for `plugin` in every scope. Returns true if
    /// something was removed.
    pub fn revoke(&mut self, plugin: &PluginId, capability: &Capability) -> bool {
        let key = (plugin.clone(), capability.clone());
        let mut removed = false;
        for table in [&mut self.session, &mut self.project, &mut self.global] {
            removed |= table.remove(&key).is_some();
        }
        removed
    }

    /// Every stored grant of `plugin` with its scope, for Plugins →
    /// Permissions. Session entries are listed as `Project`.
    pub fn list(&self, plugin: &PluginId) -> Vec<(Capability, GrantState, GrantScope)> {
        let scoped = [
            (&self.session, GrantScope::Project),
            (&self.project, GrantScope::Project),
            (&self.global, GrantScope::Global),
        ];
        scoped
            .into_iter()
            .flat_map(|(table, scope)| {
                table
                    .iter()
                    .filter(|((p, _), _)| p == plugin)
                    .map(move |((_, c), s)| (c.clone(), *s, scope))
            })
            .collect()
    }

    /// True if any plugin holds an allowed background-network grant (the
    /// status-bar dot).
    pub fn any_background_network(&self) -> bool {
        [&self.session, &self.project, &self.global]
            .into_iter()
            .flat_map(|t| t.iter())
            .any(|((_, c), s)| *c == Capability::NetworkBackground && *s == GrantState::Allow)
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
