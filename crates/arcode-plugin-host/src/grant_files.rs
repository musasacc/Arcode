//! The grants files (`plugin-grants.toml`) and keeping the stores that
//! read them in step.
//!
//! ```toml
//! [[grant]]
//! plugin = "acme.http"
//! capability = "network"     # or workspace-read, editor-write, ...
//! host = "api.example.com"   # network only
//! state = "allow"            # or "deny"
//! ```
//!
//! Every workspace window has its own [`GrantStore`], and another Arcode
//! process may write the same files. A store therefore never trusts its
//! copy for long: it compares a [`Stamp`] of each file before deciding and
//! re-reads a file that changed, and it re-reads before writing so it never
//! puts back a grant that was revoked somewhere else.
//!
//! [`forget_everywhere`] removes a plugin's grants from every file
//! (uninstall), so a later plugin with the same id starts from nothing.
//!
//! [`GrantStore`]: crate::grant_store::GrantStore

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::grants::{Capability, GrantState, GrantTable};
use crate::manifest::PluginId;
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

/// Reads a grants file (missing or malformed: empty; bad entries skipped).
pub(crate) fn read_table(path: &Path) -> GrantTable {
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

/// Writes a grants file atomically in an owner-only directory.
pub(crate) fn write_table(path: &Path, table: &GrantTable) -> std::io::Result<()> {
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

/// Identifies one version of a grants file: modification time, size and
/// (Unix) inode. Writes replace the file, so the inode changes too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    inode: u64,
}

/// The stamp of `path`, `None` if there is no such file.
pub(crate) fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let inode = 0;
    Some(Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
        inode,
    })
}

/// Adds to `out` the plugins whose file grants (static capabilities)
/// differ between `old` and `new`.
pub(crate) fn static_changes(old: &GrantTable, new: &GrantTable, out: &mut BTreeSet<PluginId>) {
    let statics = |t: &GrantTable| -> BTreeSet<_> {
        t.iter()
            .filter(|((_, c), _)| c.is_static())
            .map(|(k, s)| (k.clone(), *s))
            .collect()
    };
    let (old, new) = (statics(old), statics(new));
    out.extend(old.symmetric_difference(&new).map(|((p, _), _)| p.clone()));
}

/// Removes every grant of `plugin` from the global grants file and from
/// every project's grants file in the user's data dir. Returns how many
/// files were rewritten.
pub fn forget_everywhere(paths: &PluginPaths, plugin: &PluginId) -> usize {
    let mut files = vec![paths.global_grants.clone()];
    if let Ok(dirs) = fs::read_dir(&paths.projects) {
        files.extend(
            dirs.flatten()
                .map(|d| d.path().join("plugin-grants.toml"))
                .filter(|p| p.is_file()),
        );
    }
    let mut rewritten = 0;
    for file in files {
        let mut table = read_table(&file);
        let before = table.len();
        table.retain(|(p, _), _| p != plugin);
        if table.len() == before {
            continue;
        }
        match write_table(&file, &table) {
            Ok(()) => rewritten += 1,
            Err(e) => log::warn!("cannot save plugin grants to {}: {e}", file.display()),
        }
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant_store::GrantStore;
    use crate::grants::{Capability, GrantScope, GrantState};
    use crate::manifest::{Capabilities, NetworkCapability, WorkspaceAccess};

    fn caps() -> Capabilities {
        Capabilities {
            workspace: WorkspaceAccess::Read,
            network: Some(NetworkCapability {
                hosts: vec!["*".into()],
                background: false,
            }),
            ..Capabilities::default()
        }
    }

    fn net(host: &str) -> Capability {
        Capability::Network { host: host.into() }
    }

    #[test]
    fn stores_see_each_others_changes_and_never_write_back_stale_grants() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = PluginPaths::with_bases(
            None,
            &tmp.path().join("c"),
            &tmp.path().join("d"),
            &tmp.path().join("k"),
        );
        let id = PluginId::parse("acme.tools").unwrap();
        let mut a = GrantStore::open(&paths, None);
        let mut b = GrantStore::open(&paths, None);
        a.record(&id, &net("a.test"), GrantState::Allow, GrantScope::Global);
        let allow = crate::grants::Decision::Allow;
        assert_eq!(b.decide(&id, &net("a.test"), &caps(), true), allow);
        assert!(a.revoke(&id, &net("a.test")));
        assert_ne!(b.decide(&id, &net("a.test"), &caps(), true), allow);
        assert!(b.take_static_changes().is_empty());
        let read = Capability::WorkspaceRead;
        a.record(&id, &read, GrantState::Allow, GrantScope::Global);
        b.refresh();
        assert_eq!(
            b.take_static_changes().into_iter().collect::<Vec<_>>(),
            std::slice::from_ref(&id)
        );
        b.record(&id, &net("b.test"), GrantState::Allow, GrantScope::Global);
        let fresh = GrantStore::open(&paths, None);
        let hosts: Vec<_> = fresh.grants().list(&id).into_iter().map(|g| g.0).collect();
        assert_eq!(hosts, [read, net("b.test")]);
    }

    #[test]
    fn forgetting_a_plugin_clears_every_project() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = PluginPaths::with_bases(
            None,
            &tmp.path().join("c"),
            &tmp.path().join("d"),
            &tmp.path().join("k"),
        );
        let gone = PluginId::parse("acme.gone").unwrap();
        let kept = PluginId::parse("acme.kept").unwrap();
        for project in ["p1", "p2"] {
            let root = tmp.path().join(project);
            let mut store = GrantStore::open(&paths, Some(&root));
            for id in [&gone, &kept] {
                let read = Capability::WorkspaceRead;
                store.record(id, &read, GrantState::Allow, GrantScope::Project);
            }
        }
        let mut global = GrantStore::open(&paths, None);
        global.record(&gone, &net("x.test"), GrantState::Allow, GrantScope::Global);
        assert_eq!(forget_everywhere(&paths, &gone), 3);
        for project in ["p1", "p2"] {
            let store = GrantStore::open(&paths, Some(&tmp.path().join(project)));
            assert!(store.grants().list(&gone).is_empty());
            assert_eq!(store.grants().list(&kept).len(), 1);
        }
        assert_eq!(forget_everywhere(&paths, &gone), 0);
    }
}
