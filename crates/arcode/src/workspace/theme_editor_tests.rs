//! The theme editor in a workspace: live theming, coalesced drags,
//! discard, "Save as" into the themes folder and the unsaved-changes
//! question on close.

use std::fs;
use std::path::PathBuf;

use arcode_config::theme::TokenRef;
use arcode_config::{Color, Config};
use arcode_ui_kit::{hsla, theme};
use gpui::{TestAppContext, VisualTestContext, point, px};

use super::tests::open;
use super::*;
use crate::actions::CloseTab;
use crate::prefs::store::ConfigPathOverride;
use crate::theme_editor::{EditCurrentTheme, ThemeEditor};

pub(super) const RED: Color = Color::rgb(220, 30, 40);
pub(super) const GREEN: Color = Color::rgb(20, 160, 60);

/// A workspace using `config.toml` in a temporary folder (themes go to its
/// `themes` folder), with the theme editor open on Arcode Ember.
pub(super) fn editor_on_ember(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<ThemeEditor>,
    &mut VisualTestContext,
    tempfile::TempDir,
    PathBuf,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    let source = "[ui]\ntheme = \"Arcode Ember\"\n";
    fs::write(&path, source).expect("write");
    let (ws, cx) = open(cx);
    let p = path.clone();
    cx.update(|window, cx| {
        cx.set_global(ConfigPathOverride(p));
        arcode_ui_kit::set_settings(Config::from_toml_str(source).0, cx);
        crate::theme_sync::restore(window.appearance(), cx);
    });
    cx.dispatch_action(EditCurrentTheme);
    cx.run_until_parked();
    let editor = ws.read_with(cx, |ws, _| ws.theme_editor().cloned().expect("editor"));
    (ws, editor, cx, dir, path)
}

pub(super) fn active_accent(cx: &mut VisualTestContext) -> gpui::Hsla {
    cx.update(|_, cx| theme(cx).colors.accent)
}

#[gpui::test]
fn changing_a_token_updates_the_live_theme(cx: &mut TestAppContext) {
    let (ws, editor, cx, _dir, _) = editor_on_ember(cx);
    ws.read_with(cx, |ws, cx| {
        let page = ws
            .active_pane()
            .read(cx)
            .active_page()
            .map(|p| p.key.to_string());
        assert_eq!(page.as_deref(), Some(crate::theme_editor::PAGE_KEY));
    });
    let before = editor.read_with(cx, |e, _| e.applies());
    editor.update(cx, |e, cx| e.set_color(&TokenRef::ui("accent"), RED, cx));
    assert_eq!(active_accent(cx), hsla(RED));
    editor.update(cx, |e, cx| {
        e.set_color(&TokenRef::syntax("keyword"), GREEN, cx)
    });
    cx.update(|_, cx| {
        let t = theme(cx);
        assert_eq!(t.name, "Arcode Ember", "the preview keeps the name");
        assert_eq!(t.syntax("keyword").color, Some(hsla(GREEN)));
    });
    // One re-theme per change; the same colour again costs nothing.
    editor.update(cx, |e, cx| e.set_color(&TokenRef::ui("accent"), RED, cx));
    editor.read_with(cx, |e, _| {
        assert_eq!(e.applies(), before + 2);
        assert!(e.has_unsaved_changes());
    });
    // Opening it again shows the same page.
    cx.dispatch_action(EditCurrentTheme);
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.theme_editor(), Some(&editor));
        assert_eq!(ws.active_pane().read(cx).pages().len(), 1);
    });
}

#[gpui::test]
fn picker_drags_apply_once(cx: &mut TestAppContext) {
    let (_ws, editor, cx, _dir, _) = editor_on_ember(cx);
    let accent = TokenRef::ui("accent");
    editor.update_in(cx, |e, window, cx| {
        e.open_picker(accent.clone(), point(px(100.), px(100.)), window, cx)
    });
    let before = editor.read_with(cx, |e, _| e.applies());
    // Many moves before the next frame: the edit follows each one, the
    // app is re-themed once.
    editor.update_in(cx, |e, window, cx| {
        for v in 0..20u8 {
            e.set_color_coalesced(&accent, Color::rgb(200, 10 * v, 40), window, cx);
        }
        assert_eq!(e.applies(), before);
        assert_eq!(e.edit().working().ui.accent, Color::rgb(200, 190, 40));
    });
    assert_eq!(
        editor.read_with(cx, |e, _| e.picker_token().cloned()),
        Some(accent.clone())
    );
    // Closing the picker shows the last colour.
    editor.update(cx, |e, cx| e.close_picker(cx));
    editor.read_with(cx, |e, _| assert_eq!(e.applies(), before + 1));
    assert_eq!(active_accent(cx), hsla(Color::rgb(200, 190, 40)));
}

