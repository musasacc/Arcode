//! Plugin commands in the palette and the actions of the Plugins feature.
//!
//! Every contributed command is registered once as a [`RunPluginCommand`]
//! carrying its namespaced id (`<plugin>/<command>`), listed under its
//! manifest title ("HTTP: Send Request at Cursor"). Titles without a
//! `Category:` prefix are listed under "Plugins". A command with a `when`
//! clause is only listed while the active document has that language.
//!
//! A plugin never takes over what the app already has: a title another
//! command is listed under gets the plugin id appended, and a manifest key
//! that a built-in (or another plugin) binding uses in editors is not bound.

use std::collections::HashMap;

use arcode_plugin_host::QualifiedId;
use arcode_plugin_host::manifest::When;
use arcode_ui_kit::IconName;
use gpui::{
    Action, App, Global, KeyBinding, KeyContext, Keystroke, SharedString, WindowId, actions,
};

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
        /// Shows the installed plugins (Plugins activity).
        ShowPlugins,
        /// Installs a plugin from a folder (copied, then listed disabled).
        InstallPluginFromFolder,
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
    crate::commands::register(cx, "Plugins: Show Installed", Box::new(ShowPlugins));
    crate::commands::register(
        cx,
        "Plugins: Install from Folder…",
        Box::new(InstallPluginFromFolder),
    );
}

/// The palette label of a plugin command: its title, or `Plugins: title`
/// when the title has no category.
pub fn palette_label(title: &str) -> String {
    match title.split_once(": ") {
        Some((category, name)) if !category.is_empty() && !name.is_empty() => title.to_owned(),
        _ => format!("Plugins: {title}"),
    }
}

/// The plugin command id behind a palette or key binding action.
fn plugin_command(action: &dyn Action) -> Option<&str> {
    action
        .as_any()
        .downcast_ref::<RunPluginCommand>()
        .map(|a| a.id.as_ref())
}

/// The label `command` is listed under: its [`palette_label`], or that
/// label with the plugin id appended when another command (built in, or
/// another plugin's) already has it. A plugin never replaces a command it
/// does not own; `None` when even the suffixed label is taken.
fn unique_label(command: &PaletteCommand, cx: &App) -> Option<String> {
    let base = palette_label(&command.title);
    let suffixed = format!("{base} ({})", command.id.plugin());
    let free = |label: &str| {
        crate::commands::commands(cx)
            .iter()
            .filter(|c| c.label() == label)
            .all(|c| plugin_command(c.action_ref()) == Some(command.id.as_str()))
    };
    [base, suffixed].into_iter().find(|label| free(label))
}

/// Whether binding `key` in editors would shadow, or be shadowed by, a
/// binding already in the keymap (Ctrl+S, Ctrl+C, a chord starting with
/// it, another plugin's key) in an editor of a pane of the workspace.
pub fn key_taken(key: &str, cx: &App) -> bool {
    let Ok(keys) = key
        .split_whitespace()
        .map(Keystroke::parse)
        .collect::<Result<Vec<_>, _>>()
    else {
        return true;
    };
    let stacks = [
        vec![
            crate::actions::WORKSPACE_CONTEXT,
            crate::actions::PANE_CONTEXT,
            arcode_editor::KEY_CONTEXT,
        ],
        vec![
            crate::actions::WORKSPACE_CONTEXT,
            arcode_editor::KEY_CONTEXT,
        ],
    ];
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    stacks.iter().any(|names| {
        let stack: Vec<KeyContext> = names
            .iter()
            .filter_map(|n| KeyContext::parse(n).ok())
            .collect();
        // Every prefix: a built-in `ctrl-k` shadows a plugin's
        // `ctrl-k ctrl-x`; the whole sequence: `ctrl-k` alone would break
        // a built-in `ctrl-k ctrl-r` (`pending`).
        (1..=keys.len()).any(|n| {
            let (bindings, pending) = keymap.bindings_for_input(&keys[..n], &stack);
            !bindings.is_empty() || (n == keys.len() && pending)
        })
    })
}

