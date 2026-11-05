//! One recent project row of the welcome screen (docs/DESIGN.md 6.15).

use arcode_ui_kit::metrics::{ROW_H_WELCOME, WEIGHT_MEDIUM, icon_size, type_scale};
use arcode_ui_kit::{IconName, Tooltip, icon, mono_font_family, theme, truncated};
use gpui::{
    AnyView, Context, Div, MouseButton, MouseDownEvent, SharedString, Stateful, div, prelude::*, px,
};

use super::Welcome;
use super::list::Entry;

impl Welcome {
    /// A small icon button of a row: shown while the row is hovered or
    /// selected, or always when `shown`.
    fn row_button(
        &self,
        id: (&'static str, usize),
        glyph: IconName,
        row: &SharedString,
        shown: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let own = SharedString::from(format!("{}-{}", id.0, id.1));
        div()
            .id(id)
            .group(own.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded(px(7.))
            .when(!shown, |el| {
                el.opacity(0.).group_hover(row.clone(), |s| s.opacity(1.))
            })
            .child(
                icon(glyph)
                    .size(icon_size::TOOLBAR)
                    .color(c.text_muted)
                    .group_hover_color(own, c.text),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    /// One project row (`ix` is its place among the visible rows).
    pub(super) fn render_row(
        &self,
        ix: usize,
        entry: Entry,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let selected = self.selected == Some(ix);
        let group = SharedString::from(format!("recent-{ix}"));
        let Entry {
            path,
            name,
            shown,
            when,
            pinned,
            missing,
        } = entry;
        let (name_color, path_color) = if missing {
            (c.text_muted, c.text_disabled)
        } else {
            (c.text, c.text_muted)
        };
        let folder_icon = if missing {
            IconName::FolderX
        } else {
            IconName::Folder
        };
        let pin_tip = if pinned { "Unpin" } else { "Pin to Top" };
        let (pin_path, window_path, remove_path, menu_path, open_path) =
            (path.clone(), path.clone(), path.clone(), path.clone(), path);
        div()
            .id(("welcome-recent", ix))
            .group(group.clone())
            .flex()
            .items_center()
            .gap(px(10.))
            .h(ROW_H_WELCOME)
            .px(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |el| {
                let marker = super::reveal::marker(self.scroll.clone(), self.reveal.clone());
                el.relative().bg(c.pill).child(marker)
            })
            .when(!selected, |el| el.hover(|s| s.bg(c.pill)))
            .child(
                icon(folder_icon)
                    .size(px(16.))
                    .color(if selected && !missing {
                        c.accent
                    } else {
                        c.text_muted
                    })
                    .when(!missing, |el| el.group_hover_color(group.clone(), c.accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(scale.body)
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(name_color)
                            .overflow_hidden()
                            .child(truncated(name)),
                    )
                    .child(
                        div()
                            .font_family(mono_font_family())
                            .text_size(scale.micro)
                            .text_color(path_color)
                            .overflow_hidden()
                            .child(truncated(shown).middle()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(scale.meta)
                    .text_color(c.text_muted)
                    .child(if missing {
                        "Folder not found".to_owned()
                    } else {
                        when
                    }),
            )
            .child(
                self.row_button(
                    ("welcome-pin", ix),
                    IconName::Pin,
                    &group,
                    pinned || selected,
                    cx,
                )
                .tooltip(move |_, cx| -> AnyView { Tooltip::text(pin_tip, cx) })
                .on_click(cx.listener(move |welcome, _, _, cx| {
                    cx.stop_propagation();
                    welcome.toggle_pin(&pin_path, cx);
                })),
            )
            // A missing folder keeps the button's place, so the times line up.
            .when(missing, |el| el.child(div().flex_none().size(px(24.))))
            .when(!missing, |el| {
                el.child(
                    self.row_button(
                        ("welcome-new-window", ix),
                        IconName::ExternalLink,
                        &group,
                        selected,
                        cx,
                    )
                    .tooltip(|_, cx| -> AnyView { Tooltip::text("Open in New Window", cx) })
                    .on_click(cx.listener(move |welcome, _, window, cx| {
                        cx.stop_propagation();
                        welcome.open(window_path.clone(), true, window, cx);
                    })),
                )
            })
            .child(
                self.row_button(("welcome-remove", ix), IconName::X, &group, selected, cx)
                    .tooltip(|_, cx| -> AnyView {
                        Tooltip::text("Remove from Recent Projects", cx)
                    })
                    .on_click(cx.listener(move |welcome, _, _, cx| {
                        cx.stop_propagation();
                        welcome.remove(&remove_path, cx);
                    })),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |welcome, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    welcome.open_menu(menu_path.clone(), event.position, cx);
                }),
            )
            .on_click(cx.listener(move |welcome, _, window, cx| {
                welcome.open(open_path.clone(), false, window, cx)
            }))
    }
}
