//! The section nav at the left of the Settings page (docs/DESIGN.md
//! 6.14): 196 wide with a hairline at its right, 30 px rows, the selected
//! one on a `pill` with its icon in `accent`; under a hairline, "Open
//! config.toml". Icons only when the page is narrow.

use arcode_ui_kit::metrics::{WEIGHT_MEDIUM, icon_size, type_scale};
use arcode_ui_kit::{IconName, Tooltip, icon, theme};
use gpui::{AnyView, Context, Div, SharedString, div, prelude::*, px};

use super::{Section, SettingsEvent, SettingsPage};

/// Width of the nav.
const NAV_W: f32 = 196.;
/// Width of the nav when the page is narrow (icons only).
const NAV_W_COMPACT: f32 = 44.;
/// Height of a nav row.
const ROW_H: f32 = 30.;

impl SettingsPage {
    pub(super) fn render_nav(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let body = type_scale(cx).body;
        let compact = self.compact;
        // While the filter shows every section, no single one is "here".
        let filtering = self.filtering(cx);
        let row = |id: SharedString, glyph: IconName, label: &'static str, on: bool| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(9.))
                .h(px(ROW_H))
                .px(px(9.))
                .rounded(px(7.))
                .text_size(body)
                .cursor_pointer()
                .when(on, |el| {
                    el.bg(c.pill).text_color(c.text).font_weight(WEIGHT_MEDIUM)
                })
                .when(!on, |el| el.text_color(c.text).hover(|s| s.bg(c.hover)))
                .child(icon(glyph).size(icon_size::ROW).color(if on {
                    c.accent
                } else {
                    c.text_muted
                }))
                .when(!compact, |el| el.child(label))
                .when(compact, |el| {
                    el.tooltip(move |_, cx| -> AnyView { Tooltip::text(label, cx) })
                })
        };
        let mut nav = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(if compact { NAV_W_COMPACT } else { NAV_W }))
            .h_full()
            .gap(px(2.))
            .p(px(8.))
            .border_r_1()
            .border_color(c.border);
        for section in Section::ALL {
            let on = !filtering && section == self.section;
            let id = SharedString::from(format!("settings-nav-{}", section.name()));
            nav = nav.child(
                row(id, section.icon(), section.name(), on).on_click(cx.listener(
                    move |page, _, window, cx| {
                        page.clear_filter(window, cx);
                        page.set_section(section, cx)
                    },
                )),
            );
        }
        nav.child(div().h(px(1.)).mx(px(2.)).my(px(7.)).bg(c.border))
            .child(
                row(
                    "open-config-file".into(),
                    IconName::ExternalLink,
                    "Open config.toml",
                    false,
                )
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
            )
    }
}
