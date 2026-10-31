//! Small labels of the Packages view: manager and kind badges, and the
//! tooltip text of a dependency row.

use arcode_project::deps::{DepSource, Dependency, Manager};
use arcode_ui_kit::ThemeColors;
use gpui::{Div, Hsla, SharedString, div, prelude::*};

/// The badge color of a manager.
pub fn manager_color(manager: Manager, c: &ThemeColors) -> Hsla {
    match manager {
        Manager::Cargo | Manager::Pnpm => c.warning,
        Manager::Npm | Manager::Bundler => c.error,
        Manager::Yarn | Manager::Go => c.info,
        Manager::Uv | Manager::Poetry | Manager::Pipenv | Manager::Pip => c.accent,
        Manager::Composer => c.accent_alt,
        Manager::Bun => c.text_muted,
    }
}

/// A small rounded label.
pub fn badge(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_none()
        .px_1()
        .rounded_sm()
        .text_xs()
        .bg(color.opacity(0.15))
        .text_color(color)
        .child(text.into())
}

/// The full details of a dependency for its tooltip.
pub(super) fn dep_tooltip(d: &Dependency) -> SharedString {
    let mut lines = vec![d.name.clone()];
    if d.package != d.name {
        lines.push(format!("package: {}", d.package));
    }
    if !d.requested.is_empty() {
        lines.push(format!("requested: {}", d.requested));
    }
    if let Some(locked) = &d.locked {
        lines.push(format!("locked: {locked}"));
    }
    if let Some(group) = &d.group {
        lines.push(format!("group: {group}"));
    }
    match &d.source {
        DepSource::Git(u) | DepSource::Url(u) | DepSource::Path(u) => {
            lines.push(format!("{}: {u}", d.source.label()))
        }
        DepSource::Workspace => lines.push("from the workspace".into()),
        DepSource::Registry => {}
    }
    lines.join("\n").into()
}
