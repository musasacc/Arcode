//! Keymap: a read-only, filterable list of every command and its key
//! bindings.

use arcode_ui_kit::{kbd, theme};
use gpui::{App, Context, Div, Entity, div, prelude::*, px};

use super::{SettingsPage, group_title};
use crate::commands::{binding_label, commands};
use crate::text_field::TextField;

/// One row: the command label and its bindings (`Ctrl+K Ctrl+T`), in
/// registration order, without duplicates.
pub(crate) fn keymap_rows(cx: &App) -> Vec<(String, Vec<String>)> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    commands(cx)
        .iter()
        .map(|command| {
            let mut keys: Vec<String> = Vec::new();
            for binding in keymap.bindings_for_action(command.action_ref()) {
                let label = binding_label(binding);
                if !keys.contains(&label) {
                    keys.push(label);
                }
            }
            (command.label(), keys)
        })
        .collect()
}

/// Rows whose label or keys contain every word of `query`.
pub(crate) fn filter_rows<'a>(
    rows: &'a [(String, Vec<String>)],
    query: &str,
) -> Vec<&'a (String, Vec<String>)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    rows.iter()
        .filter(|(label, keys)| {
            let text = format!("{label} {}", keys.join(" ")).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .collect()
}

pub(super) fn render(
    _page: &SettingsPage,
    filter: &Entity<TextField>,
    cx: &mut Context<SettingsPage>,
) -> Div {
    let c = theme(cx).colors.clone();
    let rows = keymap_rows(cx);
    let query = filter.read(cx).text().to_owned();
    let shown = filter_rows(&rows, &query);
    let count = shown.len();
    let list = shown.into_iter().map(|(label, keys)| {
        let mut row = div()
            .flex()
            .items_center()
            .gap_3()
            .py_1p5()
            .border_b_1()
            .border_color(c.border.opacity(0.5))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(label.clone()),
            );
        if keys.is_empty() {
            row = row.child(div().text_xs().text_color(c.text_disabled).child("—"));
        }
        for key in keys {
            row = row.child(kbd(key.clone()));
        }
        row
    });
    div()
        .child(group_title("Key bindings", cx))
        .child(
            div()
                .text_xs()
                .text_color(c.text_muted)
                .pb_2()
                .child("Read-only. Shortcuts use Cmd on macOS where this list shows Ctrl."),
        )
        .child(
            div()
                .flex()
                .items_center()
                .h(px(30.))
                .px_2()
                .mb_2()
                .rounded_md()
                .border_1()
                .border_color(c.border)
                .bg(c.surface)
                .child(filter.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(c.text_muted)
                .pb_1()
                .child(format!("{count} commands")),
        )
        .children(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_labels_and_keys() {
        let rows = vec![
            (
                "Preferences: Select Theme".to_owned(),
                vec!["Ctrl+K Ctrl+T".to_owned()],
            ),
            ("View: Toggle Panel".to_owned(), vec!["Ctrl+J".to_owned()]),
        ];
        assert_eq!(filter_rows(&rows, "").len(), 2);
        assert_eq!(
            filter_rows(&rows, "theme")[0].0,
            "Preferences: Select Theme"
        );
        assert_eq!(filter_rows(&rows, "ctrl+j")[0].0, "View: Toggle Panel");
        assert!(filter_rows(&rows, "theme ctrl+j").is_empty());
    }

    /// Copy Line Down / Up keys reach Duplicate Line in an editor of a
    /// pane, and nothing else is bound to them there (multi-cursor keeps
    /// Shift+Alt+Up / Down).
    #[gpui::test]
    fn duplicate_line_keys_do_not_clash(cx: &mut gpui::TestAppContext) {
        use gpui::{KeyContext, Keystroke};
        crate::test_support::init(cx);
        cx.update(|cx| {
            let stack: Vec<KeyContext> = [
                crate::actions::WORKSPACE_CONTEXT,
                crate::actions::PANE_CONTEXT,
                arcode_editor::KEY_CONTEXT,
            ]
            .iter()
            .filter_map(|n| KeyContext::parse(n).ok())
            .collect();
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let resolve = |key: &str| {
                let keys = [Keystroke::parse(key).expect("keystroke")];
                let (bindings, _) = keymap.bindings_for_input(&keys, &stack);
                bindings
                    .iter()
                    .map(|b| b.action().name())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                resolve("secondary-alt-shift-down"),
                ["editor::DuplicateLine"]
            );
            assert_eq!(
                resolve("secondary-alt-shift-up"),
                ["editor::DuplicateLineUp"]
            );
            assert_eq!(resolve("alt-shift-down"), ["editor::AddCursorBelow"]);
            assert_eq!(resolve("alt-shift-up"), ["editor::AddCursorAbove"]);
        });
        let rows = cx.update(|cx| keymap_rows(cx));
        let keys = |label: &str| {
            rows.iter()
                .find(|(l, _)| l == label)
                .map(|(_, k)| k.clone())
                .unwrap_or_default()
        };
        assert_eq!(keys("Editor: Duplicate Line").len(), 1);
        assert_eq!(keys("Editor: Duplicate Line Up").len(), 1);
    }
}
