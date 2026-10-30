//! The user configuration file (`config.toml`).
//!
//! Loading is forgiving: syntax errors, values of the wrong type and unknown
//! keys never fail the load. They produce human-readable warnings and the
//! affected settings keep their defaults. Built-in `[language.*]` and
//! `[lsp.*]` entries are merged under the user's entries field by field.

mod language;
mod sections;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::fsutil::write_atomic;
use crate::lenient::{Section, apply_table, one_line};
use crate::paths;

pub use language::{
    LanguageSettings, LspServerSettings, NamedServer, builtin_languages, builtin_servers,
};
pub use sections::{
    DEFAULT_EDITOR_FONT, EditorSettings, Keymap, TerminalSettings, ThemeChoice, UiSettings,
};

/// The commented template written for new users; see [`Config::default_toml`].
const DEFAULT_TOML: &str = include_str!("../../assets/default-config.toml");

/// The complete user configuration.
///
/// Direct serde deserialization replaces whole maps; use
/// [`Config::from_toml_str`] / [`Config::load_from`] to get lenient parsing
/// and merging with the built-in language and server defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `[editor]`
    pub editor: EditorSettings,
    /// `[ui]`
    pub ui: UiSettings,
    /// `[terminal]`
    pub terminal: TerminalSettings,
    /// `[language.<id>]`, keyed by language id (e.g. `rust`).
    #[serde(rename = "language")]
    pub languages: BTreeMap<String, LanguageSettings>,
    /// `[lsp.<name>]`, keyed by server name.
    pub lsp: BTreeMap<String, LspServerSettings>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: EditorSettings::default(),
            ui: UiSettings::default(),
            terminal: TerminalSettings::default(),
            languages: builtin_languages(),
            lsp: builtin_servers(),
        }
    }
}

impl Config {
    /// Parses TOML source on top of the defaults. Never fails; problems are
    /// returned as warnings.
    pub fn from_toml_str(source: &str) -> (Self, Vec<String>) {
        let mut config = Self::default();
        let mut warnings = Vec::new();
        match toml::from_str::<Table>(source) {
            Ok(table) => config.apply(&table, &mut warnings),
            Err(err) => warnings.push(format!(
                "config is not valid TOML ({}); using defaults",
                one_line(&err.to_string())
            )),
        }
        (config, warnings)
    }

    /// Loads `path`. A missing file yields the defaults without warnings;
    /// unreadable or invalid files yield defaults plus warnings.
    pub fn load_from(path: &Path) -> (Self, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(source) => {
                let (config, warnings) = Self::from_toml_str(&source);
                let warnings = warnings
                    .into_iter()
                    .map(|w| format!("{}: {w}", path.display()))
                    .collect();
                (config, warnings)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => (Self::default(), Vec::new()),
            Err(err) => (
                Self::default(),
                vec![format!(
                    "cannot read {}: {err}; using defaults",
                    path.display()
                )],
            ),
        }
    }

    /// Loads [`paths::config_file`].
    pub fn load() -> (Self, Vec<String>) {
        match paths::config_file() {
            Some(path) => Self::load_from(&path),
            None => (
                Self::default(),
                vec!["no configuration directory found; using defaults".to_owned()],
            ),
        }
    }

    /// A commented, valid TOML template documenting every option. Parsing it
    /// yields [`Config::default`].
    pub fn default_toml() -> String {
        DEFAULT_TOML.to_owned()
    }

    /// Writes [`Config::default_toml`] to `path` unless the file exists.
    /// Returns whether a file was written.
    pub fn write_default_if_missing(path: &Path) -> io::Result<bool> {
        if path.try_exists()? {
            return Ok(false);
        }
        write_atomic(path, DEFAULT_TOML.as_bytes())?;
        Ok(true)
    }

