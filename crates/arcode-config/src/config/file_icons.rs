//! `ui.file_icons`: how files are drawn in the file tree, tabs and lists.

use serde::{Deserialize, Serialize};

/// The icons next to file names (file tree, tabs, pickers, search results,
/// the Source Control list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileIcons {
    /// The logo of the file's language (Rust, Python, ...) when Arcode has
    /// one; other files keep the plain icons.
    #[default]
    Logos,
    /// The plain monochrome icons, tinted by kind of file.
    Plain,
}

impl FileIcons {
    /// Every choice, in the order a picker lists them.
    pub const ALL: [Self; 2] = [Self::Logos, Self::Plain];

    /// The TOML value (`"logos"`, `"plain"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logos => "logos",
            Self::Plain => "plain",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn logos_by_default_and_plain_on_request() {
        assert_eq!(Config::default().ui.file_icons, FileIcons::Logos);
        let (config, warnings) = Config::from_toml_str("[ui]\nfile_icons = \"plain\"\n");
        assert_eq!(config.ui.file_icons, FileIcons::Plain);
        assert!(warnings.is_empty(), "{warnings:?}");
        for choice in FileIcons::ALL {
            let toml = format!("[ui]\nfile_icons = \"{}\"\n", choice.as_str());
            assert_eq!(Config::from_toml_str(&toml).0.ui.file_icons, choice);
        }
    }

    #[test]
    fn an_unknown_value_warns_and_keeps_logos() {
        let (config, warnings) = Config::from_toml_str("[ui]\nfile_icons = \"emoji\"\n");
        assert_eq!(config.ui.file_icons, FileIcons::Logos);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("ui.file_icons"), "{warnings:?}");
    }
}
