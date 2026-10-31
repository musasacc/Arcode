//! Buttons (docs/DESIGN.md 6.13): text buttons in three weights and square
//! icon buttons. Hover fills are solid colors, never alpha.

use gpui::{
    AnyView, App, ClickEvent, ElementId, Hsla, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, Styled, Window, div, prelude::*, px,
};

use crate::metrics::{CONTROL_H, CONTROL_H_PANEL, CONTROL_RADIUS, ICON_BUTTON_RADIUS};
use crate::metrics::{WEIGHT_MEDIUM, type_scale};
use crate::{IconName, Tooltip, icon, theme};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Visual weight of a [`Button`]. A surface has at most one Primary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Solid `accent` fill with `accent_text` (Continue, Open Folder, the
    /// confirm button of a dialog).
    Primary,
    /// No fill, 1 px `border_strong` outline, `text`.
    #[default]
    Secondary,
    /// No fill and no outline until hovered.
    Ghost,
}

/// The `(fill, text, hover fill, outline)` of a button style.
fn style_colors(style: ButtonStyle, c: &crate::ThemeColors) -> (Option<Hsla>, Hsla, Hsla, bool) {
    match style {
        ButtonStyle::Primary => (Some(c.accent), c.accent_text, c.accent_hover, false),
        ButtonStyle::Secondary => (None, c.text, c.hover, true),
        ButtonStyle::Ghost => (None, c.text, c.hover, false),
    }
}

/// The `(fill, text, outline)` of a disabled button: `text_disabled` (an
/// outline drops to `border`), and a disabled Primary trades its accent for
/// a solid `pill` fill, never alpha (docs/DESIGN.md 2).
fn disabled_colors(style: ButtonStyle, c: &crate::ThemeColors) -> (Option<Hsla>, Hsla, bool) {
    match style {
        ButtonStyle::Primary => (Some(c.pill), c.text_disabled, false),
        ButtonStyle::Secondary => (None, c.text_disabled, true),
        ButtonStyle::Ghost => (None, c.text_disabled, false),
    }
}

/// A text button with an optional leading icon: 28 tall (26 with
/// `.compact()`), radius 8, padding 0 11, body size Medium, icon 15.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    icon_color: Option<Hsla>,
    style: ButtonStyle,
    compact: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
}

/// A Secondary button labelled `label`.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button {
        id: id.into(),
        label: label.into(),
        icon: None,
        icon_color: None,
        style: ButtonStyle::default(),
        compact: false,
        disabled: false,
        on_click: None,
        tooltip: None,
    }
}

impl Button {
    /// A leading icon.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Tints the icon apart from the label (the idle Run button's `accent`
    /// play icon).
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// Primary, Secondary or Ghost.
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// 26 tall, for buttons inside panels.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Draws the button in `text_disabled` (a Primary on a solid `pill`
    /// fill) and ignores clicks.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Tooltip text.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Called when the button is clicked (not while disabled).
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let (mut bg, mut fg, hover, outline) = style_colors(self.style, &c);
        if self.disabled {
            (bg, fg, _) = disabled_colors(self.style, &c);
        }
        let outline_color = if self.disabled {
            c.border
        } else {
            c.border_strong
        };
        let icon_color = match self.icon_color {
            Some(color) if !self.disabled => color,
            _ => fg,
        };
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(if self.compact {
                CONTROL_H_PANEL
            } else {
                CONTROL_H
            })
            .px(px(11.))
            .rounded(CONTROL_RADIUS)
            .text_size(type_scale(cx).body)
            .font_weight(WEIGHT_MEDIUM)
            .text_color(fg)
            .when_some(bg, |el, bg| el.bg(bg))
            .when(outline, |el| el.border_1().border_color(outline_color))
            .when(!self.disabled, |el| {
                el.cursor_pointer().hover(move |s| s.bg(hover))
            })
            .when_some(self.icon, |el, name| {
                el.child(icon(name).size(px(15.)).color(icon_color))
            })
            .child(self.label);
        if let Some(handler) = self.on_click.filter(|_| !self.disabled) {
            el = el.on_click(handler);
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |_, cx| -> AnyView { Tooltip::text(text.clone(), cx) });
        }
        el
    }
}

