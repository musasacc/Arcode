//! `[language.<id>]` and `[lsp.<name>]` sections plus their built-in defaults.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// Per-language overrides (`[language.<id>]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageSettings {
    /// Whether the language's pack is on. A disabled language starts no
    /// language servers and shows no run buttons; it is still highlighted.
    #[serde(skip_serializing_if = "is_true")]
    pub enabled: bool,
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
    /// The configured `servers` of a disabled language (they are moved
    /// here, so nothing starts them). Not read from or written to TOML.
    #[serde(skip)]
    pub disabled_servers: Vec<String>,
}

impl Default for LanguageSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: Vec::new(),
            formatter: None,
            tab_size: None,
            insert_spaces: None,
            disabled_servers: Vec::new(),
        }
    }
}

fn is_true(value: &bool) -> bool {
    *value
}

impl_section!(LanguageSettings {
    enabled,
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

    /// The servers the language would start when enabled.
    pub fn configured_servers(&self) -> &[String] {
        if self.enabled {
            &self.servers
        } else {
            &self.disabled_servers
        }
    }

    /// Moves the servers of a disabled language aside (see
    /// [`LanguageSettings::disabled_servers`]).
    pub(crate) fn apply_enabled(&mut self) {
        if !self.enabled && !self.servers.is_empty() {
            self.disabled_servers = std::mem::take(&mut self.servers);
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
    /// Which features to use from this server (all on by default), e.g.
    /// `features = { hover = false, completion = false }` for a linter.
    #[serde(skip_serializing_if = "LspFeatures::is_all_on")]
    pub features: LspFeatures,
}

impl_section!(LspServerSettings {
    command,
    args,
    env,
    root_markers,
    initialization_options,
    features,
});

/// Per-server feature switches (`[lsp.<name>].features`). A feature that
/// is off is never asked from this server; `diagnostics = false` hides
/// what it reports. Unknown keys make the whole table invalid (a warning).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LspFeatures {
    /// Diagnostics (squiggles, Problems panel, status bar counts).
    pub diagnostics: bool,
    /// Completion items.
    pub completion: bool,
    /// Hover documentation.
    pub hover: bool,
    /// Signature help (parameter hints).
    pub signature_help: bool,
    /// Go to definition / type definition / implementation.
    pub definition: bool,
    /// Find all references.
    pub references: bool,
    /// Document and workspace symbols.
    pub symbols: bool,
    /// Occurrence highlights.
    pub highlights: bool,
    /// Rename symbol.
    pub rename: bool,
    /// Formatting.
    pub formatting: bool,
    /// Code actions and quick fixes.
    pub code_actions: bool,
    /// Inlay hints.
    pub inlay_hints: bool,
}

impl Default for LspFeatures {
    fn default() -> Self {
        Self {
            diagnostics: true,
            completion: true,
            hover: true,
            signature_help: true,
            definition: true,
            references: true,
            symbols: true,
            highlights: true,
            rename: true,
            formatting: true,
            code_actions: true,
            inlay_hints: true,
        }
    }
}

impl LspFeatures {
    /// True when every feature is on (the default).
    pub fn is_all_on(&self) -> bool {
        *self == Self::default()
    }
}

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
        LspServerSettings::with("vscode-html-language-server", &["--stdio"], &[]),
        LspServerSettings::with("vscode-css-language-server", &["--stdio"], &[]),
        LspServerSettings::with("bash-language-server", &["start"], &[]),
        LspServerSettings::with("yaml-language-server", &["--stdio"], &[]),
        LspServerSettings::with("marksman", &["server"], &[".marksman.toml"]),
        LspServerSettings::with(
            "jdtls",
            &[],
            &[
                "pom.xml",
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
            ],
        ),
        LspServerSettings::with("lua-language-server", &[], &[".luarc.json"]),
        LspServerSettings::with("ruby-lsp", &[], &["Gemfile"]),
        LspServerSettings::with("intelephense", &["--stdio"], &["composer.json"]),
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
        ("typescriptreact", "typescript-language-server"),
        ("javascriptreact", "typescript-language-server"),
        // Older names of the two above, kept for configs that use them.
        ("tsx", "typescript-language-server"),
        ("jsx", "typescript-language-server"),
        ("python", "pyright-langserver"),
        ("go", "gopls"),
        ("c", "clangd"),
        ("cpp", "clangd"),
        ("toml", "taplo"),
        ("json", "vscode-json-language-server"),
        ("html", "vscode-html-language-server"),
        ("css", "vscode-css-language-server"),
        ("shellscript", "bash-language-server"),
        ("yaml", "yaml-language-server"),
        ("markdown", "marksman"),
        ("java", "jdtls"),
        ("lua", "lua-language-server"),
        ("ruby", "ruby-lsp"),
        ("php", "intelephense"),
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
    fn server_features_parse_leniently() {
        let (config, warnings) = crate::Config::from_toml_str(
            "[lsp.typos]\ncommand = \"typos-lsp\"\nfeatures = { hover = false, completion = false }\n\
             [lsp.bad]\nfeatures = { hovr = false }\n",
        );
        let typos = &config.lsp["typos"].features;
        assert!(!typos.hover && !typos.completion);
        assert!(typos.diagnostics && typos.code_actions);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("lsp.bad.features"), "{warnings:?}");
        assert!(config.lsp["bad"].features.is_all_on());
    }

    #[test]
    fn disabled_languages_keep_their_servers_aside() {
        let (config, warnings) = crate::Config::from_toml_str(
            "[language.rust]\nenabled = false\n[language.go]\nenabled = true\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let rust = config.language("rust").unwrap();
        assert!(!config.language_enabled("rust"));
        assert!(rust.servers.is_empty());
        assert_eq!(rust.configured_servers(), ["rust-analyzer"]);
        assert!(config.servers_for_language("rust").is_empty());
        assert!(config.language_enabled("go"));
        assert_eq!(
            config.language("go").unwrap().configured_servers(),
            ["gopls"]
        );
        assert!(config.language_enabled("no-such-language"));
    }

    #[test]
    fn builtin_server_details() {
        let servers = builtin_servers();
        assert_eq!(servers.len(), 16);
        assert_eq!(servers["taplo"].args, ["lsp", "stdio"]);
        assert_eq!(servers["rust-analyzer"].root_markers, ["Cargo.toml"]);
        assert_eq!(
            servers["pyright-langserver"].root_markers,
            ["pyproject.toml", "setup.py", "requirements.txt"]
        );
    }
}
