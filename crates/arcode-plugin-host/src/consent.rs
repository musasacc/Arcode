//! Consent prompts (§4): the [`ConsentProvider`] trait the app implements
//! with host-drawn dialogs, and [`Permissions`], which combines stored
//! grants with prompts.
//!
//! Prompts are host chrome. A [`ConsentRequest`] carries only data the host
//! derived itself (the parsed URL's host, method and port; the manifest's
//! display name) and plugin-supplied text is quoted and capped by
//! [`quote`]. The provider is called on the plugin's thread and may block
//! until the user answers; the UI thread never waits on a plugin.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::cancel::{CancelToken, lock};
use crate::grant_store::GrantStore;
use crate::grants::{Capability, Decision, GrantScope, GrantState};
use crate::manifest::{Capabilities, Manifest, PluginId, WorkspaceAccess};

/// Longest plugin-supplied string shown in a prompt.
pub const MAX_QUOTED_CHARS: usize = 80;

/// What the user is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsentKind {
    /// First activation in a project: read (and optionally write) files.
    Workspace {
        /// Whether the manifest asks for read-write (a separate checkbox).
        write: bool,
        /// Whether the plugin can also reach the network (combined wording).
        network: bool,
    },
    /// First request to a host.
    Network {
        /// HTTP method, from the parsed request.
        method: String,
        /// `http` or `https`, from the parsed URL.
        scheme: String,
        /// Host, from the parsed URL (lower-case).
        host: String,
        /// Port, from the parsed URL (default port filled in).
        port: u16,
    },
    /// First edit per project.
    EditorWrite,
}

/// A prompt for one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequest {
    /// The plugin.
    pub plugin: PluginId,
    /// Its display name, quoted.
    pub plugin_name: String,
    /// Display name of the project, if one is open.
    pub project: Option<String>,
    /// What is asked.
    pub kind: ConsentKind,
}

impl ConsentRequest {
    /// The sentence a dialog shows, e.g. `"HTTP Client" can read this
    /// project and send data to hosts you approve.`
    pub fn message(&self) -> String {
        let who = &self.plugin_name;
        match &self.kind {
            ConsentKind::Workspace { write, network } => {
                let access = if *write { "read and change" } else { "read" };
                if *network {
                    format!("{who} can {access} this project and send data to hosts you approve.")
                } else {
                    format!("{who} can {access} files in this project.")
                }
            }
            ConsentKind::Network {
                method,
                scheme,
                host,
                port,
            } => format!("{who} wants to send a {method} request to {scheme}://{host}:{port}."),
            ConsentKind::EditorWrite => format!("{who} wants to edit documents in this project."),
        }
    }
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentAnswer {
    /// Allowed for the given scope. For workspace prompts `write` is the
    /// write checkbox (ignored elsewhere).
    Allow {
        /// Once, this project, or always.
        scope: GrantScope,
        /// Workspace only: the write checkbox.
        write: bool,
    },
    /// Denied (remembered for the project).
    Deny,
    /// Closed without an answer: denied this time, asked again next time.
    Dismissed,
}

/// Shows consent prompts. Called on plugin threads; may block.
pub trait ConsentProvider: Send + Sync {
    /// Asks the user. Implementations should return
    /// [`ConsentAnswer::Dismissed`] once `cancel` fires.
    fn ask(&self, request: &ConsentRequest, cancel: &CancelToken) -> ConsentAnswer;
}

/// Answers every prompt with [`ConsentAnswer::Dismissed`].
#[derive(Debug, Default, Clone, Copy)]
pub struct DenyAll;

impl ConsentProvider for DenyAll {
    fn ask(&self, _: &ConsentRequest, _: &CancelToken) -> ConsentAnswer {
        ConsentAnswer::Dismissed
    }
}

/// Answers prompts from a script and records every request (tests and
/// end-to-end runs).
#[derive(Debug, Default)]
pub struct ScriptedConsent {
    answers: Mutex<VecDeque<ConsentAnswer>>,
    fallback: Option<ConsentAnswer>,
    asked: Mutex<Vec<ConsentRequest>>,
}

impl ScriptedConsent {
    /// Pops answers in order; once empty, answers `Dismissed`.
    pub fn new(answers: impl IntoIterator<Item = ConsentAnswer>) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().collect()),
            ..Self::default()
        }
    }

    /// Always gives `answer`.
    pub fn always(answer: ConsentAnswer) -> Self {
        Self {
            fallback: Some(answer),
            ..Self::default()
        }
    }

    /// Every request asked so far.
    pub fn asked(&self) -> Vec<ConsentRequest> {
        lock(&self.asked).clone()
    }
}

