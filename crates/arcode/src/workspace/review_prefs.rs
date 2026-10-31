//! Review tests (Settings unit): live previews that leak, the first-run
//! dialog, and settings the page shows wrongly. Each failing test
//! reproduces a bug found in review.

use std::fs;
use std::path::PathBuf;

use gpui::{TestAppContext, VisualTestContext};

use super::tests::open;
use super::*;
use crate::actions::ToggleCommandPalette;
use crate::list_picker::ListPickerEvent;
use crate::prefs::ShowWelcomeSetup;
use crate::prefs::pickers::{FontTarget, ThemeSlot};
use crate::prefs::store::ConfigPathOverride;

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
    if source.is_some() {
        cx.update(|_, cx| crate::prefs::store::reload(cx));
        cx.run_until_parked();
    }
    (ws, cx, dir, path)
}

fn theme_name(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| arcode_ui_kit::theme(cx).name.clone())
}

fn ui_font(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| arcode_ui_kit::settings(cx).ui.font_family.clone())
}

/// Font picker previewing a family, then Ctrl+K Ctrl+T opens the theme
/// picker (it replaces the font picker without restoring) and Escape: the
/// previewed font stays active although it was never chosen.
#[gpui::test]
fn review_font_preview_survives_opening_another_picker(cx: &mut TestAppContext) {
    let (ws, cx, _dir, _path) = open_with_config(cx, Some(""));
    ws.update_in(cx, |ws, window, cx| {
        ws.open_font_picker(FontTarget::Ui, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| p.set_query("jetbrains", cx));
    cx.run_until_parked();
    assert_eq!(ui_font(cx).as_deref(), Some("JetBrains Mono"), "previewing");

    ws.update_in(cx, |ws, window, cx| {
        ws.open_theme_picker(ThemeSlot::Main, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |_, cx| cx.emit(ListPickerEvent::Dismissed));
    cx.run_until_parked();
    assert_eq!(ui_font(cx), None, "the font preview must not stick");
}

/// Theme picker previewing Midnight, then "Help: Start Tour" from the
/// palette: the tour drops the picker without restoring the theme, so
/// Midnight stays on screen although config.toml says dark.
#[gpui::test]
fn review_tour_drops_picker_without_restoring_preview(cx: &mut TestAppContext) {
    let (ws, cx, _dir, _path) = open_with_config(cx, Some("[ui]\ntheme = \"dark\"\n"));
    assert_eq!(theme_name(cx), "Arcode Ember");
    ws.update_in(cx, |ws, window, cx| {
        ws.open_theme_picker(ThemeSlot::Main, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| p.set_query("midnight", cx));
    cx.run_until_parked();
    assert_eq!(theme_name(cx), "Arcode Midnight", "previewing");
    ws.update_in(cx, |ws, window, cx| ws.start_tour(window, cx));
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.end_tour(window, cx));
    cx.run_until_parked();
    assert_eq!(theme_name(cx), "Arcode Ember", "the preview must be undone");
}

/// The command palette opened while a preferences picker is open is drawn
/// under the picker (the picker overlay comes later in the root), and the
/// picker keeps its preview. Opening the palette should close (and
/// restore) the picker.
#[gpui::test]
fn review_palette_while_prefs_picker_open(cx: &mut TestAppContext) {
    let (ws, cx, _dir, _path) = open_with_config(cx, Some("[ui]\ntheme = \"dark\"\n"));
    ws.update_in(cx, |ws, window, cx| {
        ws.open_theme_picker(ThemeSlot::Main, window, cx)
    });
    cx.run_until_parked();
    let picker = ws.read_with(cx, |ws, _| ws.prefs_picker().cloned().expect("picker"));
    picker.update(cx, |p, cx| p.set_query("midnight", cx));
    cx.run_until_parked();
    cx.dispatch_action(ToggleCommandPalette);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.palette().is_some());
        assert!(
            ws.prefs_picker().is_none(),
            "two overlays at once; the picker covers the palette"
        );
    });
    assert_eq!(theme_name(cx), "Arcode Ember");
}

/// The first-run dialog is modal. When config.toml cannot be written
/// (read-only or missing config folder) neither Skip nor Save closes it,
/// so the window is unusable.
#[gpui::test]
fn review_first_run_can_be_left_when_writing_fails(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    // The "config folder" is a regular file: nothing can be created in it.
    let blocker = dir.path().join("not-a-dir");
    fs::write(&blocker, "").expect("write");
    let (ws, cx) = open(cx);
    let path = blocker.join("config.toml");
    cx.update(|_, cx| cx.set_global(ConfigPathOverride(path)));
    cx.dispatch_action(ShowWelcomeSetup);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.first_run_dialog().is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws.first_run_dialog().is_none()),
        "Skip must close the modal dialog even when the file cannot be written"
    );
}

