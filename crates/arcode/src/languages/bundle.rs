//! A language pack as the Languages page shows it: the tool pack of
//! `arcode-project` joined with the grammars, comment tokens and file
//! extensions of `arcode-syntax`, the language servers and file
//! associations of the configuration and the built-in snippets.

use std::collections::BTreeSet;

use arcode_config::Config;
use arcode_project::{InstallAction, LanguagePack, Os};

/// One language of a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRow {
    /// LSP language id.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// `.rs`, `Cargo.lock`, ... (extensions with a dot, then file names).
    pub files: Vec<String>,
    /// `//`, `/* */` ...
    pub comments: Vec<String>,
    /// Whether a tree-sitter grammar is built in.
    pub grammar: bool,
}

/// A language server of a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRow {
    /// `[lsp.<name>]` key.
    pub name: String,
    /// The executable it runs.
    pub command: String,
    /// Command to copy that installs it (downloads; never run by Arcode).
    pub install_command: Option<String>,
    /// Instructions when there is no single command.
    pub install_note: Option<String>,
}

/// One row of the Languages page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRow {
    /// Pack id (`rust`, `web`, ...).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Every LSP language id of the pack (written by the toggle).
    pub language_ids: Vec<String>,
    /// The pack's languages Arcode can open.
    pub languages: Vec<LanguageRow>,
    /// Configured servers of those languages, in order, without repeats.
    pub servers: Vec<ServerRow>,
    /// Built-in snippets for the pack's languages.
    pub snippets: usize,
    /// Typical run commands.
    pub run_commands: Vec<String>,
    /// `[files] associations` patterns pointing at the pack's languages.
    pub associations: Vec<String>,
    /// Whether every language of the pack is enabled.
    pub enabled: bool,
}

/// Rows for every built-in pack under `config`.
pub fn pack_rows(config: &Config) -> Vec<PackRow> {
    let snippets = arcode_completion::SnippetLibrary::builtin();
    arcode_project::packs()
        .into_iter()
        .map(|pack| pack_row(&pack, config, snippets.all()))
        .collect()
}

fn pack_row(
    pack: &LanguagePack,
    config: &Config,
    snippets: &[arcode_completion::SnippetDef],
) -> PackRow {
    let languages: Vec<LanguageRow> = pack
        .languages
        .iter()
        .filter_map(|id| arcode_syntax::language_by_id(id))
        .map(language_row)
        .collect();
    let mut seen = BTreeSet::new();
    let servers = languages
        .iter()
        .filter_map(|l| config.language(l.id))
        .flat_map(|settings| settings.configured_servers().iter())
        .filter(|name| seen.insert((*name).clone()))
        .map(|name| server_row(pack, config, name))
        .collect();
    let snippets = snippets
        .iter()
        .filter(|s| !s.languages.is_empty())
        .filter(|s| languages.iter().any(|l| s.matches_language(l.id)))
        .count();
    let associations = config
        .files
        .associations
        .iter()
        .filter(|(_, value)| {
            super::resolve::language_named(value)
                .is_some_and(|l| languages.iter().any(|row| row.id == l.id))
        })
        .map(|(pattern, _)| pattern.clone())
        .collect();
    let enabled = languages.iter().all(|l| config.language_enabled(l.id));
    PackRow {
        id: pack.id.clone(),
        name: pack.name.clone(),
        language_ids: pack.languages.clone(),
        languages,
        servers,
        snippets,
        run_commands: pack.run_commands.clone(),
        associations,
        enabled,
    }
}

fn language_row(language: &'static arcode_syntax::Language) -> LanguageRow {
    let mut files: Vec<String> = language
        .extensions
        .iter()
        .map(|e| format!(".{e}"))
        .collect();
    files.extend(language.file_names.iter().map(|n| (*n).to_string()));
    let mut comments: Vec<String> = language
        .line_comment
        .iter()
        .map(|t| (*t).to_string())
        .collect();
    if let Some((open, close)) = language.block_comment {
        comments.push(format!("{open} {close}"));
    }
    LanguageRow {
        id: language.id,
        name: language.name,
        files,
        comments,
        grammar: language.has_grammar(),
    }
}

fn server_row(pack: &LanguagePack, config: &Config, name: &str) -> ServerRow {
    let command = config
        .lsp_server(name)
        .map(|s| s.command.clone())
        .unwrap_or_else(|| name.to_string());
    let component = pack
        .component_providing(&command)
        .or_else(|| pack.component_providing(name));
    let os = Os::current();
    let mut install_command = None;
    let mut notes = Vec::new();
    if let Some(component) = component {
        for step in component.install_steps(os) {
            match &step.action {
                InstallAction::Run { .. } if install_command.is_none() => {
                    install_command = step.command_line();
                }
                InstallAction::Run { .. } => {}
                InstallAction::Manual { instructions } => notes.push(instructions.clone()),
            }
        }
    }
    ServerRow {
        name: name.to_string(),
        command,
        install_command,
        install_note: (!notes.is_empty()).then(|| notes.join(" ")),
    }
}

/// Whether each server command is on `PATH` (blocks: a `PATH` walk per
/// command; call it off the UI thread).
pub fn installed_commands(rows: &[PackRow]) -> Vec<(String, bool)> {
    let commands: BTreeSet<&str> = rows
        .iter()
        .flat_map(|r| r.servers.iter().map(|s| s.command.as_str()))
        .collect();
    commands
        .into_iter()
        .map(|c| {
            let found = if std::path::Path::new(c).is_absolute() {
                std::path::Path::new(c).is_file()
            } else {
                arcode_project::packs::find_executable(c).is_some()
            };
            (c.to_string(), found)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_with_a_grammar_has_a_pack() {
        let rows = pack_rows(&Config::default());
        for language in arcode_syntax::all_languages() {
            if language.has_grammar() {
                assert!(
                    rows.iter()
                        .any(|r| r.languages.iter().any(|l| l.id == language.id)),
                    "no pack for {}",
                    language.id
                );
            }
        }
    }

    #[test]
    fn rust_pack_details() {
        let rows = pack_rows(&Config::default());
        let rust = rows.iter().find(|r| r.id == "rust").unwrap();
        assert!(rust.enabled);
        assert_eq!(rust.languages[0].files[0], ".rs");
        assert!(rust.languages[0].comments.contains(&"//".to_string()));
        assert!(rust.languages[0].grammar);
        assert_eq!(rust.servers[0].name, "rust-analyzer");
        assert_eq!(
            rust.servers[0].install_command.as_deref(),
            Some("rustup component add rust-analyzer")
        );
        assert!(rust.snippets > 0);
        assert!(rust.run_commands.iter().any(|c| c == "cargo run"));
    }

    #[test]
    fn disabled_packs_and_associations_show() {
        let (config, warnings) = Config::from_toml_str(
            "[language.python]\nenabled = false\n[files]\nassociations = { \"*.pyw2\" = \"Python\" }\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let rows = pack_rows(&config);
        let python = rows.iter().find(|r| r.id == "python").unwrap();
        assert!(!python.enabled);
        assert_eq!(python.associations, ["*.pyw2"]);
        assert_eq!(python.servers[0].name, "pyright-langserver");
        assert_eq!(
            python.servers[0].install_command.as_deref(),
            Some("npm install --global pyright")
        );
        let web = rows.iter().find(|r| r.id == "web").unwrap();
        let names: Vec<_> = web.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "vscode-html-language-server",
                "vscode-css-language-server",
                "vscode-json-language-server"
            ]
        );
    }
}
