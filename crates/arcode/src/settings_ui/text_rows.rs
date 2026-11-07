//! Rows with a text field: the terminal shell and its arguments, the
//! projects folder, search excludes, TODO tags and fallback fonts. A field
//! shows the setting while it is not being edited; Enter or leaving the
//! field writes it, Escape puts the setting back.

use arcode_config::config::keys;
use arcode_config::{Config, SettingEdit, SettingValue};
use arcode_ui_kit::{input_frame, settings};
use gpui::{
    App, AppContext, Context, Entity, Focusable, KeyDownEvent, Window, div, prelude::*, px,
};

use super::SettingsPage;
use super::text_parse::{join_args, split_args, split_list};
use crate::text_field::TextField;

/// Width of a text field in a row.
const FIELD_W: f32 = 260.;

/// How a field reads and writes its setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TextKind {
    /// An optional string; an empty field removes the key.
    Optional,
    /// A list of strings, separated by commas (not inside a glob's
    /// `{…}` or `[…]`).
    List,
    /// Command arguments, separated by spaces; `"…"` or `'…'` keeps
    /// spaces in one.
    Args,
    /// A string whose default is empty; an empty field writes `""`.
    Text,
}

/// The text settings: key, kind and the placeholder of an empty field.
const TEXT_SETTINGS: [(&str, TextKind, &str); 7] = [
    ("terminal.shell", TextKind::Optional, SHELL_PLACEHOLDER),
    ("terminal.args", TextKind::Args, "No arguments"),
    (
        "workspace.projects_folder",
        TextKind::Optional,
        "Next to the current project",
    ),
    ("search.exclude", TextKind::List, "e.g. *.min.js, dist"),
    ("todo.tags", TextKind::List, "No tags"),
    ("editor.font_fallbacks", TextKind::List, "No fallback fonts"),
    ("discord.application_id", TextKind::Text, APP_ID_PLACEHOLDER),
];

const APP_ID_PLACEHOLDER: &str = if arcode_presence::DEFAULT_APPLICATION_ID.is_empty() {
    "Discord application ID"
} else {
    "Arcode's own"
};

const SHELL_PLACEHOLDER: &str = if cfg!(windows) {
    "PowerShell (default)"
} else {
    "$SHELL (default)"
};

/// One field of the page.
pub(crate) struct TextSetting {
    path: &'static str,
    kind: TextKind,
    field: Entity<TextField>,
    /// The text the field was last given (the setting, or what was
    /// written): the field is being edited while it holds anything else.
    synced: String,
}

impl TextSetting {
    fn edited(&self, cx: &App) -> bool {
        self.field.read(cx).text() != self.synced
    }

    /// The key it edits.
    #[cfg(test)]
    pub(crate) fn path(&self) -> &'static str {
        self.path
    }

    /// The field.
    #[cfg(test)]
    pub(crate) fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// What the field shows.
    #[cfg(test)]
    pub(crate) fn text<'a>(&self, cx: &'a App) -> &'a str {
        self.field.read(cx).text()
    }
}

/// The fields of a new page.
pub(super) fn new_fields(cx: &mut Context<SettingsPage>) -> Vec<TextSetting> {
    TEXT_SETTINGS
        .into_iter()
        .map(|(path, kind, placeholder)| {
            let field = cx.new(|cx| TextField::new(placeholder, cx));
            // Tab and Shift+Tab move between the fields (see `tab_focus`).
            let _ = field.focus_handle(cx).tab_stop(true);
            TextSetting {
                path,
                kind,
                field,
                synced: String::new(),
            }
        })
        .collect()
}

