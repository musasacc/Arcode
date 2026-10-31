//! A small popup list shown as an editor popover: the code action menu and
//! the context menu. The editor keeps focus; the popover's key handler
//! moves the selection (up/down/home/end), picks (Enter) or closes
//! (Escape). Any other key closes the menu and reaches the editor, so
//! typing simply continues.

use std::rc::Rc;

use arcode_buffer::Point;
use arcode_ui_kit::{settings, theme, truncated};
use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, Keystroke, MouseButton,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::*, px,
};

use crate::Editor;
use crate::ext::EditorPopover;

/// Height of an item row.
const ROW_HEIGHT: f32 = 24.;
/// Tallest the list gets before it scrolls.
const MAX_HEIGHT: f32 = 380.;

/// One row of a popup menu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MenuRow {
    /// A group title (not selectable).
    Header(SharedString),
    /// A thin line between groups.
    Separator,
    /// A selectable entry.
    Item(MenuItem),
}

/// A menu entry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuItem {
    pub label: SharedString,
    /// Shown muted on the right (a key binding, a reason it is disabled).
    pub detail: Option<SharedString>,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        MenuItem {
            label: label.into(),
            detail: None,
            enabled: true,
        }
    }

    pub fn detail(mut self, detail: Option<impl Into<SharedString>>) -> Self {
        self.detail = detail.map(Into::into);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Called with the index of the picked row (into the rows given to
/// [`Editor::show_menu`]), after the menu closed.
pub(crate) type OnPick = Rc<dyn Fn(&Entity<Editor>, usize, &mut Window, &mut App)>;

/// What a keystroke did to the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKey {
    /// The selection moved (or the key was swallowed).
    Moved,
    /// Enter on this row.
    Pick(usize),
    /// Escape.
    Close,
    /// Not a menu key: close and let the editor have it.
    Other,
}

/// The menu's state and view.
pub(crate) struct PopupMenu {
    editor: WeakEntity<Editor>,
    key: SharedString,
    rows: Vec<MenuRow>,
    selected: Option<usize>,
    on_pick: OnPick,
    scroll: ScrollHandle,
}

impl PopupMenu {
    pub fn new(
        editor: WeakEntity<Editor>,
        key: SharedString,
        rows: Vec<MenuRow>,
        on_pick: OnPick,
    ) -> Self {
        let mut menu = PopupMenu {
            editor,
            key,
            rows,
            selected: None,
            on_pick,
            scroll: ScrollHandle::new(),
        };
        menu.selected = menu.next_from(0, 1);
        menu
    }

    fn selectable(&self, ix: usize) -> bool {
        matches!(self.rows.get(ix), Some(MenuRow::Item(item)) if item.enabled)
    }

    /// The first selectable row at or after `from` going in `dir` (+1 or
    /// -1), wrapping around.
    fn next_from(&self, from: usize, dir: isize) -> Option<usize> {
        let n = self.rows.len() as isize;
        (0..n)
            .map(|step| (from as isize + step * dir).rem_euclid(n.max(1)) as usize)
            .find(|&ix| self.selectable(ix))
    }

    /// Labels of the rows: headers in brackets, disabled items in
    /// parentheses, the selected item prefixed with `> `.
    pub fn labels(&self) -> Vec<String> {
        self.rows
            .iter()
            .enumerate()
            .map(|(ix, row)| match row {
                MenuRow::Header(h) => format!("[{h}]"),
                MenuRow::Separator => "-".into(),
                MenuRow::Item(item) if self.selected == Some(ix) => format!("> {}", item.label),
                MenuRow::Item(item) if item.enabled => item.label.to_string(),
                MenuRow::Item(item) => format!("({})", item.label),
            })
            .collect()
    }

    fn select(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        if ix != self.selected {
            self.selected = ix;
            if let Some(ix) = ix {
                self.scroll.scroll_to_item(ix);
            }
            cx.notify();
        }
    }

    /// Handles a keystroke.
    pub fn handle_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> MenuKey {
        let m = &ks.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return MenuKey::Other;
        }
        let last = self.rows.len().saturating_sub(1);
        let target = match ks.key.as_str() {
            "escape" => return MenuKey::Close,
            "enter" | "tab" => {
                return self.selected.map_or(MenuKey::Close, MenuKey::Pick);
            }
            "down" => self.next_from(self.selected.map_or(0, |s| s + 1), 1),
            "up" => self.next_from(self.selected.map_or(last, |s| s + last), -1),
            "home" | "pageup" => self.next_from(0, 1),
            "end" | "pagedown" => self.next_from(last, -1),
            _ => return MenuKey::Other,
        };
        self.select(target, cx);
        MenuKey::Moved
    }

    fn pick(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let key = self.key.clone();
        editor.update(cx, |e, cx| e.hide_popover(&key, cx));
        let on_pick = self.on_pick.clone();
        on_pick(&editor, ix, window, cx);
    }

    fn render_row(&self, ix: usize, row: &MenuRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = theme(cx).colors.clone();
        let small = px(settings(cx).ui.font_size - 1.);
        match row {
            MenuRow::Header(title) => div()
                .px_2()
                .pt_1()
                .pb_0p5()
                .text_size(small)
                .text_color(colors.text_muted)
                .child(title.clone())
                .into_any_element(),
            MenuRow::Separator => div()
                .my_1()
                .mx_1()
                .h(px(1.))
                .bg(colors.border)
                .into_any_element(),
            MenuRow::Item(item) => {
                let selected = self.selected == Some(ix);
                let enabled = item.enabled;
                div()
                    .id(("menu-row", ix))
                    .h(px(ROW_HEIGHT))
                    .mx_1()
                    .px_2()
                    .rounded_sm()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .when(selected, |d| d.bg(colors.selection))
                    .when(enabled && !selected, |d| d.hover(|d| d.bg(colors.hover)))
                    .when(enabled, |d| d.cursor_pointer())
                    .text_color(if enabled {
                        colors.text
                    } else {
                        colors.text_muted
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |menu, _, window, cx| {
                            cx.stop_propagation();
                            if menu.selectable(ix) {
                                menu.pick(ix, window, cx);
                            }
                        }),
                    )
                    .child(div().flex_none().child(item.label.clone()))
                    .child(div().flex_1())
                    .children(item.detail.clone().map(|detail| {
                        div()
                            .flex_shrink()
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(small)
                            .text_color(colors.text_muted)
                            .child(truncated(detail))
                    }))
                    .into_any_element()
            }
        }
    }
}

