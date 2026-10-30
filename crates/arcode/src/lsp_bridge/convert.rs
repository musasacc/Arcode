//! Pure conversions between configuration, LSP data and editor types.

use std::collections::{BTreeMap, HashMap};

use arcode_buffer::{Buffer, Edit};
use arcode_config::Config;
use arcode_editor::{DiagnosticSeverity, EditorDiagnostic};
use arcode_lsp::lsp_types::{self, Diagnostic, Position, Range};
use arcode_lsp::{PositionEncoding, ServerConfig, ServerStatus, TextChange, source_label};

/// Server definitions and the language -> servers table for
/// [`arcode_lsp::LspManager::new`], built from the configuration.
/// Language ids are lowercased.
pub fn server_configs(config: &Config) -> (Vec<ServerConfig>, HashMap<String, Vec<String>>) {
    let mut servers: BTreeMap<String, ServerConfig> = BTreeMap::new();
    let mut languages = HashMap::new();
    for language_id in config.languages.keys() {
        let named = config.servers_for_language(language_id);
        if named.is_empty() {
            continue;
        }
        let names = named.iter().map(|n| n.name.clone()).collect();
        languages.insert(language_id.to_lowercase(), names);
        for server in named {
            let s = server.settings;
            servers
                .entry(server.name.clone())
                .or_insert_with(|| ServerConfig {
                    name: server.name,
                    command: s.command,
                    args: s.args,
                    env: s.env.into_iter().collect(),
                    root_markers: s.root_markers,
                    initialization_options: s
                        .initialization_options
                        .as_ref()
                        .and_then(|v| serde_json::to_value(v).ok()),
                });
        }
    }
    (servers.into_values().collect(), languages)
}

/// The buffer's name for an LSP position encoding.
pub fn buffer_encoding(encoding: PositionEncoding) -> arcode_buffer::PositionEncoding {
    match encoding {
        PositionEncoding::Utf8 => arcode_buffer::PositionEncoding::Utf8,
        PositionEncoding::Utf16 => arcode_buffer::PositionEncoding::Utf16,
        PositionEncoding::Utf32 => arcode_buffer::PositionEncoding::Utf32,
    }
}

/// Incremental `didChange` entries for buffer edits (applied in order).
pub fn text_changes(edits: &[Edit], encoding: PositionEncoding) -> Vec<TextChange> {
    let encoding = buffer_encoding(encoding);
    edits
        .iter()
        .map(|edit| {
            let r = edit.lsp_range(encoding);
            TextChange {
                range: Some(Range::new(
                    Position::new(r.start.0, r.start.1),
                    Position::new(r.end.0, r.end.1),
                )),
                text: edit.new_text.clone(),
            }
        })
        .collect()
}

/// The editor severity of an LSP diagnostic (missing means error).
pub fn severity(diagnostic: &Diagnostic) -> DiagnosticSeverity {
    match diagnostic.severity {
        Some(lsp_types::DiagnosticSeverity::WARNING) => DiagnosticSeverity::Warning,
        Some(lsp_types::DiagnosticSeverity::INFORMATION) => DiagnosticSeverity::Info,
        Some(lsp_types::DiagnosticSeverity::HINT) => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Error,
    }
}

/// Converts an LSP position to a buffer [`arcode_buffer::Point`] (clamped).
pub fn to_point(
    buffer: &Buffer,
    position: Position,
    encoding: PositionEncoding,
) -> arcode_buffer::Point {
    let ix = buffer.from_lsp_position(position.line, position.character, buffer_encoding(encoding));
    buffer.char_to_point(ix)
}

/// Converts one server diagnostic into the editor's form.
pub fn editor_diagnostic(
    buffer: &Buffer,
    server: &str,
    diagnostic: &Diagnostic,
    encoding: PositionEncoding,
) -> EditorDiagnostic {
    let start = to_point(buffer, diagnostic.range.start, encoding);
    let end = to_point(buffer, diagnostic.range.end, encoding).max(start);
    EditorDiagnostic {
        range: start..end,
        severity: severity(diagnostic),
        message: diagnostic.message.clone(),
        source: source_label(server, diagnostic).to_string(),
    }
}

/// Work-done progress reported by a server.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// Title from the `begin` report, e.g. `Indexing`.
    pub title: Option<String>,
    /// Latest message.
    pub message: Option<String>,
    /// Latest percentage.
    pub percentage: Option<u32>,
}

/// What the UI knows about one server.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerState {
    /// Lifecycle state.
    pub status: ServerStatus,
    /// Running progress reports by token, oldest first.
    pub progress: Vec<(String, Progress)>,
}

impl Default for ServerState {
    fn default() -> Self {
        Self {
            status: ServerStatus::Starting,
            progress: Vec::new(),
        }
    }
}