    /// Serializes the full configuration (without comments).
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }

    /// Settings for a language id (exact match, then case-insensitive).
    pub fn language(&self, language_id: &str) -> Option<&LanguageSettings> {
        self.languages.get(language_id).or_else(|| {
            self.languages
                .iter()
                .find(|(id, _)| id.eq_ignore_ascii_case(language_id))
                .map(|(_, settings)| settings)
        })
    }

    /// Tab size for a language: `[language.<id>].tab_size` or `editor.tab_size`.
    pub fn effective_tab_size(&self, language_id: &str) -> usize {
        self.language(language_id)
            .and_then(|l| l.tab_size)
            .unwrap_or(self.editor.tab_size)
    }

    /// Whether to insert spaces for a language, falling back to the editor.
    pub fn effective_insert_spaces(&self, language_id: &str) -> bool {
        self.language(language_id)
            .and_then(|l| l.insert_spaces)
            .unwrap_or(self.editor.insert_spaces)
    }

    /// The language servers to start for a language, in configured order.
    /// A server name without an `[lsp.<name>]` table runs `<name>` itself.
    pub fn servers_for_language(&self, language_id: &str) -> Vec<NamedServer> {
        let Some(language) = self.language(language_id) else {
            return Vec::new();
        };
        language
            .servers
            .iter()
            .map(|name| NamedServer {
                name: name.clone(),
                settings: self
                    .lsp
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| LspServerSettings::new(name.clone())),
            })
            .collect()
    }

    /// Terminal font family (falls back to the editor font).
    pub fn terminal_font_family(&self) -> &str {
        self.terminal
            .font_family
            .as_deref()
            .unwrap_or(&self.editor.font_family)
    }

    /// Terminal font size (falls back to the editor font size).
    pub fn terminal_font_size(&self) -> f32 {
        self.terminal.font_size.unwrap_or(self.editor.font_size)
    }

    fn apply(&mut self, table: &Table, warnings: &mut Vec<String>) {
        for (key, value) in table {
            match key.as_str() {
                "editor" => apply_table(&mut self.editor, value, "editor", warnings),
                "ui" => apply_table(&mut self.ui, value, "ui", warnings),
                "terminal" => apply_table(&mut self.terminal, value, "terminal", warnings),
                "language" => self.apply_languages(value, warnings),
                "lsp" => self.apply_servers(value, warnings),
                other => warnings.push(format!("unknown section `{other}` (ignored)")),
            }
        }
        self.editor.validate(warnings);
        self.ui.validate(warnings);
        self.terminal.validate(warnings);
    }

    fn apply_languages(&mut self, value: &Value, warnings: &mut Vec<String>) {
        for_each_entry(value, "language", warnings, |id, entry, warnings| {
            let path = format!("language.{id}");
            let settings = self.languages.entry(id.to_owned()).or_default();
            apply_table(settings, entry, &path, warnings);
            settings.validate(&path, warnings);
        });
    }

    fn apply_servers(&mut self, value: &Value, warnings: &mut Vec<String>) {
        for_each_entry(value, "lsp", warnings, |name, entry, warnings| {
            let path = format!("lsp.{name}");
            let settings = self
                .lsp
                .entry(name.to_owned())
                .or_insert_with(|| LspServerSettings::new(name));
            apply_table(settings, entry, &path, warnings);
            settings.validate(name, &path, warnings);
        });
    }
}

fn for_each_entry(
    value: &Value,
    section: &str,
    warnings: &mut Vec<String>,
    mut f: impl FnMut(&str, &Value, &mut Vec<String>),
) {
    let Some(table) = value.as_table() else {
        warnings.push(format!(
            "`{section}` must be a table of tables, found {}; ignored",
            value.type_str()
        ));
        return;
    };
    for (name, entry) in table {
        if entry.is_table() {
            f(name, entry, warnings);
        } else {
            warnings.push(format!(
                "`{section}.{name}` must be a table, found {}; ignored",
                entry.type_str()
            ));
        }
    }
}

/// Every key understood in each fixed section, for documentation checks.
pub fn known_keys() -> [(&'static str, &'static [&'static str]); 5] {
    [
        ("editor", EditorSettings::KEYS),
        ("ui", UiSettings::KEYS),
        ("terminal", TerminalSettings::KEYS),
        ("language.<id>", LanguageSettings::KEYS),
        ("lsp.<name>", LspServerSettings::KEYS),
    ]
}

#[cfg(test)]
mod tests;
