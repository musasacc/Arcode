//! Languages: each configured language with its servers, indentation and
//! an enable switch (`[language.<id>].enabled`).

use arcode_config::SettingEdit;
use arcode_ui_kit::{settings, theme, type_scale};
use gpui::{Context, Div, SharedString, div, prelude::*};

use super::widgets::{setting_row, toggle};
use super::{SettingsPage, group_title};

/// The display name of a language id (`rust` → `Rust`), from the syntax
/// registry when it knows the id.
fn language_name(id: &str) -> String {
    arcode_syntax::all_languages()
        .iter()
        .find(|l| l.id.eq_ignore_ascii_case(id))
        .map(|l| l.name.to_owned())
        .unwrap_or_else(|| id.to_owned())
}

/// The edit for turning a language on (removing `enabled`, on being the
/// default) or off.
pub(crate) fn enabled_edit(id: &str, on: bool) -> SettingEdit {
    let path = format!("language.{}.enabled", id.to_lowercase());
    if on {
        SettingEdit::remove(path)
    } else {
        SettingEdit::set(path, false)
    }
}

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let c = theme(cx).colors.clone();
    let mut rows = Vec::new();
    for (id, lang) in &config.languages {
        let servers = if lang.servers.is_empty() {
            "no language server".to_owned()
        } else {
            lang.servers.join(", ")
        };
        let tab = config.effective_tab_size(id);
        let spaces = if config.effective_insert_spaces(id) {
            "spaces"
        } else {
            "tabs"
        };
        let detail = format!("{servers} · indent {tab} {spaces}");
        let writer = page.writer();
        let id_owned = id.clone();
        let control = toggle(
            SharedString::from(format!("lang-enabled-{id}")),
            lang.enabled,
            move |on, _, cx| writer.write(vec![enabled_edit(&id_owned, on)], cx),
        );
        rows.push(setting_row(language_name(id), detail, control, cx));
    }
    div()
        .child(group_title("Language packs", cx))
        .child(
            div()
                .text_size(type_scale(cx).meta)
                .text_color(c.text_muted)
                .pb_2()
                .child("Turning a language off stops its servers, run buttons and snippets. The Languages view (activity bar) shows which servers are installed."),
        )
        .children(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_removes_the_key() {
        assert_eq!(
            enabled_edit("Rust", true),
            SettingEdit::remove("language.rust.enabled")
        );
        assert_eq!(
            enabled_edit("go", false),
            SettingEdit::set("language.go.enabled", false)
        );
    }
}
