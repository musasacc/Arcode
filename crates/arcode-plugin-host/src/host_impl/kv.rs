//! `arcode:plugin/kv`: keys are 1..=128 bytes of printable ASCII, and each
//! (plugin, scope) holds at most [`KV_QUOTA_BYTES`]: keys plus values plus
//! [`KV_ENTRY_BYTES`] per entry, so tiny entries cannot make the host keep
//! far more memory than the quota says.
//!
//! A scope is read from its file on first use (never at startup) and kept
//! in memory per plugin, so it survives instance restarts. Changes are
//! written back once per call ([`KvStore::flush`], called when a call
//! returns) rather than on every `set`. Without a file (no data dir, or
//! project scope with no project open) a scope lives for the session only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::bindings::arcode::plugin::kv::{self as wit, KvError, Scope};
use crate::cancel::lock;
use crate::kv_file;
use crate::paths::PluginPaths;
use crate::state::HostState;

/// Bytes per scope: keys plus values plus [`KV_ENTRY_BYTES`] per entry.
pub const KV_QUOTA_BYTES: usize = 1024 * 1024;
/// What one entry costs against the quota on top of its key and value (about
/// the host's per-entry memory: a `String`, a `Vec` and B-tree node space).
pub const KV_ENTRY_BYTES: usize = 64;
/// Longest key.
pub const MAX_KEY_BYTES: usize = 128;
/// Largest kv file read (quota plus framing).
const MAX_FILE_BYTES: u64 = 2 * KV_QUOTA_BYTES as u64;

/// One scope's entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KvScope {
    entries: BTreeMap<String, Vec<u8>>,
    bytes: usize,
}

impl KvScope {
    fn from_entries(entries: BTreeMap<String, Vec<u8>>) -> Self {
        let bytes = entries.iter().map(|(k, v)| cost(k, v)).sum();
        Self { entries, bytes }
    }

