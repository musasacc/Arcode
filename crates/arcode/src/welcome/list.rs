//! What the welcome screen lists: the recent projects in display order
//! (pinned first), their filter, and how times and paths read.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use arcode_config::RecentProject;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// The filter field shows when there are more projects than this.
pub const FILTER_AFTER: usize = 8;

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Project root folder.
    pub path: PathBuf,
    /// Folder name.
    pub name: String,
    /// The path as shown (`~/code/demo`).
    pub shown: String,
    /// When it was last opened, in words.
    pub when: String,
    /// Pinned to the top.
    pub pinned: bool,
    /// The folder no longer exists.
    pub missing: bool,
}

/// The rows for `recent` (stored most recent first): pinned projects
/// first, then the others, each group most recent first.
pub fn entries(
    recent: &[RecentProject],
    missing: &HashSet<PathBuf>,
    home: Option<&Path>,
    now: u64,
) -> Vec<Entry> {
    let pinned = recent.iter().filter(|p| p.pinned);
    let others = recent.iter().filter(|p| !p.pinned);
    pinned
        .chain(others)
        .map(|p| Entry {
            path: p.path.clone(),
            name: p.name.clone(),
            shown: tilde_path(&p.path, home),
            when: relative_time(p.last_opened, now),
            pinned: p.pinned,
            missing: missing.contains(&p.path),
        })
        .collect()
}

/// The indices of `entries` matching `query`, best first: projects whose
/// name matches (fuzzy, best score first, display order on ties), then
/// those found by a folder on their path (in display order). A folder is
/// matched by the start of its name (`work` finds `~/work/api`), or the
/// path as written when the query has a `/` in it, so a few letters never
/// match every project under a shared parent folder. An empty query
/// matches everything, in display order.
pub fn filter(entries: &[Entry], query: &str) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..entries.len()).collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut by_name: Vec<(u32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let score = pattern.score(Utf32Str::new(&e.name, &mut buf), &mut matcher)?;
            Some((score, i))
        })
        .collect();
    by_name.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut hits: Vec<usize> = by_name.into_iter().map(|(_, i)| i).collect();
    let query = query.to_lowercase();
    let in_folder: Vec<usize> = (0..entries.len())
        .filter(|i| !hits.contains(i) && folder_matches(&entries[*i], &query))
        .collect();
    hits.extend(in_folder);
    hits
}

/// Whether a folder on `entry`'s shown path (before its name) matches
/// `query` (lower case): a folder name starting with it, or the whole
/// path containing it when the query has a separator.
fn folder_matches(entry: &Entry, query: &str) -> bool {
    let shown = entry.shown.to_lowercase();
    if query.contains(['/', '\\']) {
        // Either separator matches either.
        return shown.replace('\\', "/").contains(&query.replace('\\', "/"));
    }
    let parent = shown
        .strip_suffix(&entry.name.to_lowercase())
        .unwrap_or(&shown);
    parent
        .split(['/', '\\'])
        .any(|folder| !folder.is_empty() && folder.starts_with(query))
}

/// Moves a list selection by `delta` rows among `len`, stopping at the
/// ends. Without a selection, Down starts at the top and Up at the bottom.
pub fn step(selected: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    Some(match selected {
        None if delta >= 0 => 0,
        None => last,
        Some(ix) => ix.min(last).saturating_add_signed(delta).min(last),
    })
}

/// When a project was last opened, in words: "just now", "5 minutes ago",
/// "2 hours ago", "yesterday", "3 days ago", "last week", "3 weeks ago",
/// "last month", "4 months ago", "over a year ago". `then` and `now` are
/// seconds since the Unix epoch; a time in the future reads "just now".
pub fn relative_time(then: u64, now: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    let ago = now.saturating_sub(then);
    let plural = |n: u64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    match ago {
        0..MINUTE => "just now".into(),
        MINUTE..HOUR => plural(ago / MINUTE, "minute"),
        HOUR..DAY => plural(ago / HOUR, "hour"),
        _ if ago < 2 * DAY => "yesterday".into(),
        _ if ago < 7 * DAY => plural(ago / DAY, "day"),
        _ if ago < 14 * DAY => "last week".into(),
        _ if ago < 30 * DAY => plural(ago / (7 * DAY), "week"),
        _ if ago < 60 * DAY => "last month".into(),
        _ if ago < 365 * DAY => plural(ago / (30 * DAY), "month"),
        _ => "over a year ago".into(),
    }
}

/// `path` with the home folder written as `~` (`~/code/demo`).
pub fn tilde_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