/// Adds `commands` to the palette under labels no other command has (see
/// [`unique_label`]) and binds their keys where no built-in key is taken
/// (see [`key_taken`]). Registering a command again replaces its entry.
pub fn register_commands(commands: &[PaletteCommand], cx: &mut App) {
    for command in commands {
        let id: SharedString = command.id.as_str().to_owned().into();
        cx.default_global::<CommandContext>()
            .when
            .insert(id.clone(), command.when.clone());
        let action = RunPluginCommand { id: id.clone() };
        if let Some(key) = command.key.as_deref().filter(|k| valid_key(k))
            && !cx.default_global::<CommandContext>().bound.contains(&id)
        {
            if key_taken(key, cx) {
                log::warn!("plugin command {id}: key `{key}` is already in use; not bound");
            } else {
                cx.default_global::<CommandContext>()
                    .bound
                    .insert(id.clone());
                // In editors only; outside its `when` clause the key does
                // nothing (the action propagates).
                cx.bind_keys([KeyBinding::new(
                    key,
                    action.clone(),
                    Some(arcode_editor::KEY_CONTEXT),
                )]);
            }
        }
        crate::commands::unregister(cx, |a| plugin_command(a) == Some(id.as_ref()));
        match unique_label(command, cx) {
            Some(label) => crate::commands::register(cx, &label, Box::new(action)),
            None => log::warn!("plugin command {id}: title {:?} is taken", command.title),
        }
    }
}

/// Removes plugin commands from the palette (gone after a reload). Their
/// key bindings stay but do nothing: the action propagates.
pub fn unregister_commands(ids: &[QualifiedId], cx: &mut App) {
    crate::commands::unregister(cx, |a| {
        plugin_command(a).is_some_and(|id| ids.iter().any(|i| i.as_str() == id))
    });
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

    fn command(id: &str, title: &str) -> PaletteCommand {
        PaletteCommand {
            id: QualifiedId::parse(id).unwrap(),
            title: title.into(),
            when: None,
            key: None,
        }
    }

    fn labels_of(id: &str, cx: &App) -> Vec<String> {
        crate::commands::commands(cx)
            .iter()
            .filter(|c| plugin_command(c.action_ref()) == Some(id))
            .map(|c| c.label())
            .collect()
    }

    #[gpui::test]
    fn plugin_titles_never_replace_other_commands(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::commands::init(cx);
            init(cx);
            register_commands(
                &[
                    command("acme.one/run", "Tools: Run"),
                    command("acme.two/run", "Tools: Run"),
                    command("acme.two/reload", "Plugins: Reload"),
                ],
                cx,
            );
            assert_eq!(labels_of("acme.one/run", cx), ["Tools: Run"]);
            assert_eq!(labels_of("acme.two/run", cx), ["Tools: Run (acme.two)"]);
            assert_eq!(
                labels_of("acme.two/reload", cx),
                ["Plugins: Reload (acme.two)"]
            );
            // Registering again (a reload) replaces the plugin's own entry,
            // also under a new title; gone commands can be removed.
            register_commands(&[command("acme.one/run", "Tools: Run Now")], cx);
            assert_eq!(labels_of("acme.one/run", cx), ["Tools: Run Now"]);
            unregister_commands(&[QualifiedId::parse("acme.two/run").unwrap()], cx);
            assert!(labels_of("acme.two/run", cx).is_empty());
            assert_eq!(labels_of("acme.two/reload", cx).len(), 1);
        });
    }

    #[gpui::test]
    fn manifest_keys_only_take_free_editor_keys(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            arcode_editor::init(cx);
            crate::actions::bind_keys(cx);
            crate::commands::init(cx);
            // Ctrl+Enter commits in the Git panel only: free in editors.
            assert!(!key_taken("secondary-enter", cx));
            assert!(key_taken("secondary-s", cx), "Save");
            assert!(key_taken("secondary-w", cx), "Close Tab (pane)");
            assert!(key_taken("secondary-k", cx), "starts Ctrl+K Ctrl+R");
            assert!(key_taken("secondary-p secondary-x", cx), "Ctrl+P first");
            let mut send = command("arcode.http-client/send", "HTTP: Send");
            send.key = Some("secondary-enter".into());
            register_commands(&[send], cx);
            assert!(key_taken("secondary-enter", cx), "now the plugin's");
            let mut other = command("acme.tools/go", "Acme: Go");
            other.key = Some("secondary-enter".into());
            register_commands(&[other], cx);
            let keymap = cx.key_bindings();
            let bound: Vec<String> = keymap
                .borrow()
                .bindings()
                .filter_map(|b| plugin_command(b.action()).map(str::to_owned))
                .collect();
            assert_eq!(bound, ["arcode.http-client/send"]);
        });
    }

    #[test]
    fn icons_fall_back_to_the_plugin_icon() {
        assert_eq!(icon_for(Some("database")), IconName::Rows);
        assert_eq!(icon_for(Some("globe")), IconName::ExternalLink);
        assert_eq!(icon_for(Some("nope")), IconName::Puzzle);
        assert_eq!(icon_for(None), IconName::Puzzle);
    }
}