/// A square, icon-only button: 28 by default (`.size()`), radius 7, icon
/// about 57 % of the button in `text_muted`, `text` on hover. `.active()`
/// gives the selected look: `pill` fill and an `accent` icon.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: Pixels,
    width: Option<Pixels>,
    icon_size: Option<Pixels>,
    icon_color: Option<Hsla>,
    active: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
}

/// A 28 px icon button.
pub fn icon_button(id: impl Into<ElementId>, icon: IconName) -> IconButton {
    IconButton {
        id: id.into(),
        icon,
        size: px(28.),
        width: None,
        icon_size: None,
        icon_color: None,
        active: false,
        on_click: None,
        tooltip: None,
    }
}

/// The icon size for a button of `size`: 16 at 28, 15 at 26, 14 at 24.
pub fn icon_size_for(size: Pixels) -> Pixels {
    px((f32::from(size) * 0.57).round())
}

impl IconButton {
    /// Button side length (28 in the title bar, 26 in tab strips, 24 in
    /// panel titles).
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    /// Makes the button wider than tall (the debug toolbar's 28x26
    /// buttons); the icon size still follows the height.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Overrides the icon size.
    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = Some(size);
        self
    }

    /// Overrides the icon color (e.g. `success` for Continue, `error` for
    /// Stop, `accent` for Settings while its tab is active).
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// Draws the button in its selected state: `pill` fill, `accent` icon.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Tooltip text.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Called when the button is clicked.
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let fg = match (self.icon_color, self.active) {
            (Some(color), _) => color,
            (None, true) => c.accent,
            (None, false) => c.text_muted,
        };
        let group = SharedString::from(format!("icon-button-{:?}", self.id));
        let hover_fg = if self.icon_color.is_some() || self.active {
            fg
        } else {
            c.text
        };
        let mut el = div()
            .id(self.id)
            .group(group.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(self.size)
            .w(self.width.unwrap_or(self.size))
            .rounded(ICON_BUTTON_RADIUS)
            .cursor_pointer()
            .when(self.active, |el| el.bg(c.pill))
            .when(!self.active, |el| el.hover(|s| s.bg(c.hover)))
            .child(
                icon(self.icon)
                    .size(self.icon_size.unwrap_or(icon_size_for(self.size)))
                    .color(fg)
                    .group_hover_color(group, hover_fg),
            );
        if let Some(handler) = self.on_click {
            el = el.on_click(handler);
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |_, cx| -> AnyView { Tooltip::text(text.clone(), cx) });
        }
        el
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_sizes_follow_the_button() {
        assert_eq!(icon_size_for(px(28.)), px(16.));
        assert_eq!(icon_size_for(px(26.)), px(15.));
        assert_eq!(icon_size_for(px(24.)), px(14.));
    }

    #[test]
    fn styles_use_solid_colors() {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.get("Arcode Ember").unwrap().clone();
        let c = crate::UiTheme::new(std::sync::Arc::new(theme)).colors;
        let (fill, fg, hover, outline) = style_colors(ButtonStyle::Primary, &c);
        assert_eq!(
            (fill, fg, hover, outline),
            (Some(c.accent), c.accent_text, c.accent_hover, false)
        );
        let (fill, _, hover, outline) = style_colors(ButtonStyle::Secondary, &c);
        assert!(fill.is_none() && outline && hover == c.hover);
        assert_eq!(hover.a, 1., "hover is opaque");
        let (fill, _, _, outline) = style_colors(ButtonStyle::Ghost, &c);
        assert!(fill.is_none() && !outline);
    }

    #[test]
    fn disabled_buttons_stay_opaque() {
        let registry = arcode_config::ThemeRegistry::builtin();
        for name in ["Arcode Ember", "Arcode Chalk"] {
            let theme = registry.get(name).unwrap().clone();
            let c = crate::UiTheme::new(std::sync::Arc::new(theme)).colors;
            let (fill, fg, outline) = disabled_colors(ButtonStyle::Primary, &c);
            assert_eq!((fill, fg, outline), (Some(c.pill), c.text_disabled, false));
            assert_eq!(fill.unwrap().a, 1., "{name}: no alpha in the chrome");
            for style in [ButtonStyle::Secondary, ButtonStyle::Ghost] {
                let (fill, fg, outline) = disabled_colors(style, &c);
                assert!(fill.is_none() && fg == c.text_disabled);
                assert_eq!(outline, style == ButtonStyle::Secondary);
            }
        }
    }
}
