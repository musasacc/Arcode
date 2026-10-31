//! Drawing the tree: header, rows (with the inline name field), drag and
//! drop, and the context menu.

use std::path::PathBuf;

use arcode_ui_kit::metrics::{
    PANEL_TITLE_H, ROW_RADIUS, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD, icon_size,
};
use arcode_ui_kit::{IconName, icon, icon_button, readout, theme, truncated, type_scale};
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, SharedString, Window, canvas, div,
    prelude::*, px, uniform_list,
};

use super::decor::FileMark;
use super::*;
use crate::file_icons::file_icon_tinted;

/// What is dragged when a row is dragged.
#[derive(Clone)]
pub(crate) struct DraggedEntry {
    pub(crate) path: PathBuf,
    name: SharedString,
}

impl Render for DraggedEntry {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        div()
            .px_2()
            .py_1()
            .rounded(ROW_RADIUS)
            .border_1()
            .border_color(c.border_strong)
            .bg(c.panel_raised())
            .text_size(type_scale(cx).body)
            .text_color(c.text)
            .child(self.name.clone())
    }
}

impl FileTree {
    /// The folder a row accepts drops into: itself, or a file's folder.
    fn drop_dir(&self, ix: usize) -> PathBuf {
        let row = &self.rows[ix];
        if row.is_dir {
            row.path.clone()
        } else {
            row.path
                .parent()
                .map_or_else(|| self.root.clone(), |p| p.to_path_buf())
        }
    }

    fn render_name_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let Some(edit) = &self.edit else {
            return div().into_any_element();
        };
        div()
            .key_context(EDIT_CONTEXT)
            .on_action(cx.listener(Self::confirm_edit))
            .on_action(cx.listener(|tree, _: &Cancel, window, cx| tree.cancel_edit(window, cx)))
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(ROW_HEIGHT - 4.))
            .px_1()
            .flex()
            .items_center()
            .rounded_sm()
            .border_1()
            .border_color(if edit.error.is_some() {
                c.error
            } else {
                c.accent
            })
            .bg(c.panel())
            .child(edit.input.clone())
            .into_any_element()
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let ui = theme(cx);
        let c = ui.colors.clone();
        let row = &self.rows[ix];
        let renaming = matches!(self.editing(), Some(EditKind::Rename(p)) if *p == row.path);
        let selected = !row.placeholder && self.selected.as_ref() == Some(&row.path);
        let mark = if row.is_dir || row.placeholder {
            FileMark::default()
        } else {
            self.decor.file(&row.path)
        };
        let dir_changed = row.is_dir && !row.placeholder && self.decor.dir_changed(&row.path);
        let (icon_name, icon_color) = if row.is_dir {
            let name = if row.expanded {
                IconName::FolderOpen
            } else {
                IconName::Folder
            };
            (name, c.text_muted)
        } else if row.placeholder {
            (IconName::File, c.text_muted)
        } else if selected {
            (file_icon_tinted(&row.name, ui).0, c.accent)
        } else {
            file_icon_tinted(&row.name, ui)
        };
        let chevron = if row.expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let indent = px(LEFT_PAD + row.depth as f32 * INDENT);
        let name: AnyElement = if row.placeholder || renaming {
            self.render_name_field(cx)
        } else {
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(truncated(row.name.clone()))
                .into_any_element()
        };
        let trailing = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .when(mark.errors > 0, |el| {
                el.child(readout(mark.errors.to_string()).color(c.error))
            })
            .when_some(mark.git, |el, git| {
                el.child(crate::sidebar::git_letter(git.letter(), git.color(&c), cx))
            })
            .when(dir_changed, |el| {
                el.child(div().size(px(6.)).rounded_full().bg(c.diff_modified))
            });
        let mut el = div()
            .id(ix)
            .relative()
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(5.))
            .h(px(ROW_HEIGHT))
            .pl(indent)
            .pr(px(8.))
            .rounded(ROW_RADIUS)
            .cursor_pointer()
            .text_color(if mark.errors > 0 { c.error } else { c.text })
            .when(selected, |el| el.bg(c.pill).font_weight(WEIGHT_MEDIUM))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .children((0..row.depth).map(|d| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(LEFT_PAD + d as f32 * INDENT + 7.))
                    .w(px(1.))
                    .bg(c.border)
            }))
            .child(
                div()
                    .size(px(14.))
                    .flex_none()
                    .when(row.is_dir && !row.placeholder, |el| {
                        el.child(icon(chevron).size(px(14.)).color(c.text_muted))
                    }),
            )
            .child(icon(icon_name).size(icon_size::ROW).color(icon_color))
            .child(name)
            .child(trailing);
        if row.placeholder || renaming {
            return crate::sidebar::inset(el).into_any_element();
        }
        let path = row.path.clone();
        let drop_dir = self.drop_dir(ix);
        let dragged = DraggedEntry {
            path: row.path.clone(),
            name: row.name.clone(),
        };
        el = el
            .on_click(cx.listener(move |tree, _, window, cx| {
                window.focus(&tree.focus_handle);
                tree.click(ix, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |tree, event: &MouseDownEvent, window, cx| {
                    window.focus(&tree.focus_handle);
                    tree.open_menu(path.clone(), event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<DraggedEntry>(move |style, _, _, cx| style.bg(theme(cx).colors.hover))
            .on_drop(cx.listener(move |tree, dragged: &DraggedEntry, _, cx| {
                tree.move_entry(&dragged.path.clone(), &drop_dir.clone(), cx);
            }));
        crate::sidebar::inset(el).into_any_element()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let button = |id: &'static str, glyph: IconName, tip: &'static str| {
            icon_button(id, glyph).size(px(24.)).tooltip(tip)
        };
        let buttons = div()
            .flex()
            .flex_none()
            .items_center()
            .ml(px(4.))
            .child(
                button("tree-new-file", IconName::Plus, "New File…")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewFile), cx)),
            )
            .child(
                button("tree-new-folder", IconName::Folder, "New Folder…")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewFolder), cx)),
            )
            .child(
                button("tree-refresh", IconName::Refresh, "Refresh")
                    .on_click(cx.listener(|tree, _, _, cx| tree.refresh(cx))),
            )
            .child(
                button(
                    "tree-collapse",
                    IconName::ChevronsDownUp,
                    "Collapse folders",
                )
                .on_click(cx.listener(|tree, _, _, cx| tree.collapse_all(cx))),
            );
        // `panel_title`'s metrics, but the project name can shrink to
        // nothing (its gap lives inside it) and the buttons sit edge to
        // edge, so all four fit at the narrowest sidebar.
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(PANEL_TITLE_H)
            .pl(px(14.))
            .pr(px(8.))
            .child(
                div()
                    .flex_none()
                    .text_size(scale.title)
                    .font_weight(WEIGHT_SEMIBOLD)
                    .text_color(c.text)
                    .child("Files"),
            )
            .child(
                div().flex_1().min_w_0().overflow_hidden().child(
                    div()
                        .pl(px(8.))
                        .text_size(scale.meta)
                        .text_color(c.text_muted)
                        .child(truncated(self.title.clone())),
                ),
            )
            .child(buttons)
    }
}

