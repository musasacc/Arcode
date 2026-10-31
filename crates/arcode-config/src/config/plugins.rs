//! The `[plugins]` section of `config.toml`.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[plugins]`: the WASM plugin system (docs/PLUGINS.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Master switch for plugin network access. Off, every plugin request
    /// is refused before any prompt. On, a plugin still sends only when
    /// you click, and only to hosts you allowed.
    pub network: bool,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self { network: true }
    }
}

impl_section!(PluginSettings { network });

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn plugins_section_parses() {
        assert!(Config::default().plugins.network);
        let (config, warnings) = Config::from_toml_str("[plugins]\nnetwork = false\n");
        assert!(!config.plugins.network);
        assert!(warnings.is_empty(), "{warnings:?}");
        let (config, warnings) = Config::from_toml_str("[plugins]\nnetwork = \"no\"\n");
        assert!(config.plugins.network, "a wrong type keeps the default");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
