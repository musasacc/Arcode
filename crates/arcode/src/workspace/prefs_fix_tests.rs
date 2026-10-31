//! Fixes of the Settings unit review: the tour traps workspace shortcuts,
//! theme card hover previews end with their card, and Settings > Keymap
//! shows Vim while `vim_mode` is on.

use std::fs;

use gpui::{SharedString, TestAppContext, VisualTestContext};

use super::tests::{open, project_dir};
use super::*;
use crate::actions::OpenSettings;
use crate::prefs::store::ConfigPathOverride;

fn with_config<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (
    Entity<Workspace>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(&path, source).expect("write");
    let (ws, cx) = open(cx);
    cx.update(|_, cx| cx.set_global(ConfigPathOverride(path)));
    cx.update(|_, cx| crate::prefs::store::reload(cx));
    cx.run_until_parked();
    (ws, cx, dir)
}

fn theme_name(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| arcode_ui_kit::theme(cx).name.clone())
}

fn midnight(cx: &mut VisualTestContext) -> arcode_config::Theme {
    cx.update(|_, cx| {
        let themes = &cx.global::<crate::theme_sync::Themes>().0;
        themes.get("Arcode Midnight").expect("built in").clone()
    })
}

#[gpui::test]
fn the_tour_blocks_workspace_shortcuts(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx, _cfg) = with_config(cx, "");
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.start_tour(window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("secondary-p");
    cx.simulate_keystrokes("secondary-shift-p");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.tour().is_some());
        assert!(ws.finder.is_none() && ws.palette().is_none());
    });
    // The tour's own keys still work, and the shortcuts come back after.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| assert!(ws.tour().is_none()));
    cx.simulate_keystrokes("secondary-p");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| assert!(ws.finder.is_some()));
}

#[gpui::test]
fn moving_between_theme_cards_keeps_the_new_preview(cx: &mut TestAppContext) {
    let (ws, cx, _dir) = with_config(cx, "[ui]\ntheme = \"dark\"\n");
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    let page = ws.read_with(cx, |ws, _| ws.settings_page().cloned().expect("page"));
    let theme = midnight(cx);
    let (dark, night) = (SharedString::from("a"), SharedString::from("b"));
    page.update_in(cx, |page, window, cx| {
        page.hover_theme(&dark, None, true, window, cx);
        // The next card is entered before the previous one reports leaving.
        page.hover_theme(&night, Some(&theme), true, window, cx);
        page.hover_theme(&dark, None, false, window, cx);
    });
    assert_eq!(theme_name(cx), "Arcode Midnight");
    page.update_in(cx, |page, window, cx| {
        page.hover_theme(&night, Some(&theme), false, window, cx)
    });
    assert_eq!(theme_name(cx), "Arcode Ember");
}

#[gpui::test]
fn closing_the_settings_tab_ends_a_hover_preview(cx: &mut TestAppContext) {
    let (ws, cx, _dir) = with_config(cx, "[ui]\ntheme = \"dark\"\n");
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    let page = ws.read_with(cx, |ws, _| ws.settings_page().cloned().expect("page"));
    let theme = midnight(cx);
    page.update_in(cx, |page, window, cx| {
        page.hover_theme(&"b".into(), Some(&theme), true, window, cx)
    });
    assert_eq!(theme_name(cx), "Arcode Midnight");
    let pane = ws.read_with(cx, |ws, _| ws.active_pane().clone());
    pane.update_in(cx, |pane, window, cx| pane.close_page(0, window, cx));
    cx.run_until_parked();
    assert_eq!(
        theme_name(cx),
        "Arcode Ember",
        "the preview ended with the tab"
    );
}
