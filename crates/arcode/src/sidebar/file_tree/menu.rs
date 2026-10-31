//! The tree's context menu (right click, Shift+F10 or the menu key).
//! Entries dispatch the tree's actions, so they show the same key
//! bindings; Up / Down / Enter / Escape work while it is open.

use arcode_ui_kit::Elevated;
use std::path::PathBuf;

use arcode_ui_kit::{IconName, icon, theme};
use gpui::{
    Action, AnyElement, Context, Pixels, Point, Window, anchored, deferred, div, prelude::*, px,
};

use super::*;

/// The open menu.
pub(crate) struct ContextMenu {
    /// The entry the commands apply to (the root for the empty area).
    pub(crate) target: PathBuf,
    /// Window position of the top-left corner.
    position: Point<Pixels>,
    /// Entry highlighted with the keyboard.
    highlighted: Option<usize>,
}

/// One entry, or `None` for a separator.
type Entry = Option<(&'static str, IconName, Box<dyn Action>)>;

impl FileTree {
    /// The entries for the menu's target.
    fn menu_entries(&self, target: &std::path::Path) -> Vec<Entry> {
        let is_root = target == self.root;
        let is_dir = target.is_dir();
        let mut entries: Vec<Entry> = Vec::new();
        if !is_dir {
            entries.push(Some(("Open", IconName::File, Box::new(Open))));
            entries.push(None);
        }
        entries.push(Some(("New File…", IconName::Plus, Box::new(NewFile))));
        entries.push(Some(("New Folder…", IconName::Folder, Box::new(NewFolder))));
        entries.push(None);
        if !is_root {
            entries.push(Some(("Rename…", IconName::Type, Box::new(Rename))));
            entries.push(Some(("Delete", IconName::Trash, Box::new(Delete))));
            entries.push(None);
        }
        entries.push(Some(("Copy Path", IconName::Copy, Box::new(CopyPath))));
        entries.push(Some((
            "Copy Relative Path",
            IconName::Copy,
            Box::new(CopyRelativePath),
        )));
        entries.push(None);
        let reveal = if cfg!(target_os = "macos") {
            "Reveal in Finder"
        } else if cfg!(windows) {
            "Reveal in File Explorer"
        } else {
            "Reveal in File Manager"
        };
        entries.push(Some((
            reveal,
            IconName::ExternalLink,
            Box::new(RevealInFileManager),
        )));
        entries.push(Some((
            "Open in Terminal",
            IconName::Terminal,
            Box::new(OpenInTerminal),
        )));
        entries
    }

    /// Opens the menu for `target` at `position` (window coordinates).
    pub fn open_menu(&mut self, target: PathBuf, position: Point<Pixels>, cx: &mut Context<Self>) {
        if target != self.root {
            self.selected = Some(target.clone());
        }
        self.menu = Some(ContextMenu {
            target,
            position,
            highlighted: None,
        });
        cx.notify();
    }

    /// Whether the context menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Labels of the open menu's entries (`-` for separators).
    pub fn menu_labels(&self) -> Vec<&'static str> {
        self.menu.as_ref().map_or_else(Vec::new, |m| {
            self.menu_entries(&m.target)
                .into_iter()
                .map(|e| e.map_or("-", |(label, _, _)| label))
                .collect()
        })
    }

    pub(super) fn show_context_menu(
        &mut self,
        _: &ShowContextMenu,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.selected.clone().unwrap_or_else(|| self.root.clone());
        // Below the selected row (as laid out in the last frame), else at
        // the top of the list.
        let list = self.list_bounds.get();
        let scroll_y = self.scroll.0.borrow().base_handle.offset().y;
        let row_top = self
            .selected_index()
            .map_or(px(0.), |ix| px((ix + 1) as f32 * ROW_HEIGHT) + scroll_y);
        let y = row_top.clamp(px(0.), list.size.height.max(px(0.)));
        let position = Point::new(list.origin.x + px(24.), list.origin.y + y);
        self.open_menu(target, position, cx);
        if let Some(menu) = &mut self.menu {
            menu.highlighted = Some(0);
        }
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Moves the highlight by `delta` actionable entries (wrapping).
    fn step_menu(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu else { return };
        let entries = self.menu_entries(&menu.target);
        let actionable: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i].is_some())
            .collect();
        let n = actionable.len() as isize;
        if n == 0 {
            return;
        }
        let current = menu
            .highlighted
            .and_then(|h| actionable.iter().position(|&i| i == h));
        let pos = current.map_or(if delta > 0 { 0 } else { n - 1 }, |c| {
            (c as isize + delta).rem_euclid(n)
        });
        self.set_highlight(actionable.get(pos as usize).copied(), cx);
    }

    pub(super) fn menu_prev(&mut self, _: &MenuPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.step_menu(-1, cx);
    }

    pub(super) fn menu_next(&mut self, _: &MenuNext, _: &mut Window, cx: &mut Context<Self>) {
        self.step_menu(1, cx);
    }

    pub(super) fn menu_confirm(
        &mut self,
        _: &MenuConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.menu else { return };
        let action = menu.highlighted.and_then(|ix| {
            self.menu_entries(&menu.target)
                .into_iter()
                .nth(ix)
                .flatten()
                .map(|(_, _, action)| action)
        });
        // The command reads the menu's target and closes the menu itself
        // (dispatching is deferred, so closing here would lose the target).
        match action {
            Some(action) => window.dispatch_action(action, cx),
            None => self.close_menu(cx),
        }
    }

    fn set_highlight(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.menu {
            menu.highlighted = ix;
            cx.notify();
        }
    }

    /// The menu element, drawn on top of everything.
    pub(super) fn render_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let c = theme(cx).colors.clone();
        let focus = self.focus_handle.clone();
        let mut list = div()
            .id("file-tree-menu")
            .min_w(px(220.))
            .py_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .occlude()
            .on_mouse_down_out(cx.listener(|tree, _, _, cx| tree.close_menu(cx)));
        for (ix, entry) in self.menu_entries(&menu.target).into_iter().enumerate() {
            let Some((label, icon_name, action)) = entry else {
                list = list.child(div().h(px(1.)).my_1().bg(c.border));
                continue;
            };
            let binding = window
                .highest_precedence_binding_for_action_in(&*action, &focus)
                .map(|b| crate::commands::binding_label(&b));
            let highlighted = menu.highlighted == Some(ix);
            list = list.child(
                div()
                    .id(("tree-menu-item", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(26.))
                    .px_2()
                    .mx_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(highlighted, |el| el.bg(c.hover))
                    .hover(|s| s.bg(c.hover))
                    .child(icon(icon_name).size(px(14.)).color(c.text_muted))
                    .child(div().flex_1().whitespace_nowrap().child(label))
                    .when_some(binding, |el, keys| {
                        el.child(div().text_xs().text_color(c.text_muted).child(keys))
                    })
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx)
                    }),
            );
        }
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
