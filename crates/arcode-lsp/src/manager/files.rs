//! Files created, renamed or deleted inside Arcode (file tree, refactors):
//! the servers of their project hear about it at once instead of relying
//! on their own file watching, which can miss or reorder the change.

use std::path::{Path, PathBuf};

use lsp_types::notification::DidChangeWatchedFiles;
use lsp_types::request::Request;
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent};

use super::LspManager;
use super::actor::Command;
use crate::client::LspClient;
use crate::uri::path_to_uri;

/// rust-analyzer's request to run build scripts again; answering it
/// rebuilds the crate graph from the files as they are now.
enum RebuildProcMacros {}

impl Request for RebuildProcMacros {
    type Params = ();
    type Result = ();
    const METHOD: &'static str = "rust-analyzer/rebuildProcMacros";
}

/// One change made by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    /// A new file or folder.
    Created(PathBuf),
    /// A file or folder moved from the first path to the second.
    Renamed(PathBuf, PathBuf),
    /// A removed file or folder.
    Deleted(PathBuf),
}

/// The `didChangeWatchedFiles` events of `changes` under `root` (a rename
/// is a delete plus a create).
pub(crate) fn watched_events(root: &Path, changes: &[FileChange]) -> Vec<FileEvent> {
    let mut events = Vec::new();
    let mut push = |path: &Path, typ| {
        if path.starts_with(root) {
            events.push(FileEvent::new(path_to_uri(path), typ));
        }
    };
    for change in changes {
        match change {
            FileChange::Created(path) => push(path, FileChangeType::CREATED),
            FileChange::Deleted(path) => push(path, FileChangeType::DELETED),
            FileChange::Renamed(from, to) => {
                push(from, FileChangeType::DELETED);
                push(to, FileChangeType::CREATED);
            }
        }
    }
    events
}

/// Whether rust-analyzer must rebuild its crate graph after `changes`: a
/// Rust file or a folder (which may hold some) was added, moved or removed.
/// Otherwise it can keep a graph without a renamed crate root (`main.rs`
/// renamed and back, or to the `path` of a `[[bin]]`): completion, hover
/// and inlay hints then stay empty until it restarts.
pub(crate) fn needs_crate_graph_rebuild(changes: &[FileChange]) -> bool {
    let rusty = |p: &Path| {
        p.extension().is_none_or(|e| e == "rs") || p.file_name().is_some_and(|n| n == "Cargo.toml")
    };
    changes.iter().any(|c| match c {
        FileChange::Created(p) | FileChange::Deleted(p) => rusty(p),
        FileChange::Renamed(from, to) => rusty(from) || rusty(to),
    })
}

fn is_rust_analyzer(client: &LspClient) -> bool {
    client
        .server_info()
        .is_some_and(|i| i.name == "rust-analyzer")
}

impl LspManager {
    /// Reports files the user created, renamed or deleted to every started
    /// server whose project root contains one of them: a
    /// `workspace/didChangeWatchedFiles` notification, and for
    /// rust-analyzer a crate graph rebuild when Rust files moved (see
    /// [`needs_crate_graph_rebuild`]). Servers still starting get it once
    /// they are initialized. Call it after the change is on disk and the
    /// documents were reopened under their new paths.
    pub fn files_changed(&self, changes: &[FileChange]) {
        let rebuild = needs_crate_graph_rebuild(changes);
        let state = self.lock();
        for slot in state.servers.values() {
            let events = watched_events(&slot.root, changes);
            if events.is_empty() {
                continue;
            }
            slot.send(Command::Run(Box::new(move |client: &LspClient| {
                let params = DidChangeWatchedFilesParams { changes: events };
                if let Err(err) = client.notify::<DidChangeWatchedFiles>(params) {
                    log::debug!("didChangeWatchedFiles to {}: {err}", client.name());
                }
                if rebuild && is_rust_analyzer(client) {
                    let request = client.request::<RebuildProcMacros>(());
                    // Dropping the request would cancel it.
                    tokio::spawn(async move {
                        if let Err(err) = request.await {
                            log::debug!("rust-analyzer/rebuildProcMacros: {err}");
                        }
                    });
                }
            })));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn rename_is_a_delete_and_a_create_inside_the_root() {
        let changes = [
            FileChange::Renamed(p("/ws/src/main.rs"), p("/ws/src/app.rs")),
            FileChange::Created(p("/other/x.rs")),
            FileChange::Deleted(p("/ws/old")),
        ];
        let events = watched_events(Path::new("/ws"), &changes);
        let got: Vec<(lsp_types::Uri, FileChangeType)> =
            events.into_iter().map(|e| (e.uri, e.typ)).collect();
        let uri = |s: &str| path_to_uri(Path::new(s));
        assert_eq!(
            got,
            [
                (uri("/ws/src/main.rs"), FileChangeType::DELETED),
                (uri("/ws/src/app.rs"), FileChangeType::CREATED),
                (uri("/ws/old"), FileChangeType::DELETED),
            ]
        );
        assert!(watched_events(Path::new("/nowhere"), &changes).is_empty());
    }

    #[test]
    fn rebuild_only_for_rust_files_folders_and_manifests() {
        let rename = |a: &str, b: &str| FileChange::Renamed(p(a), p(b));
        assert!(needs_crate_graph_rebuild(&[rename(
            "/w/main.rs",
            "/w/app.rs"
        )]));
        assert!(needs_crate_graph_rebuild(&[rename(
            "/w/notes.txt",
            "/w/lib.rs"
        )]));
        assert!(needs_crate_graph_rebuild(&[FileChange::Deleted(p(
            "/w/src"
        ))]));
        assert!(needs_crate_graph_rebuild(&[FileChange::Created(p(
            "/w/b/Cargo.toml"
        ))]));
        assert!(!needs_crate_graph_rebuild(&[rename("/w/a.md", "/w/b.md")]));
        assert!(!needs_crate_graph_rebuild(&[FileChange::Created(p(
            "/w/x.py"
        ))]));
    }
}
