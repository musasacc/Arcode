//! Terminal and Advanced sections.

use arcode_config::SettingEdit;
use arcode_ui_kit::{ButtonStyle, IconName, button, settings, theme};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::widgets::{picker_button, setting_row, stepped, stepper};
use super::{SettingsEvent, SettingsPage, Steps, group_title};
use crate::prefs::pickers::FontTarget;

pub(super) fn render_terminal(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let t = &config.terminal;
    let c = theme(cx).colors.clone();
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
    let size_control = div()
        .flex()
        .items_center()
        .gap_2()
        .when(t.font_size.is_some(), |el| {
            let writer = writer.clone();
            el.child(
                button("terminal-size-reset", "Use editor size")
                    .style(ButtonStyle::Ghost)
                    .on_click(move |_, _, cx| {
                        writer.write(vec![SettingEdit::remove("terminal.font_size")], cx)
                    }),
            )
        })
        .child(stepper(
            "terminal.font_size",
            format!("{size:.1}"),
            move |delta, _, cx| {
                let next = stepped(size, delta, 1., 6., 72.);
                writer.set("terminal.font_size", next, cx);
            },
            cx,
        ));
    let shell = t.shell.clone().unwrap_or_else(|| {
        if cfg!(windows) {
            "PowerShell (default)".into()
        } else {
            "$SHELL (default)".into()
        }
    });
    div()
        .child(group_title("Font", cx))
        .child(setting_row("Family", "", font, cx))
        .child(setting_row("Size", "", size_control, cx))
        .child(group_title("Shell", cx))
        .child(setting_row(
            "Program",
            "Set `shell` and `args` under [terminal] in config.toml to change it.",
            div().text_sm().text_color(c.text_muted).child(shell),
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
    let w = &config.workspace;
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
    div()
        .child(group_title("Workspace", cx))
        .child(page.bool_row(
            "Restore sessions",
            "Reopen a project's tabs, splits, cursors and panels.",
            "workspace.restore_session",
            w.restore_session,
            cx,
        ))
        .child(page.bool_row(
            "Reopen projects at start",
            "",
            "workspace.reopen_projects",
            w.reopen_projects,
            cx,
        ))
        .child(page.bool_row(
            "Confirm deletes",
            "",
            "workspace.confirm_delete",
            w.confirm_delete,
            cx,
        ))
        .child(page.bool_row(
            "Watch files",
            "Show changes made outside Arcode (no polling).",
            "workspace.watch_files",
            w.watch_files,
            cx,
        ))
        .child(page.bool_row(
            "Package banner",
            "Say when a project's packages are not installed (nothing installs by itself).",
            "workspace.package_banner",
            w.package_banner,
            cx,
        ))
        .child(group_title("Search", cx))
        .child(page.bool_row(
            "Respect .gitignore",
            "",
            "search.respect_gitignore",
            config.search.respect_gitignore,
            cx,
        ))
        .child(page.bool_row(
            "Include hidden files",
            "",
            "search.include_hidden",
            config.search.include_hidden,
            cx,
        ))
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
        .child(group_title("Large files", cx))
        .child({
            let stepper = large_file_stepper(page, config.editor.large_file_bytes, cx);
            setting_row(
                "Large file size (MB)",
                "From this size only the visible part is parsed for highlighting.",
                stepper,
                cx,
            )
        })
        .child(group_title("Configuration file", cx))
        .child(div().py_2().text_xs().text_color(c.text_muted).child(path))
        .child(div().w_full().max_w(px(600.)).child(actions))
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
