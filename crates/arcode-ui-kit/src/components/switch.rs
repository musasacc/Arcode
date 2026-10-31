//! An on/off switch (docs/DESIGN.md 6.13): a 30x18 track, radius 9; on is
//! an `accent` track with an `accent_text` knob, off a 1.5 px
//! `border_strong` outline with a `border_strong` knob. A disabled switch
//! is an outline in `text_disabled` and ignores clicks.

use gpui::{
    AnyView, App, ClickEvent, ElementId, Hsla, IntoElement, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};

use crate::components::{Tooltip, border_width};
use crate::theme;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A pill switch. The knob sits right (on an `accent` track) when on.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
}

/// A switch showing `on`; give it a click handler that flips the state.
pub fn switch(id: impl Into<ElementId>, on: bool) -> Switch {
    Switch {
        id: id.into(),
        on,
        disabled: false,
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

    /// Shows the switch as unavailable (a `text_disabled` outline, the
    /// knob left wherever `on` puts it) and ignores clicks.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Tooltip text.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}

/// Track and knob colors, and whether the track is an outline (off or
/// disabled).
fn switch_colors(on: bool, disabled: bool, c: &crate::ThemeColors) -> (Hsla, Hsla, bool) {
    if disabled {
        (c.text_disabled, c.text_disabled, true)
    } else if on {
        (c.accent, c.accent_text, false)
    } else {
        (c.border_strong, c.border_strong, true)
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let (track, knob, outline) = switch_colors(self.on, self.disabled, &c);
        let el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .w(px(30.))
            .h(px(18.))
            .rounded(px(9.))
            .when(!self.disabled, |el| el.cursor_pointer())
            .when(self.on, |el| el.justify_end())
            .when(!outline, |el| el.bg(track))
            // The 1.5 px outline sits inside the 30x18 box; the padding
            // keeps the knob 3 px from the edge in both states.
            .px(px(if outline { 1.5 } else { 3. }))
            .child(div().size(px(12.)).rounded_full().bg(knob));
        let mut el = if outline {
            border_width(el, px(1.5)).border_color(track)
        } else {
            el
        };
        if let Some(handler) = self.on_click.filter(|_| !self.disabled) {
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
    fn on_is_filled_and_off_is_an_outline() {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.get("Arcode Chalk").unwrap().clone();
        let c = crate::UiTheme::new(std::sync::Arc::new(theme)).colors;
        assert_eq!(
            switch_colors(true, false, &c),
            (c.accent, c.accent_text, false)
        );
        assert_eq!(
            switch_colors(false, false, &c),
            (c.border_strong, c.border_strong, true)
        );
    }

    #[test]
    fn disabled_is_a_muted_outline_either_way() {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.get("Arcode Ember").unwrap().clone();
        let c = crate::UiTheme::new(std::sync::Arc::new(theme)).colors;
        for on in [true, false] {
            assert_eq!(
                switch_colors(on, true, &c),
                (c.text_disabled, c.text_disabled, true)
            );
        }
    }
}
