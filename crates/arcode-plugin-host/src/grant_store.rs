//! Loading and saving grants (§3 "Locations", §4 "Repository trust").
//!
//! Grants live only in the user's config dir (global) and data dir (per
//! project, keyed by a hash of the project root). [`GrantStore::open`] takes
//! its paths from [`PluginPaths`], never from the project, so a file such as
//! `<project>/.arcode/plugin-grants.toml` is simply never read.
//!
//! Files are written atomically (temp file + rename) in owner-only
//! directories. Malformed entries are skipped with a warning: a broken file
//! means "ask again", never "allow". Other windows and processes write the
//! same files, so a store re-reads a file that changed before it decides
//! and before it writes (`grant_files`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::grant_files::{Stamp, read_table, stamp, static_changes, write_table};
use crate::grants::{Capability, Decision, GrantScope, GrantState, Grants};
use crate::manifest::{Capabilities, PluginId};
use crate::paths::PluginPaths;

/// Grants plus the files they persist to.
#[derive(Debug, Clone)]
pub struct GrantStore {
    grants: Grants,
    global_path: PathBuf,
    project_path: Option<PathBuf>,
    /// The file versions `grants.global` / `grants.project` were read from.
    global_stamp: Option<Stamp>,
    project_stamp: Option<Stamp>,
    /// See [`GrantStore::take_static_changes`].
    static_changed: BTreeSet<PluginId>,
}

impl GrantStore {
    /// Loads the global grants and, if `project_root` is given, that
    /// project's grants from the user's data dir.
    pub fn open(paths: &PluginPaths, project_root: Option<&Path>) -> Self {
        let project_path = project_root.map(|root| paths.project_grants(root));
        let grants = Grants {
            has_project: project_path.is_some(),
            ..Grants::default()
        };
        let mut store = Self {
            grants,
            global_path: paths.global_grants.clone(),
            project_path,
            global_stamp: None,
            project_stamp: None,
            static_changed: BTreeSet::new(),
        };
        store.reload();
        store.static_changed.clear();
        store
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
            global_stamp: None,
            project_stamp: None,
            static_changed: BTreeSet::new(),
        }
    }

    /// The grants as last read ([`GrantStore::refresh`] first for the
    /// files' current state).
    pub fn grants(&self) -> &Grants {
        &self.grants
    }

    /// Re-reads the files another window or process changed since they
    /// were read.
    pub fn refresh(&mut self) {
        self.read_files(false);
    }

    /// Re-reads both files.
    fn reload(&mut self) {
        self.read_files(true);
    }

    fn read_files(&mut self, always: bool) {
        if self.global_path.as_os_str().is_empty() {
            return;
        }
        let global = stamp(&self.global_path);
        if always || global != self.global_stamp {
            let table = read_table(&self.global_path);
            static_changes(&self.grants.global, &table, &mut self.static_changed);
            self.grants.global = table;
            self.global_stamp = global;
        }
        if let Some(path) = &self.project_path {
            let project = stamp(path);
            if always || project != self.project_stamp {
                let table = read_table(path);
                static_changes(&self.grants.project, &table, &mut self.static_changed);
                self.grants.project = table;
                self.project_stamp = project;
            }
        }
    }

    /// The plugins whose file grants (`/project` access) were changed by
    /// another window or process since the last call: their running
    /// instances should be recycled.
    pub fn take_static_changes(&mut self) -> BTreeSet<PluginId> {
        std::mem::take(&mut self.static_changed)
    }

    /// See [`Grants::decide`] (after [`GrantStore::refresh`]).
    pub fn decide(
        &mut self,
        plugin: &PluginId,
        cap: &Capability,
        caps: &Capabilities,
        network_enabled: bool,
    ) -> Decision {
        self.refresh();
        self.grants.decide(plugin, cap, caps, network_enabled)
    }

    /// Records an answer and saves the affected files (re-read first, so
    /// changes made elsewhere are kept).
    pub fn record(
        &mut self,
        plugin: &PluginId,
        cap: &Capability,
        state: GrantState,
        scope: GrantScope,
    ) -> Decision {
        if scope != GrantScope::Once {
            self.reload();
        }
        let decision = self.grants.record(plugin, cap, state, scope);
        match scope {
            GrantScope::Once => {}
            GrantScope::Project => self.save(GrantScope::Project),
            // A global answer also clears the project's entry.
            GrantScope::Global => {
                self.save(GrantScope::Project);
                self.save(GrantScope::Global);
            }
        }
        decision
    }

    /// Revokes a grant in every scope and saves (re-read first, as for
    /// [`GrantStore::record`]).
    pub fn revoke(&mut self, plugin: &PluginId, cap: &Capability) -> bool {
        self.reload();
        let removed = self.grants.revoke(plugin, cap);
        if removed {
            self.save(GrantScope::Project);
            self.save(GrantScope::Global);
        }
        removed
    }

    fn save(&mut self, scope: GrantScope) {
        let (table, path, stamp_slot) = match scope {
            GrantScope::Once => return,
            GrantScope::Project => match &self.project_path {
                Some(path) => (&self.grants.project, path, &mut self.project_stamp),
                None => return,
            },
            GrantScope::Global => (
                &self.grants.global,
                &self.global_path,
                &mut self.global_stamp,
            ),
        };
        if path.as_os_str().is_empty() {
            return;
        }
        match write_table(path, table) {
            Ok(()) => *stamp_slot = stamp(path),
            Err(e) => log::warn!("cannot save plugin grants to {}: {e}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

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
        let mut store = GrantStore::open(&paths, Some(&project));
        assert_eq!(store.decide(&id(), &net, &caps(), true), Decision::Allow);
        let read = &Capability::WorkspaceRead;
        assert_eq!(store.decide(&id(), read, &caps(), true), Decision::Allow);
        assert_eq!(
            store.decide(&id(), &Capability::EditorWrite, &caps(), true),
            Decision::Ask
        );
        // The project grant stays with its project.
        let mut elsewhere = GrantStore::open(&paths, Some(&tmp.path().join("other")));
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
        let mut honest = GrantStore::open(&paths, Some(&project));
        let write = Capability::WorkspaceWrite;
        assert_eq!(honest.decide(&id(), &write, &caps(), true), Decision::Allow);
        fs::remove_file(&paths.global_grants).unwrap();

        let mut store = GrantStore::open(&paths, Some(&project));
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
