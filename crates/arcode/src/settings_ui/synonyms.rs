//! Other words people use for settings, matched by the page filter like
//! a row's label ([`super::filter`]).

/// Other words people use for a setting, matched like its label.
pub(crate) const SYNONYMS: &[(&str, &str)] = &[
    ("ui.theme", "colour color scheme dark light"),
    ("ui.font_size", "zoom scale interface"),
    ("ui.low_end_mode", "performance slow battery power saving"),
    ("editor.font_size", "zoom text"),
    ("editor.word_wrap", "soft wrap line wrap"),
    ("editor.minimap", "overview"),
    ("editor.cursor_blink", "caret"),
    ("editor.show_line_numbers", "gutter"),
    ("editor.insert_spaces", "indent indentation soft tabs"),
    (
        "editor.keymap",
        "vim bindings shortcuts modal editor.vim_mode",
    ),
    ("editor.format_on_save", "formatter prettier rustfmt black"),
    ("editor.inlay_hints", "types parameter names"),
    ("editor.inline_diagnostics", "errors warnings problems lens"),
    (
        "editor.fade_unused",
        "dim grey gray opacity strikethrough strike deprecated",
    ),
    (
        "editor.completion.auto_trigger",
        "autocomplete intellisense suggestions",
    ),
    ("git.inline_blame", "annotate author"),
    ("terminal.shell", "bash zsh fish powershell cmd"),
    ("search.exclude", "ignore skip globs"),
    ("todo.tags", "fixme hack note"),
    ("workspace.confirm_delete", "trash remove"),
    ("plugins.network", "internet http offline"),
    ("discord.application_id", "client id developer portal"),
];

/// Words for every setting of a group, by key prefix.
pub(crate) const GROUP_SYNONYMS: &[(&str, &str)] = &[(
    "discord.",
    "rich presence rpc status activity playing privacy",
)];

/// The synonyms of the setting at `key`, its own and its group's (each
/// empty when there are none).
pub(crate) fn synonyms(key: &str) -> (&'static str, &'static str) {
    let own = SYNONYMS
        .iter()
        .find(|(k, _)| *k == key)
        .map_or("", |(_, words)| words);
    let group = GROUP_SYNONYMS
        .iter()
        .find(|(prefix, _)| key.starts_with(prefix))
        .map_or("", |(_, words)| words);
    (own, group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_discord_setting_has_the_group_words() {
        for key in [
            "discord.enabled",
            "discord.show_file_name",
            "discord.show_project_name",
            "discord.show_elapsed",
            "discord.application_id",
        ] {
            assert!(synonyms(key).1.contains("rich presence"), "{key}");
        }
        assert_eq!(
            synonyms("discord.application_id").0,
            "client id developer portal"
        );
        assert_eq!(synonyms("editor.minimap"), ("overview", ""));
    }
}
