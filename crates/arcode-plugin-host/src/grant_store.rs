//! Loading and saving grants (§3 "Locations", §4 "Repository trust").
//!
//! Grants live only in the user's config dir (global) and data dir (per
//! project, keyed by a hash of the project root). [`GrantStore::open`] takes
//! its paths from [`PluginPaths`], never from the project, so a file such as
//! `<project>/.arcode/plugin-grants.toml` is simply never read.
//!
//! Files are written atomically (temp file + rename) in owner-only
//! directories. Malformed entries are skipped with a warning: a broken file
//! means "ask again", never "allow".

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::grants::{Capability, Decision, GrantScope, GrantState, GrantTable, Grants};
use crate::manifest::{Capabilities, PluginId};
use crate::paths::{PluginPaths, create_private_dir};

/// Largest grants file read.
const MAX_GRANTS_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Default, Serialize, Deserialize)]
struct GrantFile {
    #[serde(default, rename = "grant")]
    grants: Vec<GrantEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantEntry {
    plugin: String,
    capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    state: String,
}

impl GrantEntry {
    fn from_grant(plugin: &PluginId, cap: &Capability, state: GrantState) -> Self {
        let (capability, host) = match cap {
            Capability::WorkspaceRead => ("workspace-read", None),
            Capability::WorkspaceWrite => ("workspace-write", None),
            Capability::EditorAllOpen => ("editor-all-open", None),
            Capability::EditorWrite => ("editor-write", None),
            Capability::Network { host } => ("network", Some(host.clone())),
            Capability::NetworkBackground => ("network-background", None),
        };
        let state = match state {
            GrantState::Allow => "allow",
            GrantState::Deny => "deny",
        };
        Self {
            plugin: plugin.to_string(),
            capability: capability.into(),
            host,
            state: state.into(),
        }
    }

    fn to_grant(&self) -> Option<(PluginId, Capability, GrantState)> {
        let plugin = PluginId::parse(&self.plugin).ok()?;
        let cap = match (self.capability.as_str(), &self.host) {
            ("workspace-read", None) => Capability::WorkspaceRead,
            ("workspace-write", None) => Capability::WorkspaceWrite,
            ("editor-all-open", None) => Capability::EditorAllOpen,
            ("editor-write", None) => Capability::EditorWrite,
            ("network", Some(host)) if !host.is_empty() => Capability::Network {
                host: host.to_ascii_lowercase(),
            },
            ("network-background", None) => Capability::NetworkBackground,
            _ => return None,
        };
        let state = match self.state.as_str() {
            "allow" => GrantState::Allow,
            "deny" => GrantState::Deny,
            _ => return None,
        };
        Some((plugin, cap, state))
    }
}

/// Grants plus the files they persist to.
#[derive(Debug, Clone)]
pub struct GrantStore {
    grants: Grants,
    global_path: PathBuf,
    project_path: Option<PathBuf>,
}

impl GrantStore {
    /// Loads the global grants and, if `project_root` is given, that
    /// project's grants from the user's data dir.
    pub fn open(paths: &PluginPaths, project_root: Option<&Path>) -> Self {
        let project_path = project_root.map(|root| paths.project_grants(root));
        let grants = Grants {
            global: read_table(&paths.global_grants),
            project: project_path.as_deref().map(read_table).unwrap_or_default(),
            session: GrantTable::new(),
            has_project: project_path.is_some(),
        };
        Self {
            grants,
            global_path: paths.global_grants.clone(),
            project_path,
        }
    }

    /// An in-memory store that never touches disk (tests, headless use).
    pub fn in_memory(has_project: bool) -> Self {
        Self {
            grants: Grants {
                has_project,
                ..Grants::default()
            },
            global_path: PathBuf::new(),
            project_path: None,
        }
    }

    /// The current grants.
    pub fn grants(&self) -> &Grants {
        &self.grants
    }

    /// See [`Grants::decide`].
    pub fn decide(
        &self,
        plugin: &PluginId,
        cap: &Capability,
        caps: &Capabilities,
        network_enabled: bool,
    ) -> Decision {
        self.grants.decide(plugin, cap, caps, network_enabled)
    }

    /// Records an answer and saves the affected file.
    pub fn record(
        &mut self,
        plugin: &PluginId,
        cap: &Capability,
        state: GrantState,
        scope: GrantScope,
    ) -> Decision {
        let decision = self.grants.record(plugin, cap, state, scope);
        self.save(scope);
        decision
    }

    /// Revokes a grant in every scope and saves.
    pub fn revoke(&mut self, plugin: &PluginId, cap: &Capability) -> bool {
        let removed = self.grants.revoke(plugin, cap);
        if removed {
            self.save(GrantScope::Project);
            self.save(GrantScope::Global);
        }
        removed
    }

