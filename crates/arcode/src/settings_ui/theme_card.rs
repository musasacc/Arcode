//! Theme cards of the Settings page and the first-run dialog
//! (docs/DESIGN.md 6.14): a miniature of the workbench drawn from the
//! theme's own colors, its name and a muted sub-line.

use arcode_config::theme::BUILTIN_THEME_NAMES;
use arcode_config::{Appearance, Theme};
use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{Tooltip, border_width, ember, hsla, theme, truncated};
use gpui::{
    AnyElement, AnyView, App, Div, ElementId, Hsla, Pixels, SharedString, Stateful, div,
    prelude::*, px,
};

/// Width of a card.
pub const CARD_W: f32 = 168.;
/// Height of a card's preview.
const PREVIEW_H: f32 = 84.;
/// Corner radius of a card's preview.
const PREVIEW_RADIUS: f32 = 9.;

/// What a card stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardKind<'a> {
    /// "Follow system": the dark and the light theme, split.
    System {
        /// The dark theme's name.
        dark: &'a str,
        /// The light theme's name.
        light: &'a str,
    },
    /// One theme.
    Named {
        /// Its name.
        name: &'a str,
        /// Whether it is dark or light.
        appearance: Appearance,
    },
}

/// A theme name without the "Arcode " prefix of the built-ins.
fn short_name(name: &str) -> &str {
    name.strip_prefix("Arcode ").unwrap_or(name)
}

/// The muted line under a card's name: "Ember / Chalk", "Dark", "Classic ·
/// dark", "Custom · light", with " · in use" on the selected card.
pub fn card_subtitle(kind: CardKind, in_use: bool) -> String {
    let mode = |a: Appearance| match a {
        Appearance::Dark => "dark",
        Appearance::Light => "light",
    };
    let mut text = match kind {
        CardKind::System { dark, light } => {
            format!("{} / {}", short_name(dark), short_name(light))
        }
        CardKind::Named { name, appearance } => {
            let builtin = BUILTIN_THEME_NAMES.iter().position(|n| *n == name);
            match builtin {
                // Ember and Chalk, the current look.
                Some(0 | 1) => match appearance {
                    Appearance::Dark => "Dark".to_owned(),
                    Appearance::Light => "Light".to_owned(),
                },
                Some(_) => format!("Classic · {}", mode(appearance)),
                None => format!("Custom · {}", mode(appearance)),
            }
        }
    };
    if in_use {
        text.push_str(" · in use");
    }
    text
}

/// The colors a miniature needs, read from a theme.
struct Mini {
    canvas: Hsla,
    panel: Hsla,
    border: Hsla,
    focused: Hsla,
    pill: Hsla,
    muted: Hsla,
    accent: Hsla,
    bars: [Hsla; 5],
}

impl Mini {
    fn of(theme: &Theme) -> Self {
        let u = &theme.ui;
        let syntax = |capture: &str| hsla(theme.syntax_color(capture).unwrap_or(u.text));
        Mini {
            canvas: hsla(u.canvas),
            panel: hsla(u.background),
            border: hsla(u.border),
            focused: hsla(u.border_focused),
            pill: hsla(u.pill),
            muted: hsla(u.text_muted),
            accent: hsla(u.accent),
            bars: [
                syntax("keyword"),
                syntax("function"),
                syntax("type"),
                syntax("string"),
                syntax("number"),
            ],
        }
    }
}

/// A box placed at `(x, y)` with size `(w, h)` inside the preview.
fn at(x: f32, y: f32, w: f32, h: f32) -> Div {
    div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h))
}

/// The workbench in miniature, `CARD_W` x `PREVIEW_H`, without the canvas
/// fill (the caller paints it with rounded corners): two rail marks, the
/// sidebar, the focused editor with a tab pill, the ember diamond and five
/// syntax bars, and the bottom panel.
fn miniature(m: &Mini) -> Div {
    let bar = |x: f32, y: f32, w: f32, color: Hsla| at(x, y, w, 3.).rounded(px(1.5)).bg(color);
    let [keyword, function, ty, string, number] = m.bars;
    div()
        .absolute()
        .top_0()
        .left_0()
        .w(px(CARD_W))
        .h(px(PREVIEW_H))
        // Rail: the active tile and two icons.
        .child(at(5., 9., 8., 8.).rounded(px(2.)).bg(m.pill))
        .child(at(6., 22., 6., 2.).rounded(px(1.)).bg(m.muted))
        .child(at(6., 29., 6., 2.).rounded(px(1.)).bg(m.muted))
        // Sidebar panel.
        .child(
            at(18., 8., 40., 68.)
                .rounded(px(4.))
                .bg(m.panel)
                .border_1()
                .border_color(m.border),
        )
        // Editor panel, framed as focused.
        .child(
            at(62., 8., 100., 50.)
                .rounded(px(4.))
                .bg(m.panel)
                .border_1()
                .border_color(m.focused)
                .child(
                    at(4., 4., 30., 9.)
                        .rounded(px(3.))
                        .bg(m.pill)
                        .flex()
                        .items_center()
                        .pl(px(3.))
                        .child(ember().size(px(5.)).color(m.accent)),
                )
                .child(bar(8., 20., 14., keyword))
                .child(bar(25., 20., 18., function))
                .child(bar(14., 28., 12., ty))
                .child(bar(29., 28., 20., string))
                .child(bar(14., 36., 9., number)),
        )
        // Bottom panel.
        .child(
            at(62., 62., 100., 14.)
                .rounded(px(4.))
                .bg(m.panel)
                .border_1()
                .border_color(m.border),
        )
}

