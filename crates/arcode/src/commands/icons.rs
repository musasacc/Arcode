//! The Lucide icon of each command in the palette (docs/DESIGN.md 6.11):
//! a few commands have their own (the debugger's steps, Save, Undo), every
//! other one gets its category's icon.

use arcode_ui_kit::IconName;

/// The icon of a category (`"Git"`, `"Debug"`); [`IconName::Command`] for
/// categories without one (plugin categories, an empty one).
pub fn category_icon(category: &str) -> IconName {
    match category.to_ascii_lowercase().as_str() {
        "editor" => IconName::CodeXml,
        "file" => IconName::File,
        "explorer" => IconName::Files,
        "git" => IconName::GitBranch,
        "debug" => IconName::Bug,
        "run" => IconName::Play,
        "search" => IconName::Search,
        "view" => IconName::Columns,
        "terminal" => IconName::Terminal,
        "packages" => IconName::Package,
        "todo" => IconName::ListTodo,
        "preferences" => IconName::Settings,
        "plugins" | "custom" => IconName::Puzzle,
        "languages" | "lsp" => IconName::Type,
        "help" => IconName::Info,
        "application" => IconName::Monitor,
        _ => IconName::Command,
    }
}

/// Commands with an icon of their own, as `(category, name prefix, icon)`;
/// the name matches when it starts with the prefix (case-insensitive).
const OWN_ICONS: &[(&str, &str, IconName)] = &[
    ("debug", "step over", IconName::RedoDot),
    ("debug", "step into", IconName::ArrowDownToDot),
    ("debug", "step out", IconName::ArrowUpFromDot),
    ("debug", "start", IconName::CirclePlay),
    ("debug", "continue", IconName::CirclePlay),
    ("debug", "pause", IconName::Pause),
    ("debug", "restart", IconName::RotateCcw),
    ("debug", "stop", IconName::Square),
    ("debug", "toggle breakpoint", IconName::CircleDot),
    ("debug", "edit breakpoint", IconName::CircleDot),
    ("debug", "add watch", IconName::Plus),
    ("debug", "open launch", IconName::FileText),
    ("run", "run", IconName::Play),
    ("file", "save", IconName::Save),
    ("file", "open folder", IconName::FolderOpen),
    ("file", "new project", IconName::Plus),
    ("file", "find file", IconName::Search),
    ("file", "close", IconName::X),
    ("editor", "undo", IconName::Undo),
    ("editor", "cursor undo", IconName::Undo),
    ("editor", "copy", IconName::Copy),
    ("editor", "duplicate", IconName::Copy),
    ("editor", "move line up", IconName::ArrowUp),
    ("editor", "move line down", IconName::ArrowDown),
    ("editor", "code actions", IconName::Lightbulb),
    ("editor", "refactor", IconName::Lightbulb),
    ("editor", "rename", IconName::Replace),
    ("editor", "format", IconName::Type),
    ("editor", "change language", IconName::Type),
    ("editor", "toggle vim", IconName::Keyboard),
    ("editor", "show hover", IconName::Info),
    ("git", "commit", IconName::GitCommit),
    ("git", "open changes", IconName::GitCompare),
    ("git", "next conflict", IconName::GitMerge),
    ("git", "previous conflict", IconName::GitMerge),
    ("git", "toggle blame", IconName::History),
    ("git", "refresh", IconName::Refresh),
    ("search", "replace", IconName::Replace),
    ("view", "command palette", IconName::Command),
    ("view", "toggle sidebar", IconName::PanelLeft),
    ("view", "toggle panel", IconName::PanelBottom),
    ("terminal", "new", IconName::Plus),
    ("preferences", "select theme", IconName::Palette),
    ("preferences", "select ui font", IconName::Type),
    ("packages", "add", IconName::Plus),
    ("packages", "refresh", IconName::Refresh),
    ("packages", "check for updates", IconName::Refresh),
    ("plugins", "reload", IconName::Refresh),
    ("plugins", "show log", IconName::FileText),
    ("todo", "rescan", IconName::Refresh),
    ("application", "quit", IconName::X),
];

/// The icon of the command `name` in `category`: its own, else the
/// category's.
pub fn command_icon(category: &str, name: &str) -> IconName {
    let category = category.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    OWN_ICONS
        .iter()
        .find(|(cat, prefix, _)| *cat == category && name.starts_with(prefix))
        .map_or_else(|| category_icon(&category), |(_, _, icon)| *icon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_get_their_own_icon_or_the_category_one() {
        assert_eq!(command_icon("Debug", "Step Over"), IconName::RedoDot);
        assert_eq!(command_icon("Debug", "Show Debug Panel"), IconName::Bug);
        assert_eq!(command_icon("Git", "Switch Branch"), IconName::GitBranch);
        assert_eq!(command_icon("File", "Save"), IconName::Save);
        assert_eq!(command_icon("TODO", "Show TODOs"), IconName::ListTodo);
        // Unknown categories (plugins name their own) fall back.
        assert_eq!(command_icon("HTTP", "Send Request"), IconName::Command);
        assert_eq!(command_icon("", "Anything"), IconName::Command);
    }

    #[test]
    fn every_registered_category_has_an_icon() {
        let mut labels: Vec<String> = crate::actions::commands()
            .into_iter()
            .chain(crate::actions::editor_commands())
            .map(|(label, _)| label.to_owned())
            .collect();
        labels.sort();
        for label in labels {
            let Some((category, _)) = label.split_once(": ") else {
                continue;
            };
            assert_ne!(
                category_icon(category),
                IconName::Command,
                "no icon for the category of {label}"
            );
        }
    }
}
