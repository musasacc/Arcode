//! Readouts and badges (docs/DESIGN.md 1, 6.13): every number the app
//! reports is set in JetBrains Mono, like readings on an instrument, so it
//! never jitters as values change.

use gpui::{App, Hsla, IntoElement, Pixels, RenderOnce, SharedString, Window, div, prelude::*, px};

use crate::metrics::type_scale;
use crate::{mono_font_family, theme};

/// Mono text in the readout size (12 at the default UI size), muted by
/// default: `Ln 15, Col 18`, `exit 101 · 0.92 s`, `main.rs:11`.
#[derive(IntoElement)]
pub struct Readout {
    text: SharedString,
    color: Option<Hsla>,
    size: Option<Pixels>,
}

/// A muted mono readout.
pub fn readout(text: impl Into<SharedString>) -> Readout {
    Readout {
        text: text.into(),
        color: None,
        size: None,
    }
}

impl Readout {
    /// Overrides the color (e.g. `error` for a failed exit code).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the size (11 for `file:line` in the call stack).
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = Some(size);
        self
    }
}

impl RenderOnce for Readout {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(theme(cx).colors.text_muted);
        div()
            .flex_none()
            .whitespace_nowrap()
            .font_family(mono_font_family())
            .text_size(self.size.unwrap_or(type_scale(cx).readout))
            .text_color(color)
            .child(self.text)
    }
}

/// A count badge: 16 tall, at least 18 wide, radius 5, `pill` fill, mono
/// 11 muted (`Problems 2`, `Breakpoints 3`). `.rail()` gives the icon-rail
/// badge instead: 14 tall, radius 7, `panel_raised` fill, 1 px `border`,
/// mono 10 in `text`.
#[derive(IntoElement)]
pub struct Badge {
    text: SharedString,
    color: Option<Hsla>,
    fill: Option<Hsla>,
    rail: bool,
}

/// A count badge showing `text`.
pub fn badge(text: impl Into<SharedString>) -> Badge {
    Badge {
        text: text.into(),
        color: None,
        fill: None,
        rail: false,
    }
}

impl Badge {
    /// Overrides the text color (e.g. `error` for the Problems count).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the fill (e.g. `panel` on an active pill tab).
    pub fn fill(mut self, fill: Hsla) -> Self {
        self.fill = Some(fill);
        self
    }

    /// The icon-rail look (smaller, outlined, `text` color).
    pub fn rail(mut self) -> Self {
        self.rail = true;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = &theme(cx).colors;
        let micro = type_scale(cx).micro;
        let (h, min_w, radius, size) = if self.rail {
            (px(14.), px(14.), px(7.), micro - px(1.))
        } else {
            (px(16.), px(18.), px(5.), micro)
        };
        let fill = self
            .fill
            .unwrap_or(if self.rail { c.panel_raised() } else { c.pill });
        let color = self
            .color
            .unwrap_or(if self.rail { c.text } else { c.text_muted });
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(h)
            .min_w(min_w)
            .px(px(4.))
            .rounded(radius)
            .bg(fill)
            .when(self.rail, |el| el.border_1().border_color(c.border))
            .font_family(mono_font_family())
            .text_size(size)
            .line_height(h)
            .text_color(color)
            .child(self.text)
    }
}
