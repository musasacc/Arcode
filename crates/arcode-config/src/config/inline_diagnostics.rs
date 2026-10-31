//! `editor.inline_diagnostics`: which diagnostics get a lens after the line.

use serde::{Deserialize, Serialize};

/// Which diagnostics the editor shows as an inline lens after the end of
/// their line (in addition to the underline and the gutter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InlineDiagnostics {
    /// Only errors.
    #[default]
    Errors,
    /// Errors, warnings, info and hints.
    All,
    /// No lens.
    Off,
}

impl InlineDiagnostics {
    /// Every choice, in the order a picker lists them.
    pub const ALL: [Self; 3] = [Self::Errors, Self::All, Self::Off];

    /// The TOML value (`"errors"`, `"all"`, `"off"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Errors => "errors",
            Self::All => "all",
            Self::Off => "off",
        }
    }

    /// Whether a diagnostic gets a lens; `is_error` is its severity.
    pub fn shows(self, is_error: bool) -> bool {
        match self {
            Self::Errors => is_error,
            Self::All => true,
            Self::Off => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn parses_and_filters() {
        assert_eq!(
            Config::default().editor.inline_diagnostics,
            InlineDiagnostics::Errors
        );
        let (config, warnings) = Config::from_toml_str("[editor]\ninline_diagnostics = \"all\"");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.editor.inline_diagnostics, InlineDiagnostics::All);
        let (config, warnings) = Config::from_toml_str("[editor]\ninline_diagnostics = \"some\"");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(config.editor.inline_diagnostics, InlineDiagnostics::Errors);

        assert!(InlineDiagnostics::Errors.shows(true));
        assert!(!InlineDiagnostics::Errors.shows(false));
        assert!(InlineDiagnostics::All.shows(false));
        assert!(!InlineDiagnostics::Off.shows(true));
        let names: Vec<_> = InlineDiagnostics::ALL.iter().map(|d| d.as_str()).collect();
        assert_eq!(names, ["errors", "all", "off"]);
    }
}
