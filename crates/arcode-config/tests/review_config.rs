//! Adversarial review tests for arcode-config.
//!
//! Each test demonstrates a defect found during review. They are expected to
//! FAIL against the current implementation.

use arcode_config::{Config, Theme, ThemeRegistry};

/// A recent project whose path is not valid UTF-8 (legal on Linux) makes
/// `save_to` fail for the whole state, forever (the entry is persisted in
/// memory and every later save fails too).
#[cfg(unix)]
#[test]
fn non_utf8_project_path_does_not_break_state_saving() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use arcode_config::AppState;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.toml");
    let mut state = AppState::default();
    state.touch_recent_at(Path::new("/work/ok"), 1);
    state.touch_recent_at(Path::new(OsStr::from_bytes(b"/work/bad\xff")), 2);
    state.first_run_complete = true;
    state
        .save_to(&path)
        .expect("one odd path must not make the whole state unsaveable");
    let loaded = AppState::load_from(&path).unwrap();
    assert!(loaded.first_run_complete);
}

/// TOML's unquoted dotted keys are the natural way to write
/// `function.method = "#..."`. That produces a nested table under
/// `function`, which replaces the inherited `function` style with an empty
/// one (no color) instead of setting `function.method`.
#[test]
fn unquoted_dotted_syntax_key_does_not_wipe_parent_style() {
    let mut registry = ThemeRegistry::builtin();
    let builtin_function = registry
        .get("Arcode Ember")
        .unwrap()
        .syntax_color("function");
    assert!(builtin_function.is_some());
    registry
        .add_user_theme(
            "name = \"Dotted\"\nappearance = \"dark\"\n[syntax]\nfunction.method = \"#ff0000\"\n",
            None,
        )
        .unwrap();
    let theme: &Theme = registry.get("Dotted").unwrap();
    assert_eq!(
        theme.syntax_color("function"),
        builtin_function,
        "inherited `function` color must survive"
    );
    assert_eq!(
        theme.syntax_color("function.method").map(|c| c.to_hex()),
        Some("#ff0000".to_owned())
    );
}

/// `Config::language` is documented as case-insensitive, but a user entry
/// `[language.Rust]` is stored next to the built-in `rust` entry and the
/// exact-match lookup for `rust` (the id the syntax layer uses) returns the
/// built-in, so the user's override is silently ignored.
#[test]
fn language_override_with_different_case_is_applied() {
    let (config, warnings) = Config::from_toml_str("[language.Rust]\ntab_size = 2\n");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.effective_tab_size("rust"), 2);
}

/// `extends` only sees themes loaded earlier (files are loaded in path
/// order), so `a.toml` extending a theme defined in `b.toml` silently
/// inherits from the built-in instead, with an "unknown theme" warning.
#[test]
fn extends_resolves_themes_from_later_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.toml"),
        "name = \"Child\"\nextends = \"Parent\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b.toml"),
        "name = \"Parent\"\nappearance = \"dark\"\n[ui]\naccent = \"#123456\"\n",
    )
    .unwrap();
    let (registry, warnings) = ThemeRegistry::load(Some(dir.path()));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(registry.get("Child").unwrap().ui.accent.to_hex(), "#123456");
}
