//! Small marks of the Ember/Chalk identity (docs/DESIGN.md 1, 2.4, 6.17):
//! the ember diamond, the unsaved ring, status dots and the spark logo.

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
    /// Overrides the size (the logo uses a larger spark).
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

/// The spark logo: the arc in `text` with the ember diamond in `accent`,
/// drawn from two monochrome SVGs stacked in one `size` box (22 in the
/// title bar, 56 on the welcome screen). No tile behind it.
#[derive(IntoElement)]
pub struct Logo {
    size: Pixels,
}

/// The spark logo at `size`.
pub fn logo(size: Pixels) -> Logo {
    Logo { size }
}

/// Where the spark sits in the logo's 24-unit box: `(left, top, side)`.
/// The diamond `M19.5 14l3 3-3 3-3-3z` spans x 16.5..22.5, y 14..20.
pub fn logo_spark_box(size: Pixels) -> (Pixels, Pixels, Pixels) {
    let unit = size / 24.;
    (unit * 16.5, unit * 14., unit * 6.)
}

impl RenderOnce for Logo {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = &theme(cx).colors;
        let (left, top, side) = logo_spark_box(self.size);
        div()
            .relative()
            .flex_none()
            .size(self.size)
            .child(icon(IconName::LogoArc).size(self.size).color(c.text))
            .child(
                div()
                    .absolute()
                    .left(left)
                    .top(top)
                    .child(ember().size(side)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_sits_below_the_arc_end() {
        let (left, top, side) = logo_spark_box(px(24.));
        assert_eq!((left, top, side), (px(16.5), px(14.), px(6.)));
        let (left, top, side) = logo_spark_box(px(48.));
        assert_eq!((left, top, side), (px(33.), px(28.), px(12.)));
        // The spark stays inside the box at every size.
        let (left, _, side) = logo_spark_box(px(22.));
        assert!(left + side <= px(22.));
    }
}
