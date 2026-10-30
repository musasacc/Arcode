//! The in-memory network log (§4 "Visibility and revocation"): every
//! request a plugin made, with time, method, host and status. Bodies,
//! headers, paths and query strings are never logged.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::SystemTime;

use crate::cancel::lock;

/// Entries kept per plugin (oldest dropped first).
pub const MAX_ENTRIES: usize = 500;

/// How a request ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkOutcome {
    /// The final response's status.
    Status(u16),
    /// The host refused it before anything was sent (no gesture, no
    /// consent, master switch, host not in the manifest).
    Refused(String),
    /// It failed in transit (DNS, connect, TLS, timeout, cancelled, ...).
    Failed(String),
}

/// One request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkEntry {
    /// When it was made.
    pub at: SystemTime,
    /// HTTP method (upper-case).
    pub method: String,
    /// Host from the parsed URL (empty if the URL did not parse).
    pub host: String,
    /// Port from the parsed URL.
    pub port: u16,
    /// What happened.
    pub outcome: NetworkOutcome,
}

/// A bounded log of one plugin's requests.
#[derive(Debug, Default)]
pub struct NetworkLog {
    entries: Mutex<VecDeque<NetworkEntry>>,
}

impl NetworkLog {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an entry, dropping the oldest beyond [`MAX_ENTRIES`].
    pub fn push(&self, entry: NetworkEntry) {
        let mut entries = lock(&self.entries);
        if entries.len() >= MAX_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Every entry, oldest first.
    pub fn entries(&self) -> Vec<NetworkEntry> {
        lock(&self.entries).iter().cloned().collect()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        lock(&self.entries).len()
    }

    /// True if no request was logged.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_entries() {
        let log = NetworkLog::new();
        for i in 0..(MAX_ENTRIES as u16 + 3) {
            log.push(NetworkEntry {
                at: SystemTime::now(),
                method: "GET".into(),
                host: "h".into(),
                port: i,
                outcome: NetworkOutcome::Status(200),
            });
        }
        let entries = log.entries();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].port, 3);
    }
}
