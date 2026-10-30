//! Server configuration and project-root detection.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::ServerSpec;

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

#[cfg(test)]
mod tests {
    use super::*;

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
