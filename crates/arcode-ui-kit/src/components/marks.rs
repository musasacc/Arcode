//! Small marks of the Ember/Chalk identity (docs/DESIGN.md 1, 2.4, 6.17):
//! the ember diamond, the unsaved ring, status dots and the `</>` logo.

use gpui::{App, Div, Hsla, IntoElement, Pixels, RenderOnce, Window, div, prelude::*, px};

use crate::metrics::EMBER;
use crate::{IconName, border_width, icon, theme};

/// The ember mark: a 7 px diamond in `accent` that marks where you are
/// (the focused pane's active tab, the current frame, the paused readout).
#[derive(IntoElement)]
pub struct EmberMark {
    size: Pixels,
    color: Option<Hsla>,
}

/// The ember mark at its default 7 px size.
pub fn ember() -> EmberMark {
    EmberMark {
        size: EMBER,
        color: None,
    }
}

impl EmberMark {
    /// Overrides the size.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Overrides the color (default `accent`).
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for EmberMark {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(theme(cx).colors.accent);
        icon(IconName::Ember).size(self.size).color(color)
    }
}

/// The unsaved mark: a hollow 8 px ring with a 1.5 px `accent_alt` stroke
/// (never a filled dot). `color` overrides the stroke, e.g. `warning` when
/// the file also changed on disk.
pub fn unsaved_ring(color: Option<Hsla>, cx: &App) -> Div {
    let color = color.unwrap_or(theme(cx).colors.accent_alt);
    border_width(
        div()
            .flex_none()
            .size(px(8.))
            .rounded_full()
            .border_color(color),
        px(1.5),
    )
}

/// A 7 px filled status dot (run state, LSP state).
pub fn status_dot(color: Hsla) -> Div {
    div().flex_none().size(px(7.)).rounded_full().bg(color)
}

/// The logo `</>` in one colour, the theme's `text` (white on dark themes,
/// black on light ones), `size` square (22 in the title bar, 56 on the
/// welcome screen). No tile behind it.
#[derive(IntoElement)]
pub struct Logo {
    size: Pixels,
}

/// The logo at `size`.
pub fn logo(size: Pixels) -> Logo {
    Logo { size }
}

impl RenderOnce for Logo {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = &theme(cx).colors;
        icon(IconName::Logo).size(self.size).color(c.text)
    }
}
