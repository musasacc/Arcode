//! `<config_dir>/arcode/plugins.toml` (§3 "State"): per plugin, whether the
//! user enabled it and the sha256 of the `plugin.wasm` they approved.
//!
//! ```toml
//! [[plugin]]
//! id = "acme.todo"
//! enabled = true
//! sha256 = "9f86d0…"
//! ```
//!
//! The file lives in the user's config dir only, never in a project. A
//! malformed file reads as empty (every installed plugin then starts
//! disabled again); malformed entries are skipped. Writes are atomic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::PluginId;

/// Largest `plugins.toml` read.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// What is stored for one plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginState {
    /// The user enabled it.
    pub enabled: bool,
    /// Lower-case hex sha256 of the approved `plugin.wasm` (installed
    /// plugins; bundled ones ship with the app and have none).
    pub sha256: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default, rename = "plugin")]
    plugins: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

/// The states of every plugin the user enabled or disabled.
#[derive(Debug, Default)]
pub struct PluginStates {
    /// `None`: kept in memory only.
    path: Option<PathBuf>,
    map: BTreeMap<String, PluginState>,
}

fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl PluginStates {
    /// Reads `path` (`None`: in memory only).
    pub fn load(path: Option<PathBuf>) -> Self {
        let map = path.as_deref().map(read).unwrap_or_default();
        Self { path, map }
    }

    /// The stored state of `plugin`.
    pub fn get(&self, plugin: &PluginId) -> Option<&PluginState> {
        self.map.get(plugin.as_str())
    }

    /// Stores `state` for `plugin` and writes the file.
    pub fn set(&mut self, plugin: &PluginId, state: PluginState) {
        self.map.insert(plugin.to_string(), state);
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let file = StateFile {
            plugins: self
                .map
                .iter()
                .map(|(id, s)| Entry {
                    id: id.clone(),
                    enabled: s.enabled,
                    sha256: s.sha256.clone(),
                })
                .collect(),
        };
        let written = toml::to_string(&file)
            .map_err(std::io::Error::other)
            .and_then(|text| crate::kv_file::write_atomic(path, text.as_bytes()));
        if let Err(e) = written {
            log::warn!("cannot save {}: {e}", path.display());
        }
    }
}

fn read(path: &Path) -> BTreeMap<String, PluginState> {
    let bytes = match crate::kv_file::read(path, MAX_FILE_BYTES) {
        Ok(Some(b)) => b,
        Ok(None) => return BTreeMap::new(),
        Err(e) => {
            log::warn!("cannot read {}: {e}", path.display());
            return BTreeMap::new();
        }
    };
    let file: StateFile = match std::str::from_utf8(&bytes)
        .map_err(|e| e.to_string())
        .and_then(|t| toml::from_str(t).map_err(|e| e.to_string()))
    {
        Ok(f) => f,
        Err(e) => {
            log::warn!("ignoring malformed {}: {e}", path.display());
            return BTreeMap::new();
        }
    };
    file.plugins
        .into_iter()
        .filter(|e| PluginId::parse(&e.id).is_ok())
        .filter(|e| e.sha256.as_deref().is_none_or(valid_sha))
        .map(|e| {
            let state = PluginState {
                enabled: e.enabled,
                sha256: e.sha256,
            };
            (e.id, state)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_persist_and_bad_entries_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("arcode/plugins.toml");
        let id = PluginId::parse("acme.todo").unwrap();
        let sha = "a".repeat(64);
        let mut states = PluginStates::load(Some(path.clone()));
        assert_eq!(states.get(&id), None);
        states.set(
            &id,
            PluginState {
                enabled: true,
                sha256: Some(sha.clone()),
            },
        );
        let again = PluginStates::load(Some(path.clone()));
        assert_eq!(again.get(&id).and_then(|s| s.sha256.clone()), Some(sha));
        let text = "[[plugin]]\nid = \"ok.one\"\nenabled = true\n\
                    [[plugin]]\nid = \"BAD\"\nenabled = true\n\
                    [[plugin]]\nid = \"ok.two\"\nenabled = true\nsha256 = \"xyz\"\n";
        std::fs::write(&path, text).unwrap();
        let read = PluginStates::load(Some(path.clone()));
        assert_eq!(read.map.keys().collect::<Vec<_>>(), ["ok.one"]);
        std::fs::write(&path, "not toml [").unwrap();
        assert!(PluginStates::load(Some(path)).map.is_empty());
    }
}
