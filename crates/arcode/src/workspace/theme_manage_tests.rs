//! New Theme, Rename and Delete in a workspace: the dialog, names it
//! refuses, the theme it writes from the chosen base, renaming a theme in
//! use and deleting the theme in use.

use std::fs;
use std::path::{Path, PathBuf};

use arcode_config::theme::{SaveTarget, save_user_theme};
use arcode_config::{Config, Theme, ThemeRegistry};
use arcode_ui_kit::theme;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::Workspace;
use super::tests::open;
use crate::prefs::store::ConfigPathOverride;
use crate::theme_editor::{DeleteTheme, NewTheme, RenameTheme};
use crate::theme_sync::Themes;

/// A workspace whose `config.toml` (and `themes` folder) is in a temporary
/// folder, with `config` in it and the themes `user` saved there.
fn workspace_with<'a>(
    cx: &'a mut TestAppContext,
    config: &str,
    user: &[Theme],
) -> (
    Entity<Workspace>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(&path, config).expect("write");
    let themes = dir.path().join("themes");
    for theme in user {
        save_user_theme(&themes, theme, &[], SaveTarget::New { replace: false }).expect("save");
    }
    let (ws, cx) = open(cx);
    let source = config.to_owned();
    cx.update(|window, cx| {
        cx.set_global(ConfigPathOverride(path));
        let (registry, warnings) = ThemeRegistry::load(Some(&themes));
        assert!(warnings.is_empty(), "{warnings:?}");
        cx.set_global(Themes(registry));
        arcode_ui_kit::set_settings(Config::from_toml_str(&source).0, cx);
        crate::theme_sync::restore(window.appearance(), cx);
    });
    (ws, cx, dir)
}

fn named(name: &str, base: &str) -> Theme {
    Theme {
        name: name.into(),
        ..ThemeRegistry::builtin()
            .get(base)
            .cloned()
            .expect("built in")
    }
}

fn config_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("config.toml")).expect("config")
}

fn theme_file(dir: &Path, stem: &str) -> PathBuf {
    dir.join("themes").join(format!("{stem}.toml"))
}

#[gpui::test]
fn a_new_theme_starts_from_the_chosen_theme_and_opens_the_editor(cx: &mut TestAppContext) {
    let (ws, cx, dir) = workspace_with(cx, "[ui]\ntheme = \"Arcode Ember\"\n", &[]);
    cx.dispatch_action(NewTheme);
    cx.run_until_parked();
    let dialog = ws.read_with(cx, |ws, _| ws.new_theme_dialog().cloned().expect("dialog"));
    dialog.read_with(cx, |d, cx| {
        assert_eq!(d.base().map(|b| b.name.as_ref()), Some("Arcode Ember"));
        assert_eq!(d.name(cx), "My Ember");
    });
    dialog.update(cx, |d, cx| {
        d.select_base("Arcode Chalk", cx);
        d.set_name("Paper", cx);
    });
    cx.dispatch_action(crate::theme_editor::new_theme::Create);
    cx.run_until_parked();

    let text = fs::read_to_string(theme_file(dir.path(), "paper")).expect("written");
    assert!(
        text.contains("name = \"Paper\"\nappearance = \"light\""),
        "{text}"
    );
    assert!(ws.read_with(cx, |ws, _| ws.new_theme_dialog().is_none()));
    assert!(config_text(dir.path()).contains("theme = \"Paper\""));
    let chalk = ThemeRegistry::builtin()
        .get("Arcode Chalk")
        .cloned()
        .unwrap();
    cx.update(|_, cx| {
        let registry = &cx.global::<Themes>().0;
        let paper = registry.get("Paper").expect("in the list");
        assert_eq!(paper.ui, chalk.ui);
        assert_eq!(paper.syntax, chalk.syntax);
        assert_eq!(theme(cx).name, "Paper");
    });
    // The editor is open on it, ready to change colours.
    let editor = ws.read_with(cx, |ws, _| ws.theme_editor().cloned().expect("editor"));
    editor.read_with(cx, |e, _| {
        assert_eq!(e.theme_name(), "Paper");
        assert!(!e.edit().is_builtin());
        assert!(!e.has_unsaved_changes());
    });
}

