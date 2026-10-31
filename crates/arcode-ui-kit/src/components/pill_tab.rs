//! Pill tabs (docs/DESIGN.md 6.6, 6.8): editor tabs, bottom panel tabs and
//! terminal sessions. No underline, no top bar: the active tab is a `pill`
//! filled chip, and the focused pane's active tab carries the ember mark.

use std::rc::Rc;

use gpui::{
    AnyView, App, ClickEvent, Div, ElementId, Hsla, IntoElement, RenderOnce, SharedString,
    Stateful, Window, div, prelude::*, px,
};

use super::tab_layout::{TabMetrics, TabSize, TabTrailing, tab_trailing};
use crate::metrics::{CONTROL_RADIUS, WEIGHT_MEDIUM, type_scale};
use crate::{IconName, Tooltip, badge, ember, icon, theme, truncated, unsaved_ring};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A pill tab. Build with [`pill_tab`], then either use it as an element or
/// call [`PillTab::build`] to add more handlers (drag, middle click).
#[derive(IntoElement)]
pub struct PillTab {
    id: ElementId,
    label: SharedString,
    icon: Option<(IconName, Option<Hsla>)>,
    active: bool,
    focused: bool,
    dirty: bool,
    ring_color: Option<Hsla>,
    closable: bool,
    badge: Option<(SharedString, Option<Hsla>)>,
    italic: bool,
    strikethrough: bool,
    label_color: Option<Hsla>,
    size: TabSize,
    max_label_w: gpui::Pixels,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
    on_close: Option<ClickHandler>,
}

/// A tab labelled `label`. Ids must be unique within the strip; the close
/// button gets `("pill-tab-close", <id>)` semantics through its own id.
pub fn pill_tab(id: impl Into<ElementId>, label: impl Into<SharedString>) -> PillTab {
    PillTab {
        id: id.into(),
        label: label.into(),
        icon: None,
        active: false,
        focused: false,
        dirty: false,
        ring_color: None,
        closable: false,
        badge: None,
        size: TabSize::Editor,
        italic: false,
        strikethrough: false,
        label_color: None,
        max_label_w: px(220.),
        tooltip: None,
        on_click: None,
        on_close: None,
    }
}

impl PillTab {
    /// A leading icon (15 px; 14 in panel tabs, 13 in session pills),
    /// tinted `color` or muted.
    pub fn icon(mut self, icon: IconName, color: Option<Hsla>) -> Self {
        self.icon = Some((icon, color));
        self
    }

    /// The selected tab: `pill` fill, `text`, Medium.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// The tab's pane or panel has focus: the active tab gets the ember
    /// mark before its icon. Ignored on inactive tabs.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Unsaved: the hollow `accent_alt` ring replaces the close `x`.
    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    /// Overrides the unsaved ring color (e.g. `warning` when the file also
    /// changed on disk).
    pub fn ring_color(mut self, color: Hsla) -> Self {
        self.ring_color = Some(color);
        self
    }

    /// Shows a close `x` (always on the active tab, on hover otherwise).
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// A mono count badge after the label; `color` overrides its text
    /// (`error` for the Problems count).
    pub fn badge(mut self, text: impl Into<SharedString>, color: Option<Hsla>) -> Self {
        self.badge = Some((text.into(), color));
        self
    }

    /// The tab size: editor (default), bottom panel or terminal session.
    pub fn size(mut self, size: TabSize) -> Self {
        self.size = size;
        self
    }

    /// Bottom panel tab: icon 14, padding 0 9. Same as
    /// `.size(TabSize::Panel)`.
    pub fn panel(self) -> Self {
        self.size(TabSize::Panel)
    }

    /// 24 tall with a 12 px label: terminal-session pills. Same as
    /// `.size(TabSize::Session)`.
    pub fn compact(self) -> Self {
        self.size(TabSize::Session)
    }

    /// An italic label (preview tabs).
    pub fn italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// A struck-through label (file deleted on disk).
    pub fn strikethrough(mut self, strikethrough: bool) -> Self {
        self.strikethrough = strikethrough;
        self
    }

    /// Overrides the label color.
    pub fn label_color(mut self, color: Hsla) -> Self {
        self.label_color = Some(color);
        self
    }

    /// Longest label before it ends in `…` (220 by default).
    pub fn max_label_width(mut self, width: gpui::Pixels) -> Self {
        self.max_label_w = width;
        self
    }

