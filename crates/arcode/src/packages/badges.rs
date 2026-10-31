//! Small labels of the Packages view: manager and kind badges, and the
//! tooltip text of a dependency row.

use arcode_project::deps::{DepSource, Dependency, Manager};
use arcode_ui_kit::{ThemeColors, theme};
use gpui::{App, Div, Hsla, SharedString, div, prelude::*, px};

/// The badge color of a manager.
pub fn manager_color(manager: Manager, c: &ThemeColors) -> Hsla {
    match manager {
        Manager::Cargo | Manager::Pnpm => c.warning,
        Manager::Npm | Manager::Bundler => c.error,
        Manager::Yarn | Manager::Go => c.info,
        Manager::Uv | Manager::Poetry | Manager::Pipenv | Manager::Pip => c.success,
        Manager::Composer => c.accent_alt,
        Manager::Bun => c.text_muted,
    }
}

/// A small rounded label: a solid `pill` chip with the text in `color`
/// (docs/DESIGN.md 6.13, no translucent fills).
pub fn badge(text: impl Into<SharedString>, color: Hsla, cx: &App) -> Div {
    div()
        .flex_none()
        .h(px(16.))
        .flex()
        .items_center()
        .px(px(5.))
        .rounded(px(5.))
        .text_size(arcode_ui_kit::type_scale(cx).micro)
        .font_weight(arcode_ui_kit::metrics::WEIGHT_MEDIUM)
        .bg(theme(cx).colors.pill)
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
