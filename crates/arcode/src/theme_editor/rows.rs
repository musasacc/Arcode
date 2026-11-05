//! The token list: a heading per group and a row per colour with its
//! swatch (opens the picker), key, what it colours, whether it is derived,
//! the unsaved ring and the hex value (click to type one).

use arcode_config::Color;
use arcode_config::theme::{
    TokenGroup, TokenRef, TokenSection, ansi_index, derived_sources, token_groups, ui_description,
};
use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{
    ButtonStyle, IconName, button, hsla, input_frame, mono_font_family, theme, unsaved_ring,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, Focusable, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use super::ThemeEditor;

/// Smallest height of a token row.
const ROW_H: f32 = 40.;
/// Width of the hex column.
const HEX_W: f32 = 112.;

impl ThemeEditor {
    /// Starts typing a hex value for `token`.
    pub(crate) fn edit_hex(
        &mut self,
        token: TokenRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(color) = self.edit.working().token_color(&token) else {
            return;
        };
        self.close_picker(cx);
        self.hex.update(cx, |field, cx| {
            field.set_text(&color.to_hex(), cx);
            field.select_all_text(cx);
        });
        self.hex_error = false;
        self.hex_token = Some(token);
        window.focus(&self.hex.focus_handle(cx));
        cx.notify();
    }

    /// Applies the typed hex value. A text that is not a colour keeps the
    /// field open and marks it.
    pub(crate) fn commit_hex(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(token) = self.hex_token.clone() else {
            return;
        };
        match self.hex.read(cx).text().parse::<Color>() {
            Ok(color) => {
                self.hex_token = None;
                self.set_color(&token, color, cx);
                window.focus(&self.focus);
            }
            Err(_) => {
                self.hex_error = true;
                cx.notify();
            }
        }
    }

    fn cancel_hex(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.hex_token.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    /// Every group of the edited theme.
    pub(super) fn render_groups(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let groups = token_groups(self.edit.working());
        let mut list = div().flex().flex_col();
        for group in groups {
            list = list.child(self.render_group(group, window, cx));
        }
        list
    }

    fn render_group(&self, group: TokenGroup, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let heading = div()
            .pt(px(20.))
            .pb(px(6.))
            .text_size(type_scale(cx).title)
            .font_weight(WEIGHT_SEMIBOLD)
            .text_color(c.text)
            .child(group.title);
        let mut el = div().flex().flex_col().child(heading);
        for token in group.tokens {
            el = el.child(self.render_row(token, window, cx));
        }
        el
    }

    fn render_row(
        &self,
        token: TokenRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let Some(color) = self.edit.working().token_color(&token) else {
            return div().into_any_element();
        };
        let path = token.path();
        let key = SharedString::from(token.key.clone());
        let swatch_token = token.clone();
        let swatch = div()
            .id(SharedString::from(format!("swatch-{path}")))
            .flex_none()
            .size(px(22.))
            .rounded(px(6.))
            .border_1()
            .border_color(c.border_strong)
            .bg(hsla(color))
            .cursor_pointer()
            .on_click(cx.listener(move |editor, e: &ClickEvent, window, cx| {
                editor.open_picker(swatch_token.clone(), e.position(), window, cx)
            }));
        let derivable = token.section == TokenSection::Ui && derived_sources(&token.key).is_some();
        let derived = derivable && self.edit.is_derived(&token.key);
        let sources = derived_sources(&token.key).unwrap_or(&[]).join(" and ");
        let detail: SharedString = match token.section {
            TokenSection::Ui if derived => format!("Derived from {sources}").into(),
            TokenSection::Ui => ui_description(&token.key).into(),
            TokenSection::Syntax => {
                let style = self.edit.working().syntax.get(&token.key).copied();
                let flags: Vec<&str> = style
                    .map(|s| {
                        [
                            (s.bold, "bold"),
                            (s.italic, "italic"),
                            (s.underline, "underline"),
                        ]
                        .into_iter()
                        .filter_map(|(on, name)| on.then_some(name))
                        .collect()
                    })
                    .unwrap_or_default();
                flags.join(", ").into()
            }
            TokenSection::Terminal => match ansi_index(&token.key) {
                Some(ix) => format!("ANSI {ix}").into(),
                None => "".into(),
            },
        };
        // The key is never cut; the description gives way.
        let label = div()
            .flex()
            .flex_col()
            .flex_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .font_family(mono_font_family())
                            .text_size(scale.body)
                            .text_color(c.text)
                            .whitespace_nowrap()
                            .child(key),
                    )
                    .when(self.edit.is_modified(&token), |el| {
                        el.child(unsaved_ring(None, cx))
                    }),
            )
            .when(!detail.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(scale.meta)
                        .text_color(c.text_muted)
                        .truncate()
                        .child(detail),
                )
            });
        // A derived key can be set on its own; an overridden one can follow
        // its sources again.
        let follow = derivable.then(|| {
            let key = token.key.clone();
            let id = SharedString::from(format!("follow-{path}"));
            let (label, tip) = if derived {
                ("Override", format!("Set {} on its own", token.key))
            } else {
                ("Derive", format!("Derive it from {sources} again"))
            };
            button(id, label)
                .icon(if derived {
                    IconName::Plus
                } else {
                    IconName::RotateCcw
                })
                .style(ButtonStyle::Ghost)
                .compact()
                .tooltip(tip)
                .on_click(
                    cx.listener(move |editor, _, _, cx| editor.set_follow(&key, !derived, cx)),
                )
        });
        let hex = self.render_hex(&token, color, window, cx);
        div()
            .id(SharedString::from(format!("token-{path}")))
            .flex()
            .items_center()
            .gap(px(10.))
            .min_h(px(ROW_H))
            .py(px(4.))
            .px(px(6.))
            .rounded(px(7.))
            .hover(|s| s.bg(c.hover))
            .child(swatch)
            .child(label)
            .children(follow)
            .child(hex)
            .into_any_element()
    }

    fn render_hex(
        &self,
        token: &TokenRef,
        color: Color,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        if self.hex_token.as_ref() == Some(token) {
            let focused = self.hex.focus_handle(cx).is_focused(window);
            let mut frame = input_frame(focused, false, cx);
            if self.hex_error {
                frame = frame.border_color(c.error);
            }
            return div()
                .flex_none()
                .w(px(HEX_W))
                .on_key_down(cx.listener(|editor, e: &KeyDownEvent, window, cx| {
                    match e.keystroke.key.as_str() {
                        "enter" => editor.commit_hex(window, cx),
                        "escape" => editor.cancel_hex(window, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .child(
                    frame.h(px(26.)).font_family(mono_font_family()).child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .overflow_hidden()
                            .child(self.hex.clone()),
                    ),
                )
                .into_any_element();
        }
        let edit_token = token.clone();
        let hex =
            button(
                SharedString::from(format!("hex-{}", token.path())),
                color.to_hex(),
            )
            .style(ButtonStyle::Ghost)
            .compact()
            .tooltip("Type a colour: #rgb, #rrggbb or #rrggbbaa")
            .on_click(cx.listener(move |editor, _, window, cx| {
                editor.edit_hex(edit_token.clone(), window, cx)
            }));
        div()
            .flex_none()
            .w(px(HEX_W))
            .flex()
            .justify_end()
            .font_family(mono_font_family())
            .child(hex)
            .into_any_element()
    }
}
