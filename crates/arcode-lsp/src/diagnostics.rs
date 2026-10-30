//! Merged diagnostics from all servers, as the UI displays them.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::event::LspEvent;

/// Number of diagnostics per severity. A missing severity counts as error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiagnosticCounts {
    /// Errors.
    pub errors: usize,
    /// Warnings.
    pub warnings: usize,
    /// Informational messages.
    pub infos: usize,
    /// Hints.
    pub hints: usize,
}

impl DiagnosticCounts {
    /// Sum of all severities.
    pub fn total(&self) -> usize {
        self.errors + self.warnings + self.infos + self.hints
    }

    fn add(&mut self, d: &Diagnostic) {
        match d.severity {
            Some(DiagnosticSeverity::WARNING) => self.warnings += 1,
            Some(DiagnosticSeverity::INFORMATION) => self.infos += 1,
            Some(DiagnosticSeverity::HINT) => self.hints += 1,
            _ => self.errors += 1,
        }
    }
}

/// The label shown next to a diagnostic: its `source`, else the server name.
pub fn source_label<'a>(server: &'a str, diagnostic: &'a Diagnostic) -> &'a str {
    diagnostic.source.as_deref().unwrap_or(server)
}

/// Diagnostics per `(path, server)`. Pure data; no I/O.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticStore {
    by_path: BTreeMap<PathBuf, BTreeMap<String, Vec<Diagnostic>>>,
}

impl DiagnosticStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces `server`'s diagnostics for `path`; an empty list removes them.
    pub fn update(&mut self, server: &str, path: impl Into<PathBuf>, diagnostics: Vec<Diagnostic>) {
        let path = path.into();
        if diagnostics.is_empty() {
            if let Some(servers) = self.by_path.get_mut(&path) {
                servers.remove(server);
                if servers.is_empty() {
                    self.by_path.remove(&path);
                }
            }
            return;
        }
        self.by_path
            .entry(path)
            .or_default()
            .insert(server.to_string(), diagnostics);
    }

    /// Applies a [`LspEvent::Diagnostics`] event; returns true if it was one.
    pub fn apply_event(&mut self, event: &LspEvent) -> bool {
        match event {
            LspEvent::Diagnostics {
                server,
                path,
                diagnostics,
                ..
            } => {
                self.update(server, path.clone(), diagnostics.clone());
                true
            }
            _ => false,
        }
    }

    /// Removes everything `server` reported (e.g. after it stopped).
    pub fn clear_server(&mut self, server: &str) {
        self.by_path.retain(|_, servers| {
            servers.remove(server);
            !servers.is_empty()
        });
    }

    /// Removes all diagnostics for `path`.
    pub fn clear_path(&mut self, path: &Path) {
        self.by_path.remove(path);
    }

    /// All diagnostics for `path` from every server, sorted by position,
    /// then severity, then server name.
    pub fn for_path(&self, path: &Path) -> Vec<(&str, &Diagnostic)> {
        let mut out: Vec<(&str, &Diagnostic)> = self
            .by_path
            .get(path)
            .into_iter()
            .flat_map(|servers| {
                servers
                    .iter()
                    .flat_map(|(s, ds)| ds.iter().map(move |d| (s.as_str(), d)))
            })
            .collect();
        out.sort_by(|a, b| compare(a.1, b.1).then_with(|| a.0.cmp(b.0)));
        out
    }

    /// Every diagnostic as `(path, server, diagnostic)`, sorted by path then
    /// position.
    pub fn all(&self) -> impl Iterator<Item = (&Path, &str, &Diagnostic)> + '_ {
        self.by_path.keys().flat_map(move |path| {
            self.for_path(path)
                .into_iter()
                .map(move |(server, d)| (path.as_path(), server, d))
        })
    }

    /// Paths that currently have diagnostics, sorted.
    pub fn paths(&self) -> impl Iterator<Item = &Path> + '_ {
        self.by_path.keys().map(PathBuf::as_path)
    }

    /// Totals over all files.
    pub fn counts(&self) -> DiagnosticCounts {
        let mut c = DiagnosticCounts::default();
        for servers in self.by_path.values() {
            servers.values().flatten().for_each(|d| c.add(d));
        }
        c
    }

    /// Totals for one file.
    pub fn counts_for(&self, path: &Path) -> DiagnosticCounts {
        let mut c = DiagnosticCounts::default();
        if let Some(servers) = self.by_path.get(path) {
            servers.values().flatten().for_each(|d| c.add(d));
        }
        c
    }

    /// Totals per file, sorted by path.
    pub fn counts_by_path(&self) -> Vec<(&Path, DiagnosticCounts)> {
        self.by_path
            .keys()
            .map(|p| (p.as_path(), self.counts_for(p)))
            .collect()
    }

    /// True when no diagnostics are stored.
    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

