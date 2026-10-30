//! The last 50 exchanges, kept in project-scope `kv` (docs/PLUGINS.md §9).
//!
//! Only a summary is stored (time, name, method, URL, outcome, timing,
//! size), never headers or bodies, so the history stays far below the
//! 1 MiB kv quota and secrets in headers are not written to disk.

use arcode_plugin::kv;
use serde::{Deserialize, Serialize};

/// How many exchanges are kept.
pub const MAX_ENTRIES: usize = 50;
/// kv key (project scope).
const KEY: &str = "history";
/// URLs longer than this are cut before they are stored.
const MAX_URL_CHARS: usize = 2048;

/// One exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Milliseconds since the Unix epoch.
    pub time_ms: u64,
    /// The request's name, if it has one.
    pub name: Option<String>,
    /// Method.
    pub method: String,
    /// Resolved URL (capped).
    pub url: String,
    /// The response status, or `None` when no response arrived.
    pub status: Option<u16>,
    /// Why no response arrived (`denied`, `cancelled`, ...).
    pub error: Option<String>,
    /// Total time in ms (0 without a response).
    pub total_ms: u32,
    /// Response body size in bytes.
    pub size: u64,
}

impl Entry {
    /// Caps the URL so one entry stays small.
    pub fn capped(mut self) -> Self {
        if self.url.chars().count() > MAX_URL_CHARS {
            self.url = self.url.chars().take(MAX_URL_CHARS).collect::<String>() + "…";
        }
        self
    }
}

/// The stored entries, newest first. A damaged value reads as empty.
pub fn load() -> Vec<Entry> {
    kv::get(kv::Scope::Project, KEY)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Adds `entry` in front, keeps [`MAX_ENTRIES`] and stores the list.
/// Returns the new list.
pub fn push(entry: Entry) -> Result<Vec<Entry>, String> {
    let mut entries = load();
    entries.insert(0, entry.capped());
    entries.truncate(MAX_ENTRIES);
    let bytes = serde_json::to_vec(&entries).map_err(|e| e.to_string())?;
    kv::set(kv::Scope::Project, KEY, &bytes)
        .map_err(|e| format!("cannot save the history: {e:?}"))?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_plugin::testing;

    fn entry(i: u64) -> Entry {
        Entry {
            time_ms: i,
            name: None,
            method: "GET".into(),
            url: format!("http://x/{i}"),
            status: Some(200),
            error: None,
            total_ms: 1,
            size: 2,
        }
    }

    #[test]
    fn keeps_the_newest_fifty() {
        testing::reset();
        assert!(load().is_empty());
        for i in 0..60 {
            push(entry(i)).unwrap();
        }
        let all = load();
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0].time_ms, 59);
        assert_eq!(all[49].time_ms, 10);
        let stored = testing::with(|h| h.kv[&(kv::Scope::Project, KEY.to_owned())].len());
        assert!(stored < 16 * 1024, "{stored} bytes");
    }

    #[test]
    fn damaged_values_read_as_empty_and_urls_are_capped() {
        testing::reset();
        kv::set(kv::Scope::Project, KEY, b"{broken").unwrap();
        assert!(load().is_empty());
        let mut long = entry(1);
        long.url = "x".repeat(5000);
        let list = push(long).unwrap();
        assert_eq!(list[0].url.chars().count(), MAX_URL_CHARS + 1);
    }
}
