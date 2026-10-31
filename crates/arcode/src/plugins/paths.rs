//! Where the app looks for plugins and keeps their files (docs/PLUGINS.md
//! §3 "Locations"), derived from Arcode's own config and data folders so
//! `ARCODE_CONFIG_DIR` / `ARCODE_DATA_DIR` isolate plugins too.
//!
//! Discovery precedence is the registry's: bundled < installed < dev (a
//! later source replaces a plugin with the same id).

use std::path::{Path, PathBuf};

use arcode_plugin_host::paths::{PluginPaths, bundled_roots};

/// The bundled plugins of a development build: `target/wasm-plugins` of
/// this checkout (`scripts/build-plugins.sh`), when it exists. Release
/// builds only use the folder next to the executable.
pub fn dev_bundled_dir() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/wasm-plugins");
    dir.is_dir()
        .then(|| std::fs::canonicalize(&dir).unwrap_or(dir))
}

/// Plugin locations for the given folders.
///
/// - bundled: `<exe>/../share/arcode/plugins` (and `target/wasm-plugins`
///   beside a cargo build directory), then `dev_bundled` if given;
/// - installed: `<data>/plugins`;
/// - `plugins.toml` and global grants: `<config>/`;
/// - project grants and kv data: `<data>/projects`, `<data>/plugin-data`;
/// - compiled components: `cache` (`<user cache>/arcode/plugins`).
///
/// `config` and `data` are Arcode's own folders (they already end in
/// `arcode`), unlike [`PluginPaths::with_bases`], which appends it.
pub fn plugin_paths(
    exe: Option<&Path>,
    config: &Path,
    data: &Path,
    cache: &Path,
    dev_bundled: Option<PathBuf>,
) -> PluginPaths {
    let mut bundled = exe.map(bundled_roots).unwrap_or_default();
    if let Some(dir) = dev_bundled
        && !bundled.contains(&dir)
    {
        bundled.push(dir);
    }
    PluginPaths {
        bundled,
        installed: data.join("plugins"),
        state_file: config.join("plugins.toml"),
        global_grants: config.join("plugin-grants.toml"),
        projects: data.join("projects"),
        plugin_data: data.join("plugin-data"),
        cache: cache.to_path_buf(),
    }
}

/// Plugin locations for this process; `None` without config and data
/// folders (no home directory). With `ARCODE_DATA_DIR` set, compiled
/// components are cached below it too, so a test run leaves nothing in the
/// user's cache.
pub fn current() -> Option<PluginPaths> {
    let config = arcode_config::paths::config_dir()?;
    let data = arcode_config::paths::data_dir()?;
    let isolated = std::env::var_os(arcode_config::paths::DATA_DIR_ENV).is_some();
    let cache = PluginPaths::for_current_user()
        .filter(|_| !isolated)
        .map_or_else(|| data.join("cache").join("plugins"), |p| p.cache);
    let exe = std::env::current_exe().ok();
    Some(plugin_paths(
        exe.as_deref(),
        &config,
        &data,
        &cache,
        dev_bundled_dir(),
    ))
}

/// Session-only "Load Unpacked" folders from `ARCODE_PLUGIN_DEV`
/// (separated like `PATH`). Folders that do not exist are skipped.
pub fn dev_folders_from_env() -> Vec<PathBuf> {
    std::env::var_os("ARCODE_PLUGIN_DEV")
        .map(|v| std::env::split_paths(&v).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_uses_arcode_folders_verbatim() {
        let p = plugin_paths(
            Some(Path::new("/opt/arcode/bin/arcode")),
            Path::new("/c/arcode"),
            Path::new("/d/arcode"),
            Path::new("/k/arcode/plugins"),
            Some(PathBuf::from("/src/target/wasm-plugins")),
        );
        assert_eq!(
            p.bundled,
            [
                PathBuf::from("/opt/arcode/share/arcode/plugins"),
                PathBuf::from("/src/target/wasm-plugins"),
            ]
        );
        assert_eq!(p.installed, Path::new("/d/arcode/plugins"));
        assert_eq!(p.state_file, Path::new("/c/arcode/plugins.toml"));
        assert_eq!(p.global_grants, Path::new("/c/arcode/plugin-grants.toml"));
        assert_eq!(p.projects, Path::new("/d/arcode/projects"));
        assert_eq!(p.plugin_data, Path::new("/d/arcode/plugin-data"));
        assert_eq!(p.cache, Path::new("/k/arcode/plugins"));
    }

    #[test]
    fn dev_bundled_dir_is_not_listed_twice() {
        // A cargo build in `target/debug` already finds `target/wasm-plugins`.
        let p = plugin_paths(
            Some(Path::new("/w/target/debug/arcode")),
            Path::new("/c"),
            Path::new("/d"),
            Path::new("/k"),
            Some(PathBuf::from("/w/target/wasm-plugins")),
        );
        assert_eq!(
            p.bundled,
            [
                PathBuf::from("/w/target/share/arcode/plugins"),
                PathBuf::from("/w/target/wasm-plugins"),
            ]
        );
        let none = plugin_paths(
            None,
            Path::new("/c"),
            Path::new("/d"),
            Path::new("/k"),
            None,
        );
        assert!(none.bundled.is_empty());
    }
}