#[gpui::test]
fn names_are_checked_like_save_as(cx: &mut TestAppContext) {
    let mine = named("Night", "Arcode Midnight");
    let (ws, cx, dir) = workspace_with(cx, "[ui]\ntheme = \"Arcode Ember\"\n", &[mine]);
    cx.dispatch_action(NewTheme);
    cx.run_until_parked();
    let dialog = ws.read_with(cx, |ws, _| ws.new_theme_dialog().cloned().expect("dialog"));
    let try_name = |name: &str, cx: &mut VisualTestContext| {
        dialog.update(cx, |d, cx| d.set_name(name, cx));
        cx.dispatch_action(crate::theme_editor::new_theme::Create);
        cx.run_until_parked();
        dialog.read_with(cx, |d, _| {
            let (hint, error) = d.hint();
            (hint.to_owned(), error)
        })
    };
    let (hint, error) = try_name("   ", cx);
    assert!(error && hint.contains("name"), "{hint}");
    let (hint, error) = try_name("arcode chalk", cx);
    assert!(error && hint.contains("built-in"), "{hint}");
    let (hint, error) = try_name("ÄÖÜ", cx);
    assert!(error && hint.contains("letters or digits"), "{hint}");
    assert!(!cx.has_pending_prompt());

    // One of the user's themes: asks before replacing it.
    try_name("night", cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    let file = theme_file(dir.path(), "night");
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("appearance = \"dark\"")
    );
    dialog.update(cx, |d, cx| d.select_base("Arcode Chalk", cx));
    try_name("Night", cx);
    cx.simulate_prompt_answer("Replace");
    cx.run_until_parked();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("appearance = \"light\""), "{text}");
    assert_eq!(fs::read_dir(dir.path().join("themes")).unwrap().count(), 1);
}

#[gpui::test]
fn renaming_moves_the_file_and_follows_the_settings(cx: &mut TestAppContext) {
    let mine = named("Night", "Arcode Midnight");
    let config = "[ui]\ntheme = \"system\"\ndark_theme = \"Night\"\n";
    let (ws, cx, dir) = workspace_with(cx, config, &[mine]);
    // The editor is open on it with an edit not saved yet.
    ws.update_in(cx, |ws, window, cx| {
        let night = cx.global::<Themes>().0.get("Night").cloned();
        ws.open_theme_editor_on(night, window, cx)
    });
    let editor = ws.read_with(cx, |ws, _| ws.theme_editor().cloned().expect("editor"));
    let red = arcode_config::Color::rgb(220, 30, 40);
    editor.update(cx, |e, cx| {
        e.set_color(&arcode_config::theme::TokenRef::ui("accent"), red, cx)
    });

    cx.dispatch_action(RenameTheme {
        name: "Night".into(),
    });
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.rename_theme_dialog().is_some()));
    ws.update_in(cx, |ws, window, cx| {
        ws.rename_theme("Night", "Late Night", false, window, cx)
    });
    cx.run_until_parked();
    assert!(!theme_file(dir.path(), "night").exists());
    assert!(theme_file(dir.path(), "late-night").exists());
    assert!(config_text(dir.path()).contains("dark_theme = \"Late Night\""));
    assert!(ws.read_with(cx, |ws, _| ws.rename_theme_dialog().is_none()));
    cx.update(|_, cx| {
        let registry = &cx.global::<Themes>().0;
        assert!(registry.get("Night").is_none());
        assert!(registry.is_user_theme("Late Night"));
    });
    editor.read_with(cx, |e, _| {
        assert_eq!(e.theme_name(), "Late Night");
        assert!(e.has_unsaved_changes(), "the edits stay");
    });
}

