//! `[terminal]` settings: the integrated terminal.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[terminal]`: the integrated terminal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    /// Shell program; `None` uses `$SHELL` (Unix) or PowerShell/cmd (Windows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Arguments passed to the shell.
    pub args: Vec<String>,
    /// Font family; `None` uses the editor font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Font size in points; `None` uses the editor font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// Lines of scrollback history.
    pub scrollback: usize,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            shell: None,
            args: Vec::new(),
            font_family: None,
            font_size: None,
            scrollback: 10_000,
        }
    }
}

impl_section!(TerminalSettings {
    shell,
    args,
    font_family,
    font_size,
    scrollback,
});

impl TerminalSettings {
    /// Maximum accepted scrollback.
    pub const MAX_SCROLLBACK: usize = 1_000_000;

    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        if let Some(size) = self.font_size
            && !(size.is_finite() && (4.0..=200.0).contains(&size))
        {
            warnings.push(format!(
                "`terminal.font_size` must be between 4 and 200, got {size}; using editor size"
            ));
            self.font_size = None;
        }
        if self.scrollback > Self::MAX_SCROLLBACK {
            warnings.push(format!(
                "`terminal.scrollback` capped at {}",
                Self::MAX_SCROLLBACK
            ));
            self.scrollback = Self::MAX_SCROLLBACK;
        }
        if self.shell.as_deref().is_some_and(|s| s.trim().is_empty()) {
            self.shell = None;
        }
        if self
            .font_family
            .as_deref()
            .is_some_and(|f| f.trim().is_empty())
        {
            self.font_family = None;
        }
    }
}
