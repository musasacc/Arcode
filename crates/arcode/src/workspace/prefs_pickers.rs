//! The theme and font pickers of the workspace, with a live preview that
//! is undone when the picker closes without a choice (or is replaced).

use arcode_ui_kit::{IconName, font_choices, settings};
use gpui::{AppContext, Context, Focusable, Window};

use super::Workspace;
use crate::list_picker::{ListPicker, ListPickerEvent, PickItem};
use crate::prefs::PrefsPick;
use crate::prefs::pickers::{
    FontTarget, ThemeSlot, font_edit, font_items, preview_theme, theme_edit, theme_items, with_font,
};
use crate::prefs::store::write_settings;
use crate::theme_sync::{self, Themes};

impl Workspace {
    /// Opens the theme picker for `slot`, after reading the themes folder
    /// again (new theme files show up without a restart).
    pub fn open_theme_picker(
        &mut self,
        slot: ThemeSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reload = theme_sync::reload_themes(cx);
        cx.spawn_in(window, async move |this, cx| {
            reload.await;
            this.update_in(cx, |ws, window, cx| {
                let registry = &cx.global::<Themes>().0;
                let (items, options) = theme_items(slot, registry, settings(cx));
                let placeholder = match slot {
                    ThemeSlot::Main => "Select Color Theme (arrows preview)",
                    ThemeSlot::Dark => "Select Dark Theme",
                    ThemeSlot::Light => "Select Light Theme",
                };
                let pick = PrefsPick::Theme(slot, options);
                ws.show_prefs_picker(placeholder, IconName::Palette, items, pick, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Opens the font picker for `target` over the system fonts.
    pub fn open_font_picker(
        &mut self,
        target: FontTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text_system = cx.text_system().clone();
        let names = cx.background_spawn(async move { font_choices(text_system.all_font_names()) });
        cx.spawn_in(window, async move |this, cx| {
            let names = names.await;
            this.update_in(cx, |ws, window, cx| {
                let (items, values) = font_items(target, &names, settings(cx));
                let pick = PrefsPick::Font(target, values);
                ws.show_prefs_picker(
                    target.placeholder(),
                    IconName::Type,
                    items,
                    pick,
                    window,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    fn show_prefs_picker(
        &mut self,
        placeholder: &'static str,
        icon: IconName,
        items: Vec<PickItem>,
        pick: PrefsPick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A picker replacing another undoes the other's preview first, so
        // the new picker's base is the saved settings.
        self.close_prefs_picker(true, window, cx);
        self.close_palette(window, cx);
        self.close_finder(window, cx);
        self.project_menu_open = false;
        let picker = cx.new(|cx| ListPicker::new(placeholder, icon, items, cx));
        let sub = cx.subscribe_in(
            &picker,
            window,
            |ws, _, event: &ListPickerEvent, window, cx| {
                ws.prefs_picker_event(event.clone(), window, cx)
            },
        );
        self.subs.set("prefs_picker", sub);
        window.focus(&picker.focus_handle(cx));
        self.prefs.picker = Some((picker, pick));
        cx.notify();
    }

    /// The open theme or font picker.
    pub fn prefs_picker(&self) -> Option<&gpui::Entity<ListPicker>> {
        self.prefs.picker.as_ref().map(|(p, _)| p)
    }

    /// Closes the picker; `restore` undoes its live preview.
    pub(super) fn close_prefs_picker(
        &mut self,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, pick)) = self.prefs.picker.take() else {
            return;
        };
        if restore {
            match pick {
                PrefsPick::Theme(..) => theme_sync::apply(window.appearance(), cx),
                PrefsPick::Font(..) => {
                    if let Some(base) = self.prefs.preview_base.take() {
                        arcode_ui_kit::set_settings(base, cx);
                    }
                }
            }
        }
        self.prefs.preview_base = None;
        self.active_pane().read(cx).focus(window, cx);
        cx.notify();
    }

    fn prefs_picker_event(
        &mut self,
        event: ListPickerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, pick)) = &self.prefs.picker else {
            return;
        };
        match event {
            ListPickerEvent::Highlighted(ix) => match pick {
                PrefsPick::Theme(_, options) => {
                    if let Some(option) = options.get(ix) {
                        let dark = theme_sync::is_dark(window.appearance());
                        let registry = &cx.global::<Themes>().0;
                        let shown = preview_theme(option, registry, settings(cx), dark).clone();
                        theme_sync::preview(&shown, cx);
                    }
                }
                PrefsPick::Font(target, values) => {
                    if let Some(family) = values.get(ix) {
                        let (target, family) = (*target, family.clone());
                        let base = self
                            .prefs
                            .preview_base
                            .get_or_insert_with(|| settings(cx).clone());
                        let preview = with_font(base, target, &family);
                        arcode_ui_kit::set_settings(preview, cx);
                    }
                }
            },
            ListPickerEvent::Picked(ix) => {
                let is_theme = matches!(pick, PrefsPick::Theme(..));
                let edit = match pick {
                    PrefsPick::Theme(slot, options) => {
                        options.get(ix).map(|o| theme_edit(*slot, o))
                    }
                    PrefsPick::Font(target, values) => {
                        values.get(ix).map(|f| font_edit(*target, f))
                    }
                };
                // The preview stays until the written file is applied; a
                // failed write undoes it.
                let base = self.prefs.preview_base.take();
                self.close_prefs_picker(false, window, cx);
                let Some(edit) = edit else {
                    return;
                };
                let write = write_settings(vec![edit], cx);
                cx.spawn_in(window, async move |ws, cx| {
                    let Err(err) = write.await else {
                        return;
                    };
                    ws.update_in(cx, |ws, window, cx| {
                        if is_theme {
                            theme_sync::apply(window.appearance(), cx);
                        } else if let Some(base) = base {
                            arcode_ui_kit::set_settings(base, cx);
                        }
                        ws.show_toast_as(err, super::ToastLevel::Error, cx);
                    })
                    .ok();
                })
                .detach();
            }
            ListPickerEvent::Dismissed => self.close_prefs_picker(true, window, cx),
        }
    }
}
