//! Saving from the theme editor: "Save" writes a user theme's own file,
//! "Save as…" asks for a name and writes `<themes dir>/<slug>.toml`. The
//! saved theme joins the theme list and becomes the theme in use.

use std::path::PathBuf;

use arcode_config::theme::{SaveError, SaveTarget, save_user_theme, validate_theme_name};
use arcode_config::{Appearance, SettingEdit, Theme, ThemeChoice};
use arcode_ui_kit::{IconName, settings};
use gpui::{App, AppContext, Context, Focusable, PromptLevel, Window};

use super::{ThemeEditor, ThemeEditorEvent};
use crate::text_prompt::{TextPrompt, TextPromptEvent};
use crate::theme_sync::{self, Themes};

/// The user themes folder: `themes` next to the configuration file.
pub(crate) fn themes_dir(cx: &App) -> Option<PathBuf> {
    let config = crate::prefs::store::config_path(cx)?;
    Some(arcode_config::paths::themes_dir_in(config.parent()?))
}

/// The settings that make the saved theme `name` the one in use while
/// keeping the mode: following the system or a fixed dark or light mode
/// sets the matching dark or light theme when `name` is shown in that mode
/// now, any other case picks it by name.
pub fn selection_edits(
    choice: &ThemeChoice,
    name: &str,
    appearance: Appearance,
    system_is_dark: bool,
) -> Vec<SettingEdit> {
    let shown_mode = match choice {
        ThemeChoice::System if system_is_dark => Some(Appearance::Dark),
        ThemeChoice::System => Some(Appearance::Light),
        ThemeChoice::Dark => Some(Appearance::Dark),
        ThemeChoice::Light => Some(Appearance::Light),
        ThemeChoice::Named(_) => None,
    };
    let key = match shown_mode {
        Some(mode) if mode == appearance => match appearance {
            Appearance::Dark => "ui.dark_theme",
            Appearance::Light => "ui.light_theme",
        },
        _ => "ui.theme",
    };
    vec![SettingEdit::set(key, name)]
}

impl ThemeEditor {
    /// "Save": a user theme in place, a built-in one through "Save as…".
    /// With `close`, the page closes once the theme is saved.
    pub fn save(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.is_builtin() {
            self.open_name_prompt(close, window, cx);
            return;
        }
        let theme = self.edit.working().clone();
        self.write(theme, SaveTarget::InPlace, close, window, cx);
    }

