//! The theme editor: a page tab that edits a copy of a theme and shows it
//! live in the whole app (in memory; nothing is written until a save).
//! Built-in themes are saved as a new user theme ("Save as…"); a user
//! theme is saved in its own file. Discarding, or closing the tab without
//! saving, shows the configured theme again.

mod manage;
pub mod new_theme;
mod picker;
mod preview;
mod render;
mod rows;
mod save;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use arcode_config::theme::{Hsv, ThemeEdit, TokenRef};
use arcode_config::{Color, Theme};
use arcode_ui_kit::settings;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Point,
    PromptLevel, ScrollHandle, SharedString, Subscription, Window,
};

use crate::text_field::{TextChanged, TextField};
use crate::text_prompt::TextPrompt;
use crate::theme_sync::{self, Themes};

pub use manage::{
    DeleteTheme, EditCurrentTheme, NewTheme, RenameTheme, commands, delete_edits, rename_edits,
};
pub use save::selection_edits;
pub(crate) use save::themes_dir;

/// Key of the page tab.
pub const PAGE_KEY: &str = "theme-editor";

/// Requests the page passes to the workspace.
#[derive(Clone, Debug, PartialEq)]
pub enum ThemeEditorEvent {
    /// Close the page (the user answered the unsaved-changes question).
    Close,
    /// Open a theme file in an editor tab.
    OpenFile(PathBuf),
    /// Show a message.
    Toast(SharedString),
}

/// The open colour picker.
pub(crate) struct PickerState {
    /// The token it edits.
    pub token: TokenRef,
    /// Its colour as hue, saturation and value; kept apart from the
    /// token's colour so the hue survives greys while dragging.
    pub hsv: Hsv,
    /// Where it opened (window coordinates).
    pub position: Point<Pixels>,
    /// Its hex field.
    pub hex: Entity<TextField>,
    /// Focused while it is open (its hex field takes the focus on a click).
    pub focus: FocusHandle,
    /// The area being dragged.
    pub drag: Option<picker::Area>,
    _sub: Subscription,
}

/// The page view.
pub struct ThemeEditor {
    focus: FocusHandle,
    edit: ThemeEdit,
    /// The hex field of the row being typed into, and its token.
    hex: Entity<TextField>,
    hex_token: Option<TokenRef>,
    /// What was typed is not a colour.
    hex_error: bool,
    picker: Option<PickerState>,
    /// The theme list under the theme button, open at a position.
    theme_menu: Option<Point<Pixels>>,
    /// The "Save as" name dialog, and whether the page closes after it.
    name_prompt: Option<(Entity<TextPrompt>, bool)>,
    /// A live apply waits for the next frame (picker drags).
    apply_scheduled: bool,
    /// How many times the edited theme was applied (tests, measurements).
    applies: usize,
    scroll: ScrollHandle,
    /// The page is narrow: the preview is under the token list. Measured
    /// while drawing; a change redraws the page once.
    stacked: bool,
    measured_stacked: Rc<Cell<bool>>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ThemeEditorEvent> for ThemeEditor {}

impl Focusable for ThemeEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The configured theme for the window's appearance.
pub(crate) fn configured_theme(window: &Window, cx: &App) -> Theme {
    let ui = &settings(cx).ui;
    cx.global::<Themes>()
        .0
        .resolve(
            &ui.theme,
            &ui.dark_theme,
            &ui.light_theme,
            theme_sync::is_dark(window.appearance()),
        )
        .clone()
}

impl ThemeEditor {
    /// A page editing `theme`, shown live at once.
    pub fn new(theme: Theme, cx: &mut Context<Self>) -> Self {
        let hex = cx.new(|cx| TextField::new("#rrggbb", cx));
        let subs = vec![cx.subscribe(&hex, |editor, _, _: &TextChanged, cx| {
            if editor.hex_error {
                editor.hex_error = false;
                cx.notify();
            }
        })];
        let mut editor = Self {
            focus: cx.focus_handle(),
            edit: ThemeEdit::new(theme),
            hex,
            hex_token: None,
            hex_error: false,
            picker: None,
            theme_menu: None,
            name_prompt: None,
            apply_scheduled: false,
            applies: 0,
            scroll: ScrollHandle::new(),
            stacked: false,
            measured_stacked: Rc::default(),
            _subs: subs,
        };
        editor.apply_now(cx);
        editor
    }

    /// The edit state.
    pub fn edit(&self) -> &ThemeEdit {
        &self.edit
    }

    /// Whether there are changes no file holds yet.
    pub fn has_unsaved_changes(&self) -> bool {
        self.edit.is_dirty()
    }

    /// How many times the edited theme was applied to the app.
    pub fn applies(&self) -> usize {
        self.applies
    }

    /// Sets a colour and shows it at once (a typed hex value).
    pub fn set_color(&mut self, token: &TokenRef, color: Color, cx: &mut Context<Self>) {
        if self.edit.set(token, color) {
            self.apply_now(cx);
        }
        cx.notify();
    }

