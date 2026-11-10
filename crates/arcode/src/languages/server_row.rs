//! One language server row of a Languages page card: its state, and for a
//! missing one the install command (to copy) with an Install button that
//! runs it in a terminal tab (see `lsp_bridge/install.rs`).

use arcode_ui_kit::{IconName, icon, icon_button, settings, theme};
use gpui::{App, ClipboardItem, SharedString, div, prelude::*, px};

use super::bundle::{Availability, ServerRow};
use crate::lsp_bridge::install::InstallLanguageServer;

/// The row of `server` (`state`: `None` while it is checked).
pub(super) fn render_server(
    state: Option<Availability>,
    pack: &str,
    ix: usize,
    server: &ServerRow,
    cx: &App,
) -> gpui::Div {
    let c = theme(cx).colors.clone();
    let (glyph, color, label) = match &state {
        Some(Availability::Installed) => (IconName::CircleCheck, c.success, "installed"),
        Some(Availability::Bundled) => (IconName::Package, c.success, "bundled"),
        Some(Availability::NeedsRuntime(_)) => {
            (IconName::TriangleAlert, c.warning, "bundled, cannot start")
        }
        Some(Availability::Missing) => (IconName::CircleX, c.warning, "not installed"),
        None => (IconName::CircleDot, c.text_muted, "checking…"),
    };
    let hint = server
        .install_command
        .clone()
        .or_else(|| server.install_note.clone());
    let runtime_note = match &state {
        Some(Availability::NeedsRuntime(note)) => Some(note.clone()),
        _ => None,
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x_1()
                .child(icon(glyph).size(px(12.)).color(color))
                .child(div().text_color(c.text).child(server.name.clone()))
                .child(div().text_color(c.text_muted).child(label)),
        )
        .when(state == Some(Availability::Missing), |el| {
            el.when_some(hint, |el, hint| {
                let copy = SharedString::from(hint.clone());
                let install = InstallLanguageServer {
                    server: server.name.clone().into(),
                };
                let mono = settings(cx).editor.font_family.clone();
                el.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_1()
                        .ml(px(16.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px_1()
                                .rounded(px(5.))
                                .bg(c.pill)
                                .text_color(c.text_muted)
                                .when(server.install_command.is_some(), |el| el.font_family(mono))
                                .child(hint),
                        )
                        .child(
                            icon_button(
                                SharedString::from(format!("copy-{pack}-{ix}")),
                                IconName::Copy,
                            )
                            .size(px(20.))
                            .tooltip("Copy")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.to_string()))
                            }),
                        )
                        .when(server.install_command.is_some(), |el| {
                            el.child(
                                icon_button(
                                    SharedString::from(format!("install-{pack}-{ix}")),
                                    IconName::Terminal,
                                )
                                .size(px(20.))
                                .tooltip("Install in a terminal")
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(Box::new(install.clone()), cx)
                                }),
                            )
                        }),
                )
            })
        })
        .when_some(runtime_note, |el, note| {
            el.child(
                div()
                    .ml(px(16.))
                    .px_1()
                    .rounded(px(5.))
                    .bg(c.pill)
                    .text_color(c.text_muted)
                    .child(note),
            )
        })
}
