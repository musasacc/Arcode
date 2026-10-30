//! Compiled-artifact cache (§2): `<cache>/<sha256(wasm)>-<compat>.cwasm`.
//!
//! Compiling happens once (at install, or on first activation without a
//! cache entry); later activations mmap the precompiled file. Only files
//! this cache wrote, named by content hash and engine compatibility hash,
//! are ever deserialized: the directory must be private (a directory found
//! open to others is tightened and emptied), the entry an owner-only file,
//! and its trailing seal (sha256 over name and artifact) must match. A file
//! that fails any check or fails to deserialize is recompiled.

use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::paths::create_private_dir;

/// Largest component accepted (§3: wasm ≤ 32 MiB).
pub const MAX_WASM_BYTES: u64 = 32 * 1024 * 1024;

/// Why a component could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Reading the wasm file failed.
    Io(String),
    /// The file is larger than [`MAX_WASM_BYTES`].
    TooLarge(u64),
    /// wasmtime rejected the component.
    Compile(String),
    /// `plugin.wasm` is not the file the user approved (§3: a mismatch
    /// disables the plugin until the user enables it again).
    Changed,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read plugin component: {e}"),
            Self::TooLarge(n) => write!(f, "plugin component is too large ({n} bytes)"),
            Self::Compile(e) => write!(f, "invalid plugin component: {e}"),
            Self::Changed => f.write_str(
                "plugin.wasm changed since the plugin was enabled; enable it again to approve it",
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// The `.cwasm` cache plus counters for tests and the Plugins view.
#[derive(Debug)]
pub struct ComponentCache {
    dir: PathBuf,
    compiles: AtomicU64,
    cache_hits: AtomicU64,
}

impl ComponentCache {
    /// A cache in `dir` (created with mode 0700 on first write).
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            compiles: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
        }
    }

    /// Components compiled by this process.
    pub fn compiles(&self) -> u64 {
        self.compiles.load(Ordering::SeqCst)
    }

    /// Components loaded from a precompiled file.
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits.load(Ordering::SeqCst)
    }

    /// Precompiles `wasm` if no valid cache entry exists (install time).
    pub fn prepare(&self, engine: &Engine, wasm: &Path) -> Result<PathBuf, LoadError> {
        let bytes = read_wasm(wasm)?;
        let (path, name) = self.entry_path(engine, &bytes);
        if !self.trusted_dir() {
            return Err(LoadError::Io(format!(
                "the cache directory {} is not private",
                self.dir.display()
            )));
        }
        if read_entry(&path, &name).is_none() {
            self.compile_to(engine, &bytes, &path, &name)?;
        }
        Ok(path)
    }

    /// Loads `wasm`, from the cache when possible.
    ///
    /// An entry is used only if the cache directory is private (and ours),
    /// the entry is a regular file of ours that nobody else may write, and
    /// its seal matches this source and engine. It is deserialized from the
    /// bytes that were checked, never re-read from disk.
    pub fn load(&self, engine: &Engine, wasm: &Path) -> Result<Component, LoadError> {
        self.load_approved(engine, wasm, None)
    }

    /// [`ComponentCache::load`], refused with [`LoadError::Changed`] unless
    /// the file's sha256 (lower-case hex) is `approved` (when given).
    pub fn load_approved(
        &self,
        engine: &Engine,
        wasm: &Path,
        approved: Option<&str>,
    ) -> Result<Component, LoadError> {
        let bytes = read_wasm(wasm)?;
        if approved.is_some_and(|sha| sha != hex(&Sha256::digest(&bytes))) {
            return Err(LoadError::Changed);
        }
        let (path, name) = self.entry_path(engine, &bytes);
        if !self.trusted_dir() {
            // No cache we can trust: compile in memory every time.
            self.compiles.fetch_add(1, Ordering::SeqCst);
            return Component::new(engine, &bytes).map_err(|e| LoadError::Compile(e.to_string()));
        }
        if let Some(artifact) = read_entry(&path, &name) {
            // SAFETY: `artifact` was read from a private, owner-only file in
            // our private cache dir, and its seal (sha256 over the entry
            // name, which holds the source hash and the engine's
            // compatibility hash, and the artifact) matches, so it is what
            // `precompile_component` produced for this source.
            match unsafe { Component::deserialize(engine, &artifact) } {
                Ok(component) => {
                    self.cache_hits.fetch_add(1, Ordering::SeqCst);
                    return Ok(component);
                }
                Err(e) => log::warn!("recompiling {}: {e}", path.display()),
            }
        }
        let artifact = self.compile_to(engine, &bytes, &path, &name)?;
        // SAFETY: just produced by `precompile_component` in this process.
        unsafe { Component::deserialize(engine, &artifact) }
            .map_err(|e| LoadError::Compile(e.to_string()))
    }

    /// Creates the cache dir or checks that it is still private. A dir that
    /// was readable or writable by others is tightened and emptied: what it
    /// holds may have been planted.
    fn trusted_dir(&self) -> bool {
        match create_private_dir(&self.dir) {
            Ok(false) => true,
            Ok(true) => {
                log::warn!("{} was not private; clearing it", self.dir.display());
                purge(&self.dir);
                true
            }
            Err(e) => {
                log::warn!("not caching compiled plugins: {e}");
                false
            }
        }
    }

    /// The entry for `wasm`: its path and file name.
    fn entry_path(&self, engine: &Engine, wasm: &[u8]) -> (PathBuf, String) {
        let source = hex(&Sha256::digest(wasm));
        let mut compat = DigestHasher::default();
        engine.precompile_compatibility_hash().hash(&mut compat);
        let compat = hex(&compat.0.finalize()[..8]);
        let name = format!("{source}-{compat}.cwasm");
        (self.dir.join(&name), name)
    }

    /// Compiles `wasm` and writes it, sealed, to `path`; returns the
    /// artifact.
    fn compile_to(
        &self,
        engine: &Engine,
        wasm: &[u8],
        path: &Path,
        name: &str,
    ) -> Result<Vec<u8>, LoadError> {
        self.compiles.fetch_add(1, Ordering::SeqCst);
        let compiled = engine
            .precompile_component(wasm)
            .map_err(|e| LoadError::Compile(e.to_string()))?;
        // Unique per writer: two threads (or two runtimes in one process)
        // may compile the same component at once; each renames its own file.
        static WRITERS: AtomicU64 = AtomicU64::new(0);
        let writer = WRITERS.fetch_add(1, Ordering::Relaxed);
        let write = || -> std::io::Result<()> {
            create_private_dir(&self.dir)?;
            let tmp = path.with_extension(format!("tmp{}-{writer}", std::process::id()));
            let mut f = private_file(&tmp)?;
            f.write_all(&compiled)?;
            f.write_all(&seal(name, &compiled))?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, path)
        };
        write().map_err(|e| LoadError::Io(format!("cannot write {}: {e}", path.display())))?;
        Ok(compiled)
    }
}