impl Render for FileTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let header = self.render_header(cx);
        let list = uniform_list(
            "file-tree",
            self.rows.len(),
            cx.processor(|tree, range: std::ops::Range<usize>, _, cx| {
                range.map(|ix| tree.render_row(ix, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full()
        .text_size(type_scale(cx).body);
        let empty = self.rows.is_empty().then(|| {
            div()
                .px(px(14.))
                .py_2()
                .text_size(type_scale(cx).body)
                .text_color(c.text_muted)
                .child("This folder is empty.")
        });
        let error = self.edit.as_ref().and_then(|e| e.error.clone()).map(|err| {
            div()
                .mx_2()
                .px_2()
                .py_1()
                .rounded_sm()
                .border_1()
                .border_color(c.error)
                .bg(c.panel_raised())
                .text_size(type_scale(cx).meta)
                .text_color(c.error)
                .child(err)
        });
        let bounds = self.list_bounds.clone();
        let root = self.root.clone();
        let root_drop = self.root.clone();
        let body = div()
            .id("file-tree-body")
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(empty)
            .child(list)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |tree, event: &MouseDownEvent, window, cx| {
                    window.focus(&tree.focus_handle);
                    tree.open_menu(root.clone(), event.position, cx);
                }),
            )
            .on_drop(cx.listener(move |tree, dragged: &DraggedEntry, _, cx| {
                tree.move_entry(&dragged.path.clone(), &root_drop.clone(), cx);
            }));
        let menu = self.render_menu(window, cx);
        div()
            .key_context(if self.menu.is_some() {
                "FileTree menu"
            } else {
                CONTEXT
            })
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::menu_prev))
            .on_action(cx.listener(Self::menu_next))
            .on_action(cx.listener(Self::menu_confirm))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::expand))
            .on_action(cx.listener(Self::collapse))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::copy_path))
            .on_action(cx.listener(Self::copy_relative_path))
            .on_action(cx.listener(Self::reveal_in_file_manager))
            .on_action(cx.listener(Self::open_in_terminal))
            .on_action(cx.listener(Self::show_context_menu))
            .on_action(cx.listener(|tree, _: &Cancel, _, cx| tree.close_menu(cx)))
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .children(error)
            .child(body)
            .children(menu)
    }
}
