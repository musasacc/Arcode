//! The central command registry behind the command palette.
//!
//! Every user-facing action is registered once with a human-readable name
//! and a category (`"Git"`, `"Editor"`, ...). The palette lists them with
//! their current key binding, fuzzy filtered, recently used first, and runs
//! the chosen one where focus was before the palette opened. Later features
//! call [`register`] (or add an entry to [`crate::actions::commands`]).

mod keys;
pub mod palette;
mod rank;

use gpui::{Action, App, Global, SharedString};

pub use keys::{binding_label, keystroke_label};
pub use rank::{Candidate, Ranked, rank_commands};

/// One command of the palette.
pub struct Command {
    /// Category shown before the name, e.g. `"Git"` (may be empty).
    pub category: SharedString,
    /// Human-readable name, e.g. `"Toggle Blame"`.
    pub name: SharedString,
    /// Listed even when no view on the focus path handles it (actions with
    /// an app-wide handler, such as Quit).
    pub global: bool,
    action: Box<dyn Action>,
}

impl Command {
    /// `"Category: Name"` (or just the name without a category). Also the
    /// key under which recent use is remembered.
    pub fn label(&self) -> String {
        if self.category.is_empty() {
            self.name.to_string()
        } else {
            format!("{}: {}", self.category, self.name)
        }
    }

    /// A fresh copy of the action to dispatch.
    pub fn action(&self) -> Box<dyn Action> {
        self.action.boxed_clone()
    }

    /// The action itself (for key binding lookups).
    pub fn action_ref(&self) -> &dyn Action {
        self.action.as_ref()
    }
}

/// All registered commands (a GPUI global).
#[derive(Default)]
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl Global for CommandRegistry {}

impl CommandRegistry {
    /// The commands in registration order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Adds a command, replacing one with the same label.
    pub fn add(&mut self, command: Command) {
        let label = command.label();
        self.commands.retain(|c| c.label() != label);
        self.commands.push(command);
    }
}

/// Splits `"Git: Toggle Blame"` into category and name. Without a prefix
/// the category comes from the action's namespace (`editor::Undo` gives
/// `"Editor"`).
pub fn split_label(label: &str, action_name: &str) -> (String, String) {
    if let Some((category, name)) = label.split_once(": ")
        && !category.is_empty()
        && !name.is_empty()
    {
        return (category.to_owned(), name.to_owned());
    }
    let namespace = action_name.split("::").next().unwrap_or_default();
    let mut chars = namespace.chars();
    let category = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    (category, label.to_owned())
}

/// Registers `action` under `label` (`"Category: Name"`).
pub fn register(cx: &mut App, label: &str, action: Box<dyn Action>) {
    let (category, name) = split_label(label, action.name());
    cx.default_global::<CommandRegistry>().add(Command {
        category: category.into(),
        name: name.into(),
        global: false,
        action,
    });
}

/// Like [`register`] for actions handled app-wide (`cx.on_action`), which
/// are available whatever has focus.
pub fn register_global(cx: &mut App, label: &str, action: Box<dyn Action>) {
    register(cx, label, action);
    if let Some(last) = cx.default_global::<CommandRegistry>().commands.last_mut() {
        last.global = true;
    }
}

/// The registered commands.
pub fn commands(cx: &App) -> &[Command] {
    cx.try_global::<CommandRegistry>()
        .map_or(&[], |r| r.commands.as_slice())
}

/// Installs the registry with the application's commands.
pub fn init(cx: &mut App) {
    cx.set_global(CommandRegistry::default());
    for (label, action) in crate::actions::commands() {
        if action.name() == crate::actions::Quit.name() {
            register_global(cx, label, action);
        } else {
            register(cx, label, action);
        }
    }
    for (label, action) in crate::actions::editor_commands() {
        register(cx, label, action);
    }
}

#[cfg(test)]
mod tests;