    /// Tooltip text (e.g. the full path).
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Called when the tab is clicked.
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    /// Called when the close `x` is clicked (the click does not reach
    /// `on_click`). Implies `.closable(true)`.
    pub fn on_close(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self.closable = true;
        self
    }

    /// The tab as a styled element, for adding handlers such as
    /// `.on_mouse_down(MouseButton::Middle, ..)` or `.on_drag(..)`.
    pub fn build(self, cx: &App) -> Stateful<Div> {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let group = SharedString::from(format!("pill-tab-{:?}", self.id));
        let TabMetrics {
            height,
            text: text_size,
            icon: icon_size,
            pad: pad_l,
        } = self.size.metrics(&scale);
        let active_fill = if self.size == TabSize::Session {
            c.hover
        } else {
            c.pill
        };
        let label_color =
            self.label_color
                .unwrap_or(if self.active { c.text } else { c.text_muted });
        let trailing = tab_trailing(self.active, self.dirty, self.closable);
        let close = |visible_always: bool| {
            let on_close = self.on_close.clone();
            div()
                .id(SharedString::from(format!("{group}-close")))
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .hover(|s| s.bg(c.hover))
                .when(!visible_always, |el| {
                    el.invisible().group_hover(group.clone(), |s| s.visible())
                })
                .child(icon(IconName::X).size(px(13.)).color(c.text_muted))
                .when_some(on_close, |el, f| {
                    el.on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        f(event, window, cx);
                    })
                })
        };
        let trailing_slot = match trailing {
            TabTrailing::None => None,
            TabTrailing::Close => Some(div().child(close(true))),
            TabTrailing::CloseOnHover => Some(div().child(close(false))),
            TabTrailing::Ring { closable } => Some(
                div()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(closable, |el| {
                                el.group_hover(group.clone(), |s| s.invisible())
                            })
                            .child(unsaved_ring(self.ring_color, cx)),
                    )
                    .when(closable, |el| el.child(close(false))),
            ),
        };
        let mut el = div()
            .id(self.id.clone())
            .group(group.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(7.))
            .h(height)
            .pl(pad_l)
            .pr(if trailing_slot.is_some() {
                px(4.)
            } else {
                pad_l
            })
            .rounded(CONTROL_RADIUS)
            .text_size(text_size)
            .text_color(label_color)
            .cursor_pointer()
            .when(self.active, |el| {
                el.bg(active_fill).font_weight(WEIGHT_MEDIUM)
            })
            .when(!self.active, |el| el.hover(|s| s.bg(c.hover)))
            .when(self.active && self.focused, |el| el.child(ember()))
            .when_some(self.icon, |el, (name, color)| {
                el.child(
                    icon(name)
                        .size(icon_size)
                        .color(color.unwrap_or(c.text_muted)),
                )
            })
            .child(
                div()
                    .max_w(self.max_label_w)
                    .min_w_0()
                    .overflow_hidden()
                    .when(self.italic, |el| el.italic())
                    .when(self.strikethrough, |el| el.line_through())
                    .child(truncated(self.label)),
            )
            .when_some(self.badge, |el, (text, color)| {
                let mut b = badge(text);
                if let Some(color) = color {
                    b = b.color(color);
                }
                if self.active {
                    b = b.fill(c.panel());
                }
                el.child(b)
            })
            .when_some(trailing_slot, |el, slot| {
                el.child(slot.relative().flex_none().size(px(18.)))
            });
        if let Some(f) = self.on_click {
            el = el.on_click(move |event, window, cx| f(event, window, cx));
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |_, cx| -> AnyView { Tooltip::text(text.clone(), cx) });
        }
        el
    }
}

impl RenderOnce for PillTab {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.build(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypeScale;

    #[test]
    fn sizes_follow_the_spec() {
        let scale = TypeScale::new(13.);
        let editor = TabSize::default().metrics(&scale);
        assert_eq!(
            (editor.height, editor.text, editor.icon, editor.pad),
            (px(26.), px(13.), px(15.), px(10.))
        );
        let panel = TabSize::Panel.metrics(&scale);
        assert_eq!(
            (panel.height, panel.text, panel.icon, panel.pad),
            (px(26.), px(13.), px(14.), px(9.))
        );
        let session = TabSize::Session.metrics(&scale);
        assert_eq!(
            (session.height, session.text, session.icon, session.pad),
            (px(24.), px(12.), px(13.), px(9.))
        );
        assert_eq!(pill_tab("t", "x").panel().size, TabSize::Panel);
        assert_eq!(pill_tab("t", "x").compact().size, TabSize::Session);
    }
}
