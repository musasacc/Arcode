//! Plugins section: the master switch for plugin network access and the
//! way to the Plugins page (list, permissions, install).

use arcode_ui_kit::{IconName, button, settings, theme, type_scale};
use gpui::{Context, Div, div, prelude::*};

use super::widgets::setting_row;
use super::{SettingsPage, group_title};

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let network = settings(cx).plugins.network;
    let c = theme(cx).colors.clone();
    let el = div().child(group_title("Network", cx)).child(page.bool_row(
        "Allow plugin network access",
        "Off: every plugin request is refused. On: a plugin sends only when you click, \
         and asks before it contacts a host for the first time.",
        "plugins.network",
        network,
        cx,
    ));
    if cfg!(not(feature = "plugins")) {
        return el.child(
            div()
                .pt_4()
                .text_size(type_scale(cx).body)
                .text_color(c.text_muted)
                .child("This build of Arcode has no plugin system."),
        );
    }
    // The buttons wrap under the text (a row's control does not shrink).
    el.child(group_title("Plugins", cx))
        .child(setting_row(
            "Installed plugins",
            "Turn plugins on or off, review and revoke what you allowed them, install one \
             from a folder. Nothing is downloaded.",
            div(),
            cx,
        ))
        .child(div().py_2().child(actions()))
}

#[cfg(feature = "plugins")]
fn actions() -> Div {
    use crate::plugins::commands::{InstallPluginFromFolder, ShowPluginLog, ShowPlugins};
    div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(
            button("settings-plugins-show", "Manage Plugins")
                .icon(IconName::Puzzle)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ShowPlugins), cx)),
        )
        .child(
            button("settings-plugins-install", "Install from Folder…")
                .icon(IconName::FolderOpen)
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(InstallPluginFromFolder), cx)
                }),
        )
        .child(
            button("settings-plugins-log", "Show Log")
                .icon(IconName::FileText)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ShowPluginLog), cx)),
        )
}

#[cfg(not(feature = "plugins"))]
fn actions() -> Div {
    let _ = (IconName::Puzzle, button("x", ""));
    div()
}
