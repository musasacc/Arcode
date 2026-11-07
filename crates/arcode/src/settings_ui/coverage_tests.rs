//! Every setting has a row on the page, rows are found by their key, and
//! resetting a row writes the default while keeping the file's comments.

use arcode_config::config::keys::documented_keys;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

use super::*;
use crate::prefs::store::ConfigPathOverride;

/// Settings deliberately left off the page, with the reason. A new
/// setting needs a row, an entry here or one in [`super::filter::ELSEWHERE`]
/// (tables set in `config.toml`, which the filter still finds).
const EXCLUDED: &[(&str, &str)] = &[(
    "editor.vim_mode",
    "the Keymap row covers it: Vim there is `editor.keymap = \"vim\"`",
)];

/// Every key without a row, with the reason.
fn excluded() -> Vec<(&'static str, &'static str)> {
    let elsewhere = super::filter::ELSEWHERE
        .iter()
        .map(|(key, _, note)| (*key, *note));
    EXCLUDED.iter().copied().chain(elsewhere).collect()
}

/// Settings without a row on this platform.
fn platform_excluded() -> Vec<&'static str> {
    let mut keys = Vec::new();
    if !arcode_config::TEXT_RENDERING_TUNABLE {
        keys.extend(["ui.text_gamma", "ui.text_contrast"]);
    }
    if cfg!(target_os = "macos") {
        // macOS always shows the menus in its own menu bar.
        keys.push("ui.menu_bar");
    }
    keys
}

/// A page on `config` (written to a fresh `config.toml` and loaded).
fn page_with<'a>(
    cx: &'a mut TestAppContext,
    config: &str,
) -> (
    Entity<SettingsPage>,
    &'a mut VisualTestContext,
    tempfile::TempDir,
    std::path::PathBuf,
) {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, config).expect("write config");
    cx.update(|cx| {
        cx.set_global(ConfigPathOverride(path.clone()));
        crate::prefs::store::reload(cx);
    });
    cx.run_until_parked();
    let (page, cx) = cx.add_window_view(|_, cx| SettingsPage::new(WeakEntity::new_invalid(), cx));
    (page, cx, dir, path)
}

#[gpui::test]
fn every_documented_setting_has_a_row(cx: &mut TestAppContext) {
    let (page, cx, _dir, _) = page_with(cx, "");
    super::filter::collect_drawn_keys();
    for section in Section::ALL {
        page.update(cx, |page, cx| page.set_section(section, cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
    }
    let drawn = super::filter::take_drawn_keys();
    let platform = platform_excluded();
    let excluded_keys = excluded();
    let excluded =
        |key: &str| excluded_keys.iter().any(|(k, _)| *k == key) || platform.contains(&key);
    let documented = documented_keys();
    let missing: Vec<_> = documented
        .iter()
        .filter(|key| !drawn.contains(*key) && !excluded(key))
        .collect();
    assert!(
        missing.is_empty(),
        "settings with neither a row on the Settings page nor an entry in EXCLUDED: {missing:?}"
    );
    for (key, reason) in &excluded_keys {
        assert!(!reason.is_empty());
        assert!(
            documented.iter().any(|k| k == key),
            "{key} is excluded but is not a setting"
        );
        assert!(!drawn.contains(*key), "{key} is excluded but has a row");
    }
    // Every language has its switch.
    assert!(drawn.contains("language.rust.enabled"), "{drawn:?}");
}

/// A page whose filter holds `query` from its first draw (GPUI keeps the
/// debug bounds of earlier frames, so the filter must be set before).
fn filtered<'a>(cx: &'a mut TestAppContext, query: &str) -> &'a mut VisualTestContext {
    crate::test_support::init(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    cx.update(|cx| cx.set_global(ConfigPathOverride(dir.path().join("config.toml"))));
    let query = query.to_owned();
    let (_page, cx) = cx.add_window_view(move |_, cx| {
        let page = SettingsPage::new(WeakEntity::new_invalid(), cx);
        page.filter.update(cx, |f, cx| f.set_text(&query, cx));
        page
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    cx
}

#[gpui::test]
fn filter_finds_a_row_by_its_key(cx: &mut TestAppContext) {
    let cx = filtered(cx, "tab_size");
    assert!(cx.debug_bounds("setting-row-Tab size").is_some());
    assert!(cx.debug_bounds("setting-row-Insert spaces").is_none());
    assert!(cx.debug_bounds("setting-row-Minimap").is_none());
}

#[gpui::test]
fn filter_finds_rows_by_a_dotted_key(cx: &mut TestAppContext) {
    let cx = filtered(cx, "editor.minimap");
    assert!(cx.debug_bounds("setting-row-Minimap").is_some());
    assert!(cx.debug_bounds("setting-row-Minimap width").is_some());
    assert!(cx.debug_bounds("setting-row-Tab size").is_none());
}

#[gpui::test]
fn filter_finds_new_rows_and_synonyms(cx: &mut TestAppContext) {
    let cx = filtered(cx, "caret");
    assert!(cx.debug_bounds("setting-row-Cursor blink").is_some());
    assert!(cx.debug_bounds("setting-row-Word wrap").is_none());
}

#[gpui::test]
fn filter_finds_every_discord_row_under_its_heading(cx: &mut TestAppContext) {
    let cx = filtered(cx, "rich presence");
    for row in [
        "setting-row-Show in Discord",
        "setting-row-File name",
        "setting-row-Project name",
        "setting-row-Elapsed time",
        "setting-row-Application ID",
    ] {
        assert!(cx.debug_bounds(row).is_some(), "{row}");
    }
    assert!(
        cx.debug_bounds("setting-group-discord").is_some(),
        "heading"
    );
    assert!(cx.debug_bounds("setting-row-Tab size").is_none());
}

#[gpui::test]
fn filter_hides_the_discord_heading_without_a_match(cx: &mut TestAppContext) {
    let cx = filtered(cx, "caret");
    assert!(cx.debug_bounds("setting-group-discord").is_none());
}

#[gpui::test]
fn filter_searches_the_new_sections(cx: &mut TestAppContext) {
    let cx = filtered(cx, "todo.tags");
    assert!(cx.debug_bounds("setting-row-Tags").is_some());
    assert!(cx.debug_bounds("setting-row-Exclude").is_none());
}

#[gpui::test]
fn filter_finds_the_keymap_row_by_vim_mode(cx: &mut TestAppContext) {
    let cx = filtered(cx, "editor.vim_mode");
    assert!(cx.debug_bounds("setting-row-Keymap").is_some());
}

#[gpui::test]
fn filter_points_to_config_toml_for_tables(cx: &mut TestAppContext) {
    let cx = filtered(cx, "files.associations");
    assert!(cx.debug_bounds("setting-row-File associations").is_some());
    assert!(cx.debug_bounds("setting-row-Debug adapters").is_none());
}

#[gpui::test]
fn tab_moves_between_text_fields_and_writes_the_one_left(cx: &mut TestAppContext) {
    let (page, cx, _dir, path) = page_with(cx, "[terminal]\n# Shell.\nshell = \"bash\"\n");
    page.update(cx, |page, cx| page.set_section(Section::Terminal, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let field = |path: &str, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| {
            page.texts
                .iter()
                .find(|t| t.path() == path)
                .map(|t| t.field().clone())
                .expect("a text field")
        })
    };
    let shell = field("terminal.shell", cx);
    let args = field("terminal.args", cx);
    cx.update(|window, cx| {
        // Focus changes are reported in an active window only.
        window.activate_window();
        window.focus(&shell.focus_handle(cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear();
        assert!(shell.focus_handle(cx).is_focused(window));
    });
    cx.run_until_parked();
    shell.update(cx, |f, cx| f.set_text("zsh", cx));
    cx.simulate_keystrokes("tab");
    // Leaving the field is noticed when the page is drawn.
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(args.focus_handle(cx).is_focused(window), "Tab moves on");
    });
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("# Shell.\nshell = \"zsh\"\n"), "{text}");
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            shell.focus_handle(cx).is_focused(window),
            "Shift+Tab moves back"
        );
    });
}

