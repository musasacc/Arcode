use gpui::{
    AnyView, App, Context, IntoElement, ParentElement, Render, RenderOnce, SharedString, Styled,
    Window, div, prelude::*, px,
};

use crate::{Elevated, theme};

/// A small floating label shown on hover.
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    /// Builds a tooltip view showing `text`.
    pub fn text(text: impl Into<SharedString>, cx: &mut App) -> AnyView {
        let text = text.into();
        cx.new(|_| Tooltip { text }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &theme(cx).colors;
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .text_color(c.text)
            .text_xs()
            .elevated(cx)
            .child(self.text.clone())
    }
}

/// A keyboard shortcut hint such as `Ctrl+P`.
#[derive(IntoElement)]
pub struct Kbd(SharedString);

pub fn kbd(keys: impl Into<SharedString>) -> Kbd {
    Kbd(keys.into())
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = &theme(cx).colors;
        div()
            .px_1p5()
            .rounded_sm()
            .border_1()
            .border_color(c.border)
            .bg(c.surface)
            .text_color(c.text_muted)
            .text_xs()
            .child(self.0)
    }
}

/// A 1px separator line.
#[derive(IntoElement)]
pub struct Divider {
    vertical: bool,
}

pub fn divider() -> Divider {
    Divider { vertical: false }
}

pub fn vertical_divider() -> Divider {
    Divider { vertical: true }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let border = theme(cx).colors.border;
        if self.vertical {
            div().w(px(1.)).h_full().flex_none().bg(border)
        } else {
            div().h(px(1.)).w_full().flex_none().bg(border)
        }
    }
}