    /// Sets a colour and shows it on the next frame: a drag sends many
    /// moves per frame, and the app is re-themed once for all of them.
    pub(crate) fn set_color_coalesced(
        &mut self,
        token: &TokenRef,
        color: Color,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.edit.set(token, color) {
            return;
        }
        if !self.apply_scheduled {
            self.apply_scheduled = true;
            let this = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                this.update(cx, |editor, cx| {
                    if editor.apply_scheduled {
                        editor.apply_now(cx);
                    }
                })
                .ok();
            });
        }
        cx.notify();
    }

    /// Shows the edited theme in every window now.
    pub fn apply_now(&mut self, cx: &mut Context<Self>) {
        self.apply_scheduled = false;
        self.applies += 1;
        let started = std::time::Instant::now();
        theme_sync::show_edited(self.edit.working(), cx);
        log::debug!(
            "theme editor: applied #{} in {:?}",
            self.applies,
            started.elapsed()
        );
        cx.notify();
    }

    /// Makes a derived key follow its sources again, or stops it.
    pub fn set_follow(&mut self, key: &str, follow: bool, cx: &mut Context<Self>) {
        if follow {
            self.edit.follow_derived(key);
        } else {
            self.edit.override_derived(key);
        }
        self.apply_now(cx);
    }

    /// Edits `theme` from now on, asking first when edits would be lost.
    pub fn start_from(&mut self, theme: Theme, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_menu = None;
        self.close_picker(cx);
        if !self.edit.is_dirty() {
            self.replace_edit(theme, cx);
            return;
        }
        let message = format!("Discard your changes to {}?", self.edit.start().name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("The changes have not been saved."),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update(cx, |editor, cx| editor.replace_edit(theme, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn replace_edit(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.edit = ThemeEdit::new(theme);
        self.hex_token = None;
        self.apply_now(cx);
    }

    /// Drops the edits and shows the theme in use before editing again.
    pub fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(cx);
        self.hex_token = None;
        self.edit = ThemeEdit::new(configured_theme(window, cx));
        self.apply_scheduled = false;
        theme_sync::restore(window.appearance(), cx);
        cx.notify();
    }

    /// The themes folder was read again (a theme file was saved): an
    /// unchanged edit starts over from the new colours of its theme, edits
    /// are shown again over the reloaded theme.
    pub fn themes_reloaded(&mut self, cx: &mut Context<Self>) {
        if self.edit.is_dirty() {
            self.apply_now(cx);
            return;
        }
        let name = self.edit.start().name.clone();
        if let Some(theme) = cx.global::<Themes>().0.get(&name).cloned() {
            self.replace_edit(theme, cx);
        }
    }

    /// The close guard: `true` when the page may close now (the
    /// configured theme is shown again); otherwise asks whether to save.
    /// Either way [`ThemeEditorEvent::Close`] follows once the page closes,
    /// so the workspace forgets it.
    pub fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.close_picker(cx);
        if !self.edit.is_dirty() {
            self.apply_scheduled = false;
            theme_sync::restore(window.appearance(), cx);
            // Delivered after the pane removed the tab.
            cx.emit(ThemeEditorEvent::Close);
            return true;
        }
        let save = if self.edit.is_builtin() {
            "Save As…"
        } else {
            "Save"
        };
        let message = format!("Save your changes to {}?", self.edit.start().name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes will be lost if you don't save them."),
            &[save, "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(choice) = answer.await else { return };
            this.update_in(cx, |editor, window, cx| match choice {
                0 => editor.save(true, window, cx),
                1 => {
                    editor.edit.discard();
                    editor.apply_scheduled = false;
                    theme_sync::restore(window.appearance(), cx);
                    cx.emit(ThemeEditorEvent::Close);
                }
                _ => {}
            })
            .ok();
        })
        .detach();
        false
    }

    /// Opens the colour picker for `token` at `position`.
    pub(crate) fn open_picker(
        &mut self,
        token: TokenRef,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(color) = self.edit.working().token_color(&token) else {
            return;
        };
        self.theme_menu = None;
        self.hex_token = None;
        let hex = cx.new(|cx| {
            let mut field = TextField::new("#rrggbb", cx);
            field.set_text(&color.to_hex(), cx);
            field
        });
        let sub = cx.subscribe_in(
            &hex,
            window,
            |editor, field, _: &TextChanged, window, cx| {
                let text = field.read(cx).text().to_owned();
                editor.picker_hex_typed(&text, window, cx);
            },
        );
        // Keys go to the picker at once: Escape and Enter close it.
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.picker = Some(PickerState {
            token,
            hsv: Hsv::from_color(color),
            position,
            hex,
            focus,
            drag: None,
            _sub: sub,
        });
        cx.notify();
    }

    /// Closes the colour picker; a pending drag apply is done now.
    pub(crate) fn close_picker(&mut self, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            if self.apply_scheduled {
                self.apply_now(cx);
            }
            cx.notify();
        }
    }

    /// Closes the colour picker from the keyboard; the page keeps the
    /// focus.
    pub(crate) fn dismiss_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had_focus = self
            .picker
            .as_ref()
            .is_some_and(|p| p.focus.contains_focused(window, cx));
        self.close_picker(cx);
        if had_focus {
            window.focus(&self.focus);
        }
    }

    /// The open picker (tests).
    #[cfg(test)]
    pub(crate) fn picker_token(&self) -> Option<&TokenRef> {
        self.picker.as_ref().map(|p| &p.token)
    }
}
