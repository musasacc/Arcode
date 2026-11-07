//! The `[discord]` section of `config.toml`: Discord Rich Presence.

use serde::{Deserialize, Serialize};

use crate::lenient::impl_section;

/// `[discord]`: show what you are working on in your Discord status. Arcode
/// only talks to the Discord app running on this machine, never to the
/// network. Off by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordSettings {
    /// Show the presence at all.
    pub enabled: bool,
    /// Show the active file's name ("Editing main.rs"); off: only its
    /// language ("Editing a Rust file").
    pub show_file_name: bool,
    /// Show the project's folder name; off: no project name at all.
    pub show_project_name: bool,
    /// Show how long the current project has been open.
    pub show_elapsed: bool,
    /// The Discord application the presence belongs to (its name is what
    /// "Playing" shows). Empty: Arcode's built-in application.
    pub application_id: String,
}

impl Default for DiscordSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_file_name: false,
            show_project_name: false,
            show_elapsed: true,
            application_id: String::new(),
        }
    }
}

impl_section!(DiscordSettings {
    enabled,
    show_file_name,
    show_project_name,
    show_elapsed,
    application_id,
});

impl DiscordSettings {
    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        let id = self.application_id.trim();
        // Discord application IDs are numbers (snowflakes).
        if !id.is_empty() && !id.bytes().all(|b| b.is_ascii_digit()) {
            warnings.push(
                "`discord.application_id` must be a number from the Discord developer portal (ignored)"
                    .to_owned(),
            );
            self.application_id = String::new();
        } else if id.len() != self.application_id.len() {
            self.application_id = id.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    #[test]
    fn discord_section_parses_and_validates() {
        let d = Config::default().discord;
        assert!(!d.enabled && !d.show_file_name && !d.show_project_name);
        assert!(d.show_elapsed);
        assert_eq!(d.application_id, "");
        let (config, warnings) = Config::from_toml_str(
            "[discord]\nenabled = true\nshow_file_name = true\napplication_id = \" 123456789012345678 \"\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(config.discord.enabled && config.discord.show_file_name);
        assert!(!config.discord.show_project_name);
        assert_eq!(config.discord.application_id, "123456789012345678");
        let (config, warnings) = Config::from_toml_str("[discord]\napplication_id = \"Arcode\"\n");
        assert_eq!(config.discord.application_id, "");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
