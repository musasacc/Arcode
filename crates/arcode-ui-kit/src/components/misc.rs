use gpui::{
    AnyView, App, Context, IntoElement, ParentElement, Render, RenderOnce, SharedString, Styled,
    Window, div, prelude::*, px,
};

use crate::metrics::type_scale;
use crate::{Elevated, theme, ui_font_family};

/// A small floating label shown on hover: `panel_raised`, 1 px
/// `border_strong`, radius 6, Inter 12, the small overlay shadow.
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
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(c.border_strong)
            .bg(c.panel_raised())
            .text_color(c.text)
            .text_size(type_scale(cx).meta)
            .font_family(ui_font_family(cx))
            .elevated(cx)
            .child(self.text.clone())
    }
}

/// A 1 px hairline in `border`.
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
