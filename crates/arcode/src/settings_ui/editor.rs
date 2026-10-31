//! Editor: indentation, display, editing aids, language features and git
//! decorations.

use arcode_config::Keymap;
use arcode_ui_kit::settings;
use gpui::{Context, Div, div, prelude::*};

use super::widgets::{segmented, setting_row};
use super::{SettingsPage, Steps, group_title};

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let e = &config.editor;
    let low_end = config.ui.low_end_mode;
    let writer = page.writer();
    let keymap = segmented(
        "editor-keymap",
        &["Default", "Vim"],
        super::keymap_index(&config),
        move |ix, _, cx| {
            let keymap = if ix == 1 {
                Keymap::Vim
            } else {
                Keymap::Default
            };
            writer.write(arcode_config::keymap_edits(keymap), cx)
        },
        cx,
    );
    div()
        .child(group_title("Indentation", cx))
        .child(page.number_row(
            "Tab size",
            "Width of a tab stop in columns (languages can override it).",
            "editor.tab_size",
            e.tab_size as f32,
            Steps {
                step: 1.,
                min: 1.,
                max: 32.,
                int: true,
            },
            cx,
        ))
        .child(page.bool_row(
            "Insert spaces",
            "Pressing Tab inserts spaces instead of a tab character.",
            "editor.insert_spaces",
            e.insert_spaces,
            cx,
        ))
        .child(page.bool_row(
            "Detect indentation",
            "Guess tabs or spaces and the indent size of each file from its content.",
            "editor.detect_indentation",
            e.detect_indentation,
            cx,
        ))
        .child(group_title("Display", cx))
        .child(page.bool_row(
            "Line numbers",
            "",
            "editor.show_line_numbers",
            e.show_line_numbers,
            cx,
        ))
        .child(page.bool_row(
            "Highlight current line",
            "",
            "editor.highlight_current_line",
            e.highlight_current_line,
            cx,
        ))
        .child(page.bool_row(
            "Word wrap",
            "Soft-wrap long lines at the edge of the editor.",
            "editor.word_wrap",
            e.word_wrap,
            cx,
        ))
        .child(page.bool_row(
            "Cursor blink",
            if low_end {
                "The cursor never blinks in low-end mode."
            } else {
                "Blinks only while the window is focused."
            },
            "editor.cursor_blink",
            e.cursor_blink,
            cx,
        ))
        .child(group_title("Editing", cx))
        .child(setting_row(
            "Keymap",
            "Vim mode: Normal, Insert and Visual modes, operators, registers, : commands.",
            keymap,
            cx,
        ))
        .child(page.bool_row(
            "Auto-close brackets",
            "Insert the closing bracket or quote when typing an opening one.",
            "editor.auto_close_brackets",
            e.auto_close_brackets,
            cx,
        ))
        .child(page.bool_row(
            "Format on save",
            "Format with the language server (or formatter) when saving.",
            "editor.format_on_save",
            e.format_on_save,
            cx,
        ))
        .child(group_title("Code intelligence", cx))
        .child(page.bool_row(
            "Completion while typing",
            "Off: only Ctrl+Space opens the completion list.",
            "editor.completion.auto_trigger",
            e.completion.auto_trigger,
            cx,
        ))
        .child(page.bool_row(
            "Snippets",
            "",
            "editor.completion.snippets",
            e.completion.snippets,
            cx,
        ))
        .child(page.bool_row(
            "Parameter hints",
            "",
            "editor.completion.signature_help",
            e.completion.signature_help,
            cx,
        ))
        .child(page.bool_row("Hover documentation", "", "editor.hover", e.hover, cx))
        .child(page.number_row(
            "Hover delay (ms)",
            "",
            "editor.hover_delay_ms",
            e.hover_delay_ms as f32,
            Steps {
                step: 50.,
                min: 0.,
                max: 5000.,
                int: true,
            },
            cx,
        ))
        .child(page.bool_row(
            "Inlay hints",
            "Types and parameter names from the language server.",
            "editor.inlay_hints",
            e.inlay_hints,
            cx,
        ))
        .child(page.bool_row(
            "Highlight occurrences",
            "",
            "editor.occurrence_highlight",
            e.occurrence_highlight,
            cx,
        ))
        .child(page.bool_row(
            "Code action light bulb",
            "",
            "editor.code_action_lightbulb",
            e.code_action_lightbulb,
            cx,
        ))
        .child(group_title("Git", cx))
        .child(page.bool_row(
            "Change bars",
            "Added, modified and deleted lines in the gutter.",
            "git.gutter",
            config.git.gutter,
            cx,
        ))
        .child(page.bool_row(
            "Inline blame",
            "Author and date of the cursor line, at its end.",
            "git.inline_blame",
            config.git.inline_blame,
            cx,
        ))
}