/// "Show Welcome Setup" on an existing config starts from the built-in
/// defaults instead of the current settings, so Save silently resets the
/// theme (and shows "Normal" while low-end mode is on).
#[gpui::test]
fn review_welcome_setup_starts_from_current_settings(cx: &mut TestAppContext) {
    let (ws, cx, _dir, path) = open_with_config(
        cx,
        Some("[ui]\ntheme = \"Arcode Midnight\"\nlow_end_mode = true\n"),
    );
    cx.dispatch_action(ShowWelcomeSetup);
    cx.run_until_parked();
    let dialog = ws.read_with(cx, |ws, _| ws.first_run_dialog().cloned().expect("dialog"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(dialog.read_with(cx, |d, _| d.saved));
    let (config, _) = arcode_config::Config::load_from(&path);
    assert_eq!(
        config.ui.theme,
        arcode_config::ThemeChoice::Named("Arcode Midnight".into()),
        "saving the dialog unchanged reset the theme"
    );
    assert!(config.ui.low_end_mode);
}

/// With `vim_mode = true` in config.toml the Editor section shows the
/// keymap as "Default", and choosing "Default" leaves Vim on.
///
/// (Fix review: the test wrote only `editor.keymap` itself, which is not
/// what the control does; the control now writes `keymap_edits`, which
/// also turns `vim_mode` off, and shows Vim while `vim_mode` is on.)
#[gpui::test]
fn review_keymap_default_turns_vim_off(cx: &mut TestAppContext) {
    let (ws, cx, _dir, _path) = open_with_config(cx, Some("[editor]\nvim_mode = true\n"));
    cx.dispatch_action(crate::actions::OpenSettings);
    cx.run_until_parked();
    let page = ws.read_with(cx, |ws, _| ws.settings_page().cloned().expect("page"));
    page.update(cx, |page, cx| {
        assert_eq!(
            crate::settings_ui::keymap_index(arcode_ui_kit::settings(cx)),
            1,
            "vim_mode = true shows as Vim"
        );
        page.writer().write(
            arcode_config::keymap_edits(arcode_config::Keymap::Default),
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(
            !arcode_editor::vim::vim_mode_enabled(cx),
            "the Settings page says Default but Vim mode is still on"
        )
    });
}

/// The first-run dialog is modal, but workspace shortcuts still reach the
/// workspace while it has focus: Ctrl+P opens the file finder under the
/// backdrop, typing goes to it, Enter opens a file behind the dialog and
/// moves focus into that editor (Escape then edits the file instead of
/// closing the dialog).
#[gpui::test]
fn review_first_run_dialog_blocks_workspace_shortcuts(cx: &mut TestAppContext) {
    let dir = super::tests::project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    let (ws, cx, _cfg, _path) = open_with_config(cx, None);
    ws.update_in(cx, |ws, window, cx| ws.open_project(root, window, cx));
    cx.run_until_parked();
    cx.dispatch_action(ShowWelcomeSetup);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.first_run_dialog().is_some()));
    cx.simulate_keystrokes("ctrl-p");
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.finder.is_none(),
            "the finder opened behind the modal first-run dialog"
        )
    });
}