impl ConsentProvider for ScriptedConsent {
    fn ask(&self, request: &ConsentRequest, _: &CancelToken) -> ConsentAnswer {
        lock(&self.asked).push(request.clone());
        lock(&self.answers)
            .pop_front()
            .or(self.fallback)
            .unwrap_or(ConsentAnswer::Dismissed)
    }
}

/// Plugin-supplied text as shown in host prompts: quoted, control
/// characters replaced, capped at [`MAX_QUOTED_CHARS`].
pub fn quote(text: &str) -> String {
    let mut out: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_QUOTED_CHARS)
        .collect();
    if text.chars().count() > MAX_QUOTED_CHARS {
        out.push('…');
    }
    format!("\u{201c}{out}\u{201d}")
}

/// Stored grants plus the provider that fills the gaps.
#[derive(Clone)]
pub struct Permissions {
    /// Shared by every instance; saved on change.
    pub store: Arc<Mutex<GrantStore>>,
    /// Shows prompts.
    pub provider: Arc<dyn ConsentProvider>,
}

impl std::fmt::Debug for Permissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Permissions").finish_non_exhaustive()
    }
}

/// The static file grant an instance is created with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGrant {
    /// No preopen.
    None,
    /// `/project` read-only.
    Read,
    /// `/project` read-write.
    ReadWrite,
}

impl Permissions {
    /// A store and a provider.
    pub fn new(store: GrantStore, provider: Arc<dyn ConsentProvider>) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            provider,
        }
    }

    /// Decides `cap`, prompting with `kind` if the grants say "ask", and
    /// records the answer. Answers to one prompt never grant more than the
    /// capability asked about.
    pub fn check(
        &self,
        manifest: &Manifest,
        project: Option<&str>,
        cap: &Capability,
        kind: ConsentKind,
        network_enabled: bool,
        cancel: &CancelToken,
    ) -> bool {
        let id = manifest.id();
        let decision = lock(&self.store).decide(id, cap, &manifest.capabilities, network_enabled);
        match decision {
            Decision::Allow => true,
            Decision::Deny => false,
            Decision::Ask => {
                let request = request(manifest, project, kind);
                match self.provider.ask(&request, cancel) {
                    ConsentAnswer::Allow { scope, .. } => {
                        lock(&self.store).record(id, cap, GrantState::Allow, scope);
                        true
                    }
                    ConsentAnswer::Deny => {
                        lock(&self.store).record(id, cap, GrantState::Deny, GrantScope::Project);
                        false
                    }
                    ConsentAnswer::Dismissed => false,
                }
            }
        }
    }

    /// The `/project` preopen for a new instance (§4: asked at first
    /// activation in a project, before the instance is created; one prompt
    /// covers read and the write checkbox).
    pub fn file_grant(
        &self,
        manifest: &Manifest,
        project: &str,
        network_enabled: bool,
        cancel: &CancelToken,
    ) -> FileGrant {
        let caps: &Capabilities = &manifest.capabilities;
        if caps.workspace == WorkspaceAccess::None {
            return FileGrant::None;
        }
        let id = manifest.id();
        let wants_write = caps.workspace == WorkspaceAccess::ReadWrite;
        let (read, write) = {
            let store = lock(&self.store);
            let read = store.decide(id, &Capability::WorkspaceRead, caps, network_enabled);
            let write = store.decide(id, &Capability::WorkspaceWrite, caps, network_enabled);
            (read, write)
        };
        let (read, write) = if read == Decision::Ask || (read == Decision::Allow && write == Decision::Ask) {
            let kind = ConsentKind::Workspace {
                write: wants_write,
                network: caps.network.is_some(),
            };
            let answer = self.provider.ask(&request(manifest, Some(project), kind), cancel);
            let mut store = lock(&self.store);
            match answer {
                ConsentAnswer::Allow { scope, write: w } => {
                    store.record(id, &Capability::WorkspaceRead, GrantState::Allow, scope);
                    let w = w && wants_write;
                    if wants_write {
                        let state = if w { GrantState::Allow } else { GrantState::Deny };
                        store.record(id, &Capability::WorkspaceWrite, state, scope);
                    }
                    (true, w)
                }
                ConsentAnswer::Deny => {
                    store.record(id, &Capability::WorkspaceRead, GrantState::Deny, GrantScope::Project);
                    (false, false)
                }
                ConsentAnswer::Dismissed => (false, false),
            }
        } else {
            (read == Decision::Allow, write == Decision::Allow)
        };
        match (read, write) {
            (true, true) => FileGrant::ReadWrite,
            (true, false) => FileGrant::Read,
            _ => FileGrant::None,
        }
    }
}

fn request(manifest: &Manifest, project: Option<&str>, kind: ConsentKind) -> ConsentRequest {
    ConsentRequest {
        plugin: manifest.id().clone(),
        plugin_name: quote(&manifest.plugin.name),
        project: project.map(quote),
        kind,
    }
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
