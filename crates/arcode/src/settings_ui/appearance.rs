//! Appearance: theme (cards with a live preview on hover), dark and light
//! themes, UI size, animations, low-end mode, file icons and the menu bar.

use arcode_config::{Appearance, FileIcons, ThemeChoice, low_end_edits};
use arcode_ui_kit::{ButtonStyle, IconName, button, settings, theme, type_scale};
use gpui::{App, Context, Div, SharedString, div, prelude::*, px};

use super::theme_card::{CardKind, card_subtitle, theme_card};
use super::widgets::{picker_button, segmented, setting_row};
use super::{SettingsEvent, SettingsPage, Steps, filter, group_title};
use crate::prefs::pickers::ThemeSlot;
use crate::theme_sync::Themes;

pub(super) fn render(page: &SettingsPage, cx: &mut Context<SettingsPage>) -> Div {
    let config = settings(cx).clone();
    let ui = &config.ui;
    let c = theme(cx).colors.clone();
    let registry = cx.global::<Themes>().0.clone();
    let writer = page.writer();
    let file_icons = segmented(
        "ui-file-icons",
        &["Language logos", "Plain"],
        FileIcons::ALL
            .iter()
            .position(|f| *f == ui.file_icons)
            .unwrap_or(0),
        move |ix, _, cx| {
            let choice = FileIcons::ALL[ix.min(FileIcons::ALL.len() - 1)];
            writer.set("ui.file_icons", choice.as_str(), cx)
        },
        cx,
    );

    let mut choices = vec![(ThemeChoice::System, "Follow system".to_owned())];
    choices.extend(
        registry
            .names()
            .into_iter()
            .map(|n| (ThemeChoice::Named(n.to_owned()), n.to_owned())),
    );
    // Dark and Light mode use the dark or light theme: its card is selected.
    let shown = match &ui.theme {
        ThemeChoice::Dark => ThemeChoice::Named(ui.dark_theme.clone()),
        ThemeChoice::Light => ThemeChoice::Named(ui.light_theme.clone()),
        other => other.clone(),
    };
    let mut cards = Vec::new();
    for (value, label) in choices {
        let (first, second) = match &value {
            ThemeChoice::System => (registry.get(&ui.dark_theme), registry.get(&ui.light_theme)),
            ThemeChoice::Dark => (registry.get(&ui.dark_theme), None),
            ThemeChoice::Light => (registry.get(&ui.light_theme), None),
            ThemeChoice::Named(name) => (registry.get(name), None),
        };
        let selected = value == shown;
        let kind = match &value {
            ThemeChoice::Named(name) => CardKind::Named {
                name,
                appearance: first.map_or(Appearance::Dark, |t| t.appearance),
            },
            _ => CardKind::System {
                dark: &ui.dark_theme,
                light: &ui.light_theme,
            },
        };
        let subtitle = card_subtitle(kind, selected);
        let preview = first.filter(|_| value != ThemeChoice::System).cloned();
        let writer = page.writer();
        let id = SharedString::from(format!("theme-card-{label}"));
        let card = theme_card(id.clone(), label, subtitle, first, second, selected, cx)
            .on_hover(cx.listener(move |page, hovered: &bool, window, cx| {
                page.hover_theme(&id, preview.as_ref(), *hovered, window, cx)
            }))
            .on_click(move |_, _, cx| writer.set("ui.theme", value.as_str(), cx));
        cards.push(card);
    }
    let cards = div()
        .flex()
        .flex_wrap()
        .gap_x(px(14.))
        .gap_y(px(16.))
        .pt(px(12.))
        .pb(px(10.))
        .border_b_1()
        .border_color(c.border)
        .children(cards);
    // While the page filter is on, only matching rows show.
    let theme_intro = (!filter::active()).then(|| {
        div()
            .child(group_title("Theme", cx))
            .child(
                div()
                    .text_size(type_scale(cx).meta)
                    .text_color(c.text_muted)
                    .child("Hover a theme to preview it, click to use it."),
            )
            .child(cards)
    });

    let mode = match ui.theme {
        ThemeChoice::System => 0,
        ThemeChoice::Dark => 1,
        ThemeChoice::Light => 2,
        // A theme picked by name is always dark or always light.
        ThemeChoice::Named(ref name) => match registry.get(name).map(|t| t.appearance) {
            Some(arcode_config::Appearance::Light) => 2,
            Some(_) => 1,
            None => usize::MAX,
        },
    };
    let writer = page.writer();
    let modes = segmented(
        "theme-mode",
        &["System", "Dark", "Light"],
        mode,
        move |ix, _, cx| {
            let value = ["system", "dark", "light"][ix.min(2)];
            writer.set("ui.theme", value, cx);
        },
        cx,
    );
    let slot_button = |slot: ThemeSlot, value: &str, cx: &mut Context<SettingsPage>| {
        picker_button(
            SharedString::from(format!("pick-{}", slot.key())),
            value.to_owned(),
            cx.listener(move |_, _, _, cx| cx.emit(SettingsEvent::PickTheme(slot))),
            cx,
        )
    };
    let themes_dir = arcode_config::paths::themes_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "the themes folder".into());
    let low_end = ui.low_end_mode;
    let writer = page.writer();
    let low_end_switch = super::widgets::toggle("ui.low_end_mode", low_end, move |on, _, cx| {
        writer.write(low_end_edits(on), cx)
    });

    div()
        .children(theme_intro)
        .child(page.keyed_row(
            "Mode",
            "Follow the system appearance, or always use the dark or the light theme.",
            "ui.theme",
            modes,
            cx,
        ))
        .child(page.keyed_row(
            "Dark theme",
            "Used in dark mode and when following a dark system.",
            "ui.dark_theme",
            slot_button(ThemeSlot::Dark, &ui.dark_theme, cx),
            cx,
        ))
        .child(page.keyed_row(
            "Light theme",
            "Used in light mode and when following a light system.",
            "ui.light_theme",
            slot_button(ThemeSlot::Light, &ui.light_theme, cx),
            cx,
        ))
        .child(setting_row(
            "All themes",
            format!(
                "Make a theme of your own, change any colour in the theme editor, or pick another theme. Your own themes are .toml files in {themes_dir}."
            ),
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(
                    button("new-theme", "New Theme…")
                        .icon(IconName::Plus)
                        .style(ButtonStyle::Secondary)
                        .tooltip("Make a theme of your own, starting from any theme")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::theme_editor::NewTheme), cx)
                        }),
                )
                .child(
                    button("edit-theme", "Edit Theme…")
                        .style(ButtonStyle::Secondary)
                        .tooltip("Change the colours of the theme in use, with a live preview")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(
                                Box::new(crate::theme_editor::EditCurrentTheme),
                                cx,
                            )
                        }),
                )
                .child(
                    button("browse-themes", "Select Theme…")
                        .icon(IconName::Palette)
                        .style(ButtonStyle::Secondary)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(SettingsEvent::PickTheme(ThemeSlot::Main))
                        })),
                ),
            cx,
        ))
        .children(user_theme_rows(&registry, cx))
        .child(group_title("Interface", cx))
        .child(page.number_row(
            "UI size",
            "Text size of the workbench; the code size is under Fonts.",
            "ui.font_size",
            ui.font_size,
            Steps { step: 1., min: 8., max: 32., int: false },
            cx,
        ))
        // Low-end mode turns animations off whatever the setting says: the
        // switch then shows them off and cannot be flipped.
        .child(page.low_end_bool_row(
            "Animations",
            "Short (100–150 ms) transitions for panels, popups and messages.",
            "Off while low-end mode is on.",
            "ui.animations",
            ui.animations,
            cx,
        ))
        .child(page.number_row(
            "Animation duration (ms)",
            "",
            "ui.animation_ms",
            ui.animation_ms as f32,
            Steps { step: 10., min: 50., max: 500., int: true },
            cx,
        ))
        .child(page.keyed_row_with_reset(
            "Low-end mode",
            "No animations or cursor blink, flat popups, no inlay hints or inline blame, slower background refreshes.",
            "ui.low_end_mode",
            low_end.then(|| low_end_edits(false)),
            low_end_switch,
            cx,
        ))
        .child(page.keyed_row(
            "File icons",
            "Next to file names in the file tree, tabs and lists: the logo of the file's language, or plain monochrome icons.",
            "ui.file_icons",
            file_icons,
            cx,
        ))
        .child(page.bool_row(
            "Show performance stats",
            "Startup time and memory use in the footer.",
            "ui.show_perf_stats",
            ui.show_perf_stats,
            cx,
        ))
        // macOS always shows the menus in its own menu bar.
        .when(!cfg!(target_os = "macos"), |el| {
            el.child(page.bool_row(
                "Menu bar",
                "Show File, Edit, Selection and the other menus in the title bar. When off, or in a narrow window, one menu button holds them; Alt opens them either way.",
                "ui.menu_bar",
                ui.menu_bar,
                cx,
            ))
        })
}

