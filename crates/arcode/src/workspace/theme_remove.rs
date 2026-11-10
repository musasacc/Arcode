//! Renaming and deleting the user's own themes: the rename dialog, the
//! confirm before a delete, the file and the settings that name the theme,
//! and the theme editor when it edits that theme.

use arcode_config::theme::{
    SaveError, delete_user_theme, is_builtin_name, rename_user_theme, validate_theme_name,
};
use arcode_ui_kit::{IconName, settings};
use gpui::{AppContext, Context, Focusable, Window};

use super::Workspace;
use crate::text_prompt::{TextPrompt, TextPromptEvent};
use crate::theme_editor::{configured_theme, delete_edits, rename_edits, themes_dir};
use crate::theme_sync::{self, Themes};

impl Workspace {
    /// Asks for a new name for the user theme `name`.
    pub(crate) fn open_rename_theme(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_builtin_name(name) {
            self.show_toast(format!("{name} is built in and cannot be renamed"), cx);
            return;
        }
        let prompt = cx.new(|cx| {
            TextPrompt::new(
                "Rename theme",
                "Its file in your themes folder gets the new name too.",
                IconName::Palette,
                name,
                cx,
            )
        });
        let old = name.to_owned();
        let sub = cx.subscribe_in(
            &prompt,
            window,
            move |ws, _, event: &TextPromptEvent, window, cx| match event {
                TextPromptEvent::Confirmed(new) => ws.rename_theme(&old, new, false, window, cx),
                TextPromptEvent::Dismissed => ws.close_theme_dialogs(window, cx),
            },
        );
        self.subs.set("rename_theme", sub);
        window.focus(&prompt.focus_handle(cx));
        self.prefs.theme_prompt = Some((prompt, name.to_owned()));
        cx.notify();
    }

    /// Renames the user theme `old` to `new` (its file too) and updates
    /// the settings that name it. `replace`: the user agreed to replace
    /// another theme or file of that name.
    pub(crate) fn rename_theme(
        &mut self,
        old: &str,
        new: &str,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new = match validate_theme_name(new) {
            Ok(new) => new,
            Err(err) => return self.theme_error(err.to_string(), cx),
        };
        if new == old {
            return self.close_theme_dialogs(window, cx);
        }
        let registry = &cx.global::<Themes>().0;
        let taken = registry
            .get(&new)
            .filter(|t| !t.name.eq_ignore_ascii_case(old))
            .map(|t| t.name.clone());
        if let (Some(taken), false) = (taken, replace) {
            let (old, question) = (old.to_owned(), format!("Replace the theme {taken}?"));
            let detail = "It is deleted, and this theme takes its name.";
            self.ask_then(
                question,
                detail,
                "Replace",
                window,
                cx,
                move |ws, window, cx| ws.rename_theme(&old, &new, true, window, cx),
            );
            return;
        }
        let Some(dir) = themes_dir(cx) else {
            return self.theme_error("No configuration folder is available.".into(), cx);
        };
        let (from, to) = (old.to_owned(), new.clone());
        let task = cx.background_spawn(async move { rename_user_theme(&dir, &from, &to, replace) });
        let old = old.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |ws, window, cx| match result {
                Ok(_) => ws.theme_renamed(&old, &new, window, cx),
                Err(SaveError::Exists(path)) => {
                    let question = format!("Replace {}?", path.display());
                    let detail = "The file already exists in your themes folder.";
                    ws.ask_then(
                        question,
                        detail,
                        "Replace",
                        window,
                        cx,
                        move |ws, window, cx| ws.rename_theme(&old, &new, true, window, cx),
                    );
                }
                Err(err) => ws.theme_error(err.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }

    fn theme_renamed(&mut self, old: &str, new: &str, window: &mut Window, cx: &mut Context<Self>) {
        let registry = &mut cx.global_mut::<Themes>().0;
        if let Some(mut theme) = registry.remove_user_theme(old) {
            registry.remove_user_themes_named(new);
            theme.name = new.to_owned();
            registry.insert(theme);
        }
        let edits = rename_edits(&settings(cx).ui, old, new);
        if !edits.is_empty() {
            crate::prefs::store::write_and_report(edits, cx.weak_entity(), cx);
        }
        if let Some(editor) = self.prefs.theme_editor.clone() {
            editor.update(cx, |e, cx| e.theme_renamed(old, new, cx));
        }
        self.close_theme_dialogs(window, cx);
        self.show_toast_as(
            format!("Renamed {old} to {new}"),
            super::ToastLevel::Info,
            cx,
        );
    }

    /// Asks, then deletes the user theme `name`. When it was in use, the
    /// default theme of its mode is used instead.
    pub(crate) fn delete_theme(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if is_builtin_name(name) {
            self.show_toast(format!("{name} is built in and cannot be deleted"), cx);
            return;
        }
        let Some(appearance) = cx.global::<Themes>().0.get(name).map(|t| t.appearance) else {
            self.show_toast(format!("There is no theme called {name}"), cx);
            return;
        };
        let question = format!("Delete the theme {name}?");
        let detail = "Its file is deleted from your themes folder. This cannot be undone.";
        let name = name.to_owned();
        self.ask_then(
            question,
            detail,
            "Delete",
            window,
            cx,
            move |ws, window, cx| {
                let Some(dir) = themes_dir(cx) else {
                    return ws.show_toast("No configuration folder is available", cx);
                };
                let from = name.clone();
                let task = cx.background_spawn(async move { delete_user_theme(&dir, &from) });
                cx.spawn_in(window, async move |this, cx| {
                    let result = task.await;
                    this.update_in(cx, |ws, window, cx| match result {
                        Ok(_) => ws.theme_deleted(&name, appearance, window, cx),
                        Err(err) => ws.show_toast_as(err.to_string(), super::ToastLevel::Error, cx),
                    })
                    .ok();
                })
                .detach();
            },
        );
    }

    fn theme_deleted(
        &mut self,
        name: &str,
        appearance: arcode_config::Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.global_mut::<Themes>().0.remove_user_theme(name);
        let edits = delete_edits(&settings(cx).ui, name, appearance);
        let write = crate::prefs::store::write_settings(edits, cx);
        let name = name.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let written = write.await;
            this.update_in(cx, |ws, window, cx| {
                if let Err(err) = written {
                    ws.show_toast_as(err, super::ToastLevel::Error, cx);
                }
                let fallback = configured_theme(window, cx);
                match ws.prefs.theme_editor.clone() {
                    Some(editor) if editor.read(cx).theme_name() == name => {
                        editor.update(cx, |e, cx| e.theme_deleted(&name, fallback, cx))
                    }
                    Some(_) => {}
                    None => theme_sync::restore(window.appearance(), cx),
                }
                ws.show_toast_as(format!("Deleted {name}"), super::ToastLevel::Info, cx);
            })
            .ok();
        })
        .detach();
    }
}
