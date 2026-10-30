//! `[language.<id>]` and `[lsp.<name>]` sections plus their built-in defaults.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// Per-language overrides (`[language.<id>]`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageSettings {
    /// Names of `[lsp.<name>]` servers to start for this language, in order.
    pub servers: Vec<String>,
    /// External formatter command line (e.g. `"prettier --stdin-filepath {file}"`);
    /// `None` formats through the language server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formatter: Option<String>,
    /// Overrides `editor.tab_size`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_size: Option<usize>,
    /// Overrides `editor.insert_spaces`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_spaces: Option<bool>,
}

impl_section!(LanguageSettings {
    servers,
    formatter,
    tab_size,
    insert_spaces,
});

impl LanguageSettings {
    /// Settings that start the given servers.
    pub fn with_servers(servers: &[&str]) -> Self {
        Self {
            servers: servers.iter().map(|s| (*s).to_owned()).collect(),
            ..Self::default()
        }
    }

    pub(crate) fn validate(&mut self, path: &str, warnings: &mut Vec<String>) {
        if let Some(size) = self.tab_size
            && !(1..=32).contains(&size)
        {
            warnings.push(format!(
                "`{path}.tab_size` must be between 1 and 32, got {size}; ignored"
            ));
            self.tab_size = None;
        }
        if self
            .formatter
            .as_deref()
            .is_some_and(|f| f.trim().is_empty())
        {
            self.formatter = None;
        }
    }
}

/// How to launch a language server (`[lsp.<name>]`).
///
/// `initialization_options` is kept as a raw [`toml::Value`]; converting it to
/// JSON for the `initialize` request is done by the LSP client.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LspServerSettings {
    /// Executable name or path. Defaults to the server's table name.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Extra environment variables for the server process.
    pub env: BTreeMap<String, String>,
    /// Files whose presence marks a workspace root (searched upwards).
    pub root_markers: Vec<String>,
    /// Passed as `initializationOptions` in the LSP `initialize` request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<toml::Value>,
}

impl_section!(LspServerSettings {
    command,
    args,
    env,
    root_markers,
    initialization_options,
});

impl LspServerSettings {
    /// Settings that run `command` with no arguments.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            ..Self::default()
        }
    }

    fn with(command: &str, args: &[&str], markers: &[&str]) -> Self {
        Self {
            command: command.to_owned(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            root_markers: markers.iter().map(|s| (*s).to_owned()).collect(),
            ..Self::default()
        }
    }

    pub(crate) fn validate(&mut self, name: &str, path: &str, warnings: &mut Vec<String>) {
        if self.command.trim().is_empty() {
            warnings.push(format!("`{path}.command` is empty; using `{name}`"));
            self.command = name.to_owned();
        }
    }
}

/// A language server resolved for a language: its config key and settings.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedServer {
    /// Key of the `[lsp.<name>]` table.
    pub name: String,
    /// Launch settings.
    pub settings: LspServerSettings,
}

/// Built-in language server definitions, merged under the user's `[lsp.*]`.
pub fn builtin_servers() -> BTreeMap<String, LspServerSettings> {
    [
        LspServerSettings::with("rust-analyzer", &[], &["Cargo.toml"]),
        LspServerSettings::with(
            "typescript-language-server",
            &["--stdio"],
            &["package.json", "tsconfig.json"],
        ),
        LspServerSettings::with(
            "pyright-langserver",
            &["--stdio"],
            &["pyproject.toml", "setup.py", "requirements.txt"],
        ),
        LspServerSettings::with("gopls", &[], &["go.mod"]),
        LspServerSettings::with("clangd", &[], &["compile_commands.json", "CMakeLists.txt"]),
        LspServerSettings::with("taplo", &["lsp", "stdio"], &[]),
        LspServerSettings::with("vscode-json-language-server", &["--stdio"], &[]),
    ]
    .into_iter()
    .map(|server| (server.command.clone(), server))
    .collect()
}

/// Built-in language → server assignments, merged under `[language.*]`.
pub fn builtin_languages() -> BTreeMap<String, LanguageSettings> {
    [
        ("rust", "rust-analyzer"),
        ("typescript", "typescript-language-server"),
        ("javascript", "typescript-language-server"),
        ("tsx", "typescript-language-server"),
        ("jsx", "typescript-language-server"),
        ("python", "pyright-langserver"),
        ("go", "gopls"),
        ("c", "clangd"),
        ("cpp", "clangd"),
        ("toml", "taplo"),
        ("json", "vscode-json-language-server"),
    ]
    .into_iter()
    .map(|(language, server)| {
        (
            language.to_owned(),
            LanguageSettings::with_servers(&[server]),
        )
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_language_references_a_builtin_server() {
        let servers = builtin_servers();
        for (language, settings) in builtin_languages() {
            for server in &settings.servers {
                assert!(servers.contains_key(server), "{language} -> {server}");
            }
        }
    }

    #[test]
    fn builtin_server_details() {
        let servers = builtin_servers();
        assert_eq!(servers.len(), 7);
        assert_eq!(servers["taplo"].args, ["lsp", "stdio"]);
        assert_eq!(servers["rust-analyzer"].root_markers, ["Cargo.toml"]);
        assert_eq!(
            servers["pyright-langserver"].root_markers,
            ["pyproject.toml", "setup.py", "requirements.txt"]
        );
    }
}
