//! The theme editor and the rest of the window: closing it or the project
//! forgets it and shows the configured theme again, the edited theme
//! stays shown while something else re-themes the app, the picker closes
//! from the keyboard and a narrow page stacks the preview.

use std::fs;

use arcode_config::theme::TokenRef;
use arcode_config::{Config, ThemeRegistry};
use arcode_ui_kit::{hsla, theme};
use gpui::{TestAppContext, VisualTestContext, point, px, size};

use super::tests::project_dir;
use super::theme_editor_tests::{RED, active_accent, editor_on_ember};
use super::*;
use crate::actions::CloseTab;

fn active_name(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| theme(cx).name.clone())
}

fn pages(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    ws.read_with(cx, |ws, cx| ws.active_pane().read(cx).pages().len())
}

fn midnight() -> arcode_config::Theme {
    ThemeRegistry::builtin()
        .get("Arcode Midnight")
        .cloned()
        .expect("built in")
}

/// An unchanged page closed with its tab is forgotten: a theme file saved
/// later does not bring its theme back.
#[gpui::test]
fn closing_an_unchanged_page_forgets_it(cx: &mut TestAppContext) {
    let (ws, editor, cx, _dir, _) = editor_on_ember(cx);
    editor.update_in(cx, |e, window, cx| e.start_from(midnight(), window, cx));
    assert_eq!(active_name(cx), "Arcode Midnight");
    cx.dispatch_action(CloseTab);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    ws.read_with(cx, |ws, _| assert!(ws.theme_editor().is_none()));
    assert_eq!(pages(&ws, cx), 0);
    assert_eq!(active_name(cx), "Arcode Ember");
    // Re-theming (a settings change) keeps the configured theme.
    cx.update(|window, cx| crate::theme_sync::apply(window.appearance(), cx));
    assert_eq!(active_name(cx), "Arcode Ember");
}

/// Closing the project, or opening another one, asks before dropping
/// theme edits; going ahead closes the page and shows the configured
/// theme again.
#[gpui::test]
fn leaving_the_project_asks_about_theme_edits(cx: &mut TestAppContext) {
    let first = project_dir();
    let second = project_dir();
    let (ws, editor, cx, _dir, _) = editor_on_ember(cx);
    let ember = active_accent(cx);
    let root = fs::canonicalize(first.path()).expect("canonical");
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    // Opening a project replaced the panes: the unchanged page went.
    ws.read_with(cx, |ws, _| assert!(ws.theme_editor().is_none()));
    assert!(!editor.read_with(cx, |e, _| e.has_unsaved_changes()));

    cx.dispatch_action(crate::theme_editor::EditCurrentTheme);
    cx.run_until_parked();
    let editor = ws.read_with(cx, |ws, _| ws.theme_editor().cloned().expect("editor"));
    editor.update(cx, |e, cx| e.set_color(&TokenRef::ui("accent"), RED, cx));

    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asks before dropping the edits");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.project_root().is_some());
        assert_eq!(ws.theme_editor(), Some(&editor));
    });
    assert_eq!(pages(&ws, cx), 1);
    assert_eq!(active_accent(cx), hsla(RED));

    let other = fs::canonicalize(second.path()).expect("canonical");
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(other.clone(), window, cx)
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asks before dropping the edits");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.project_root(), Some(other.as_path()));
        assert!(ws.theme_editor().is_none());
    });
    assert_eq!(pages(&ws, cx), 0);
    assert_eq!(active_accent(cx), ember, "the configured theme is back");
    cx.update(|window, cx| crate::theme_sync::apply(window.appearance(), cx));
    assert_eq!(active_accent(cx), ember);
}

/// While the page is open, re-theming the app (a settings change, the OS
/// switching between dark and light) keeps the edited theme shown.
#[gpui::test]
fn the_edited_theme_stays_shown(cx: &mut TestAppContext) {
    let (_ws, editor, cx, _dir, _) = editor_on_ember(cx);
    editor.update(cx, |e, cx| e.set_color(&TokenRef::ui("accent"), RED, cx));
    cx.update(|window, cx| {
        let source = "[ui]\ntheme = \"Arcode Chalk\"\n";
        arcode_ui_kit::set_settings(Config::from_toml_str(source).0, cx);
        crate::theme_sync::apply(window.appearance(), cx);
        crate::theme_sync::apply(gpui::WindowAppearance::Light, cx);
    });
    assert_eq!(active_accent(cx), hsla(RED));
    assert_eq!(active_name(cx), "Arcode Ember");

    // An unchanged edit of another theme stays shown too.
    editor.update_in(cx, |e, window, cx| e.discard(window, cx));
    assert_eq!(active_name(cx), "Arcode Chalk", "discard shows the setting");
    editor.update_in(cx, |e, window, cx| e.start_from(midnight(), window, cx));
    cx.update(|window, cx| crate::theme_sync::apply(window.appearance(), cx));
    assert_eq!(active_name(cx), "Arcode Midnight");
}

/// The picker takes the keys when it opens: Escape closes it and the page
/// keeps the focus.
#[gpui::test]
fn escape_closes_the_picker(cx: &mut TestAppContext) {
    let (_ws, editor, cx, _dir, _) = editor_on_ember(cx);
    let keyword = TokenRef::syntax("keyword");
    editor.update_in(cx, |e, window, cx| {
        e.open_picker(keyword.clone(), point(px(200.), px(200.)), window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert_eq!(
        editor.read_with(cx, |e, _| e.picker_token().cloned()),
        Some(keyword)
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.picker_token().cloned()), None);
    let page_focused = cx.update(|window, cx| {
        editor
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx)
    });
    assert!(page_focused);
}

/// A page too narrow for the token list and the preview side by side
/// shows the preview under the list.
#[gpui::test]
fn a_narrow_page_stacks_the_preview(cx: &mut TestAppContext) {
    let (_ws, editor, cx, _dir, _) = editor_on_ember(cx);
    let draw = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
    };
    cx.simulate_resize(size(px(1600.), px(900.)));
    draw(cx);
    assert!(!editor.read_with(cx, |e, _| e.is_stacked()));
    cx.simulate_resize(size(px(760.), px(650.)));
    draw(cx);
    assert!(editor.read_with(cx, |e, _| e.is_stacked()));
}
