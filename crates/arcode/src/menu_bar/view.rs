//! Drawing the bar (docs/DESIGN.md 6.2, 6.12): menu titles as quiet
//! chips on the canvas after the logo, a hairline before the project chip,
//! and the open menus as popovers. While a menu is open a transparent
//! layer over the window catches clicks outside it and lets the pointer
//! switch menus by moving over the titles. The menu and its submenus are
//! siblings in that layer, each drawn over the one before; a menu taller
//! than the window scrolls. Nothing animates.

use std::ops::Range;

use arcode_ui_kit::metrics::{CONTROL_H, ICON_BUTTON_RADIUS, WEIGHT_MEDIUM, icon_size};
use arcode_ui_kit::{
    IconName, Tooltip, ember, icon, kbd, menu_row, menu_separator, popover, spacer, theme,
    truncated, type_scale, ui_font_family,
};
use gpui::{
    AnyElement, AnyView, Context, HighlightStyle, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Render, ScrollHandle, SharedString, StyledText, UnderlineStyle, Window,
    anchored, canvas, deferred, div, point, prelude::*, px,
};

use super::model::{MENUS, mnemonic};
use super::place::{RowAnchor, beside_row};
use super::rows::{Row, RowKind};
use super::state::NavKey;
use super::{CONTEXT, MenuBar};
use crate::title_bar::TITLE_BAR_HEIGHT;

/// Window width (at UI size 13) under which the titles collapse into the
/// menu button. Every title, the project and branch chips, the Run
/// button and a usable search field fit above it.
const COLLAPSE_W: f32 = 1180.;

/// The closest a menu comes to a window edge.
const MARGIN: Pixels = px(6.);

/// The shortest a menu gets in a tiny window (then it scrolls).
const MIN_POPUP_H: Pixels = px(80.);

/// Window width under which the menus collapse at UI size `font_size`.
pub fn collapse_width(font_size: f32) -> f32 {
    COLLAPSE_W * (font_size / 13.).max(0.5)
}

/// `text` with the character at `mark` underlined when `show` is set.
fn label(text: SharedString, mark: Option<(usize, char)>, show: bool) -> StyledText {
    let styled = StyledText::new(text.clone());
    let Some((at, _)) = mark.filter(|_| show) else {
        return styled;
    };
    let end = text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
    let range: Range<usize> = at..end;
    styled.with_highlights([(
        range,
        HighlightStyle {
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: false,
            }),
            ..Default::default()
        },
    )])
}

/// Where a row is drawn and how.
#[derive(Clone, Copy)]
struct RowAt {
    level: usize,
    ix: usize,
    selected: bool,
    /// Underline the mnemonic.
    keyboard: bool,
    /// The popup is a choice (themes): leave room for the ember mark.
    choice: bool,
}

impl MenuBar {
    fn render_titles(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let open = self.open.as_ref();
        let any_open = open.is_some();
        let titles = self.titles.clone();
        let row = div()
            .flex()
            .flex_none()
            .items_center()
            .on_children_prepainted(move |bounds, _, _| *titles.borrow_mut() = bounds)
            .children(MENUS.iter().enumerate().map(|(ix, menu)| {
                let (text, mark) = mnemonic(menu.title);
                let active = open.is_some_and(|o| o.title == ix);
                let keyboard = open.is_some_and(|o| o.keyboard);
                div()
                    .id(("menu-title", ix))
                    .debug_selector(move || format!("menu-title-{ix}"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(CONTROL_H)
                    .px(px(8.))
                    .rounded(ICON_BUTTON_RADIUS)
                    .text_size(scale.body)
                    .whitespace_nowrap()
                    .text_color(if active { c.text } else { c.text_muted })
                    .when(active, |el| el.bg(c.hover))
                    .when(!any_open, |el| {
                        el.hover(|s| s.bg(c.hover).text_color(c.text))
                    })
                    .child(label(text.into(), mark, keyboard))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |bar, _, window, cx| {
                            // The click must not move focus out of the panel.
                            window.prevent_default();
                            cx.stop_propagation();
                            bar.click_title(ix, window, cx);
                        }),
                    )
            }));
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(row)
            // A hairline between the menus and the project chip.
            .child(
                div()
                    .flex_none()
                    .w(px(1.))
                    .h(px(16.))
                    .mx(px(6.))
                    .bg(c.border),
            )
            .into_any_element()
    }

