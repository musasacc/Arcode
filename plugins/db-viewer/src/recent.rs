//! Settings and the recent-queries list (project-scope `kv`, §10).

use arcode_plugin::{host, kv};
use serde::Deserialize;

/// kv key of the recent-queries list (a JSON array of strings).
const KEY: &str = "recent-queries";

/// Queries kept.
pub const MAX_RECENT: usize = 20;

/// Longest query kept in the list (bytes); longer ones are not stored.
const MAX_QUERY_BYTES: usize = 8 * 1024;

/// The plugin's settings (manifest defaults merged with the user's values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// Rows kept from one query.
    pub max_result_rows: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_result_rows: 100_000,
        }
    }
}

impl Settings {
    /// Current settings; malformed values fall back to the defaults.
    pub fn load() -> Self {
        host::settings_as::<Self>().unwrap_or_default()
    }

    /// `max-result-rows`, clamped to `1..=1_000_000`.
    pub fn max_rows(&self) -> usize {
        usize::try_from(self.max_result_rows.clamp(1, 1_000_000)).unwrap_or(1_000_000)
    }
}

/// The recent queries, newest first. A missing or damaged entry is empty.
pub fn load() -> Vec<String> {
    kv::get(kv::Scope::Project, KEY)
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
}

/// Moves `sql` to the front of `list` (deduplicated, capped) and stores it.
pub fn remember(list: &mut Vec<String>, sql: &str) {
    let sql = sql.trim();
    if sql.is_empty() || sql.len() > MAX_QUERY_BYTES {
        return;
    }
    list.retain(|q| q != sql);
    list.insert(0, sql.to_owned());
    list.truncate(MAX_RECENT);
    if let Ok(bytes) = serde_json::to_vec(list) {
        // Over quota: the list just is not saved this time.
        let _ = kv::set(kv::Scope::Project, KEY, &bytes);
    }
}

/// One line of a query for the drop-down.
pub fn label(sql: &str) -> String {
    let line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::cells::cut(&line, 80)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_plugin::testing;

    #[test]
    fn recent_queries_are_deduplicated_capped_and_stored() {
        testing::reset();
        let mut list = load();
        assert!(list.is_empty());
        for i in 0..25 {
            remember(&mut list, &format!("SELECT {i}"));
        }
        remember(&mut list, "  SELECT 3 ");
        remember(&mut list, "   ");
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0], "SELECT 3");
        assert_eq!(list.iter().filter(|q| *q == "SELECT 3").count(), 1);
        assert_eq!(load(), list);
        testing::with(|h| {
            h.kv.insert((kv::Scope::Project, KEY.into()), b"{broken".to_vec());
        });
        assert!(load().is_empty());
    }

    #[test]
    fn settings_merge_and_clamp() {
        testing::reset();
        assert_eq!(Settings::load().max_rows(), 100_000);
        testing::with(|h| h.settings = r#"{"max-result-rows": 0}"#.into());
        assert_eq!(Settings::load().max_rows(), 1);
        testing::with(|h| h.settings = r#"{"max-result-rows": "x"}"#.into());
        assert_eq!(Settings::load(), Settings::default());
        assert_eq!(label("SELECT *\n  FROM t"), "SELECT * FROM t");
    }
}
