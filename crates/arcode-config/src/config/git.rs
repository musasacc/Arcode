//! The `[git]` section of `config.toml`.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[git]`: source control integration. Everything is local; Arcode never
/// fetches, pulls or pushes on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitSettings {
    /// Show added / modified / deleted bars in the editor gutter.
    pub gutter: bool,
    /// Show who last changed the cursor line at its end.
    pub inline_blame: bool,
    /// Milliseconds the cursor must rest on a line before its blame shows.
    pub inline_blame_delay_ms: u64,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            gutter: true,
            inline_blame: true,
            inline_blame_delay_ms: 500,
        }
    }
}

impl_section!(GitSettings {
    gutter,
    inline_blame,
    inline_blame_delay_ms,
});

impl GitSettings {
    /// Longest accepted blame delay.
    pub const MAX_BLAME_DELAY_MS: u64 = 10_000;

    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        if self.inline_blame_delay_ms > Self::MAX_BLAME_DELAY_MS {
            warnings.push(format!(
                "`git.inline_blame_delay_ms` capped at {}",
                Self::MAX_BLAME_DELAY_MS
            ));
            self.inline_blame_delay_ms = Self::MAX_BLAME_DELAY_MS;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn git_section_parses_and_validates() {
        let (config, warnings) =
            Config::from_toml_str("[git]\ninline_blame = false\ninline_blame_delay_ms = 99999\n");
        assert!(!config.git.inline_blame);
        assert!(config.git.gutter);
        assert_eq!(config.git.inline_blame_delay_ms, 10_000);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(Config::default().git.inline_blame_delay_ms, 500);
    }
}