// Renaming onto another theme's name replaces every theme of that name,
// in any spelling and whatever its file is called.
#[gpui::test]
fn renaming_onto_a_theme_replaces_it_everywhere(cx: &mut TestAppContext) {
    let mine = named("My Ember", "Arcode Ember");
    let other = named("Other", "Arcode Chalk");
    let (ws, cx, dir) = workspace_with(cx, "[ui]\ntheme = \"Arcode Ember\"\n", &[mine, other]);
    let stray = dir.path().join("themes").join("zz.toml");
    fs::write(&stray, "name = \"OTHER\"\nappearance = \"light\"\n").unwrap();
    cx.update(|_, cx| {
        cx.global_mut::<Themes>()
            .0
            .insert(named("OTHER", "Arcode Chalk"))
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.rename_theme("My Ember", "other", false, window, cx)
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Replace");
    cx.run_until_parked();
    let files: Vec<String> = fs::read_dir(dir.path().join("themes"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, ["other.toml"]);
    cx.update(|_, cx| {
        let registry = &cx.global::<Themes>().0;
        let left: Vec<(&str, arcode_config::Appearance)> = registry
            .themes()
            .iter()
            .filter(|t| t.name.eq_ignore_ascii_case("other"))
            .map(|t| (t.name.as_str(), t.appearance))
            .collect();
        assert_eq!(left, [("other", arcode_config::Appearance::Dark)]);
        assert!(registry.get("My Ember").is_none());
    });
}

#[gpui::test]
fn deleting_the_theme_in_use_falls_back_to_the_default(cx: &mut TestAppContext) {
    let night = named("Night", "Arcode Midnight");
    let paper = named("Paper", "Arcode Chalk");
    let config = "[ui]\ntheme = \"Night\"\nlight_theme = \"Paper\"\n";
    let (ws, cx, dir) = workspace_with(cx, config, &[night, paper]);
    assert_eq!(cx.update(|_, cx| theme(cx).name.clone()), "Night");

    cx.dispatch_action(DeleteTheme {
        name: "Night".into(),
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asks first");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(theme_file(dir.path(), "night").exists());

    cx.dispatch_action(DeleteTheme {
        name: "Night".into(),
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    assert!(!theme_file(dir.path(), "night").exists());
    assert!(theme_file(dir.path(), "paper").exists());
    let text = config_text(dir.path());
    assert!(text.contains("theme = \"dark\""), "{text}");
    cx.update(|_, cx| {
        assert!(cx.global::<Themes>().0.get("Night").is_none());
        assert_eq!(theme(cx).name, "Arcode Ember");
    });
    let _ = ws;

    // The light theme deleted: back to Arcode Chalk.
    cx.dispatch_action(DeleteTheme {
        name: "Paper".into(),
    });
    cx.run_until_parked();
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    let text = config_text(dir.path());
    assert!(text.contains("light_theme = \"Arcode Chalk\""), "{text}");
}

#[gpui::test]
fn built_in_themes_cannot_be_renamed_or_deleted(cx: &mut TestAppContext) {
    let (ws, cx, _dir) = workspace_with(cx, "[ui]\ntheme = \"Arcode Ember\"\n", &[]);
    cx.dispatch_action(RenameTheme {
        name: "Arcode Ember".into(),
    });
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.rename_theme_dialog().is_none()));
    cx.dispatch_action(DeleteTheme {
        name: "Arcode Chalk".into(),
    });
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    let toast = ws.read_with(cx, |ws, _| ws.toast.as_ref().map(|t| t.to_string()));
    assert!(toast.unwrap_or_default().contains("built in"));
}

#[gpui::test]
fn settings_lists_the_users_themes(cx: &mut TestAppContext) {
    let mine = named("Night", "Arcode Midnight");
    let (ws, cx, _dir) = workspace_with(cx, "[ui]\ntheme = \"Arcode Ember\"\n", &[mine]);
    ws.update_in(cx, |ws, window, cx| ws.open_settings_page(window, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("setting-row-Night").is_some());
    assert!(cx.debug_bounds("setting-row-Arcode Ember").is_none());
}
