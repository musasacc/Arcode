//! Container and display widgets: `stack`, `split`, `tabs`, `scroll`,
//! `text`, `badge`, `key-values`, `spinner`, `empty-state`, `divider`.
//! Plugin text is always plain text; right-click copies it.

use std::cell::Cell;
use std::rc::Rc;

use arcode_plugin_host::bindings::ui::{
    Axis, BadgeProps, Node, SplitProps, StackProps, TabsProps, TextProps, Tone,
};
use arcode_ui_kit::{
    BUNDLED_MONO_FONT, IconName, divider as hdivider, icon, pill_tab, theme, type_scale,
    vertical_divider,
};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, Div, Hsla, MouseButton, Pixels,
    SharedString, Window, canvas, div, prelude::*, px, relative,
};

use super::PanelView;
use arcode_plugin_host::ui_tree::PanelSnapshot;

/// Host-side state of a `split`: the ratio the user dragged to.
#[derive(Default)]
pub(super) struct SplitState {
    ratio: Option<f32>,
    dragging: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// The theme color of a tone.
pub(super) fn tone_color(tone: Tone, cx: &App) -> Hsla {
    let c = &theme(cx).colors;
    match tone {
        Tone::Normal => c.text,
        Tone::Muted => c.text_muted,
        Tone::Accent => c.accent,
        Tone::Success => c.success,
        Tone::Warning => c.warning,
        Tone::Danger => c.error,
    }
}

/// Right-click copies `text` (plugin text is selectable only this way).
pub(super) fn copyable(el: Div, text: String) -> Div {
    el.on_mouse_down(MouseButton::Right, move |_, _, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    })
}

/// `parent` is the enclosing stack's axis: in a column (or alone) the
/// text takes the full width, so a long line is cut at the panel's edge.
pub(super) fn text(p: &TextProps, parent: Option<Axis>, cx: &App) -> AnyElement {
    let column = parent != Some(Axis::Horizontal);
    let el = div()
        .when(column, |el| el.w_full())
        .text_color(tone_color(p.tone, cx))
        .when(p.mono, |el| el.font_family(BUNDLED_MONO_FONT))
        .when(p.bold, |el| el.font_weight(gpui::FontWeight::SEMIBOLD))
        .when(p.wrap, |el| el.w_full().min_w_0())
        // Cut at the edge rather than with an ellipsis: GPUI measures
        // ellipsized text at its min-content width inside flex columns.
        .when(!p.wrap, |el| {
            el.min_w_0().overflow_hidden().whitespace_nowrap()
        })
        .child(SharedString::from(p.text.clone()));
    copyable(el, p.text.clone()).into_any_element()
}

pub(super) fn badge(p: &BadgeProps, cx: &App) -> AnyElement {
    let color = tone_color(p.tone, cx);
    let el = div()
        .flex_none()
        .px_1p5()
        .rounded_md()
        .text_size(type_scale(cx).meta)
        .bg(theme(cx).colors.pill)
        .text_color(color)
        .child(SharedString::from(p.text.clone()));
    copyable(el, p.text.clone()).into_any_element()
}

pub(super) fn key_values(pairs: &[(String, String)], cx: &App) -> AnyElement {
    let c = theme(cx).colors.clone();
    div()
        .flex()
        .flex_col()
        .w_full()
        .gap_0p5()
        .children(pairs.iter().map(|(k, v)| {
            div()
                .flex()
                .w_full()
                .gap_3()
                .child(copyable(
                    div()
                        .w(px(180.))
                        .flex_none()
                        .truncate()
                        .text_color(c.text_muted)
                        .child(SharedString::from(k.clone())),
                    k.clone(),
                ))
                .child(copyable(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(BUNDLED_MONO_FONT)
                        .child(SharedString::from(v.clone())),
                    v.clone(),
                ))
        }))
        .into_any_element()
}

pub(super) fn spinner(text: &str, cx: &App) -> AnyElement {
    let c = theme(cx).colors.clone();
    div()
        .flex()
        .items_center()
        .gap_2()
        .p_2()
        .text_color(c.text_muted)
        .child(icon(IconName::Loader).size(px(14.)).color(c.accent))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

pub(super) fn empty_state(title: &str, hint: &str, cx: &App) -> AnyElement {
    let c = theme(cx).colors.clone();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .p_4()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(SharedString::from(title.to_owned())),
        )
        .child(
            div()
                .text_color(c.text_muted)
                .child(SharedString::from(hint.to_owned())),
        )
        .into_any_element()
}

pub(super) fn divider(parent: Option<Axis>, _cx: &App) -> AnyElement {
    match parent {
        Some(Axis::Horizontal) => vertical_divider().into_any_element(),
        _ => hdivider().into_any_element(),
    }
}

/// Wraps a child of a container: `grow` children take the free space,
/// the others keep their size.
fn child_box(grow: bool, el: AnyElement) -> Div {
    let d = div().flex().flex_col().min_w_0().min_h_0();
    if grow { d.flex_1() } else { d.flex_none() }.child(el)
}

