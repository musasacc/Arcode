//! `arcode:plugin/kv`: keys are 1..=128 bytes of printable ASCII, and each
//! (plugin, scope) holds at most [`KV_QUOTA_BYTES`] of keys plus values.
//!
//! Kept in memory per plugin (it survives instance restarts); writing it to
//! `<data_dir>/arcode/plugin-data/<id>/{global,<hash>}.kv` is plan step 6.

use std::collections::BTreeMap;

use crate::bindings::arcode::plugin::kv::{self as wit, KvError, Scope};
use crate::cancel::lock;
use crate::state::HostState;

/// Bytes of keys plus values per scope.
pub const KV_QUOTA_BYTES: usize = 1024 * 1024;
/// Longest key.
pub const MAX_KEY_BYTES: usize = 128;

/// One scope's entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KvScope {
    entries: BTreeMap<String, Vec<u8>>,
    bytes: usize,
}

impl KvScope {
    /// Value of `key`.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.entries.get(key).map(Vec::as_slice)
    }

    /// Sets `key`, enforcing the key rules and the quota.
    pub fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), KvError> {
        if !valid_key(key) {
            return Err(KvError::InvalidKey);
        }
        let old = self.entries.get(key).map_or(0, |v| key.len() + v.len());
        let new = key.len() + value.len();
        if self.bytes - old + new > KV_QUOTA_BYTES {
            return Err(KvError::QuotaExceeded);
        }
        self.bytes = self.bytes - old + new;
        self.entries.insert(key.to_owned(), value);
        Ok(())
    }

    /// Removes `key`.
    pub fn delete(&mut self, key: &str) {
        if let Some(v) = self.entries.remove(key) {
            self.bytes -= key.len() + v.len();
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

/// Both scopes of one plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KvStore {
    /// `scope::global`.
    pub global: KvScope,
    /// `scope::project` (the open project).
    pub project: KvScope,
}

impl KvStore {
    fn scope(&mut self, scope: Scope) -> &mut KvScope {
        match scope {
            Scope::Global => &mut self.global,
            Scope::Project => &mut self.project,
        }
    }
}

fn valid_key(key: &str) -> bool {
    (1..=MAX_KEY_BYTES).contains(&key.len()) && key.bytes().all(|b| (0x20..0x7f).contains(&b))
}

impl wit::Host for HostState {
    fn get(&mut self, scope: Scope, key: String) -> wasmtime::Result<Option<Vec<u8>>> {
        let mut kv = lock(&self.ctx.shared.kv);
        Ok(kv.scope(scope).get(&key).map(<[u8]>::to_vec))
    }

    fn set(&mut self, scope: Scope, key: String, value: Vec<u8>) -> wasmtime::Result<Result<(), KvError>> {
        let mut kv = lock(&self.ctx.shared.kv);
        Ok(kv.scope(scope).set(&key, value))
    }

    fn delete(&mut self, scope: Scope, key: String) -> wasmtime::Result<()> {
        lock(&self.ctx.shared.kv).scope(scope).delete(&key);
        Ok(())
    }

    fn keys(&mut self, scope: Scope, prefix: String) -> wasmtime::Result<Vec<String>> {
        Ok(lock(&self.ctx.shared.kv).scope(scope).keys(&prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_quota() {
        let mut s = KvScope::default();
        assert_eq!(s.set("", vec![]), Err(KvError::InvalidKey));
        assert_eq!(s.set("tab\t", vec![]), Err(KvError::InvalidKey));
        assert_eq!(s.set(&"k".repeat(129), vec![]), Err(KvError::InvalidKey));
        s.set("a/1", vec![0; 10]).unwrap();
        s.set("a/2", vec![0; 10]).unwrap();
        s.set("b", vec![]).unwrap();
        assert_eq!(s.keys("a/"), ["a/1", "a/2"]);
        assert_eq!(s.bytes(), 3 + 10 + 3 + 10 + 1);
        let big = KV_QUOTA_BYTES - s.bytes() - 1;
        assert_eq!(s.set("c", vec![0; big + 1]), Err(KvError::QuotaExceeded));
        s.set("c", vec![0; big]).unwrap();
        assert_eq!(s.bytes(), KV_QUOTA_BYTES);
        // Replacing a value frees the old one first.
        s.set("c", vec![0; big]).unwrap();
        s.delete("c");
        s.delete("missing");
        assert_eq!(s.bytes(), 27);
        assert_eq!(s.get("a/1").map(<[u8]>::len), Some(10));
    }
}
