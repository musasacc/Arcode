use gpui::{
    AnyView, App, ClickEvent, ElementId, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, div, prelude::*, px,
};

use crate::{IconName, Tooltip, icon, theme};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Visual weight of a [`Button`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Filled with the accent color.
    Primary,
    /// Subtle filled surface.
    #[default]
    Secondary,
    /// No background until hovered.
    Ghost,
}

/// A text button with an optional leading icon.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    style: ButtonStyle,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button {
        id: id.into(),
        label: label.into(),
        icon: None,
        style: ButtonStyle::default(),
        on_click: None,
        tooltip: None,
    }
}

impl Button {
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let (bg, fg, hover) = match self.style {
            ButtonStyle::Primary => (Some(c.accent), c.accent_text, c.accent.opacity(0.85)),
            ButtonStyle::Secondary => (Some(c.surface_raised), c.text, c.hover),
            ButtonStyle::Ghost => (None, c.text, c.hover),
        };
        let mut el = div()
            .id(self.id)
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(28.))
            .px_3()
            .rounded_md()
            .text_sm()
            .text_color(fg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .when_some(bg, |el, bg| el.bg(bg))
            .when_some(self.icon, |el, name| {
                el.child(icon(name).size(px(14.)).color(fg))
            })
            .child(self.label);
        if let Some(handler) = self.on_click {
            el = el.on_click(handler);
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |_, cx| -> AnyView { Tooltip::text(text.clone(), cx) });
        }
        el
    }
}

/// A square, icon-only button.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: gpui::Pixels,
    active: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
}

pub fn icon_button(id: impl Into<ElementId>, icon: IconName) -> IconButton {
    IconButton {
        id: id.into(),
        icon,
        size: px(28.),
        active: false,
        on_click: None,
        tooltip: None,
    }
}

impl IconButton {
    pub fn size(mut self, size: gpui::Pixels) -> Self {
        self.size = size;
        self
    }

    /// Draws the button in its selected state (e.g. the current activity).
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let fg = if self.active { c.text } else { c.text_muted };
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(self.size)
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(c.hover))
            .when(self.active, |el| el.bg(c.hover))
            .child(icon(self.icon).size(self.size * 0.57).color(fg));
        if let Some(handler) = self.on_click {
            el = el.on_click(handler);
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |_, cx| -> AnyView { Tooltip::text(text.clone(), cx) });
        }
        el
    }
}