/// Status bar text, e.g. `rust-analyzer: indexing 40%`.
pub fn status_text(server: &str, state: &ServerState) -> String {
    match &state.status {
        ServerStatus::Starting => format!("{server}: starting…"),
        ServerStatus::Stopped => format!("{server}: stopped"),
        ServerStatus::Failed(reason) => format!("{server}: failed: {}", first_line(reason)),
        ServerStatus::Running | ServerStatus::Busy(_) => {
            if let Some((_, p)) = state.progress.last() {
                let title = p
                    .title
                    .as_deref()
                    .or(p.message.as_deref())
                    .unwrap_or("working");
                let title = lowercase_first(title);
                match p.percentage {
                    Some(pct) => format!("{server}: {title} {pct}%"),
                    None => format!("{server}: {title}…"),
                }
            } else if let ServerStatus::Busy(what) = &state.status {
                format!("{server}: {}", lowercase_first(what))
            } else {
                format!("{server}: ready")
            }
        }
    }
}

/// The first line of `text`, trimmed.
pub fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// `Indexing` -> `indexing`, but leaves acronyms like `LSP` alone.
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && !b.is_uppercase() => a
            .to_lowercase()
            .chain(text[a.len_utf8()..].chars())
            .collect(),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configs_from_settings() {
        let (config, warnings) = Config::from_toml_str(
            "[lsp.rust-analyzer]\ninitialization_options = { check = { command = \"clippy\" } }\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let (servers, languages) = server_configs(&config);
        assert_eq!(languages["rust"], vec!["rust-analyzer".to_string()]);
        let ra = servers
            .iter()
            .find(|s| s.name == "rust-analyzer")
            .expect("rust-analyzer configured");
        assert_eq!(ra.command, "rust-analyzer");
        assert_eq!(ra.root_markers, vec!["Cargo.toml".to_string()]);
        assert_eq!(
            ra.initialization_options,
            Some(serde_json::json!({"check": {"command": "clippy"}}))
        );
        let unique: std::collections::HashSet<_> = servers.iter().map(|s| &s.name).collect();
        assert_eq!(unique.len(), servers.len());
    }

    #[test]
    fn diagnostic_positions_use_the_encoding() {
        // "é" is 2 UTF-8 bytes, 1 UTF-16 unit; "😀" is 4 bytes, 2 units.
        let buffer = Buffer::new("let é😀 = x;\nnext\n");
        let d = Diagnostic {
            range: Range::new(Position::new(0, 7), Position::new(0, 8)),
            severity: Some(lsp_types::DiagnosticSeverity::WARNING),
            message: "msg".into(),
            ..Default::default()
        };
        let e = editor_diagnostic(&buffer, "ra", &d, PositionEncoding::Utf16);
        // UTF-16 col 7 = after "let é😀" (4 + 1 + 2), i.e. char 6.
        assert_eq!(e.range.start, arcode_buffer::Point::new(0, 6));
        assert_eq!(e.severity, DiagnosticSeverity::Warning);
        assert_eq!(e.source, "ra");
        let e = editor_diagnostic(&buffer, "ra", &d, PositionEncoding::Utf8);
        // UTF-8 byte 7 = after "let é" (4 + 2) plus 1 byte into the emoji:
        // rounds down to char 5.
        assert_eq!(e.range.start, arcode_buffer::Point::new(0, 5));
        // Past-the-end positions clamp.
        let far = Diagnostic {
            range: Range::new(Position::new(9, 0), Position::new(9, 3)),
            ..d
        };
        let e = editor_diagnostic(&buffer, "ra", &far, PositionEncoding::Utf16);
        assert_eq!(e.range.start, e.range.end);
        assert_eq!(e.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn edits_become_text_changes() {
        let mut buffer = Buffer::new("a😀b\n");
        let edits = buffer.edit(vec![(3..3, "X".into()), (0..1, String::new())]);
        let changes = text_changes(&edits, PositionEncoding::Utf16);
        assert_eq!(changes.len(), 2);
        let mut text: Vec<u16> = "a😀b\n".encode_utf16().collect();
        for c in &changes {
            let r = c.range.expect("range");
            assert_eq!(r.start.line, 0);
            let (s, e) = (r.start.character as usize, r.end.character as usize);
            text.splice(s..e, c.text.encode_utf16());
        }
        assert_eq!(String::from_utf16_lossy(&text), buffer.text().to_string());
    }

    #[test]
    fn status_texts() {
        let mut state = ServerState::default();
        assert_eq!(status_text("ra", &state), "ra: starting…");
        state.status = ServerStatus::Running;
        assert_eq!(status_text("ra", &state), "ra: ready");
        state.progress.push((
            "t".into(),
            Progress {
                title: Some("Indexing".into()),
                message: None,
                percentage: Some(40),
            },
        ));
        assert_eq!(status_text("ra", &state), "ra: indexing 40%");
        state.status = ServerStatus::Failed("no such file\nmore".into());
        assert_eq!(status_text("ra", &state), "ra: failed: no such file");
        assert_eq!(lowercase_first("LSP"), "LSP");
    }
}
