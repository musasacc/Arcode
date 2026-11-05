//! Resolving the menus where focus is when one opens: each command's
//! action, whether something on the focus path (or the app) handles it,
//! and its key binding there; plus recent projects and the theme list.

use std::collections::HashMap;
use std::path::PathBuf;

use arcode_config::ThemeChoice;
use arcode_ui_kit::settings;
use gpui::{Action, App, FocusHandle, Window};

use super::MenuFacts;
use super::model;
use super::rows::{self, Resolved, Sources};
use crate::commands::{binding_label, commands};

/// Every command the menus name, resolved where `focus` is. A command
/// with no key binding there shows its binding in `editor` (the active
/// editor): Go to Definition is F12 even while the file tree has focus.
fn resolve_commands(
    focus: Option<&FocusHandle>,
    editor: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> HashMap<&'static str, Resolved> {
    let wanted: Vec<&'static str> = model::command_labels()
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    let found: Vec<(&'static str, Box<dyn Action>, bool)> = commands(cx)
        .iter()
        .filter_map(|c| {
            let label = c.label();
            let name = wanted.iter().find(|w| **w == label)?;
            Some((*name, c.action(), c.global))
        })
        .collect();
    found
        .into_iter()
        .map(|(name, action, global)| {
            // App-wide handlers (New Window, Quit) are not on the focus
            // path; `App::is_action_available` sees them (it cannot look
            // into this window while it is being updated).
            let available = global
                || window.is_action_available(action.as_ref(), cx)
                || cx.is_action_available(action.as_ref());
            let binding = match focus {
                Some(focus) => window.bindings_for_action_in(action.as_ref(), focus),
                None => window.bindings_for_action(action.as_ref()),
            }
            .into_iter()
            .next()
            .or_else(|| {
                let editor = editor.filter(|e| Some(*e) != focus)?;
                window
                    .bindings_for_action_in(action.as_ref(), editor)
                    .into_iter()
                    .next()
            });
            let keys = binding.map(|b| binding_label(&b));
            (
                name,
                Resolved {
                    action,
                    available,
                    keys,
                },
            )
        })
        .collect()
}

/// The rows of every menu, resolved where `focus` is (call it before focus
/// moves into the menu).
pub(super) fn build_root(
    facts: &MenuFacts,
    focus: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<rows::Row> {
    let resolved = resolve_commands(focus, facts.editor_focus.as_ref(), window, cx);
    let lookup = |label: &str| {
        resolved.get(label).map(|r| Resolved {
            action: r.action.boxed_clone(),
            available: r.available,
            keys: r.keys.clone(),
        })
    };
    let recent: Vec<(PathBuf, String)> = crate::app_state::recent_projects(cx)
        .iter()
        .filter(|p| Some(&p.path) != facts.project_root.as_ref())
        .take(10)
        .map(|p| (p.path.clone(), p.name.clone()))
        .collect();
    let current = settings(cx).ui.theme.clone();
    let mut themes = vec![(
        "system".to_owned(),
        "Follow System".to_owned(),
        current == ThemeChoice::System,
    )];
    if let Some(registry) = cx.try_global::<crate::theme_sync::Themes>() {
        themes.extend(registry.0.names().into_iter().map(|name| {
            let in_use = current == ThemeChoice::Named(name.to_owned());
            (name.to_owned(), name.to_owned(), in_use)
        }));
    }
    let sources = Sources {
        command: &lookup,
        facts: facts.facts,
        recent: &recent,
        themes: &themes,
    };
    rows::build_root(&sources)
}