    /// Asks for the name of a new theme.
    pub fn open_name_prompt(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(cx);
        let start = &self.edit.start().name;
        let initial = if self.edit.is_builtin() {
            format!("My {}", start.trim_start_matches("Arcode "))
        } else {
            format!("{start} copy")
        };
        let prompt = cx.new(|cx| {
            TextPrompt::new(
                "Save theme as",
                "Saved in your themes folder and used from now on.",
                IconName::Palette,
                &initial,
                cx,
            )
        });
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |editor, _, event: &TextPromptEvent, window, cx| match event {
                TextPromptEvent::Confirmed(name) => editor.save_as(name, window, cx),
                TextPromptEvent::Dismissed => {
                    editor.name_prompt = None;
                    window.focus(&editor.focus);
                    cx.notify();
                }
            },
        );
        self._subs.push(sub);
        window.focus(&prompt.focus_handle(cx));
        self.name_prompt = Some((prompt, close));
        cx.notify();
    }

    /// Saves the edited theme under the name typed into the dialog.
    pub fn save_as(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let close = self.name_prompt.as_ref().is_some_and(|(_, close)| *close);
        let name = match validate_theme_name(name) {
            Ok(name) => name,
            Err(err) => {
                match &self.name_prompt {
                    Some((prompt, _)) => prompt.update(cx, |p, cx| p.set_hint(err.to_string(), cx)),
                    None => cx.emit(ThemeEditorEvent::Toast(err.to_string().into())),
                }
                return;
            }
        };
        let theme = self.edit.renamed(&name);
        let existing = cx.global::<Themes>().0.get(&name).map(|t| t.name.clone());
        let Some(existing) = existing else {
            self.write(theme, SaveTarget::New { replace: false }, close, window, cx);
            return;
        };
        // Saving over another user theme: ask first.
        let theme = Theme {
            name: existing.clone(),
            ..theme
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Replace the theme {existing}?"),
            Some("Its file is overwritten with these colours."),
            &["Replace", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |editor, window, cx| {
                    editor.write(theme, SaveTarget::InPlace, close, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Writes `theme` on the background executor, then makes it the
    /// theme in use.
    fn write(
        &mut self,
        theme: Theme,
        target: SaveTarget,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = themes_dir(cx) else {
            cx.emit(ThemeEditorEvent::Toast(
                "No configuration folder is available".into(),
            ));
            return;
        };
        let omit = self.edit.omitted_keys();
        let task = {
            let theme = theme.clone();
            let target = target.clone();
            cx.background_spawn(async move { save_user_theme(&dir, &theme, &omit, target) })
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |editor, window, cx| match result {
                Ok(path) => editor.saved(theme, path, close, window, cx),
                Err(SaveError::Exists(path)) => editor.ask_replace(theme, path, close, window, cx),
                Err(err) => {
                    let message = err.to_string();
                    match &editor.name_prompt {
                        Some((prompt, _)) => prompt.update(cx, |p, cx| p.set_hint(message, cx)),
                        None => cx.emit(ThemeEditorEvent::Toast(message.into())),
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// The file "Save as" would write exists (another theme's): ask.
    fn ask_replace(
        &mut self,
        theme: Theme,
        path: PathBuf,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Replace {}?", path.display()),
            Some("The file already exists in your themes folder."),
            &["Replace", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(0) {
                this.update_in(cx, |editor, window, cx| {
                    editor.write(theme, SaveTarget::New { replace: true }, close, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    fn saved(
        &mut self,
        theme: Theme,
        path: PathBuf,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = theme.name.clone();
        let appearance = theme.appearance;
        cx.global_mut::<Themes>().0.insert(theme);
        self.edit.saved_as(&name);
        self.name_prompt = None;
        self.apply_now(cx);
        let ui = &settings(cx).ui;
        let edits = selection_edits(
            &ui.theme,
            &name,
            appearance,
            theme_sync::is_dark(window.appearance()),
        );
        let write = crate::prefs::store::write_settings(edits, cx);
        cx.spawn(async move |this, cx| {
            if let Err(err) = write.await {
                this.update(cx, |_, cx| {
                    cx.emit(ThemeEditorEvent::Toast(
                        format!("The theme was saved but cannot be selected: {err}").into(),
                    ))
                })
                .ok();
            }
        })
        .detach();
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        cx.emit(ThemeEditorEvent::Toast(
            format!("Saved {name} as {file} in your themes folder").into(),
        ));
        if close {
            cx.emit(ThemeEditorEvent::Close);
        } else {
            window.focus(&self.focus);
        }
    }

    /// Opens the edited user theme's file in an editor tab.
    pub fn open_theme_file(&mut self, cx: &mut Context<Self>) {
        let name = self.edit.start().name.clone();
        let found =
            themes_dir(cx).and_then(|dir| arcode_config::theme::find_user_theme_file(&dir, &name));
        let event = match found {
            Some(path) => ThemeEditorEvent::OpenFile(path),
            None => ThemeEditorEvent::Toast(
                format!("{name} has no file in your themes folder; save it first").into(),
            ),
        };
        cx.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(edits: Vec<SettingEdit>) -> String {
        assert_eq!(edits.len(), 1);
        edits[0].path.clone()
    }

    #[test]
    fn saving_keeps_the_theme_mode() {
        use Appearance::{Dark, Light};
        let system = ThemeChoice::System;
        assert_eq!(
            key(selection_edits(&system, "X", Dark, true)),
            "ui.dark_theme"
        );
        assert_eq!(
            key(selection_edits(&system, "X", Light, false)),
            "ui.light_theme"
        );
        // A dark theme saved while the system is light is picked by name.
        assert_eq!(key(selection_edits(&system, "X", Dark, false)), "ui.theme");
        assert_eq!(
            key(selection_edits(&ThemeChoice::Dark, "X", Dark, false)),
            "ui.dark_theme"
        );
        assert_eq!(
            key(selection_edits(&ThemeChoice::Light, "X", Dark, false)),
            "ui.theme"
        );
        assert_eq!(
            key(selection_edits(&ThemeChoice::Light, "X", Light, true)),
            "ui.light_theme"
        );
        let named = ThemeChoice::Named("Arcode Ember".into());
        let edits = selection_edits(&named, "My Ember", Dark, true);
        assert_eq!(edits, vec![SettingEdit::set("ui.theme", "My Ember")]);
    }
}