#[gpui::test]
fn reset_writes_the_default_and_keeps_comments(cx: &mut TestAppContext) {
    let source =
        "# mine\n[editor]\n# Width of a tab stop.\ntab_size = 2 # narrow\ninsert_spaces = true\n";
    let (page, cx, _dir, path) = page_with(cx, source);
    cx.update(|_, cx| assert_eq!(arcode_ui_kit::settings(cx).editor.tab_size, 2));
    page.update(cx, |page, cx| page.set_section(Section::Editor, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    // Only the changed row offers a reset.
    assert!(cx.debug_bounds("reset-editor.insert_spaces").is_none());
    let reset = cx
        .debug_bounds("reset-editor.tab_size")
        .expect("a reset button on the changed row")
        .center();
    cx.simulate_click(reset, Modifiers::none());
    cx.run_until_parked();
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(
        text,
        "# mine\n[editor]\n# Width of a tab stop.\ntab_size = 4 # narrow\ninsert_spaces = true\n"
    );
    cx.update(|_, cx| assert_eq!(arcode_ui_kit::settings(cx).editor.tab_size, 4));
}

#[gpui::test]
fn low_end_mode_locks_the_rows_it_overrides(cx: &mut TestAppContext) {
    let (page, cx, _dir, _) = page_with(cx, "[ui]\nlow_end_mode = true\n");
    page.update(cx, |page, cx| page.set_section(Section::Editor, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("setting-row-Minimap").is_some());
    // Off because of low-end mode: nothing to reset.
    for selector in [
        "reset-editor.minimap",
        "reset-editor.cursor_blink",
        "reset-editor.inlay_hints",
    ] {
        assert!(cx.debug_bounds(selector).is_none(), "{selector}");
    }
}

#[gpui::test]
fn text_rows_write_lists_and_keep_comments(cx: &mut TestAppContext) {
    let source = "[terminal]\n# Shell program.\nshell = \"/bin/zsh\"\n# Arguments.\nargs = []\n\n[todo]\n# Tags.\ntags = [\"TODO\"]\n";
    let (page, cx, _dir, path) = page_with(cx, source);
    page.update(cx, |page, cx| {
        page.type_setting_text("todo.tags", " TODO, FIXME ", cx);
        page.type_setting_text("terminal.args", "-l \"a b\"", cx);
        page.type_setting_text("terminal.shell", "", cx);
    });
    cx.run_until_parked();
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(
        text.contains("# Tags.\ntags = [\"TODO\", \"FIXME\"]\n"),
        "{text}"
    );
    assert!(
        text.contains("# Shell program.\n# Arguments.\nargs = [\"-l\", \"a b\"]\n"),
        "{text}"
    );
    cx.update(|_, cx| {
        let config = arcode_ui_kit::settings(cx);
        assert_eq!(config.todo.tags, ["TODO", "FIXME"]);
        assert_eq!(config.terminal.shell, None);
        assert_eq!(config.terminal.args, ["-l", "a b"]);
    });
    // The fields show the settings as written.
    page.read_with(cx, |page, cx| {
        let shown = |path: &str| {
            page.texts
                .iter()
                .find(|t| t.path() == path)
                .map(|t| t.text(cx).to_owned())
        };
        assert_eq!(shown("todo.tags").as_deref(), Some("TODO, FIXME"));
        assert_eq!(shown("terminal.args").as_deref(), Some("-l \"a b\""));
    });
}
