//! Where plugin files live (docs/PLUGINS.md §3). Nothing here is ever inside
//! a project tree: grants and plugin lists in a repository are ignored.

use std::path::{Path, PathBuf};

use crate::registry::DiscoveryRoots;

/// Per-user plugin locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPaths {
    /// Read-only bundled plugin roots, in priority order.
    pub bundled: Vec<PathBuf>,
    /// `<data_dir>/arcode/plugins`
    pub installed: PathBuf,
    /// `<config_dir>/arcode/plugins.toml` (enabled + sha256 per plugin).
    pub state_file: PathBuf,
    /// `<config_dir>/arcode/plugin-grants.toml`
    pub global_grants: PathBuf,
    /// `<data_dir>/arcode/projects` (per-project grants live below it).
    pub projects: PathBuf,
    /// `<data_dir>/arcode/plugin-data` (kv stores).
    pub plugin_data: PathBuf,
    /// `<cache_dir>/arcode/plugins` (precompiled `.cwasm`).
    pub cache: PathBuf,
}

impl PluginPaths {
    /// Paths derived from explicit base directories (tests, portable mode).
    pub fn with_bases(exe: Option<&Path>, config: &Path, data: &Path, cache: &Path) -> Self {
        Self {
            bundled: exe.map(bundled_roots).unwrap_or_default(),
            installed: data.join("arcode").join("plugins"),
            state_file: config.join("arcode").join("plugins.toml"),
            global_grants: config.join("arcode").join("plugin-grants.toml"),
            projects: data.join("arcode").join("projects"),
            plugin_data: data.join("arcode").join("plugin-data"),
            cache: cache.join("arcode").join("plugins"),
        }
    }

    /// Paths for the current user and executable; `None` if the platform
    /// has no config/data/cache directories (e.g. no home directory).
    pub fn for_current_user() -> Option<Self> {
        let exe = std::env::current_exe().ok();
        Some(Self::with_bases(
            exe.as_deref(),
            &dirs::config_dir()?,
            &dirs::data_dir()?,
            &dirs::cache_dir()?,
        ))
    }

    /// `<data_dir>/arcode/projects/<hash(root)>/plugin-grants.toml`: per-project
    /// grants, outside the project tree.
    pub fn project_grants(&self, project_root: &Path) -> PathBuf {
        self.projects
            .join(project_key(project_root))
            .join("plugin-grants.toml")
    }

    /// `<data_dir>/arcode/plugin-data/<id>/{global,<hash(root)>}.kv`
    pub fn kv_file(&self, plugin: &str, project_root: Option<&Path>) -> PathBuf {
        let name = project_root.map_or_else(|| "global".to_owned(), project_key);
        self.plugin_data.join(plugin).join(format!("{name}.kv"))
    }

    /// Discovery roots for these paths plus session-only dev folders.
    pub fn discovery_roots(&self, dev: Vec<PathBuf>) -> DiscoveryRoots {
        DiscoveryRoots {
            bundled: self.bundled.clone(),
            installed: vec![self.installed.clone()],
            dev,
        }
    }
}

/// A stable key for a project root: the first 128 bits of
/// `sha256(root path)`, hex. Never derived from anything inside the project.
pub fn project_key(project_root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(project_root.to_string_lossy().as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// Creates `dir` (and parents) readable only by the user where the
/// platform supports it (mode 0700 on Unix).
///
/// On Unix an existing `dir` must belong to the current user (otherwise
/// this fails with `PermissionDenied`) and is tightened to 0700 if group or
/// others had access. Returns true when it had to be tightened: anything
/// already inside may have been written by someone else.
pub fn create_private_dir(dir: &Path) -> std::io::Result<bool> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = std::fs::metadata(dir)?;
        if !meta.is_dir() {
            return Err(std::io::Error::other(format!(
                "{} is not a directory",
                dir.display()
            )));
        }
        if meta.uid() != current_uid() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} belongs to another user", dir.display()),
            ));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// The effective user id of this process.
#[cfg(unix)]
pub fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// `<exe>/../share/arcode/plugins`, and in a cargo build directory also
/// `target/wasm-plugins`: for `target/<profile>/arcode`, a cross build's
/// `target/<triple>/<profile>/arcode` and a `CARGO_TARGET_DIR` inside
/// `target/` (`target/shared/release/arcode`).
pub fn bundled_roots(exe: &Path) -> Vec<PathBuf> {
    let Some(bin_dir) = exe.parent() else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    if let Some(prefix) = bin_dir.parent() {
        roots.push(prefix.join("share").join("arcode").join("plugins"));
    }
    let target = bin_dir
        .ancestors()
        .skip(1)
        .take(2)
        .find(|d| d.file_name().is_some_and(|n| n == "target"));
    if let Some(target) = target {
        roots.push(target.join("wasm-plugins"));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_design() {
        let p = PluginPaths::with_bases(None, Path::new("/c"), Path::new("/d"), Path::new("/k"));
        assert_eq!(p.installed, Path::new("/d/arcode/plugins"));
        assert_eq!(p.state_file, Path::new("/c/arcode/plugins.toml"));
        assert_eq!(p.global_grants, Path::new("/c/arcode/plugin-grants.toml"));
        assert_eq!(p.cache, Path::new("/k/arcode/plugins"));
        assert!(p.bundled.is_empty());
        let root = Path::new("/home/me/project");
        let key = project_key(root);
        assert_eq!(key.len(), 32);
        assert_eq!(key, project_key(root));
        assert_ne!(key, project_key(Path::new("/home/me/other")));
        assert_eq!(
            p.project_grants(root),
            Path::new("/d/arcode/projects")
                .join(&key)
                .join("plugin-grants.toml")
        );
        assert_eq!(
            p.kv_file("a.b", None),
            Path::new("/d/arcode/plugin-data/a.b/global.kv")
        );
        assert!(p.kv_file("a.b", Some(root)).ends_with(format!("{key}.kv")));
        let roots = p.discovery_roots(vec![PathBuf::from("/dev/x")]);
        assert_eq!(roots.installed, [PathBuf::from("/d/arcode/plugins")]);
    }

    #[test]
    fn bundled_roots_for_installs_and_cargo_builds() {
        assert_eq!(
            bundled_roots(Path::new("/usr/bin/arcode")),
            [PathBuf::from("/usr/share/arcode/plugins")]
        );
        assert_eq!(
            bundled_roots(Path::new("/w/target/debug/arcode")),
            [
                PathBuf::from("/w/target/share/arcode/plugins"),
                PathBuf::from("/w/target/wasm-plugins")
            ]
        );
        // A custom CARGO_TARGET_DIR inside target/, and a cross build.
        for exe in [
            "/w/target/shared/release/arcode",
            "/w/target/x86_64-pc-windows-gnu/release/arcode.exe",
        ] {
            let roots = bundled_roots(Path::new(exe));
            assert_eq!(roots.len(), 2, "{exe}");
            assert_eq!(roots[1], Path::new("/w/target/wasm-plugins"), "{exe}");
        }
        // Too deep to be a cargo build directory.
        assert_eq!(bundled_roots(Path::new("/target/a/b/bin/arcode")).len(), 1);
    }
}