    fn save(&self, scope: GrantScope) {
        let (table, path) = match scope {
            GrantScope::Once => return,
            GrantScope::Project => match &self.project_path {
                Some(path) => (&self.grants.project, path),
                None => return,
            },
            GrantScope::Global => (&self.grants.global, &self.global_path),
        };
        if path.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = write_table(path, table) {
            log::warn!("cannot save plugin grants to {}: {e}", path.display());
        }
    }
}

fn read_table(path: &Path) -> GrantTable {
    let mut table = GrantTable::new();
    let Ok(meta) = fs::metadata(path) else {
        return table;
    };
    if !meta.is_file() || meta.len() > MAX_GRANTS_BYTES {
        log::warn!("ignoring plugin grants file {}", path.display());
        return table;
    }
    let file: GrantFile = match fs::read_to_string(path).map(|t| toml::from_str(&t)) {
        Ok(Ok(file)) => file,
        Ok(Err(e)) => {
            log::warn!("ignoring malformed plugin grants {}: {e}", path.display());
            return table;
        }
        Err(e) => {
            log::warn!("cannot read plugin grants {}: {e}", path.display());
            return table;
        }
    };
    for entry in &file.grants {
        match entry.to_grant() {
            Some((plugin, cap, state)) => {
                table.insert((plugin, cap), state);
            }
            None => log::warn!("skipping invalid grant entry in {}", path.display()),
        }
    }
    table
}

fn write_table(path: &Path, table: &GrantTable) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let file = GrantFile {
        grants: table
            .iter()
            .map(|((p, c), s)| GrantEntry::from_grant(p, c, *s))
            .collect(),
    };
    let text = toml::to_string(&file).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("toml.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{EditorAccess, NetworkCapability, WorkspaceAccess};

    fn caps() -> Capabilities {
        Capabilities {
            workspace: WorkspaceAccess::ReadWrite,
            editor: EditorAccess::AllOpen,
            editor_write: true,
            network: Some(NetworkCapability {
                hosts: vec!["*".into()],
                background: true,
            }),
        }
    }

    fn id() -> PluginId {
        PluginId::parse("evil.plugin").unwrap()
    }

    fn user_paths(base: &Path) -> PluginPaths {
        PluginPaths::with_bases(
            None,
            &base.join("config"),
            &base.join("data"),
            &base.join("cache"),
        )
    }

    #[test]
    fn grants_round_trip_through_user_files() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = user_paths(tmp.path());
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let net = Capability::Network {
            host: "example.com".into(),
        };
        {
            let mut store = GrantStore::open(&paths, Some(&project));
            store.record(&id(), &net, GrantState::Allow, GrantScope::Global);
            store.record(
                &id(),
                &Capability::WorkspaceRead,
                GrantState::Allow,
                GrantScope::Project,
            );
            store.record(
                &id(),
                &Capability::EditorWrite,
                GrantState::Deny,
                GrantScope::Once,
            );
        }
        let store = GrantStore::open(&paths, Some(&project));
        assert_eq!(store.decide(&id(), &net, &caps(), true), Decision::Allow);
        let read = &Capability::WorkspaceRead;
        assert_eq!(store.decide(&id(), read, &caps(), true), Decision::Allow);
        assert_eq!(
            store.decide(&id(), &Capability::EditorWrite, &caps(), true),
            Decision::Ask
        );
        // The project grant stays with its project.
        let elsewhere = GrantStore::open(&paths, Some(&tmp.path().join("other")));
        assert_eq!(elsewhere.decide(&id(), read, &caps(), true), Decision::Ask);
        // Nothing was written into the project tree.
        assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
        assert!(paths.project_grants(&project).starts_with(&paths.projects));
    }

    /// §4 "Repository trust": a grants file inside the repository is never read.
    #[test]
    fn malicious_project_grant_file_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = user_paths(tmp.path());
        let project = tmp.path().join("cloned-repo");
        let evil = "[[grant]]\nplugin = \"evil.plugin\"\ncapability = \"workspace-write\"\nstate = \"allow\"\n\
             [[grant]]\nplugin = \"evil.plugin\"\ncapability = \"network-background\"\nstate = \"allow\"\n\
             [[grant]]\nplugin = \"evil.plugin\"\ncapability = \"network\"\nhost = \"evil.test\"\nstate = \"allow\"\n";
        for rel in [
            ".arcode/plugin-grants.toml",
            "plugin-grants.toml",
            ".arcode/plugins.toml",
        ] {
            let p = project.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, evil).unwrap();
        }
        // Sanity: the same text *is* honoured from the user's own file.
        fs::create_dir_all(paths.global_grants.parent().unwrap()).unwrap();
        fs::write(&paths.global_grants, evil).unwrap();
        let honest = GrantStore::open(&paths, Some(&project));
        let write = Capability::WorkspaceWrite;
        assert_eq!(honest.decide(&id(), &write, &caps(), true), Decision::Allow);
        fs::remove_file(&paths.global_grants).unwrap();

        let store = GrantStore::open(&paths, Some(&project));
        let evil_host = Capability::Network {
            host: "evil.test".into(),
        };
        assert_eq!(store.decide(&id(), &write, &caps(), true), Decision::Ask);
        assert_eq!(
            store.decide(&id(), &evil_host, &caps(), true),
            Decision::Ask
        );
        assert_eq!(
            store.decide(&id(), &Capability::NetworkBackground, &caps(), true),
            Decision::Deny
        );
        assert!(store.grants().global.is_empty() && store.grants().project.is_empty());
    }

    #[test]
    fn malformed_files_mean_ask() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = user_paths(tmp.path());
        fs::create_dir_all(paths.global_grants.parent().unwrap()).unwrap();
        let bad = "[[grant]]\nplugin = \"evil.plugin\"\ncapability = \"root\"\nstate = \"allow\"\n\
                   [[grant]]\nplugin = \"evil.plugin\"\ncapability = \"workspace-read\"\nstate = \"maybe\"\n";
        fs::write(&paths.global_grants, bad).unwrap();
        let store = GrantStore::open(&paths, None);
        assert!(store.grants().global.is_empty());
        fs::write(&paths.global_grants, "this is [ not toml").unwrap();
        assert!(GrantStore::open(&paths, None).grants().global.is_empty());
    }
}
