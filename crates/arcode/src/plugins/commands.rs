//! Plugin commands in the palette and the actions of the Plugins feature.
//!
//! Every contributed command is registered once as a [`RunPluginCommand`]
//! carrying its namespaced id (`<plugin>/<command>`), listed under its
//! manifest title ("HTTP: Send Request at Cursor"). Titles without a
//! `Category:` prefix are listed under "Plugins". A command with a `when`
//! clause is only listed while the active document has that language.

use std::collections::HashMap;

use arcode_plugin_host::manifest::When;
use arcode_ui_kit::IconName;
use gpui::{Action, App, Global, KeyBinding, Keystroke, SharedString, WindowId, actions};

use super::client::PaletteCommand;

/// Runs a contributed plugin command (palette, keybinding) as a user
/// gesture, about the active document.
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = plugins, no_json)]
pub struct RunPluginCommand {
    /// Namespaced command id, `<plugin>/<command>`.
    pub id: SharedString,
}

actions!(
    plugins,
    [
        /// Opens the plugin log (every plugin's `host.log` and output).
        ShowPluginLog,
        /// Re-reads every plugin manifest.
        ReloadPlugins,
        /// Loads a plugin folder for this session ("Load Unpacked").
        LoadUnpackedPlugin,
        /// Cancels the most recent running plugin task.
        CancelPluginTask,
    ]
);

/// What the palette needs to apply `when` clauses without reading the
/// workspace (it is built while the workspace is being updated): every
/// plugin command's clause and each window's active document language.
#[derive(Default)]
pub struct CommandContext {
    when: HashMap<SharedString, Option<When>>,
    languages: HashMap<WindowId, String>,
    /// Commands whose key binding is installed.
    bound: std::collections::HashSet<SharedString>,
}

impl Global for CommandContext {}

/// Records the active document's language (as plugins see it) of a window.
pub fn set_window_language(window: WindowId, language: Option<String>, cx: &mut App) {
    let context = cx.default_global::<CommandContext>();
    match language {
        Some(language) => {
            context.languages.insert(window, language);
        }
        None => {
            context.languages.remove(&window);
        }
    }
}

/// Whether the palette of `window` lists the plugin command `id`.
pub fn listed(id: &str, window: WindowId, cx: &App) -> bool {
    let Some(context) = cx.try_global::<CommandContext>() else {
        return true;
    };
    let when = context.when.get(id).and_then(Option::as_ref);
    when_holds(when, context.languages.get(&window).map(String::as_str))
}

/// Registers the app-level commands of the Plugins feature.
pub fn init(cx: &mut App) {
    crate::commands::register(cx, "Plugins: Show Log", Box::new(ShowPluginLog));
    crate::commands::register(cx, "Plugins: Reload", Box::new(ReloadPlugins));
    crate::commands::register(cx, "Plugins: Load Unpacked…", Box::new(LoadUnpackedPlugin));
    crate::commands::register(cx, "Plugins: Cancel Task", Box::new(CancelPluginTask));
}

/// The palette label of a plugin command: its title, or `Plugins: title`
/// when the title has no category.
pub fn palette_label(title: &str) -> String {
    match title.split_once(": ") {
        Some((category, name)) if !category.is_empty() && !name.is_empty() => title.to_owned(),
        _ => format!("Plugins: {title}"),
    }
}

/// Adds `commands` to the palette (the registry replaces equal labels).
pub fn register_commands(commands: &[PaletteCommand], cx: &mut App) {
    for command in commands {
        let id: SharedString = command.id.as_str().to_owned().into();
        cx.default_global::<CommandContext>()
            .when
            .insert(id.clone(), command.when.clone());
        let action = RunPluginCommand { id: id.clone() };
        if let Some(key) = command.key.as_deref().filter(|k| valid_key(k))
            && cx.default_global::<CommandContext>().bound.insert(id)
        {
            // In editors only; outside its `when` clause the key does
            // nothing (the action propagates).
            cx.bind_keys([KeyBinding::new(key, action.clone(), Some("Editor"))]);
        }
        crate::commands::register(cx, &palette_label(&command.title), Box::new(action));
    }
}

/// Whether `key` is a valid key sequence (`secondary-enter`, `ctrl-k ctrl-r`);
/// an invalid one in a manifest is ignored instead of failing.
pub fn valid_key(key: &str) -> bool {
    !key.trim().is_empty() && key.split_whitespace().all(|k| Keystroke::parse(k).is_ok())
}

/// Whether a command with `when` is enabled while the active document has
/// `language` (`None`: no document).
pub fn when_holds(when: Option<&When>, language: Option<&str>) -> bool {
    match when {
        None => true,
        Some(When::Language(id)) => language == Some(id.as_str()),
    }
}

/// The host icon for a manifest icon name (plugins name icons from the
/// host set; unknown names get the plugin icon).
pub fn icon_for(name: Option<&str>) -> IconName {
    match name.unwrap_or_default() {
        "database" | "table" | "rows" => IconName::Rows,
        "globe" | "link" | "external-link" => IconName::ExternalLink,
        "play" => IconName::Play,
        "refresh" => IconName::Refresh,
        "search" => IconName::Search,
        "file" => IconName::File,
        "file-text" => IconName::FileText,
        "folder" => IconName::Folder,
        "terminal" => IconName::Terminal,
        "settings" | "gear" => IconName::Settings,
        "history" => IconName::History,
        "bug" => IconName::Bug,
        "package" => IconName::Package,
        "info" => IconName::Info,
        "warning" => IconName::TriangleAlert,
        "error" => IconName::CircleX,
        "check" => IconName::Check,
        "copy" => IconName::Copy,
        "save" => IconName::Save,
        "trash" => IconName::Trash,
        "plus" | "add" => IconName::Plus,
        "list" => IconName::ListTodo,
        "code" => IconName::CodeXml,
        "eye" => IconName::Search,
        "git-branch" => IconName::GitBranch,
        _ => IconName::Puzzle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keep_categories() {
        assert_eq!(
            palette_label("HTTP: Send Request at Cursor"),
            "HTTP: Send Request at Cursor"
        );
        assert_eq!(palette_label("Refresh"), "Plugins: Refresh");
        assert_eq!(palette_label(": odd"), "Plugins: : odd");
    }

    #[test]
    fn manifest_keys_are_checked() {
        assert!(valid_key("secondary-enter"));
        assert!(valid_key("ctrl-k ctrl-r"));
        assert!(!valid_key(""));
        assert!(!valid_key("a-b"));
    }

    #[test]
    fn when_clauses_follow_the_active_language() {
        let http = When::Language("http".into());
        assert!(when_holds(None, None));
        assert!(when_holds(Some(&http), Some("http")));
        assert!(!when_holds(Some(&http), Some("rust")));
        assert!(!when_holds(Some(&http), None));
    }

    #[test]
    fn icons_fall_back_to_the_plugin_icon() {
        assert_eq!(icon_for(Some("database")), IconName::Rows);
        assert_eq!(icon_for(Some("globe")), IconName::ExternalLink);
        assert_eq!(icon_for(Some("nope")), IconName::Puzzle);
        assert_eq!(icon_for(None), IconName::Puzzle);
    }
}
