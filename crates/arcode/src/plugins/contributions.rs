//! What the enabled plugins contribute to the app's chrome: palette
//! commands, sidebar panels (activity-bar entries) and file languages.

use arcode_plugin_host::manifest::{PanelLocation, When};
use arcode_plugin_host::{PluginManager, QualifiedId};

use super::snapshot::ContributedLanguage;

/// A contributed sidebar panel (an activity-bar entry).
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarPanel {
    /// Namespaced panel id.
    pub id: QualifiedId,
    /// Title (tooltip and header).
    pub title: String,
    /// Host icon name from the manifest.
    pub icon: Option<String>,
}

/// A contributed palette command.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteCommand {
    /// Namespaced command id.
    pub id: QualifiedId,
    /// Palette title.
    pub title: String,
    /// Enablement (`language == <id>`).
    pub when: Option<When>,
    /// Default key binding (GPUI key syntax, e.g. `secondary-enter`).
    pub key: Option<String>,
}

/// What the app shows for the registry (read once per load).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contributions {
    /// Palette commands.
    pub commands: Vec<PaletteCommand>,
    /// Sidebar panels.
    pub sidebar: Vec<SidebarPanel>,
    /// Languages plugins add by file extension.
    pub languages: Vec<ContributedLanguage>,
}

impl Contributions {
    /// Collects the contributions of every registered plugin.
    pub fn of(manager: &PluginManager) -> Self {
        let registry = manager.registry();
        let mut out = Self::default();
        for (id, plugin, command) in registry.commands() {
            if manager.is_enabled(plugin.id()) {
                out.commands.push(PaletteCommand {
                    id,
                    title: command.title.clone(),
                    when: command.when.clone(),
                    key: command.key.clone(),
                });
            }
        }
        for plugin in registry.plugins() {
            if !manager.is_enabled(plugin.id()) {
                continue;
            }
            for panel in &plugin.manifest.contributes.panels {
                if panel.location == PanelLocation::Sidebar {
                    out.sidebar.push(SidebarPanel {
                        id: plugin.id().qualify(&panel.id),
                        title: panel.title.clone(),
                        icon: panel.icon.clone(),
                    });
                }
            }
        }
        for (plugin, language) in registry.languages() {
            if !manager.is_enabled(plugin.id()) {
                continue;
            }
            for ext in &language.extensions {
                out.languages.push(ContributedLanguage {
                    extension: ext.to_ascii_lowercase(),
                    id: language.id.clone(),
                    name: language.name.clone(),
                });
            }
        }
        out.commands.sort_by(|a, b| a.title.cmp(&b.title));
        out
    }
}
