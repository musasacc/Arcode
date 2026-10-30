//! "Plugins: Install from Folder…" (§3 "Installed"): validate a folder,
//! copy it into `<data_dir>/arcode/plugins/<id>/`, then precompile.
//!
//! - The folder must hold a valid `plugin.toml` whose id is not reserved
//!   (`arcode.` is for bundled plugins) and its `entry` component.
//! - Only `plugin.toml`, the entry (at most 32 MiB), `README.md` and
//!   `LICENSE` are copied, and only if they are regular files (no
//!   symlinks, FIFOs or devices). Nothing else in the folder is read.
//! - The copy is staged next to the destination and swapped in by rename,
//!   so a failed install leaves the previous version untouched.
//!
//! These functions block (file copies, compilation); the app runs them on
//! a background task and then calls `PluginManager::finish_install`, which
//! records the new plugin as disabled until the user enables it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cache::{LoadError, MAX_WASM_BYTES, wasm_sha256};
use crate::engine::Runtime;
use crate::manifest::{Capabilities, PluginId, PluginSource};
use crate::registry::load;

/// Optional files copied along (at most this size each).
const EXTRA_FILES: [&str; 2] = ["README.md", "LICENSE"];
const MAX_EXTRA_BYTES: u64 = 1024 * 1024;

/// A finished copy.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallReport {
    /// The plugin.
    pub id: PluginId,
    /// Its new folder, `<installed>/<id>`.
    pub dir: PathBuf,
    /// sha256 of the copied `plugin.wasm`.
    pub sha256: String,
    /// The capabilities of the version it replaced, if any.
    pub replaced: Option<Capabilities>,
    /// The new version's capabilities.
    pub capabilities: Capabilities,
}

/// Why an install failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The folder is not an installable plugin.
    Invalid(String),
    /// Copying failed.
    Io(String),
    /// The component does not compile.
    Compile(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) => write!(f, "not an installable plugin: {m}"),
            Self::Io(m) => write!(f, "cannot install: {m}"),
            Self::Compile(m) => write!(f, "invalid plugin component: {m}"),
        }
    }
}

impl std::error::Error for InstallError {}

fn io(e: std::io::Error) -> InstallError {
    InstallError::Io(e.to_string())
}

/// True if `path` is a regular file (not a symlink) of at most `max` bytes.
fn regular(path: &Path, max: u64) -> Result<bool, InstallError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() && m.len() <= max => Ok(true),
        Ok(m) if m.file_type().is_file() => Err(InstallError::Invalid(format!(
            "{} is larger than {max} bytes",
            path.display()
        ))),
        Ok(_) => Err(InstallError::Invalid(format!(
            "{} is not a regular file",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(e)),
    }
}

/// Validates `src` and copies it to `<installed_root>/<id>/`.
pub fn install_from_folder(
    src: &Path,
    installed_root: &Path,
) -> Result<InstallReport, InstallError> {
    if !regular(&src.join("plugin.toml"), 256 * 1024)? {
        return Err(InstallError::Invalid("no plugin.toml".into()));
    }
    let manifest =
        load(src, PluginSource::Dev).map_err(|e| InstallError::Invalid(e.to_string()))?;
    let id = manifest.id().clone();
    if id.is_reserved() {
        return Err(InstallError::Invalid(format!(
            "the id `{id}` is reserved for bundled plugins"
        )));
    }
    let entry = manifest.plugin.entry.clone();
    if !regular(&src.join(&entry), MAX_WASM_BYTES)? {
        return Err(InstallError::Invalid(format!("no {entry}")));
    }
    let dest = installed_root.join(id.as_str());
    let replaced = load(&dest, PluginSource::Installed)
        .ok()
        .map(|m| m.capabilities);
    static STAGES: AtomicU64 = AtomicU64::new(0);
    let n = STAGES.fetch_add(1, Ordering::Relaxed);
    let stage = installed_root.join(format!(".install-{id}-{}-{n}", std::process::id()));
    let copied = (|| {
        fs::create_dir_all(&stage).map_err(io)?;
        fs::copy(src.join("plugin.toml"), stage.join("plugin.toml")).map_err(io)?;
        fs::copy(src.join(&entry), stage.join(&entry)).map_err(io)?;
        for extra in EXTRA_FILES {
            // Optional: skipped when missing, too large or not regular.
            if regular(&src.join(extra), MAX_EXTRA_BYTES).unwrap_or(false) {
                fs::copy(src.join(extra), stage.join(extra)).map_err(io)?;
            }
        }
        // The copy must still be what was validated.
        load(&stage, PluginSource::Dev).map_err(|e| InstallError::Invalid(e.to_string()))?;
        let sha256 =
            wasm_sha256(&stage.join(&entry)).map_err(|e| InstallError::Io(e.to_string()))?;
        swap_in(&stage, &dest)?;
        Ok(sha256)
    })();
    let _ = fs::remove_dir_all(&stage);
    Ok(InstallReport {
        id,
        dir: dest,
        sha256: copied?,
        replaced,
        capabilities: manifest.capabilities,
    })
}

/// Replaces `dest` with `stage` (the old folder is removed afterwards).
fn swap_in(stage: &Path, dest: &Path) -> Result<(), InstallError> {
    let old = dest.with_extension(format!("old-{}", std::process::id()));
    let had_old = dest.exists();
    if had_old {
        fs::rename(dest, &old).map_err(io)?;
    }
    if let Err(e) = fs::rename(stage, dest) {
        if had_old {
            let _ = fs::rename(&old, dest);
        }
        return Err(io(e));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// Compiles the installed component into the `.cwasm` cache, so the first
/// activation does not compile.
pub fn precompile(runtime: &Runtime, report: &InstallReport) -> Result<(), InstallError> {
    let manifest = load(&report.dir, PluginSource::Installed)
        .map_err(|e| InstallError::Invalid(e.to_string()))?;
    let engine = runtime.engine().map_err(InstallError::Compile)?;
    runtime
        .cache()
        .prepare(engine, &report.dir.join(&manifest.plugin.entry))
        .map(drop)
        .map_err(|e| match e {
            LoadError::Compile(m) => InstallError::Compile(m),
            other => InstallError::Io(other.to_string()),
        })
}
