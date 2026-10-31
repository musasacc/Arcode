//! An on/off switch (e.g. enabling a language pack).

use gpui::{
    AnyView, App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};

use crate::components::Tooltip;
use crate::theme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A small pill switch. The knob sits right (accent track) when on.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
}

/// A switch showing `on`; give it a click handler that flips the state.
pub fn switch(id: impl Into<ElementId>, on: bool) -> Switch {
    Switch {
        id: id.into(),
        on,
        on_click: None,
        tooltip: None,
    }
}

impl Switch {
    /// Called when the switch is clicked.
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }

    /// Tooltip text.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let (track, knob) = if self.on {
            (c.accent, c.accent_text)
        } else {
            (c.border, c.text_muted)
        };
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .w(px(28.))
            .h(px(16.))
            .px(px(2.))
            .rounded_full()
            .bg(track)
            .cursor_pointer()
            .when(self.on, |el| el.justify_end())
            .child(div().size(px(12.)).rounded_full().bg(knob));
        if let Some(handler) = self.on_click {
            el = el.on_click(handler);
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |_, cx| -> AnyView { Tooltip::text(text.clone(), cx) });
        }
        el
    }
}
