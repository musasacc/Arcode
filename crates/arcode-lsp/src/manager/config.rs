//! Server configuration and project-root detection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{LspManager, State, absolute};
use crate::client::ServerSpec;
use crate::features::Features;

/// A configured language server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Unique name, referenced from the language table.
    pub name: String,
    /// Executable name or path.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Files/directories marking a project root (e.g. `Cargo.toml`, `.git`).
    pub root_markers: Vec<String>,
    /// Sent as `initializationOptions`; also answers `workspace/configuration`.
    pub initialization_options: Option<Value>,
    /// Features the client uses from this server (all by default).
    pub features: Features,
}

impl ServerConfig {
    /// A config with a name, a command and root markers.
    pub fn new(name: impl Into<String>, command: impl Into<String>, root_markers: &[&str]) -> Self {
        ServerConfig {
            name: name.into(),
            command: command.into(),
            root_markers: root_markers.iter().map(|m| m.to_string()).collect(),
            ..Default::default()
        }
    }

    /// The process spec for [`crate::LspClient::start`].
    pub fn spec(&self) -> ServerSpec {
        ServerSpec {
            name: self.name.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
            initialization_options: self.initialization_options.clone(),
            features: self.features,
        }
    }
}

/// Finds the project root for `file`: the nearest ancestor directory that
/// contains one of `markers`; else `workspace_root` if `file` lies inside it;
/// else the file's directory. Only checks for existence (`stat`).
pub fn find_root(file: &Path, markers: &[String], workspace_root: Option<&Path>) -> PathBuf {
    let dir = file.parent().unwrap_or(file);
    if !markers.is_empty() {
        for ancestor in dir.ancestors() {
            if markers.iter().any(|m| ancestor.join(m).exists()) {
                return ancestor.to_path_buf();
            }
        }
    }
    match workspace_root {
        Some(ws) if file.starts_with(ws) => ws.to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

impl LspManager {
    /// Marks `path`, a file outside the workspace that navigation is about
    /// to open (a standard library source a definition led to), to join a
    /// server already running for the workspace instead of starting one for
    /// its own root. Other files outside the workspace (another project
    /// opened from the file dialog) keep getting their own servers.
    pub fn attach_to_workspace(&self, path: &Path) {
        self.lock().attached.insert(absolute(path));
    }
}

/// Drops repeated server names from each language's list (keeping the
/// first, highest-priority entry), with a warning: a server listed twice
/// would otherwise get every `didOpen`/`didChange` twice.
pub(super) fn dedupe_languages(
    languages: HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    languages
        .into_iter()
        .map(|(language, names)| {
            let mut unique: Vec<String> = Vec::with_capacity(names.len());
            for name in names {
                if unique.contains(&name) {
                    log::warn!("language {language} lists server {name} more than once");
                } else {
                    unique.push(name);
                }
            }
            (language, unique)
        })
        .collect()
}

/// The root of the running `name` instance an attached `path` joins
/// instead of `own_root`, if any. An instance being replaced (restart or
/// reload) counts as running: its replacement is started at the same root.
pub(super) fn shared_root(
    state: &State,
    name: &str,
    path: &Path,
    own_root: &Path,
) -> Option<PathBuf> {
    let own = (name.to_string(), own_root.to_path_buf());
    if !state.attached.contains(path) || state.servers.contains_key(&own) {
        return None;
    }
    let running: Vec<&Path> = state
        .servers
        .iter()
        .filter(|((n, _), slot)| n == name && slot.is_usable())
        .map(|((_, r), _)| r.as_path())
        .chain(
            state
                .stopping
                .keys()
                .filter(|(n, _)| n == name)
                .map(|(_, r)| r.as_path()),
        )
        .collect();
    reuse_root(path, state.workspace_root.as_deref(), &running)
}

/// For a file outside `workspace_root`, the root of an already running
/// instance to send it to instead of starting a new server for the file's
/// own root: library sources reached through go-to-definition belong to
/// the project's server. Prefers the instance at the workspace root, then
/// one inside it. `None` when the file is inside the workspace, there is
/// no workspace, or no instance runs inside it.
pub(crate) fn reuse_root(
    file: &Path,
    workspace_root: Option<&Path>,
    running: &[&Path],
) -> Option<PathBuf> {
    let ws = workspace_root?;
    if file.starts_with(ws) || running.iter().any(|r| file.starts_with(r)) {
        return None;
    }
    running
        .iter()
        .find(|r| **r == ws)
        .or_else(|| running.iter().find(|r| r.starts_with(ws)))
        .map(|r| r.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_files_reuse_the_workspace_instance() {
        let ws = Path::new("/proj");
        let std_file = Path::new("/toolchain/library/core/src/option.rs");
        let sub = Path::new("/proj/crates/a");
        assert_eq!(
            reuse_root(std_file, Some(ws), &[sub, ws]),
            Some(ws.to_path_buf())
        );
        assert_eq!(
            reuse_root(std_file, Some(ws), &[sub]),
            Some(sub.to_path_buf())
        );
        // Nothing runs for the workspace, inside files, or no workspace.
        assert_eq!(reuse_root(std_file, Some(ws), &[Path::new("/other")]), None);
        assert_eq!(
            reuse_root(Path::new("/proj/src/a.rs"), Some(ws), &[ws]),
            None
        );
        assert_eq!(reuse_root(std_file, None, &[ws]), None);
        // A file inside a running instance's own root keeps its root.
        let lib = Path::new("/toolchain/library/core");
        assert_eq!(reuse_root(std_file, Some(ws), &[ws, lib]), None);
    }

    #[test]
    fn finds_nearest_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let src = root.join("crates/a/src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        std::fs::write(root.join("crates/a/Cargo.toml"), "").unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        let file = src.join("lib.rs");
        let markers = vec!["Cargo.toml".to_string()];
        assert_eq!(find_root(&file, &markers, None), root.join("crates/a"));
        let markers = vec![".git".to_string()];
        assert_eq!(find_root(&file, &markers, None), root);
    }

    #[test]
    fn falls_back_to_workspace_then_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("x/y");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f.txt");
        let markers = vec!["definitely-not-here.marker".to_string()];
        assert_eq!(find_root(&file, &markers, Some(tmp.path())), tmp.path());
        assert_eq!(
            find_root(&file, &markers, Some(Path::new("/elsewhere"))),
            dir
        );
        assert_eq!(find_root(&file, &[], None), dir);
    }

    #[test]
    fn config_deserializes_with_defaults() {
        let c: ServerConfig =
            serde_json::from_value(serde_json::json!({"name": "ra", "command": "rust-analyzer"}))
                .unwrap();
        assert_eq!(c.spec().command, "rust-analyzer");
        assert!(c.root_markers.is_empty());
        let c = ServerConfig::new("ra", "rust-analyzer", &["Cargo.toml"]);
        assert_eq!(c.root_markers, vec!["Cargo.toml"]);
    }
}
