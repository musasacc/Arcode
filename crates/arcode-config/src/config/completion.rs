//! The `[editor.completion]` section of `config.toml`.

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::lenient::{apply_table, impl_section};

/// `[editor.completion]`: the completion popup, snippets and signature help.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompletionSettings {
    /// Open the completion popup while typing (word characters and the
    /// language server's trigger characters). When off, only the manual
    /// shortcut (Ctrl+Space) opens it.
    pub auto_trigger: bool,
    /// Milliseconds to wait after a keystroke before asking for
    /// completions, so fast typing sends fewer requests.
    pub delay_ms: u64,
    /// Offer built-in and user snippets in the popup.
    pub snippets: bool,
    /// Show parameter hints after `(` and `,` in a call.
    pub signature_help: bool,
}

impl Default for CompletionSettings {
    fn default() -> Self {
        Self {
            auto_trigger: true,
            delay_ms: 50,
            snippets: true,
            signature_help: true,
        }
    }
}

impl_section!(CompletionSettings {
    auto_trigger,
    delay_ms,
    snippets,
    signature_help,
});

impl CompletionSettings {
    /// Longest accepted trigger delay.
    pub const MAX_DELAY_MS: u64 = 2000;

    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        if self.delay_ms > Self::MAX_DELAY_MS {
            warnings.push(format!(
                "`editor.completion.delay_ms` capped at {}",
                Self::MAX_DELAY_MS
            ));
            self.delay_ms = Self::MAX_DELAY_MS;
        }
    }
}

/// Splits the `completion` sub-table off an `[editor]` table: applies it to
/// `completion` key by key and returns the rest of the table for the
/// `[editor]` keys. A non-table `[editor]` value is returned unchanged.
pub(crate) fn split_completion(
    editor: &Value,
    completion: &mut CompletionSettings,
    warnings: &mut Vec<String>,
) -> Value {
    let Some(table) = editor.as_table() else {
        return editor.clone();
    };
    let mut rest = table.clone();
    if let Some(sub) = rest.remove("completion") {
        apply_table(completion, &sub, "editor.completion", warnings);
    }
    Value::Table(rest)
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn completion_section_parses_and_validates() {
        let (config, warnings) = Config::from_toml_str(
            "[editor]\nfont_size = 15\n[editor.completion]\nauto_trigger = false\ndelay_ms = 99999\nsnippets = false\n",
        );
        let c = &config.editor.completion;
        assert!(!c.auto_trigger && !c.snippets && c.signature_help);
        assert_eq!(c.delay_ms, 2000);
        assert_eq!(config.editor.font_size, 15.0);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[test]
    fn bad_completion_values_keep_defaults() {
        let (config, warnings) =
            Config::from_toml_str("[editor]\ncompletion = { delay_ms = \"x\", bogus = 1 }\n");
        assert_eq!(config.editor.completion.delay_ms, 50);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("editor.completion.bogus"))
        );
    }
}
