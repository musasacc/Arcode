//! Search: project search (`[search]`) and the TODO list (`[todo]`).

use arcode_config::SearchSettings;
use arcode_ui_kit::settings;
use gpui::{Context, Div, div, prelude::*};

use super::{SettingsPage, Steps, group_title};

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let s = settings(cx).search.clone();
    div()
        .child(group_title("Project search", cx))
        .child(page.bool_row(
            "Respect .gitignore",
            "Skip what .gitignore and .ignore files list.",
            "search.respect_gitignore",
            s.respect_gitignore,
            cx,
        ))
        .child(page.bool_row(
            "Include hidden files",
            "Also search dot files and folders.",
            "search.include_hidden",
            s.include_hidden,
            cx,
        ))
        .child(page.text_row(
            "Exclude",
            "Globs never searched, separated by commas.",
            "search.exclude",
            cx,
        ))
        .child(page.number_row(
            "Result limit",
            "A search stops after this many matches; \"Show more\" raises it.",
            "search.max_results",
            s.max_results as f32,
            Steps {
                step: 500.,
                min: SearchSettings::MIN_RESULTS as f32,
                max: SearchSettings::MAX_RESULTS as f32,
                int: true,
            },
            cx,
        ))
        .child(page.number_row(
            "Search delay (ms)",
            "Wait after the last keystroke before searching.",
            "search.debounce_ms",
            s.debounce_ms as f32,
            Steps {
                step: 50.,
                min: 0.,
                max: SearchSettings::MAX_DEBOUNCE_MS as f32,
                int: true,
            },
            cx,
        ))
        .child(group_title("TODO list", cx))
        .child(page.text_row(
            "Tags",
            "Comment tags listed in the TODO view, separated by commas (case-sensitive, whole words).",
            "todo.tags",
            cx,
        ))
}
