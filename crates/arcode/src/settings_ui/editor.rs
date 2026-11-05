//! Editor: indentation, display, editing aids, language features and git
//! decorations.

use arcode_config::{CompletionSettings, EditorSettings, GitSettings, InlineDiagnostics, Keymap};
use arcode_ui_kit::settings;
use gpui::{Context, Div, div, prelude::*};

use super::widgets::segmented;
use super::{SettingsPage, Steps, group_title};

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let e = &config.editor;
    let writer = page.writer();
    let inline_diagnostics = segmented(
        "editor-inline-diagnostics",
        &["Errors", "All", "Off"],
        InlineDiagnostics::ALL
            .iter()
            .position(|d| *d == e.inline_diagnostics)
            .unwrap_or(0),
        move |ix, _, cx| {
            let choice = InlineDiagnostics::ALL[ix.min(2)];
            writer.set("editor.inline_diagnostics", choice.as_str(), cx)
        },
        cx,
    );
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
        .child(page.low_end_bool_row(
            "Minimap",
            "The file scaled down at the right edge; click or drag it to scroll.",
            "The minimap is hidden in low-end mode.",
            "editor.minimap",
            e.minimap,
            cx,
        ))
        .child(page.number_row(
            "Minimap width",
            "In pixels.",
            "editor.minimap_width",
            e.minimap_width as f32,
            Steps {
                step: 8.,
                min: *arcode_config::EditorSettings::MINIMAP_WIDTHS.start() as f32,
                max: *arcode_config::EditorSettings::MINIMAP_WIDTHS.end() as f32,
                int: true,
            },
            cx,
        ))
        .child(page.low_end_bool_row(
            "Cursor blink",
            "Blinks only while the window is focused.",
            "The cursor never blinks in low-end mode.",
            "editor.cursor_blink",
            e.cursor_blink,
            cx,
        ))
        .child(group_title("Editing", cx))
        .child(
            page.keyed_row_with_reset(
                "Keymap",
                "Vim mode: Normal, Insert and Visual modes, operators, registers, : commands.",
                "editor.keymap",
                (super::keymap_index(&config) != 0)
                    .then(|| arcode_config::keymap_edits(Keymap::Default)),
                keymap,
                cx,
            ),
        )
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
        .child(page.number_row(
            "Format on save timeout (ms)",
            "Save without formatting when the formatter takes longer.",
            "editor.format_on_save_timeout_ms",
            e.format_on_save_timeout_ms as f32,
            Steps {
                step: 250.,
                min: 250.,
                max: EditorSettings::MAX_FORMAT_TIMEOUT_MS as f32,
                int: true,
            },
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
        .child(page.number_row(
            "Completion delay (ms)",
            "Wait after a keystroke before asking for completions.",
            "editor.completion.delay_ms",
            e.completion.delay_ms as f32,
            Steps {
                step: 10.,
                min: 0.,
                max: CompletionSettings::MAX_DELAY_MS as f32,
                int: true,
            },
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
        .child(page.low_end_off_row(
            "Inlay hints",
            "Types and parameter names from the language server.",
            "Low-end mode turned inlay hints off; switch them on to use them anyway.",
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
        .child(page.keyed_row(
            "Inline diagnostics",
            "Show diagnostics after the end of their line: errors only, all of them, or none.",
            "editor.inline_diagnostics",
            inline_diagnostics,
            cx,
        ))
        .child(page.bool_row(
            "Fade unused code",
            "Unused imports, variables and unreachable code the language server reports. \
             Deprecated code is always struck through.",
            "editor.fade_unused",
            e.fade_unused,
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
        .child(page.low_end_off_row(
            "Inline blame",
            "Author and date of the cursor line, at its end.",
            "Low-end mode turned inline blame off; switch it on to use it anyway.",
            "git.inline_blame",
            config.git.inline_blame,
            cx,
        ))
        .child(page.number_row(
            "Inline blame delay (ms)",
            "How long the cursor rests on a line before its blame shows.",
            "git.inline_blame_delay_ms",
            config.git.inline_blame_delay_ms as f32,
            Steps {
                step: 100.,
                min: 0.,
                max: GitSettings::MAX_BLAME_DELAY_MS as f32,
                int: true,
            },
            cx,
        ))
}