impl Render for PopupMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors.clone();
        let ui = &settings(cx).ui;
        let font_size = px(ui.font_size);
        let family = ui.font_family.clone();
        let rows: Vec<_> = self
            .rows
            .clone()
            .iter()
            .enumerate()
            .map(|(ix, row)| self.render_row(ix, row, cx))
            .collect();
        div()
            .id("popup-menu")
            .occlude()
            .min_w(px(220.))
            .max_w(px(520.))
            .max_h(px(MAX_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .py_1()
            .flex()
            .flex_col()
            .bg(colors.surface_raised)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_md()
            .text_size(font_size)
            .text_color(colors.text)
            .when_some(family, |d, f| d.font_family(SharedString::from(f)))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .children(rows)
    }
}

impl Editor {
    /// Shows a popup menu under popover `key` below `anchor`. Picking a row
    /// closes the menu, then calls `on_pick`.
    pub(crate) fn show_menu(
        &mut self,
        key: &'static str,
        anchor: Point,
        rows: Vec<MenuRow>,
        on_pick: OnPick,
        cx: &mut Context<Self>,
    ) -> Entity<PopupMenu> {
        self.hide_hover();
        let weak = cx.entity().downgrade();
        let view = cx.new(|_| PopupMenu::new(weak, key.into(), rows, on_pick.clone()));
        let menu = view.clone();
        let popover = EditorPopover::new(anchor, view.clone().into()).on_key(
            move |editor, ks, window, cx| match menu.update(cx, |m, cx| m.handle_key(ks, cx)) {
                MenuKey::Moved => true,
                MenuKey::Close => {
                    editor.update(cx, |e, cx| e.hide_popover(key, cx));
                    true
                }
                MenuKey::Pick(ix) => {
                    editor.update(cx, |e, cx| e.hide_popover(key, cx));
                    on_pick(editor, ix, window, cx);
                    true
                }
                MenuKey::Other => {
                    editor.update(cx, |e, cx| e.hide_popover(key, cx));
                    false
                }
            },
        );
        self.show_popover(key, popover, cx);
        view
    }

    /// The menu shown under popover `key`, if any.
    pub(crate) fn menu_view(&self, key: &str) -> Option<Entity<PopupMenu>> {
        self.popover(key)?
            .content
            .clone()
            .downcast::<PopupMenu>()
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<MenuRow> {
        vec![
            MenuRow::Header("Quick Fix".into()),
            MenuRow::Item(MenuItem::new("a")),
            MenuRow::Item(MenuItem::new("b").enabled(false)),
            MenuRow::Separator,
            MenuRow::Item(MenuItem::new("c")),
        ]
    }

    fn menu(rows: Vec<MenuRow>) -> PopupMenu {
        PopupMenu::new(
            WeakEntity::new_invalid(),
            "m".into(),
            rows,
            Rc::new(|_, _, _, _| {}),
        )
    }

    #[test]
    fn selection_skips_headers_and_disabled_rows() {
        let m = menu(rows());
        assert_eq!(m.selected, Some(1));
        assert_eq!(m.next_from(2, 1), Some(4));
        assert_eq!(m.next_from(5, 1), Some(1), "wraps");
        assert_eq!(m.next_from(3, -1), Some(1));
        assert_eq!(m.labels(), ["[Quick Fix]", "> a", "(b)", "-", "c"]);
        let empty = menu(vec![MenuRow::Header("x".into())]);
        assert_eq!(empty.selected, None);
        assert_eq!(menu(Vec::new()).selected, None);
    }
}
