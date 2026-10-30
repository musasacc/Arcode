//! End-to-end check of `ARCODE_CONFIG_DIR` / `ARCODE_DATA_DIR`.
//!
//! This is the only test in this binary, so mutating the process environment
//! cannot race with other tests.

use arcode_config::{AppState, Config, ThemeChoice, ThemeRegistry, paths};

#[test]
fn env_overrides_drive_all_paths() {
    let config_root = tempfile::tempdir().unwrap();
    let data_root = tempfile::tempdir().unwrap();
    // SAFETY: single-threaded: this is the only test in this binary.
    unsafe {
        std::env::set_var(paths::CONFIG_DIR_ENV, config_root.path());
        std::env::set_var(paths::DATA_DIR_ENV, data_root.path());
    }

    assert_eq!(paths::config_dir().as_deref(), Some(config_root.path()));
    assert_eq!(
        paths::config_file(),
        Some(config_root.path().join("config.toml"))
    );
    assert_eq!(paths::themes_dir(), Some(config_root.path().join("themes")));
    assert_eq!(
        paths::state_file(),
        Some(data_root.path().join("state.toml"))
    );

    // Nothing on disk yet: defaults, no warnings.
    let (config, warnings) = Config::load();
    assert_eq!(config, Config::default());
    assert!(warnings.is_empty());

    // First run: write the template, then customize and reload.
    let config_path = paths::config_file().unwrap();
    assert!(Config::write_default_if_missing(&config_path).unwrap());
    let edited = Config::default_toml().replace("theme = \"system\"", "theme = \"Sunrise\"");
    std::fs::write(&config_path, edited).unwrap();
    let (config, warnings) = Config::load();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.ui.theme, ThemeChoice::Named("Sunrise".into()));

    // User theme in the themes dir is picked up and resolved.
    let themes = paths::themes_dir().unwrap();
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("sunrise.toml"),
        "name = \"Sunrise\"\nappearance = \"light\"\n[ui]\naccent = \"#ff8800\"\n",
    )
    .unwrap();
    let (registry, warnings) = ThemeRegistry::load(paths::themes_dir().as_deref());
    assert!(warnings.is_empty(), "{warnings:?}");
    let theme = registry.resolve(
        &config.ui.theme,
        &config.ui.dark_theme,
        &config.ui.light_theme,
        true,
    );
    assert_eq!(theme.name, "Sunrise");
    assert_eq!(theme.ui.accent.to_u32(), 0xff88_00ff);

    // State goes to the data dir.
    let mut state = AppState::load();
    assert_eq!(state, AppState::default());
    state.first_run_complete = true;
    state.touch_recent(config_root.path());
    state.save().unwrap();
    assert!(data_root.path().join("state.toml").is_file());
    assert_eq!(AppState::load(), state);

    // Empty overrides fall back to the platform directories.
    // SAFETY: as above.
    unsafe {
        std::env::set_var(paths::CONFIG_DIR_ENV, "");
        std::env::remove_var(paths::DATA_DIR_ENV);
    }
    assert_eq!(
        paths::config_dir(),
        dirs::config_dir().map(|d| d.join("arcode"))
    );
    assert_eq!(
        paths::data_dir(),
        dirs::data_dir().map(|d| d.join("arcode"))
    );
}