/// The text a field shows for `path`.
pub(crate) fn shown(config: &Config, path: &str, kind: TextKind) -> String {
    let value = keys::setting_value(config, path);
    let items = || -> Vec<String> {
        value
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    match kind {
        TextKind::Optional | TextKind::Text => value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        TextKind::List => items().join(", "),
        TextKind::Args => join_args(&items()),
    }
}

/// The edit that stores `text` in `path`.
pub(crate) fn edit_for(path: &str, kind: TextKind, text: &str) -> SettingEdit {
    match kind {
        TextKind::Optional if text.trim().is_empty() => SettingEdit::remove(path),
        TextKind::Optional => SettingEdit::set(path, text.trim()),
        TextKind::List => SettingEdit::set(path, SettingValue::List(split_list(text))),
        TextKind::Args => SettingEdit::set(path, SettingValue::List(split_args(text))),
        TextKind::Text => SettingEdit::set(path, text.trim()),
    }
}

impl SettingsPage {
    /// Shows the settings in the fields nobody is typing in.
    pub(super) fn sync_text_fields(&mut self, cx: &mut Context<Self>) {
        let config = settings(cx).clone();
        for t in &mut self.texts {
            if t.edited(cx) {
                continue;
            }
            let text = shown(&config, t.path, t.kind);
            if t.synced != text {
                t.field.update(cx, |f, cx| f.set_text(&text, cx));
                t.synced = text;
            }
        }
    }

    /// Writes what was typed into field `ix` (unless it says what the
    /// setting already is). The field then shows the setting as written.
    fn commit_text(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(t) = self.texts.get_mut(ix) else {
            return;
        };
        let text = t.field.read(cx).text().to_owned();
        if text == t.synced {
            return;
        }
        t.synced = text.clone();
        let (path, kind) = (t.path, t.kind);
        let config = settings(cx).clone();
        let edit = edit_for(path, kind, &text);
        let unchanged = arcode_config::apply_edits("", std::slice::from_ref(&edit))
            .map(|out| shown(&Config::from_toml_str(&out).0, path, kind))
            .is_ok_and(|typed| typed == shown(&config, path, kind));
        if unchanged {
            self.sync_text_fields(cx);
        } else {
            self.writer().write(vec![edit], cx);
        }
    }

    /// Drops what was typed into field `ix` and shows the setting again.
    fn revert_text(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(t) = self.texts.get_mut(ix) {
            let text = t.field.read(cx).text().to_owned();
            t.synced = text;
        }
        self.sync_text_fields(cx);
    }

    /// Writes a field when it loses the focus. Needs a window, so it runs
    /// on the first draw.
    pub(super) fn watch_text_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.blur_subs.is_empty() {
            return;
        }
        for ix in 0..self.texts.len() {
            let handle = self.texts[ix].field.focus_handle(cx);
            self.blur_subs
                .push(cx.on_blur(&handle, window, move |page, _, cx| page.commit_text(ix, cx)));
        }
    }

    /// Tab and Shift+Tab move between the page's text fields and reset
    /// buttons (a field writes what was typed when it loses the focus).
    pub(super) fn tab_focus(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = &e.keystroke.modifiers;
        if e.keystroke.key != "tab" || m.control || m.alt || m.platform {
            return;
        }
        if m.shift {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        cx.stop_propagation();
    }

    /// The key of the field that has the focus.
    pub(super) fn focused_text(&self, window: &Window, cx: &App) -> Option<&'static str> {
        self.texts
            .iter()
            .find(|t| t.field.focus_handle(cx).is_focused(window))
            .map(|t| t.path)
    }

    /// A row with the text field of `path` (one of [`TEXT_SETTINGS`]).
    pub(crate) fn text_row(
        &self,
        label: &'static str,
        description: &'static str,
        path: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let Some(ix) = self.texts.iter().position(|t| t.path == path) else {
            return div();
        };
        let focused = self.focused_text == Some(path);
        let control = div()
            .on_key_down(cx.listener(move |page, e: &KeyDownEvent, _, cx| {
                match e.keystroke.key.as_str() {
                    "enter" => page.commit_text(ix, cx),
                    "escape" => page.revert_text(ix, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                input_frame(focused, false, cx).w(px(FIELD_W)).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .overflow_hidden()
                        .child(self.texts[ix].field.clone()),
                ),
            );
        self.keyed_row(label, description, path, control, cx)
    }

    /// Types `text` into the field of `path` and leaves it (tests).
    #[cfg(test)]
    pub(crate) fn type_setting_text(&mut self, path: &str, text: &str, cx: &mut Context<Self>) {
        let ix = self
            .texts
            .iter()
            .position(|t| t.path == path)
            .expect("a text setting");
        self.texts[ix]
            .field
            .update(cx, |f, cx| f.set_text(text, cx));
        self.commit_text(ix, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_show_and_write_their_setting() {
        let (config, _) = Config::from_toml_str(
            "[terminal]\nshell = \"/bin/zsh\"\nargs = [\"-l\", \"a b\"]\n[todo]\ntags = [\"TODO\", \"FIXME\"]\n",
        );
        assert_eq!(
            shown(&config, "terminal.shell", TextKind::Optional),
            "/bin/zsh"
        );
        assert_eq!(
            shown(&config, "terminal.args", TextKind::Args),
            "-l \"a b\""
        );
        assert_eq!(shown(&config, "todo.tags", TextKind::List), "TODO, FIXME");
        assert_eq!(
            shown(&config, "workspace.projects_folder", TextKind::Optional),
            ""
        );
        assert_eq!(
            edit_for("todo.tags", TextKind::List, " TODO,, FIXME ,"),
            SettingEdit::set(
                "todo.tags",
                SettingValue::List(vec!["TODO".into(), "FIXME".into()])
            )
        );
        // A glob's alternation is one item, before and after an edit.
        let (config, _) = Config::from_toml_str("[search]\nexclude = [\"*.{min.js,map}\"]\n");
        let text = shown(&config, "search.exclude", TextKind::List);
        assert_eq!(text, "*.{min.js,map}");
        assert_eq!(
            edit_for("search.exclude", TextKind::List, &format!("{text}, dist")),
            SettingEdit::set(
                "search.exclude",
                SettingValue::List(vec!["*.{min.js,map}".into(), "dist".into()])
            )
        );
        // Quotes inside an argument survive showing and writing it back.
        let (config, _) =
            Config::from_toml_str("[terminal]\nargs = [\"-c\", \"printf \\\"%s\\\" hi\"]\n");
        let text = shown(&config, "terminal.args", TextKind::Args);
        assert_eq!(text, r#"-c "printf \"%s\" hi""#);
        assert_eq!(
            edit_for("terminal.args", TextKind::Args, &format!("{text} -l")),
            SettingEdit::set(
                "terminal.args",
                SettingValue::List(vec!["-c".into(), r#"printf "%s" hi"#.into(), "-l".into()])
            )
        );
        assert_eq!(
            edit_for("terminal.shell", TextKind::Optional, "  "),
            SettingEdit::remove("terminal.shell")
        );
        assert_eq!(
            edit_for("terminal.shell", TextKind::Optional, " fish "),
            SettingEdit::set("terminal.shell", "fish")
        );
        // Emptied, a text setting keeps its key (its default is "").
        assert_eq!(
            edit_for("discord.application_id", TextKind::Text, " "),
            SettingEdit::set("discord.application_id", "")
        );
        let (config, _) = Config::from_toml_str("[discord]\napplication_id = \"42\"\n");
        assert_eq!(
            shown(&config, "discord.application_id", TextKind::Text),
            "42"
        );
        for (path, kind, _) in TEXT_SETTINGS {
            // Every text setting is a key the configuration knows.
            assert!(
                keys::documented_keys().iter().any(|k| k == path),
                "{path} ({kind:?})"
            );
        }
    }
}