/// Bytes of the seal appended to every entry.
const SEAL_BYTES: usize = 32;
/// Largest entry read back (compiled code is a few times the wasm size).
const MAX_ENTRY_BYTES: u64 = 16 * MAX_WASM_BYTES;

/// sha256 over a domain tag, the entry name and the artifact.
fn seal(name: &str, artifact: &[u8]) -> [u8; SEAL_BYTES] {
    let mut h = Sha256::new();
    h.update(b"arcode-cwasm-1\0");
    h.update(name.as_bytes());
    h.update(b"\0");
    h.update(artifact);
    h.finalize().into()
}

/// The artifact stored at `path` if the file is trustworthy and sealed for
/// `name`; `None` (recompile) otherwise.
fn read_entry(path: &Path, name: &str) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_ENTRY_BYTES {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.uid() != crate::paths::current_uid() || meta.permissions().mode() & 0o022 != 0 {
            log::warn!("ignoring {}: not a private file", path.display());
            return None;
        }
    }
    let mut bytes = fs::read(path).ok()?;
    let at = bytes.len().checked_sub(SEAL_BYTES)?;
    if bytes[at..] != seal(name, &bytes[..at]) {
        log::warn!("ignoring {}: seal mismatch", path.display());
        return None;
    }
    bytes.truncate(at);
    Some(bytes)
}

/// Removes every file in `dir` (best effort).
fn purge(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| !t.is_dir()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Creates `path` for writing, owner-only on Unix.
fn private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Lower-case hex sha256 of a plugin's `plugin.wasm` (read like
/// [`ComponentCache::load`] reads it).
pub fn wasm_sha256(wasm: &Path) -> Result<String, LoadError> {
    Ok(hex(&Sha256::digest(read_wasm(wasm)?)))
}

/// Reads the plugin's component. It must be a regular file (a symlink to
/// one is fine): a FIFO would block forever and a device such as
/// `/dev/zero` reports length 0 and never ends. The read is bounded either
/// way.
fn read_wasm(path: &Path) -> Result<Vec<u8>, LoadError> {
    let io = |e: std::io::Error| LoadError::Io(e.to_string());
    let not_regular = || LoadError::Io(format!("{} is not a regular file", path.display()));
    if !fs::metadata(path).map_err(io)?.is_file() {
        return Err(not_regular());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        // A FIFO swapped in after the check must not block the open.
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(io)?;
    let meta = file.metadata().map_err(io)?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    if meta.len() > MAX_WASM_BYTES {
        return Err(LoadError::TooLarge(meta.len()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_WASM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_WASM_BYTES {
        return Err(LoadError::TooLarge(bytes.len() as u64));
    }
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Feeds `Hash` output into sha256, so the file name is stable across runs
/// (unlike `DefaultHasher`, whose algorithm may change between releases).
#[derive(Default)]
struct DigestHasher(Sha256);

impl Hasher for DigestHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}
