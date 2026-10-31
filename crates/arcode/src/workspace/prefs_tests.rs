//! Settings page, pickers with live preview, first run, tour and panel
//! animations in a workspace.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext};

use super::tests::{open, project_dir};
use super::*;
use crate::actions::{CloseTab, OpenSettings, ToggleSidebar};
use crate::list_picker::ListPickerEvent;
use crate::prefs::store::ConfigPathOverride;
use crate::prefs::tour::TourTarget;
use crate::prefs::{ShowWelcomeSetup, StartTour, ToggleLowEndMode};

/// A workspace whose settings writes go to a temporary `config.toml`.
fn open_with_config<'a>(
    cx: &'a mut TestAppContext,
    source: Option<&str>,
) -> (
    Entity<Workspace>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
    PathBuf,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    if let Some(source) = source {
        fs::write(&path, source).expect("write");
    }
    let (ws, cx) = open(cx);
    let p = path.clone();
    cx.update(|_, cx| cx.set_global(ConfigPathOverride(p)));
    (ws, cx, dir, path)
}

fn theme_name(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| arcode_ui_kit::theme(cx).name.clone())
}

#[gpui::test]
fn settings_page_is_a_tab_and_writes_config(cx: &mut TestAppContext) {
    let (ws, cx, _dir, path) = open_with_config(cx, Some("# mine\n[editor]\ntab_size = 4\n"));
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    let pane = ws.read_with(cx, |ws, _| ws.active_pane().clone());
    ws.update(cx, |ws, cx| {
        assert!(
            !ws.shows_welcome(cx),
            "the page replaces the welcome screen"
        );
        assert!(ws.settings_page().is_some());
        assert!(ws.active_editor(cx).is_none());
        let pane = ws.active_pane().read(cx);
        assert_eq!(pane.active_page().map(|p| p.key.as_ref()), Some("settings"));
    });
    // Opening it again activates the same tab.
    cx.dispatch_action(OpenSettings);
    pane.read_with(cx, |pane, _| assert_eq!(pane.pages().len(), 1));

    let page = ws.read_with(cx, |ws, _| ws.settings_page().cloned().expect("page"));
    page.update(cx, |page, cx| {
        page.set_section(crate::settings_ui::Section::Editor, cx);
        page.writer().set("editor.tab_size", 2u64, cx);
    });
    cx.run_until_parked();
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.starts_with("# mine\n"), "{text}");
    assert!(text.contains("tab_size = 2"), "{text}");
    cx.update(|_, cx| assert_eq!(arcode_ui_kit::settings(cx).editor.tab_size, 2));

    cx.dispatch_action(CloseTab);
    cx.run_until_parked();
    pane.read_with(cx, |pane, _| assert!(pane.pages().is_empty()));
    ws.update(cx, |ws, cx| assert!(ws.shows_welcome(cx)));
}

