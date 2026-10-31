//! One plugin's card on the Plugins page.

use arcode_ui_kit::{ButtonStyle, IconName, button, icon, icon_button, switch, theme};
use gpui::{Context, SharedString, div, prelude::*, px};

use super::rows::{PluginRow, Status};
use super::{PluginsPage, PluginsPageEvent};
use crate::plugins::consent::source_label;

/// The card of `row`; `expanded` also lists what the plugin may ask for.
pub(super) fn render(
    row: &PluginRow,
    expanded: bool,
    cx: &mut Context<PluginsPage>,
) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let id = row.id.clone();
    let key = row.id.as_str().to_owned();
    let status_color = match row.status {
        Status::Active => c.success,
        Status::Crashed => c.error,
        Status::Inactive => c.text_muted,
        Status::Disabled => c.text_disabled,
    };
    let enabled = row.enabled;
    let toggle_id = id.clone();
    let expand_id = id.clone();
    let header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .id(SharedString::from(format!("plugin-head-{key}")))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .child(
                    icon(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(12.))
                    .color(c.text_muted),
                )
                .child(
                    div()
                        .truncate()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if enabled { c.text } else { c.text_muted })
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(c.text_muted)
                        .child(row.version.clone()),
                )
                .on_click(cx.listener(move |p, _, _, cx| p.toggle_expanded(&expand_id, cx))),
        )
        .child(
            switch(SharedString::from(format!("plugin-switch-{key}")), enabled)
                .tooltip(if enabled { "Disable" } else { "Enable" })
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PluginsPageEvent::SetEnabled(toggle_id.clone(), !enabled))
                })),
        );
    let state = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_1()
        .child(icon(IconName::CircleDot).size(px(10.)).color(status_color))
        .child(div().text_color(status_color).child(row.status.label()))
        .child(div().text_color(c.text_disabled).child("·"))
        .child(
            div()
                .text_color(c.text_muted)
                .child(source_label(Some(row.source))),
        );
    let details = expanded.then(|| {
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .pt_1()
            .child(div().text_color(c.text_muted).child(row.id.to_string()))
            .child(div().text_color(c.text_muted).child("May ask to:"))
            .children(row.capabilities.iter().map(|line| {
                div()
                    .ml(px(8.))
                    .text_color(c.text)
                    .child(format!("• {line}"))
            }))
            .child(
                div()
                    .text_color(c.text_disabled)
                    .truncate()
                    .child(row.dir.display().to_string()),
            )
    });
    let grants = row.grants.iter().enumerate().map(|(ix, grant)| {
        let plugin = id.clone();
        let capability = grant.capability.clone();
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(c.text)
                    .child(grant.label()),
            )
            .child(
                icon_button(
                    SharedString::from(format!("plugin-revoke-{key}-{ix}")),
                    IconName::X,
                )
                .size(px(18.))
                .tooltip("Revoke (asked again next time)")
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PluginsPageEvent::Revoke(plugin.clone(), capability.clone()))
                })),
            )
    });
    let permissions = (!row.grants.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .pt_1()
            .child(
                div()
                    .font_weight(arcode_ui_kit::metrics::WEIGHT_SEMIBOLD)
                    .child("Permissions"),
            )
            .children(grants)
    });
    let log_id = id.clone();
    let restart_id = id.clone();
    let remove_id = id.clone();
    let actions = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .pt_1()
        .child(
            button(SharedString::from(format!("plugin-log-{key}")), "Log")
                .icon(IconName::FileText)
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PluginsPageEvent::OpenLog(log_id.clone()))
                })),
        )
        .when(row.status == Status::Crashed && enabled, |el| {
            el.child(
                button(
                    SharedString::from(format!("plugin-restart-{key}")),
                    "Restart",
                )
                .icon(IconName::RotateCcw)
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PluginsPageEvent::Restart(restart_id.clone()))
                })),
            )
        })
        .when(row.can_uninstall(), |el| {
            el.child(
                button(
                    SharedString::from(format!("plugin-uninstall-{key}")),
                    "Uninstall",
                )
                .icon(IconName::Trash)
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(PluginsPageEvent::Uninstall(remove_id.clone()))
                })),
            )
        });
    div()
        .id(SharedString::from(format!("plugin-card-{key}")))
        // Never shrunk in the scrolling list: shrinking sizes a card by its
        // text wrapped at its narrowest, leaving blank space under it.
        .flex_none()
        .flex()
        .flex_col()
        .gap_1()
        .mx_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(c.border)
        .text_xs()
        .child(header)
        .when_some(row.description.clone(), |el, d| {
            el.child(div().text_color(c.text_muted).child(d))
        })
        .child(state)
        .children(details)
        .children(permissions)
        .child(actions)
}