/// A row for each of the user's own themes, with Rename… and Delete…
/// (built-in themes cannot be renamed or deleted).
fn user_theme_rows(registry: &arcode_config::ThemeRegistry, cx: &App) -> Vec<Div> {
    registry
        .themes()
        .iter()
        .filter(|t| registry.is_user_theme(&t.name))
        .map(|t| {
            let mode = match t.appearance {
                Appearance::Dark => "Your theme · dark",
                Appearance::Light => "Your theme · light",
            };
            let name = SharedString::from(t.name.clone());
            let (rename, delete) = (name.clone(), name.clone());
            let controls = div()
                .flex()
                .gap(px(8.))
                .child(
                    button(
                        SharedString::from(format!("rename-theme-{name}")),
                        "Rename…",
                    )
                    .style(ButtonStyle::Ghost)
                    .on_click(move |_, window, cx| {
                        let name = rename.clone();
                        window.dispatch_action(
                            Box::new(crate::theme_editor::RenameTheme { name }),
                            cx,
                        )
                    }),
                )
                .child(
                    button(
                        SharedString::from(format!("delete-theme-{name}")),
                        "Delete…",
                    )
                    .icon(IconName::Trash)
                    .style(ButtonStyle::Ghost)
                    .on_click(move |_, window, cx| {
                        let name = delete.clone();
                        window.dispatch_action(
                            Box::new(crate::theme_editor::DeleteTheme { name }),
                            cx,
                        )
                    }),
                );
            setting_row(name, mode, controls, cx)
        })
        .collect()
}
