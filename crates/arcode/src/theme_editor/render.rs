//! The page: a header with the theme being edited and the actions, the
//! token list on the left and the live preview on the right, plus the
//! theme list, the colour picker and the "Save as" dialog over it.

use arcode_ui_kit::metrics::{WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, list_row, popover, theme};
use gpui::{
    AnyElement, ClickEvent, Context, Div, MouseButton, SharedString, Window, anchored, canvas,
    deferred, div, point, prelude::*, px,
};

use super::{ThemeEditor, preview};
use crate::theme_sync::Themes;

/// Width of the preview column.
const PREVIEW_W: f32 = 380.;
/// Below this page width the preview goes under the token list.
pub(super) const STACKED_WIDTH: f32 = 880.;

impl ThemeEditor {
    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let edit = &self.edit;
        let builtin = edit.is_builtin();
        let dirty = edit.is_dirty();
        let name = edit.start().name.clone();
        let subtitle = if builtin {
            format!("{name} is built in: save your changes as a new theme.")
        } else {
            format!("Saving writes {name} to its file in your themes folder.")
        };
        let mut picker = button("theme-editor-pick", name.clone())
            .icon(IconName::Palette)
            .style(ButtonStyle::Secondary);
        // No tooltip over the open theme list.
        if self.theme_menu.is_none() {
            picker = picker.tooltip("Edit another theme");
        }
        let picker = picker.on_click(cx.listener(|editor, e: &ClickEvent, _, cx| {
            editor.close_picker(cx);
            editor.theme_menu = match editor.theme_menu {
                Some(_) => None,
                None => Some(e.position()),
            };
            cx.notify();
        }));
        let discard = button("theme-editor-discard", "Discard")
            .icon(IconName::RotateCcw)
            .style(ButtonStyle::Ghost)
            .disabled(!dirty)
            .tooltip("Drop the changes and show the theme in use")
            .on_click(cx.listener(|editor, _, window, cx| editor.discard(window, cx)));
        let open_file = button("theme-editor-open-file", "Open File")
            .icon(IconName::FileText)
            .style(ButtonStyle::Ghost)
            .disabled(builtin)
            .tooltip(if builtin {
                "Built-in themes have no file; save a copy first"
            } else {
                "Open the theme's TOML file in an editor tab"
            })
            .on_click(cx.listener(|editor, _, _, cx| editor.open_theme_file(cx)));
        let save_as = button("theme-editor-save-as", "Save As…")
            .style(if builtin {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            })
            .on_click(
                cx.listener(|editor, _, window, cx| editor.open_name_prompt(false, window, cx)),
            );
        let save = (!builtin).then(|| {
            button("theme-editor-save", "Save")
                .icon(IconName::Save)
                .style(ButtonStyle::Primary)
                .disabled(!dirty)
                .on_click(cx.listener(|editor, _, window, cx| editor.save(false, window, cx)))
        });
        // The user's own themes can be renamed and deleted; built-in ones
        // cannot.
        let manage = (!builtin).then(|| {
            let (rename, delete) = (name.clone(), name.clone());
            [
                button("theme-editor-rename", "Rename…")
                    .style(ButtonStyle::Ghost)
                    .tooltip("Give this theme another name (its file is renamed too)")
                    .on_click(move |_, window, cx| {
                        let name = rename.clone().into();
                        window.dispatch_action(Box::new(super::RenameTheme { name }), cx)
                    }),
                button("theme-editor-delete", "Delete…")
                    .icon(IconName::Trash)
                    .style(ButtonStyle::Ghost)
                    .tooltip("Delete this theme and its file")
                    .on_click(move |_, window, cx| {
                        let name = delete.clone().into();
                        window.dispatch_action(Box::new(super::DeleteTheme { name }), cx)
                    }),
            ]
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .pb(px(12.))
            .border_b_1()
            .border_color(c.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(220.))
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(scale.heading)
                            .font_weight(WEIGHT_SEMIBOLD)
                            .child("Theme Editor"),
                    )
                    .child(
                        div()
                            .text_size(scale.meta)
                            .text_color(c.text_muted)
                            .child(SharedString::from(subtitle)),
                    ),
            )
            .child(picker)
            .children(manage.into_iter().flatten())
            .child(discard)
            .child(open_file)
            .child(save_as)
            .children(save)
    }

    fn render_theme_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = self.theme_menu?;
        let registry = cx.global::<Themes>().0.clone();
        let current = self.edit.start().name.clone();
        let mut list = popover(cx)
            .id("theme-editor-menu")
            .min_w(px(220.))
            .occlude()
            .on_mouse_down_out(cx.listener(|editor, _, _, cx| {
                editor.theme_menu = None;
                cx.notify();
            }));
        for (ix, theme) in registry.themes().iter().enumerate() {
            let selected = theme.name == current;
            let picked = theme.clone();
            list = list.child(
                list_row(("theme-editor-menu-item", ix), selected, cx)
                    .mx(px(0.))
                    .h(px(26.))
                    .child(div().flex_1().whitespace_nowrap().child(theme.name.clone()))
                    .when(selected, |el| el.child(icon(IconName::Check).size(px(14.))))
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.start_from(picked.clone(), window, cx)
                    })),
            );
        }
        let border = theme(cx).colors.border;
        list = list.child(div().my(px(4.)).h(px(1.)).bg(border)).child(
            list_row("theme-editor-menu-new", false, cx)
                .mx(px(0.))
                .h(px(26.))
                .child(icon(IconName::Plus).size(px(14.)))
                .child(div().flex_1().whitespace_nowrap().child("New Theme…"))
                .on_click(cx.listener(|editor, _, window, cx| {
                    editor.theme_menu = None;
                    cx.notify();
                    window.dispatch_action(Box::new(super::NewTheme), cx)
                })),
        );
        Some(
            deferred(
                anchored()
                    .position(position + point(px(0.), px(12.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_name_prompt(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (prompt, _) = self.name_prompt.as_ref()?;
        let dim = if theme(cx).is_dark {
            theme(cx).colors.background.opacity(0.6)
        } else {
            theme(cx).colors.text.opacity(0.3)
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_start()
                .justify_center()
                .pt(px(80.))
                .bg(dim)
                .child(
                    div()
                        .id("theme-editor-name-dialog")
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(prompt.clone()),
                ),
        )
    }
}

impl ThemeEditor {
    /// Records whether the page is too narrow for the list and the preview
    /// side by side; redraws the page when that changed (only then, so an
    /// idle page draws nothing).
    fn measure_width(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = self.measured_stacked.clone();
        let shown = self.stacked;
        let page = cx.entity_id();
        canvas(
            move |bounds, _, cx| {
                let stacked = bounds.size.width < px(STACKED_WIDTH);
                measured.set(stacked);
                if stacked != shown {
                    // Not while drawing: a notification then is dropped.
                    cx.defer(move |cx| cx.notify(page));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Whether the preview is shown under the token list (a narrow page).
    pub fn is_stacked(&self) -> bool {
        self.stacked
    }
}

impl Render for ThemeEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stacked = self.measured_stacked.get();
        let c = theme(cx).colors.clone();
        let header = self.render_header(cx);
        let groups = self.render_groups(window, cx);
        let hints = self.edit.contrast_hints();
        let side = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .pt(px(20.))
            .child(preview::code_sample(cx))
            .child(preview::terminal_sample(cx))
            .child(preview::contrast_list(&hints, cx));
        let list = div()
            .id("theme-editor-scroll")
            .track_scroll(&self.scroll)
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .pr(px(8.))
            .pb(px(36.));
        // Side by side the list scrolls and the preview stays in view beside
        // it; on a narrow page both scroll as one column, the preview last.
        let body = if self.stacked {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(list.child(groups).child(side.max_w(px(PREVIEW_W + 160.))))
        } else {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .gap_x(px(28.))
                .child(list.child(groups))
                .child(
                    div()
                        .id("theme-editor-preview")
                        .flex_none()
                        .w(px(PREVIEW_W))
                        .h_full()
                        .overflow_y_scroll()
                        .pb(px(24.))
                        .child(side),
                )
        };
        div()
            .id("theme-editor")
            .key_context("ThemeEditor")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .px(px(28.))
            .pt(px(22.))
            .bg(c.panel())
            .text_color(c.text)
            .child(self.measure_width(cx))
            .child(header)
            .child(body)
            .children(self.render_theme_menu(cx))
            .children(self.render_picker(window, cx))
            .children(self.render_name_prompt(cx))
    }
}
