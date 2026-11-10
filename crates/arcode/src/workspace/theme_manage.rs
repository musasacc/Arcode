//! New Theme in the workspace: the dialog, writing the new theme file from
//! the chosen theme, making it the theme in use and opening the theme
//! editor on it. The New Theme and rename dialogs are drawn here; renaming
//! and deleting are in `theme_remove.rs`.

use std::path::PathBuf;

use arcode_config::Theme;
use arcode_config::theme::{
    SaveError, SaveTarget, ThemeEdit, save_user_theme, validate_theme_name,
};
use arcode_ui_kit::settings;
use gpui::prelude::*;
use gpui::{AppContext, Context, Div, Focusable, MouseButton, PromptLevel, Stateful, Window, div};

use super::Workspace;
use super::overlays::fade_in;
use crate::text_prompt::TextPrompt;
use crate::theme_editor::new_theme::{NewThemeDialog, NewThemeEvent, base_themes, suggested_name};
use crate::theme_editor::{configured_theme, selection_edits, themes_dir};
use crate::theme_sync::{self, Themes};

/// A file name for messages.
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Workspace {
    /// Shows the New Theme dialog, starting from the theme in use.
    pub(crate) fn open_new_theme_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        self.close_prefs_picker(true, window, cx);
        let registry = cx.global::<Themes>().0.clone();
        let in_use = configured_theme(window, cx).name;
        let name = suggested_name(&in_use, &registry);
        let bases = base_themes(&registry);
        let dialog = cx.new(|cx| NewThemeDialog::new(bases, &in_use, &name, cx));
        let sub = cx.subscribe_in(
            &dialog,
            window,
            |ws, _, event: &NewThemeEvent, window, cx| match event {
                NewThemeEvent::Create { name, base } => {
                    ws.create_theme(name, base, None, window, cx)
                }
                NewThemeEvent::Dismissed => ws.close_theme_dialogs(window, cx),
            },
        );
        self.subs.set("new_theme", sub);
        window.focus(&dialog.focus_handle(cx));
        self.prefs.new_theme = Some(dialog);
        cx.notify();
    }

    /// The New Theme dialog, while shown.
    pub fn new_theme_dialog(&self) -> Option<&gpui::Entity<NewThemeDialog>> {
        self.prefs.new_theme.as_ref()
    }

    /// The rename dialog, while shown.
    pub fn rename_theme_dialog(&self) -> Option<&gpui::Entity<TextPrompt>> {
        self.prefs.theme_prompt.as_ref().map(|(prompt, _)| prompt)
    }

    pub(super) fn close_theme_dialogs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.new_theme = None;
        self.prefs.theme_prompt = None;
        self.active_pane().read(cx).focus(window, cx);
        cx.notify();
    }

    /// Says why a theme cannot be made or renamed: in the dialog that is
    /// open, else as a message.
    pub(super) fn theme_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.prefs.new_theme {
            dialog.update(cx, |d, cx| d.set_error(message, cx));
        } else if let Some((prompt, _)) = &self.prefs.theme_prompt {
            prompt.update(cx, |p, cx| p.set_hint(message, cx));
        } else {
            self.show_toast_as(message, super::ToastLevel::Error, cx);
        }
    }

    /// Makes the theme `name` from the theme `base`: writes
    /// `<name>.toml` into the themes folder, makes it the theme in use and
    /// opens the theme editor on it. `target` is `None` until the user
    /// agreed to replace a theme or a file of that name.
    pub(crate) fn create_theme(
        &mut self,
        name: &str,
        base: &str,
        target: Option<SaveTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = match validate_theme_name(name) {
            Ok(name) => name,
            Err(err) => return self.theme_error(err.to_string(), cx),
        };
        let registry = &cx.global::<Themes>().0;
        let Some(base) = registry.get(base).cloned() else {
            return self.theme_error(format!("There is no theme called {base}."), cx);
        };
        let existing = registry.get(&name).map(|t| t.name.clone());
        let target = match (target, existing) {
            (Some(target), _) => target,
            (None, None) => SaveTarget::New { replace: false },
            (None, Some(existing)) => {
                let question = format!("Replace the theme {existing}?");
                let detail =
                    "Its file is overwritten with the colours of the theme you start from.";
                let base = base.name.clone();
                self.ask_then(
                    question,
                    detail,
                    "Replace",
                    window,
                    cx,
                    move |ws, window, cx| {
                        ws.create_theme(&existing, &base, Some(SaveTarget::InPlace), window, cx)
                    },
                );
                return;
            }
        };
        let Some(dir) = themes_dir(cx) else {
            return self.theme_error("No configuration folder is available.".into(), cx);
        };
        let edit = ThemeEdit::new(base.clone());
        let theme = edit.renamed(&name);
        let omit = edit.omitted_keys();
        let write = {
            let (theme, target) = (theme.clone(), target.clone());
            cx.background_spawn(async move { save_user_theme(&dir, &theme, &omit, target) })
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = write.await;
            this.update_in(cx, |ws, window, cx| match result {
                Ok(path) => ws.theme_created(theme, path, window, cx),
                Err(SaveError::Exists(path)) => {
                    let question = format!("Replace {}?", path.display());
                    let base = base.name.clone();
                    let detail = "The file already exists in your themes folder.";
                    ws.ask_then(
                        question,
                        detail,
                        "Replace",
                        window,
                        cx,
                        move |ws, window, cx| {
                            let target = Some(SaveTarget::New { replace: true });
                            ws.create_theme(&name, &base, target, window, cx)
                        },
                    );
                }
                Err(err) => ws.theme_error(err.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Asks `question` with `yes` and Cancel; runs `then` on `yes`.
    pub(super) fn ask_then(
        &mut self,
        question: String,
        detail: &str,
        yes: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(detail),
            &[yes, "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |ws, window, cx| then(ws, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn theme_created(
        &mut self,
        theme: Theme,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = theme.name.clone();
        cx.global_mut::<Themes>().0.insert(theme.clone());
        self.prefs.new_theme = None;
        let ui = &settings(cx).ui;
        let dark = theme_sync::is_dark(window.appearance());
        let edits = selection_edits(&ui.theme, &name, theme.appearance, dark);
        crate::prefs::store::write_and_report(edits, cx.weak_entity(), cx);
        self.open_theme_editor_on(Some(theme), window, cx);
        self.show_toast_as(
            format!(
                "Created {name} as {} in your themes folder",
                file_name(&path)
            ),
            super::ToastLevel::Info,
            cx,
        );
    }

    /// Adds the New Theme and rename dialogs to `root`.
    pub(super) fn render_theme_dialogs(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let dialog = match (&self.prefs.new_theme, &self.prefs.theme_prompt) {
            (Some(dialog), _) => dialog.clone().into_any_element(),
            (None, Some((prompt, _))) => prompt.clone().into_any_element(),
            (None, None) => return root,
        };
        let panel = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(dialog);
        root.child(
            crate::picker_frame::picker_layer("theme-dialog-backdrop")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| ws.close_theme_dialogs(window, cx)),
                )
                .child(fade_in("theme-dialog-anim", panel, cx)),
        )
    }
}