    fn render_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let open = self.open.is_some();
        let titles = self.titles.clone();
        div()
            .flex()
            .flex_none()
            .on_children_prepainted(move |bounds, _, _| *titles.borrow_mut() = bounds)
            .child(
                div()
                    .id("menu-button")
                    .debug_selector(|| "menu-button".into())
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(CONTROL_H)
                    .rounded(ICON_BUTTON_RADIUS)
                    .when(open, |el| el.bg(c.hover))
                    .when(!open, |el| el.hover(|s| s.bg(c.hover)))
                    .child(
                        icon(IconName::Menu)
                            .size(icon_size::TOOLBAR + px(2.))
                            .color(if open { c.text } else { c.text_muted }),
                    )
                    .when(!open, |el| {
                        el.tooltip(|_, cx| -> AnyView { Tooltip::text("Menu (Alt)", cx) })
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|bar, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            bar.click_button(window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    /// The title (or the menu button) under `position`, from the last frame.
    fn title_at(&self, position: gpui::Point<gpui::Pixels>) -> Option<usize> {
        self.titles
            .borrow()
            .iter()
            .position(|b| b.contains(&position))
    }

    fn backdrop_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        match self.title_at(event.position) {
            Some(ix) if !self.collapsed && event.button == MouseButton::Left => {
                self.click_title(ix, window, cx)
            }
            _ => self.close(window, cx),
        }
    }

    fn backdrop_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.collapsed {
            return;
        }
        let Some(ix) = self.title_at(event.position) else {
            return;
        };
        if let Some(open) = &mut self.open
            && (open.title != ix || open.bar_only)
        {
            open.open_title(ix, false);
            self.reveal_from = Some(0);
            cx.notify();
        }
    }

    /// The popup of `level`, at most `max_h` tall (it scrolls). When a
    /// deeper popup is open, its row records where it was drawn in
    /// `anchor`.
    fn render_popup(
        &self,
        level: usize,
        anchor: Option<RowAnchor>,
        max_h: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        let rows = open.rows(level)?;
        let selected = open.sel.get(level).copied().flatten();
        let keyboard = open.keyboard;
        let choice = rows.iter().any(|r| r.checked.is_some());
        let c = theme(cx).colors.clone();
        let mut popup = popover(cx)
            .id(("menu-popup", level))
            .debug_selector(move || format!("menu-popup-{level}"))
            .occlude()
            .min_w(px(if level == 0 && open.collapsed {
                180.
            } else {
                240.
            }))
            .max_w(px(520.))
            .max_h(max_h.max(MIN_POPUP_H))
            .overflow_y_scroll()
            .when_some(self.scroll.get(level), |el, handle: &ScrollHandle| {
                el.track_scroll(handle)
            })
            .font_family(ui_font_family(cx))
            .text_size(type_scale(cx).body)
            .text_color(c.text);
        for (ix, row) in rows.iter().enumerate() {
            let anchor = anchor.clone().filter(|_| selected == Some(ix));
            let at = RowAt {
                level,
                ix,
                selected: selected == Some(ix),
                keyboard,
                choice,
            };
            popup = popup.child(self.render_row(at, row, anchor, cx));
        }
        Some(popup.into_any_element())
    }

    fn render_row(
        &self,
        at: RowAt,
        row: &Row,
        anchor: Option<RowAnchor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let RowAt {
            level,
            ix,
            selected,
            keyboard,
            choice,
        } = at;
        if matches!(row.kind, RowKind::Separator) {
            return menu_separator(cx).into_any_element();
        }
        let c = theme(cx).colors.clone();
        let meta = type_scale(cx).meta;
        let enabled = row.selectable();
        let is_sub = row.children().is_some();
        let id = SharedString::from(format!("menu-row-{level}-{ix}"));
        // A disabled row has no hover fill: it is built as a "selected"
        // row (which has none) with the fill taken off again.
        menu_row(id.clone(), selected || !enabled, cx)
            .debug_selector(move || id.to_string())
            .relative()
            .gap(px(8.))
            .when(!enabled, |el| {
                el.bg(gpui::transparent_black()).cursor_default()
            })
            .text_color(if enabled { c.text } else { c.text_disabled })
            .when(choice, |el| {
                // The theme in use carries the ember diamond, as on the
                // Settings page's theme cards.
                el.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .w(px(14.))
                        .when(row.checked == Some(true), |el| el.child(ember())),
                )
            })
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .when(row.checked == Some(true), |el| {
                        el.font_weight(WEIGHT_MEDIUM)
                    })
                    .child(label(row.label.clone(), row.mnemonic, keyboard)),
            )
            .child(spacer().min_w(px(16.)))
            .children(row.detail.clone().map(|detail| {
                div()
                    .flex_shrink()
                    .min_w_0()
                    .max_w(px(260.))
                    .overflow_hidden()
                    .text_size(meta)
                    .text_color(c.text_muted)
                    .child(truncated(detail).middle())
            }))
            .children(row.keys.clone().map(kbd))
            .when(is_sub, |el| {
                el.child(
                    icon(IconName::ChevronRight)
                        .size(icon_size::INLINE)
                        .color(if enabled {
                            c.text_muted
                        } else {
                            c.text_disabled
                        }),
                )
            })
            .on_mouse_move(cx.listener(move |bar, _: &MouseMoveEvent, _, cx| {
                if let Some(open) = &mut bar.open {
                    let before = open.sel.clone();
                    open.hover(level, ix);
                    if open.sel != before {
                        // A submenu that opens starts at its top.
                        bar.reveal_from = Some(level + 1);
                        cx.notify();
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |bar, _, window, cx| {
                    cx.stop_propagation();
                    if let Some(open) = &mut bar.open {
                        let outcome = open.click(level, ix);
                        bar.apply(outcome, window, cx);
                    }
                }),
            )
            .children(anchor.map(|anchor| {
                // Where this row is drawn, for its submenu (see `place`).
                canvas(
                    move |bounds, _, _| anchor.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            }))
            .into_any_element()
    }

    fn render_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        if open.bar_only {
            return None;
        }
        let anchor_x = self
            .titles
            .borrow()
            .get(if open.collapsed { 0 } else { open.title })
            .map_or(px(0.), |b| b.left());
        let viewport = window.viewport_size();
        let depth = open.depth();
        // One anchor per popup that has a deeper one open.
        let anchors: Vec<RowAnchor> = (1..depth).map(|_| RowAnchor::default()).collect();
        let top = px(TITLE_BAR_HEIGHT - 2.);
        let popup = self.render_popup(
            0,
            anchors.first().cloned(),
            viewport.height - top - MARGIN,
            cx,
        )?;
        let mut backdrop = div()
            .id("menu-backdrop")
            .occlude()
            .w(viewport.width)
            .h(viewport.height)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::backdrop_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::backdrop_down))
            .on_mouse_move(cx.listener(Self::backdrop_move))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                anchored()
                    .position(point(anchor_x, top))
                    .snap_to_window_with_margin(MARGIN)
                    .child(popup),
            );
        // Each submenu after its parent, so it is drawn over it.
        for level in 1..depth {
            let max_h = viewport.height - MARGIN * 2.;
            let Some(popup) = self.render_popup(level, anchors.get(level).cloned(), max_h, cx)
            else {
                break;
            };
            backdrop = backdrop.child(beside_row(anchors[level - 1].clone(), MARGIN, popup));
        }
        Some(
            deferred(anchored().position(point(px(0.), px(0.))).child(backdrop))
                .with_priority(2)
                .into_any_element(),
        )
    }
}

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if cfg!(target_os = "macos") {
            return div().into_any_element();
        }
        let collapsed = super::folds(window, cx);
        let depth = self.open.as_ref().map_or(0, |o| o.depth());
        while self.scroll.len() < depth {
            self.scroll.push(ScrollHandle::new());
        }
        if let Some(from) = self.reveal_from.take()
            && let Some(open) = &self.open
        {
            for level in from..depth {
                let row = open.sel.get(level).copied().flatten().unwrap_or(0);
                self.scroll[level].scroll_to_item(row);
            }
        }
        if collapsed != self.collapsed {
            self.collapsed = collapsed;
            // The open menu belongs to the other layout.
            if self.open.is_some() {
                cx.defer_in(window, |bar, window, cx| bar.close(window, cx));
            }
        }
        let bar = if collapsed {
            self.render_button(cx)
        } else {
            self.render_titles(cx)
        };
        div()
            .id("menu-bar")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .flex()
            .flex_none()
            .items_center()
            .on_action(
                cx.listener(|bar, _: &super::SelectPrevious, w, cx| bar.nav(NavKey::Up, w, cx)),
            )
            .on_action(
                cx.listener(|bar, _: &super::SelectNext, w, cx| bar.nav(NavKey::Down, w, cx)),
            )
            .on_action(
                cx.listener(|bar, _: &super::SelectLeft, w, cx| bar.nav(NavKey::Left, w, cx)),
            )
            .on_action(
                cx.listener(|bar, _: &super::SelectRight, w, cx| bar.nav(NavKey::Right, w, cx)),
            )
            .on_action(
                cx.listener(|bar, _: &super::SelectFirst, w, cx| bar.nav(NavKey::Home, w, cx)),
            )
            .on_action(cx.listener(|bar, _: &super::SelectLast, w, cx| bar.nav(NavKey::End, w, cx)))
            .on_action(cx.listener(|bar, _: &super::Confirm, w, cx| bar.nav(NavKey::Enter, w, cx)))
            .on_action(cx.listener(|bar, _: &super::Dismiss, w, cx| bar.nav(NavKey::Escape, w, cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(bar)
            .children(self.render_overlay(window, cx))
            .into_any_element()
    }
}
