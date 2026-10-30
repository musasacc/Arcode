//! Locations of the configuration file, user themes and persisted state.
//!
//! By default everything lives under the platform directories reported by the
//! `dirs` crate (`<config_dir>/arcode` and `<data_dir>/arcode`). Two
//! environment variables override them, which is useful for tests and portable
//! installs:
//!
//! - [`CONFIG_DIR_ENV`] (`ARCODE_CONFIG_DIR`): used verbatim as the config dir.
//! - [`DATA_DIR_ENV`] (`ARCODE_DATA_DIR`): used verbatim as the data dir.
//!
//! Empty values are ignored. Every function also has an `*_in(root)` variant
//! that takes an explicit root directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Environment variable overriding the configuration directory.
pub const CONFIG_DIR_ENV: &str = "ARCODE_CONFIG_DIR";
/// Environment variable overriding the data (state) directory.
pub const DATA_DIR_ENV: &str = "ARCODE_DATA_DIR";

/// Name of the per-application subdirectory inside the platform dirs.
pub const APP_DIR_NAME: &str = "arcode";
/// File name of the user configuration.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// Directory name (inside the config dir) holding user themes.
pub const THEMES_DIR_NAME: &str = "themes";
/// File name of the persisted application state.
pub const STATE_FILE_NAME: &str = "state.toml";
/// File name (inside the config dir) of the user's snippets.
pub const SNIPPETS_FILE_NAME: &str = "snippets.toml";

/// The configuration directory: `$ARCODE_CONFIG_DIR`, else
/// `dirs::config_dir()/arcode`. `None` if no home/config dir is known.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_with(|key| std::env::var_os(key))
}

/// The data directory: `$ARCODE_DATA_DIR`, else `dirs::data_dir()/arcode`.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_with(|key| std::env::var_os(key))
}

/// Path of `config.toml` inside [`config_dir`].
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|dir| config_file_in(&dir))
}

/// Path of the user themes directory inside [`config_dir`].
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|dir| themes_dir_in(&dir))
}

/// Path of `state.toml` inside [`data_dir`].
pub fn state_file() -> Option<PathBuf> {
    data_dir().map(|dir| state_file_in(&dir))
}

/// Path of the user's `snippets.toml` inside [`config_dir`].
pub fn snippets_file() -> Option<PathBuf> {
    config_dir().map(|dir| snippets_file_in(&dir))
}

/// `snippets.toml` inside an explicit configuration root.
pub fn snippets_file_in(config_root: &Path) -> PathBuf {
    config_root.join(SNIPPETS_FILE_NAME)
}

/// `config.toml` inside an explicit configuration root.
pub fn config_file_in(config_root: &Path) -> PathBuf {
    config_root.join(CONFIG_FILE_NAME)
}

/// The themes directory inside an explicit configuration root.
pub fn themes_dir_in(config_root: &Path) -> PathBuf {
    config_root.join(THEMES_DIR_NAME)
}

/// `state.toml` inside an explicit data root.
pub fn state_file_in(data_root: &Path) -> PathBuf {
    data_root.join(STATE_FILE_NAME)
}

/// Like [`config_dir`] but reads environment variables through `env`.
pub fn config_dir_with(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    resolve(env(CONFIG_DIR_ENV), dirs::config_dir)
}

/// Like [`data_dir`] but reads environment variables through `env`.
pub fn data_dir_with(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    resolve(env(DATA_DIR_ENV), dirs::data_dir)
}

fn resolve(env_value: Option<OsString>, platform: fn() -> Option<PathBuf>) -> Option<PathBuf> {
    match env_value {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => platform().map(|dir| dir.join(APP_DIR_NAME)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_is_used_verbatim() {
        let dir =
            config_dir_with(|key| (key == CONFIG_DIR_ENV).then(|| OsString::from("/portable/cfg")));
        assert_eq!(dir, Some(PathBuf::from("/portable/cfg")));
        let data = data_dir_with(|key| (key == DATA_DIR_ENV).then(|| OsString::from("/p/data")));
        assert_eq!(data, Some(PathBuf::from("/p/data")));
    }

    #[test]
    fn empty_env_falls_back_to_platform_dir() {
        let dir = config_dir_with(|_| Some(OsString::new()));
        assert_eq!(dir, dirs::config_dir().map(|d| d.join("arcode")));
        let data = data_dir_with(|_| None);
        assert_eq!(data, dirs::data_dir().map(|d| d.join("arcode")));
    }

    #[test]
    fn file_helpers_join_names() {
        let root = Path::new("root");
        assert_eq!(config_file_in(root), root.join("config.toml"));
        assert_eq!(themes_dir_in(root), root.join("themes"));
        assert_eq!(state_file_in(root), root.join("state.toml"));
    }
}