#[gpui::test]
fn theme_picker_previews_and_restores(cx: &mut TestAppContext) {
    let (ws, cx, _dir, path) = open_with_config(cx, Some("[ui]\ntheme = \"dark\"\n"));
    cx.update(|_, cx| crate::prefs::store::reload(cx));
    cx.run_until_parked();
    assert_eq!(theme_name(cx), "Arcode Dark");
    ws.update_in(cx, |ws, window, cx| {
        ws.open_theme_picker(crate::prefs::pickers::ThemeSlot::Main, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| p.set_query("midnight", cx));
    cx.run_until_parked();
    assert_eq!(theme_name(cx), "Arcode Midnight", "highlight previews");
    picker.update(cx, |_, cx| cx.emit(ListPickerEvent::Dismissed));
    cx.run_until_parked();
    assert_eq!(theme_name(cx), "Arcode Dark", "dismiss restores");
    assert!(ws.read_with(cx, |ws, _| ws.prefs_picker().is_none()));

    ws.update_in(cx, |ws, window, cx| {
        ws.open_theme_picker(crate::prefs::pickers::ThemeSlot::Main, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| {
        p.set_query("midnight", cx);
        p.confirm(cx);
    });
    cx.run_until_parked();
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("theme = \"Arcode Midnight\""), "{text}");
    assert_eq!(theme_name(cx), "Arcode Midnight");
}

#[gpui::test]
fn font_picker_previews_and_writes(cx: &mut TestAppContext) {
    let (ws, cx, _dir, path) = open_with_config(cx, Some(""));
    ws.update_in(cx, |ws, window, cx| {
        ws.open_font_picker(crate::prefs::pickers::FontTarget::Ui, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| p.set_query("jetbrains", cx));
    cx.run_until_parked();
    let previewed = cx.update(|_, cx| arcode_ui_kit::settings(cx).ui.font_family.clone());
    assert_eq!(previewed.as_deref(), Some("JetBrains Mono"));
    picker.update(cx, |_, cx| cx.emit(ListPickerEvent::Dismissed));
    cx.run_until_parked();
    let restored = cx.update(|_, cx| arcode_ui_kit::settings(cx).ui.font_family.clone());
    assert_eq!(restored, None);

    ws.update_in(cx, |ws, window, cx| {
        ws.open_font_picker(crate::prefs::pickers::FontTarget::Ui, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| {
        p.set_query("jetbrains", cx);
        p.confirm(cx);
    });
    cx.run_until_parked();
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("font_family = \"JetBrains Mono\""), "{text}");
}

#[gpui::test]
fn first_run_writes_choices_then_offers_the_tour(cx: &mut TestAppContext) {
    let (ws, cx, _dir, path) = open_with_config(cx, None);
    cx.dispatch_action(ShowWelcomeSetup);
    cx.run_until_parked();
    let dialog = ws.read_with(cx, |ws, _| ws.first_run_dialog().cloned().expect("dialog"));
    dialog.update_in(cx, |d, window, cx| {
        d.choose_theme("Arcode Midnight", window, cx);
        d.choices.keymap = arcode_config::Keymap::Vim;
        d.choices.low_end = true;
    });
    assert_eq!(theme_name(cx), "Arcode Midnight", "choosing previews");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(dialog.read_with(cx, |d, _| d.saved));
    let text = fs::read_to_string(&path).expect("read");
    assert!(
        text.starts_with("# Arcode configuration"),
        "the commented template"
    );
    let (config, warnings) = arcode_config::Config::from_toml_str(&text);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.editor.keymap, arcode_config::Keymap::Vim);
    assert!(config.ui.low_end_mode && !config.editor.inlay_hints);
    cx.update(|_, cx| assert!(arcode_ui_kit::animation_duration(cx).is_none()));

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.first_run_dialog().is_none());
        assert_eq!(ws.tour().map(|t| t.step), Some(0), "the tour starts");
    });
}

#[gpui::test]
fn skipping_first_run_writes_the_template(cx: &mut TestAppContext) {
    let (ws, cx, _dir, path) = open_with_config(cx, None);
    cx.dispatch_action(ShowWelcomeSetup);
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.first_run_dialog().is_none()));
    let text = fs::read_to_string(&path).expect("read");
    assert_eq!(text, arcode_config::Config::default_toml());
}

#[gpui::test]
fn tour_walks_the_steps_and_restores_the_panel(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx, _cfg, _path) = open_with_config(cx, Some(""));
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    cx.dispatch_action(StartTour);
    cx.run_until_parked();
    let steps = ws.read_with(cx, |ws, _| ws.tour().map(|t| t.steps.len()).unwrap_or(0));
    assert!(steps >= 5);
    // The overlay has focus: arrows move between steps.
    cx.simulate_keystrokes("right right left");
    assert_eq!(ws.read_with(cx, |ws, _| ws.tour().map(|t| t.step)), Some(1));
    let mut opened = false;
    for _ in 2..steps {
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        ws.read_with(cx, |ws, _| {
            if ws.tour().map(|t| t.current().target) == Some(TourTarget::TerminalPanel) {
                assert!(ws.bottom_panel_visible, "the terminal step shows the panel");
                opened = true;
            }
        });
    }
    assert!(opened);
    ws.read_with(cx, |ws, _| {
        assert!(ws.tour().is_some_and(|t| t.is_last()));
        // Bounds of drawn targets were recorded.
        assert!(ws.tour_anchors().get(TourTarget::StatusBar).is_some());
        assert!(ws.tour_anchors().get(TourTarget::SearchBox).is_some());
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.tour().is_none(), "finish ends the tour");
        assert!(
            !ws.bottom_panel_visible,
            "the panel it opened is hidden again"
        );
    });
}

#[gpui::test]
fn panel_toggles_slide_only_with_animations(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx, _cfg, path) = open_with_config(cx, Some(""));
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    cx.dispatch_action(ToggleSidebar);
    ws.read_with(cx, |ws, _| {
        assert!(!ws.sidebar_visible);
        assert!(
            ws.sliding_closed(super::anim::Slider::Sidebar),
            "still drawn while closing"
        );
    });
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(
            !ws.prefs.anim.sidebar.running(),
            "no frames after the animation"
        )
    });

    cx.dispatch_action(ToggleLowEndMode);
    cx.run_until_parked();
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("low_end_mode = true"), "{text}");
    cx.dispatch_action(ToggleSidebar);
    ws.read_with(cx, |ws, _| {
        assert!(ws.sidebar_visible);
        assert!(
            !ws.prefs.anim.sidebar.running(),
            "low-end mode does not animate"
        );
    });
}