impl PanelView {
    fn children_of(
        &mut self,
        snap: &PanelSnapshot,
        node: &Node,
        axis: Option<Axis>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        node.children
            .iter()
            .map(|child| {
                let grow = snap.node(*child).is_some_and(|n| n.grow);
                let el = self.render_node(snap, *child, axis, window, cx);
                child_box(grow, el)
            })
            .collect()
    }

    pub(super) fn render_stack(
        &mut self,
        snap: &PanelSnapshot,
        node: &Node,
        p: StackProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let children = self.children_of(snap, node, Some(p.axis), window, cx);
        let el = div()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .gap(px(4. * f32::from(p.gap)))
            .p(px(4. * f32::from(p.padding)));
        match p.axis {
            Axis::Horizontal => el.flex_row().flex_wrap().items_center(),
            Axis::Vertical => el.flex_col(),
        }
        .children(children)
        .into_any_element()
    }

    pub(super) fn render_scroll(
        &mut self,
        snap: &PanelSnapshot,
        node: &Node,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let children = self.children_of(snap, node, None, window, cx);
        div()
            .id(("scroll", node.id))
            .size_full()
            .overflow_y_scroll()
            .children(children)
            .into_any_element()
    }

    pub(super) fn render_split(
        &mut self,
        snap: &PanelSnapshot,
        node: &Node,
        p: SplitProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let id = node.id;
        let state = self.splits.entry(id).or_default();
        let ratio = state.ratio.unwrap_or(p.ratio).clamp(0.05, 0.95);
        let bounds = state.bounds.clone();
        let dragging = state.dragging;
        let horizontal = p.axis == Axis::Horizontal;
        let mut kids = Vec::with_capacity(2);
        for child in node.children.iter().take(2) {
            let el = self.render_node(snap, *child, None, window, cx);
            kids.push(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(el),
            );
        }
        let mut kids = kids.into_iter();
        let (Some(first), Some(second)) = (kids.next(), kids.next()) else {
            return div().into_any_element();
        };
        let first = if horizontal {
            first.h_full().w(relative(ratio))
        } else {
            first.w_full().h(relative(ratio))
        };
        let handle = div()
            .id(("split-handle", id))
            .flex_none()
            .bg(if dragging { c.accent } else { c.border })
            .hover(|s| s.bg(c.accent))
            .when(horizontal, |el| {
                el.w(px(3.)).h_full().cursor(CursorStyle::ResizeLeftRight)
            })
            .when(!horizontal, |el| {
                el.h(px(3.)).w_full().cursor(CursorStyle::ResizeUpDown)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _, _, cx| {
                    if let Some(s) = view.splits.get_mut(&id) {
                        s.dragging = true;
                        cx.notify();
                    }
                }),
            );
        let record = bounds.clone();
        div()
            .relative()
            .size_full()
            .flex()
            .when(horizontal, |el| el.flex_row())
            .when(!horizontal, |el| el.flex_col())
            .child(
                canvas(move |b, _, _| record.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(first)
            .child(handle)
            .child(second.flex_1())
            .on_mouse_move(cx.listener(move |view, e: &gpui::MouseMoveEvent, _, cx| {
                let Some(s) = view.splits.get_mut(&id) else {
                    return;
                };
                if !s.dragging {
                    return;
                }
                if !e.dragging() {
                    s.dragging = false;
                    cx.notify();
                    return;
                }
                let b = bounds.get();
                let r = if horizontal {
                    (e.position.x - b.origin.x) / b.size.width.max(px(1.))
                } else {
                    (e.position.y - b.origin.y) / b.size.height.max(px(1.))
                };
                s.ratio = Some(r.clamp(0.05, 0.95));
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |view, _, _, cx| {
                    if let Some(s) = view.splits.get_mut(&id)
                        && s.dragging
                    {
                        s.dragging = false;
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }

    pub(super) fn render_tabs(
        &mut self,
        snap: &PanelSnapshot,
        node: &Node,
        p: &TabsProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = theme(cx).colors.clone();
        let id = node.id;
        let count = node.children.len();
        let active =
            (*self.tabs.get(&id).unwrap_or(&p.active) as usize).min(count.saturating_sub(1));
        // Pill tabs (docs/DESIGN.md 6.6) over a hairline.
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(3.))
            .h(px(38.))
            .px(px(6.))
            .border_b_1()
            .border_color(c.border)
            .children(p.titles.iter().enumerate().map(|(i, title)| {
                pill_tab(
                    ("tab", id as usize * 64 + i.min(63)),
                    SharedString::from(title.clone()),
                )
                .panel()
                .active(i == active)
                .on_click(cx.listener(move |view, _, _, cx| {
                    let i = i as u32;
                    if view.tabs.get(&id) != Some(&i) {
                        view.tabs.insert(id, i);
                        cx.notify();
                    }
                    view.send(id, arcode_plugin_host::bindings::ui::EventKind::Tab(i), cx);
                }))
            }));
        let body = node
            .children
            .get(active)
            .map(|child| self.render_node(snap, *child, None, window, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(body),
            )
            .into_any_element()
    }
}
