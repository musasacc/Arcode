//! Terminal and Advanced sections.

use arcode_ui_kit::{IconName, button, settings, theme, type_scale};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::widgets::{picker_button, segmented, stepped, stepper};
use super::{SettingsEvent, SettingsPage, Steps, group_title};
use crate::prefs::pickers::FontTarget;

pub(super) fn render_terminal(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let t = &config.terminal;
    let family = t
        .font_family
        .clone()
        .unwrap_or_else(|| format!("Same as Editor ({})", config.editor.font_family));
    let font = picker_button(
        SharedString::from("pick-terminal-font"),
        family,
        cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::PickFont(FontTarget::Terminal))),
        cx,
    );
    let size = config.terminal_font_size();
    let writer = page.writer();
    let size_control = stepper(
        "terminal.font_size",
        format!("{size:.1}"),
        move |delta, _, cx| {
            let next = stepped(size, delta, 1., 6., 72.);
            writer.set("terminal.font_size", next, cx);
        },
        cx,
    );
    div()
        .child(group_title("Font", cx))
        .child(page.keyed_row(
            "Family",
            "Unset: the editor font.",
            "terminal.font_family",
            font,
            cx,
        ))
        .child(page.keyed_row(
            "Size",
            "Unset: the editor font size.",
            "terminal.font_size",
            size_control,
            cx,
        ))
        .child(group_title("Shell", cx))
        .child(page.text_row(
            "Program",
            "A program name or path. Empty: $SHELL on Linux and macOS, PowerShell on Windows. New terminals use it.",
            "terminal.shell",
            cx,
        ))
        .child(page.text_row(
            "Arguments",
            "Passed to the shell, separated by spaces. Put an argument with spaces in quotes: -c \"echo hi\" or -c 'echo hi'.",
            "terminal.args",
            cx,
        ))
        .child(page.number_row(
            "Scrollback lines",
            "",
            "terminal.scrollback",
            t.scrollback as f32,
            Steps {
                step: 1000.,
                min: 1000.,
                max: 1_000_000.,
                int: true,
            },
            cx,
        ))
}

pub(super) fn render_advanced(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let c = theme(cx).colors.clone();
    let path = crate::prefs::store::config_path(cx)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no configuration folder)".into());
    let actions = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(
            button("adv-open-file", "Open config.toml")
                .icon(IconName::FileText)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
        )
        .child(
            button("adv-reload", "Reload from Disk")
                .icon(IconName::Refresh)
                .on_click(|_, _, cx| crate::prefs::store::reload(cx)),
        )
        .child(
            button("adv-tour", "Start Tour")
                .icon(IconName::CirclePlay)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::StartTour))),
        )
        .child(
            button("adv-welcome", "Welcome Setup…")
                .icon(IconName::Sun)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::ShowWelcome))),
        );
    let run_output = run_output_choice(page, config.run.output, cx);
    div()
        .child(group_title("Run and debug", cx))
        .child(page.bool_row(
            "Run buttons",
            "Run and Debug buttons next to mains and tests.",
            "debug.run_buttons",
            config.debug.run_buttons,
            cx,
        ))
        .child(page.bool_row(
            "Open the Debug panel on start",
            "",
            "debug.open_panel_on_start",
            config.debug.open_panel_on_start,
            cx,
        ))
        .child(page.keyed_row(
            "Compiled programs",
            "Where Run Current File puts a program built from one file (rustc, gcc): next to the file, where the file tree and git show it, or in Arcode's cache.",
            "run.output",
            run_output,
            cx,
        ))
        .child(group_title("Large files", cx))
        .child({
            let stepper = large_file_stepper(page, config.editor.large_file_bytes, cx);
            page.keyed_row(
                "Large file size (MB)",
                "From this size only the visible part is parsed for highlighting.",
                "editor.large_file_bytes",
                stepper,
                cx,
            )
        })
        .child(page.number_row(
            "Large file lines",
            "Files with this many lines count as large too.",
            "editor.large_file_lines",
            config.editor.large_file_lines as f32,
            Steps {
                step: 10_000.,
                min: 1000.,
                max: 10_000_000.,
                int: true,
            },
            cx,
        ))
        .child(group_title("Configuration file", cx))
        .child(
            div()
                .py_2()
                .text_size(type_scale(cx).meta)
                .text_color(c.text_muted)
                .child(path),
        )
        .child(div().w_full().max_w(px(600.)).child(actions))
}

/// The `run.output` choice: next to the source or in the cache.
fn run_output_choice(
    page: &SettingsPage,
    output: arcode_config::RunOutput,
    cx: &Context<SettingsPage>,
) -> Div {
    use arcode_config::RunOutput;
    let writer = page.writer();
    segmented(
        "run-output",
        &["Next to the file", "Cache"],
        RunOutput::ALL
            .iter()
            .position(|o| *o == output)
            .unwrap_or(0),
        move |ix, _, cx| {
            let choice = RunOutput::ALL[ix.min(RunOutput::ALL.len() - 1)];
            writer.set("run.output", choice.as_str(), cx)
        },
        cx,
    )
}

/// Stepper over `editor.large_file_bytes`, shown and stepped in MiB.
fn large_file_stepper(page: &SettingsPage, bytes: u64, cx: &Context<SettingsPage>) -> Div {
    const MIB: u64 = 1024 * 1024;
    let mib = (bytes / MIB).max(1) as f32;
    let writer = page.writer();
    stepper(
        "editor.large_file_bytes",
        format!("{mib}"),
        move |delta, _, cx| {
            let next = stepped(mib, delta, 1., 1., 512.) as u64;
            writer.set("editor.large_file_bytes", next * MIB, cx);
        },
        cx,
    )
}