/// The preview: one miniature, or two split down the middle (dark on the
/// left) for "follow the system".
fn preview(first: Option<&Theme>, second: Option<&Theme>) -> Div {
    let radius = px(PREVIEW_RADIUS);
    let full = || div().relative().w(px(CARD_W)).h(px(PREVIEW_H));
    match (first, second) {
        (Some(a), Some(b)) => {
            let (left, right) = (Mini::of(a), Mini::of(b));
            let half = CARD_W / 2.;
            full()
                .child(
                    at(0., 0., half, PREVIEW_H)
                        .overflow_hidden()
                        .rounded_l(radius)
                        .bg(left.canvas)
                        .child(miniature(&left)),
                )
                .child(
                    at(half, 0., half, PREVIEW_H)
                        .overflow_hidden()
                        .rounded_r(radius)
                        .bg(right.canvas)
                        .child(miniature(&right).left(px(-half))),
                )
        }
        (Some(t), None) | (None, Some(t)) => {
            let m = Mini::of(t);
            full().rounded(radius).bg(m.canvas).child(miniature(&m))
        }
        (None, None) => full().rounded(radius),
    }
}

/// A theme card: the preview (a 1.5 px `accent` edge when `selected`, else
/// a `border` hairline), the name (SemiBold with the ember diamond when
/// selected) and `subtitle` in muted. Long names end in `…`; the tooltip
/// has all of it.
pub fn theme_card(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
    first: Option<&Theme>,
    second: Option<&Theme>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let label: SharedString = label.into();
    let tooltip = label.clone();
    let edge = if selected { c.accent } else { c.border };
    let frame = border_width(
        div()
            .rounded(px(PREVIEW_RADIUS + 1.5))
            .border_color(edge)
            .p(px(0.)),
        px(1.5),
    )
    .child(preview(first, second));
    let name: AnyElement = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w_0()
        .text_size(scale.body)
        .text_color(c.text)
        .when(selected, |el| {
            el.font_weight(WEIGHT_SEMIBOLD).child(ember())
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .debug_selector(|| "theme-card-label".into())
                .child(truncated(label)),
        )
        .into_any_element();
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(6.))
        .w(px(CARD_W + 3.))
        .cursor_pointer()
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tooltip.clone(), cx) })
        .child(frame)
        .child(
            div().flex().flex_col().gap(px(1.)).child(name).child(
                div()
                    .text_size(scale.meta)
                    .text_color(c.text_muted)
                    .child(truncated(subtitle.into())),
            ),
        )
}

/// Width taken by `n` cards in a row with `gap` between them.
pub fn cards_width(n: usize, gap: Pixels) -> Pixels {
    let n = n as f32;
    px((CARD_W + 3.) * n) + gap * (n - 1.).max(0.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitles_name_the_kind_and_the_theme_in_use() {
        let system = CardKind::System {
            dark: "Arcode Ember",
            light: "Arcode Chalk",
        };
        assert_eq!(card_subtitle(system, false), "Ember / Chalk");
        assert_eq!(card_subtitle(system, true), "Ember / Chalk · in use");
        let named = |name, appearance| CardKind::Named { name, appearance };
        assert_eq!(
            card_subtitle(named("Arcode Ember", Appearance::Dark), false),
            "Dark"
        );
        assert_eq!(
            card_subtitle(named("Arcode Chalk", Appearance::Light), true),
            "Light · in use"
        );
        assert_eq!(
            card_subtitle(named("Arcode Dark", Appearance::Dark), false),
            "Classic · dark"
        );
        assert_eq!(
            card_subtitle(named("Arcode Midnight", Appearance::Dark), false),
            "Classic · dark"
        );
        assert_eq!(
            card_subtitle(named("Solarized", Appearance::Light), false),
            "Custom · light"
        );
        let mixed = CardKind::System {
            dark: "Night Owl",
            light: "Arcode Light",
        };
        assert_eq!(card_subtitle(mixed, false), "Night Owl / Light");
        assert_eq!(cards_width(3, px(8.)), px(171. * 3. + 16.));
    }
}