#[gpui::test]
fn discard_restores_the_theme_in_use(cx: &mut TestAppContext) {
    let (_ws, editor, cx, _dir, _) = editor_on_ember(cx);
    let ember = active_accent(cx);
    editor.update(cx, |e, cx| e.set_color(&TokenRef::ui("accent"), RED, cx));
    assert_ne!(active_accent(cx), ember);
    editor.update_in(cx, |e, window, cx| e.discard(window, cx));
    assert_eq!(active_accent(cx), ember);
    editor.read_with(cx, |e, _| assert!(!e.has_unsaved_changes()));
}

#[gpui::test]
fn save_as_writes_a_user_theme_and_selects_it(cx: &mut TestAppContext) {
    let (_ws, editor, cx, dir, config) = editor_on_ember(cx);
    editor.update(cx, |e, cx| {
        e.set_color(&TokenRef::ui("accent"), RED, cx);
        e.set_color(&TokenRef::syntax("keyword"), GREEN, cx);
    });
    // A built-in name is refused.
    editor.update_in(cx, |e, window, cx| e.save_as("arcode chalk", window, cx));
    cx.run_until_parked();
    assert!(!dir.path().join("themes").exists());

    editor.update_in(cx, |e, window, cx| e.save_as("My Ember", window, cx));
    cx.run_until_parked();
    let file = dir.path().join("themes/my-ember.toml");
    let text = fs::read_to_string(&file).expect("saved");
    assert!(
        text.contains("name = \"My Ember\"\nappearance = \"dark\""),
        "{text}"
    );
    assert!(text.contains("accent = \"#dc1e28\""), "{text}");
    let config_text = fs::read_to_string(&config).expect("config");
    assert!(
        config_text.contains("theme = \"My Ember\""),
        "{config_text}"
    );
    cx.update(|_, cx| {
        let t = theme(cx);
        assert_eq!(t.name, "My Ember");
        assert_eq!(t.colors.accent, hsla(RED));
        let registry = &cx.global::<crate::theme_sync::Themes>().0;
        let builtin = arcode_config::ThemeRegistry::builtin();
        assert_eq!(registry.get("Arcode Ember"), builtin.get("Arcode Ember"));
        assert_eq!(registry.get("My Ember").map(|t| t.ui.accent), Some(RED));
    });
    editor.read_with(cx, |e, _| {
        assert!(!e.has_unsaved_changes());
        assert!(!e.edit().is_builtin());
    });
    // The saved theme loads from the folder as it was saved.
    let (registry, warnings) = arcode_config::ThemeRegistry::load(Some(&dir.path().join("themes")));
    assert!(warnings.is_empty(), "{warnings:?}");
    let saved = editor.read_with(cx, |e, _| e.edit().working().clone());
    assert_eq!(registry.get("My Ember"), Some(&saved));

    // Editing the user theme saves in place.
    editor.update_in(cx, |e, window, cx| {
        e.set_color(&TokenRef::ui("accent"), GREEN, cx);
        e.save(false, window, cx);
    });
    cx.run_until_parked();
    let text = fs::read_to_string(&file).expect("saved");
    assert!(text.contains("accent = \"#14a03c\""), "{text}");
    assert_eq!(fs::read_dir(dir.path().join("themes")).unwrap().count(), 1);
}

#[gpui::test]
fn closing_with_unsaved_changes_asks(cx: &mut TestAppContext) {
    let (ws, editor, cx, _dir, _) = editor_on_ember(cx);
    let ember = active_accent(cx);
    // Unchanged: closes at once.
    cx.dispatch_action(CloseTab);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    ws.read_with(cx, |ws, cx| {
        assert!(ws.active_pane().read(cx).pages().is_empty());
        assert!(ws.theme_editor().is_none(), "the closed page is forgotten");
    });

    cx.dispatch_action(EditCurrentTheme);
    cx.run_until_parked();
    let editor2 = ws.read_with(cx, |ws, _| ws.theme_editor().cloned().expect("editor"));
    assert_ne!(editor2, editor);
    editor2.update(cx, |e, cx| e.set_color(&TokenRef::ui("accent"), RED, cx));
    cx.dispatch_action(CloseTab);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asks before losing the edits");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.active_pane().read(cx).pages().len(), 1)
    });
    assert_eq!(active_accent(cx), hsla(RED));

    cx.dispatch_action(CloseTab);
    cx.run_until_parked();
    cx.simulate_prompt_answer("Don't Save");
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert!(ws.active_pane().read(cx).pages().is_empty());
        assert!(ws.theme_editor().is_none());
    });
    assert_eq!(active_accent(cx), ember, "the theme in use is back");
}