    /// Value of `key`.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.entries.get(key).map(Vec::as_slice)
    }

    /// Sets `key`, enforcing the key rules and the quota.
    pub fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), KvError> {
        if !valid_key(key) {
            return Err(KvError::InvalidKey);
        }
        let old = self.entries.get(key).map_or(0, |v| cost(key, v));
        let new = cost(key, &value);
        if self.bytes - old + new > KV_QUOTA_BYTES {
            return Err(KvError::QuotaExceeded);
        }
        self.bytes = self.bytes - old + new;
        self.entries.insert(key.to_owned(), value);
        Ok(())
    }

    /// Removes `key`; true if it existed.
    pub fn delete(&mut self, key: &str) -> bool {
        match self.entries.remove(key) {
            Some(v) => {
                self.bytes -= cost(key, &v);
                true
            }
            None => false,
        }
    }

    /// Keys starting with `prefix`, sorted.
    pub fn keys(&self, prefix: &str) -> Vec<String> {
        self.entries
            .range(prefix.to_owned()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Bytes used.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Where a plugin's two scopes are stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KvFiles {
    /// `scope::global`; `None`: session only.
    pub global: Option<PathBuf>,
    /// `scope::project`; `None`: session only.
    pub project: Option<PathBuf>,
}

impl KvFiles {
    /// No files: both scopes live for the session only.
    pub fn none() -> Self {
        Self::default()
    }

    /// The §3 locations for `plugin` and the open project (if any).
    pub fn for_plugin(paths: &PluginPaths, plugin: &str, project_root: Option<&Path>) -> Self {
        Self {
            global: Some(paths.kv_file(plugin, None)),
            project: project_root.map(|root| paths.kv_file(plugin, Some(root))),
        }
    }
}

#[derive(Debug, Default)]
struct Slot {
    scope: KvScope,
    file: Option<PathBuf>,
    loaded: bool,
    dirty: bool,
}

impl Slot {
    fn load(&mut self) -> &mut KvScope {
        if !self.loaded {
            self.loaded = true;
            if let Some(file) = &self.file {
                self.scope = read_scope(file);
            }
        }
        &mut self.scope
    }

    fn flush(&mut self) {
        let (Some(file), true) = (&self.file, self.dirty) else {
            return;
        };
        self.dirty = false;
        if let Err(e) = kv_file::write_atomic(file, &kv_file::encode(&self.scope.entries)) {
            log::warn!("cannot save plugin data {}: {e}", file.display());
        }
    }
}

fn read_scope(file: &Path) -> KvScope {
    let bytes = match kv_file::read(file, MAX_FILE_BYTES) {
        Ok(Some(b)) => b,
        Ok(None) => return KvScope::default(),
        Err(e) => {
            log::warn!("cannot read plugin data {}: {e}", file.display());
            return KvScope::default();
        }
    };
    match kv_file::decode(&bytes, valid_key, KV_QUOTA_BYTES) {
        Ok(entries) => {
            let scope = KvScope::from_entries(entries);
            if scope.bytes > KV_QUOTA_BYTES {
                log::warn!("ignoring over-quota plugin data {}", file.display());
                return KvScope::default();
            }
            scope
        }
        Err(e) => {
            log::warn!("ignoring damaged plugin data {}: {e}", file.display());
            KvScope::default()
        }
    }
}

/// Both scopes of one plugin.
#[derive(Debug, Default)]
pub struct KvStore {
    global: Slot,
    project: Slot,
}

impl KvStore {
    /// A store backed by `files` (read lazily).
    pub fn new(files: KvFiles) -> Self {
        Self {
            global: Slot {
                file: files.global,
                ..Slot::default()
            },
            project: Slot {
                file: files.project,
                ..Slot::default()
            },
        }
    }

    fn slot(&mut self, scope: Scope) -> &mut Slot {
        match scope {
            Scope::Global => &mut self.global,
            Scope::Project => &mut self.project,
        }
    }

    /// Value of `key` in `scope`.
    pub fn get(&mut self, scope: Scope, key: &str) -> Option<Vec<u8>> {
        self.slot(scope).load().get(key).map(<[u8]>::to_vec)
    }

    /// Sets `key` in `scope`.
    pub fn set(&mut self, scope: Scope, key: &str, value: Vec<u8>) -> Result<(), KvError> {
        let slot = self.slot(scope);
        slot.load().set(key, value)?;
        slot.dirty = true;
        Ok(())
    }

    /// Removes `key` from `scope`.
    pub fn delete(&mut self, scope: Scope, key: &str) {
        let slot = self.slot(scope);
        if slot.load().delete(key) {
            slot.dirty = true;
        }
    }

    /// Keys in `scope` starting with `prefix`.
    pub fn keys(&mut self, scope: Scope, prefix: &str) -> Vec<String> {
        self.slot(scope).load().keys(prefix)
    }

    /// Bytes used in `scope`.
    pub fn bytes(&mut self, scope: Scope) -> usize {
        self.slot(scope).load().bytes()
    }

    /// Writes changed scopes to their files.
    pub fn flush(&mut self) {
        self.global.flush();
        self.project.flush();
    }
}

/// What `key = value` costs against the quota.
fn cost(key: &str, value: &[u8]) -> usize {
    key.len() + value.len() + KV_ENTRY_BYTES
}

fn valid_key(key: &str) -> bool {
    (1..=MAX_KEY_BYTES).contains(&key.len()) && key.bytes().all(|b| (0x20..0x7f).contains(&b))
}

impl wit::Host for HostState {
    fn get(&mut self, scope: Scope, key: String) -> wasmtime::Result<Option<Vec<u8>>> {
        Ok(lock(&self.ctx.shared.kv).get(scope, &key))
    }

    fn set(
        &mut self,
        scope: Scope,
        key: String,
        value: Vec<u8>,
    ) -> wasmtime::Result<Result<(), KvError>> {
        Ok(lock(&self.ctx.shared.kv).set(scope, &key, value))
    }

    fn delete(&mut self, scope: Scope, key: String) -> wasmtime::Result<()> {
        lock(&self.ctx.shared.kv).delete(scope, &key);
        Ok(())
    }

    fn keys(&mut self, scope: Scope, prefix: String) -> wasmtime::Result<Vec<String>> {
        Ok(lock(&self.ctx.shared.kv).keys(scope, &prefix))
    }
}

#[cfg(test)]
#[path = "kv_tests.rs"]
mod tests;