fn severity_rank(d: &Diagnostic) -> u8 {
    match d.severity {
        Some(DiagnosticSeverity::WARNING) => 2,
        Some(DiagnosticSeverity::INFORMATION) => 3,
        Some(DiagnosticSeverity::HINT) => 4,
        _ => 1,
    }
}

fn compare(a: &Diagnostic, b: &Diagnostic) -> Ordering {
    let key = |d: &Diagnostic| (d.range.start.line, d.range.start.character);
    key(a)
        .cmp(&key(b))
        .then_with(|| severity_rank(a).cmp(&severity_rank(b)))
        .then_with(|| a.message.cmp(&b.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range};

    fn diag(
        line: u32,
        col: u32,
        sev: Option<DiagnosticSeverity>,
        source: Option<&str>,
    ) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, col), Position::new(line, col + 1)),
            severity: sev,
            source: source.map(str::to_string),
            message: format!("{line}:{col}"),
            ..Default::default()
        }
    }

    #[test]
    fn merges_two_servers_on_one_file() {
        let mut s = DiagnosticStore::new();
        let p = PathBuf::from("/p/main.rs");
        s.update(
            "rust-analyzer",
            &p,
            vec![
                diag(5, 0, Some(DiagnosticSeverity::ERROR), Some("rustc")),
                diag(1, 4, Some(DiagnosticSeverity::WARNING), None),
            ],
        );
        s.update(
            "typos",
            &p,
            vec![diag(3, 2, Some(DiagnosticSeverity::HINT), None)],
        );
        let merged = s.for_path(&p);
        let lines: Vec<u32> = merged.iter().map(|(_, d)| d.range.start.line).collect();
        assert_eq!(lines, vec![1, 3, 5]);
        let labels: Vec<&str> = merged.iter().map(|(srv, d)| source_label(srv, d)).collect();
        assert_eq!(labels, vec!["rust-analyzer", "typos", "rustc"]);
        let c = s.counts_for(&p);
        assert_eq!((c.errors, c.warnings, c.infos, c.hints), (1, 1, 0, 1));
        assert_eq!(c.total(), 3);

        // Replacing one server's set leaves the other untouched.
        s.update("rust-analyzer", &p, vec![]);
        let merged = s.for_path(&p);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].0, "typos");
        s.update("typos", &p, vec![]);
        assert!(s.is_empty());
    }

    #[test]
    fn all_is_sorted_by_path_then_position() {
        let mut s = DiagnosticStore::new();
        s.update("a", "/z.rs", vec![diag(0, 0, None, None)]);
        s.update(
            "a",
            "/b.rs",
            vec![diag(9, 0, None, None), diag(2, 3, None, None)],
        );
        s.update(
            "b",
            "/b.rs",
            vec![diag(2, 1, Some(DiagnosticSeverity::INFORMATION), None)],
        );
        let all: Vec<(String, u32, u32)> = s
            .all()
            .map(|(p, _, d)| {
                (
                    p.display().to_string(),
                    d.range.start.line,
                    d.range.start.character,
                )
            })
            .collect();
        assert_eq!(
            all,
            vec![
                ("/b.rs".into(), 2, 1),
                ("/b.rs".into(), 2, 3),
                ("/b.rs".into(), 9, 0),
                ("/z.rs".into(), 0, 0)
            ]
        );
        let c = s.counts();
        assert_eq!((c.errors, c.infos), (3, 1));
        let per_path = s.counts_by_path();
        assert_eq!(per_path.len(), 2);
        assert_eq!(per_path[0].1.total(), 3);
        assert_eq!(s.paths().count(), 2);
    }

    #[test]
    fn same_position_orders_by_severity() {
        let mut s = DiagnosticStore::new();
        s.update(
            "a",
            "/x",
            vec![diag(1, 1, Some(DiagnosticSeverity::HINT), None)],
        );
        s.update(
            "b",
            "/x",
            vec![diag(1, 1, Some(DiagnosticSeverity::ERROR), None)],
        );
        let v = s.for_path(Path::new("/x"));
        assert_eq!(v[0].0, "b");
    }

    #[test]
    fn clear_server_and_events() {
        let mut s = DiagnosticStore::new();
        let ev = LspEvent::Diagnostics {
            server: "a".into(),
            path: "/x".into(),
            version: Some(1),
            diagnostics: vec![diag(0, 0, None, None)],
        };
        assert!(s.apply_event(&ev));
        s.update("b", "/x", vec![diag(0, 0, None, None)]);
        s.clear_server("a");
        assert_eq!(s.for_path(Path::new("/x")).len(), 1);
        s.clear_path(Path::new("/x"));
        assert!(s.is_empty());
        let other = LspEvent::ShowMessage {
            server: "a".into(),
            typ: lsp_types::MessageType::INFO,
            message: "m".into(),
        };
        assert!(!s.apply_event(&other));
    }
}
