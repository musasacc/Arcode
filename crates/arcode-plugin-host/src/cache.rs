//! Compiled-artifact cache (§2): `<cache>/<sha256(wasm)>-<compat>.cwasm`.
//!
//! Compiling happens once (at install, or on first activation without a
//! cache entry); later activations mmap the precompiled file. Only files
//! this cache wrote, named by content hash and engine compatibility hash,
//! are ever deserialized. A file that fails to deserialize is recompiled.

use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
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
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read plugin component: {e}"),
            Self::TooLarge(n) => write!(f, "plugin component is too large ({n} bytes)"),
            Self::Compile(e) => write!(f, "invalid plugin component: {e}"),
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
        let path = self.entry_path(engine, &bytes);
        if !path.is_file() {
            self.compile_to(engine, &bytes, &path)?;
        }
        Ok(path)
    }

    /// Loads `wasm`, from the cache when possible.
    pub fn load(&self, engine: &Engine, wasm: &Path) -> Result<Component, LoadError> {
        let bytes = read_wasm(wasm)?;
        let path = self.entry_path(engine, &bytes);
        if path.is_file() {
            // SAFETY: the file is in our private cache dir and named by the
            // sha256 of its source and the engine's compatibility hash; we
            // only deserialize files this cache wrote.
            match unsafe { Component::deserialize_file(engine, &path) } {
                Ok(component) => {
                    self.cache_hits.fetch_add(1, Ordering::SeqCst);
                    return Ok(component);
                }
                Err(e) => log::warn!("recompiling {}: {e}", path.display()),
            }
        }
        self.compile_to(engine, &bytes, &path)?;
        // SAFETY: as above; just written by `compile_to`.
        unsafe { Component::deserialize_file(engine, &path) }
            .map_err(|e| LoadError::Compile(e.to_string()))
    }

    fn entry_path(&self, engine: &Engine, wasm: &[u8]) -> PathBuf {
        let source = hex(&Sha256::digest(wasm));
        let mut compat = DigestHasher::default();
        engine.precompile_compatibility_hash().hash(&mut compat);
        let compat = hex(&compat.0.finalize()[..8]);
        self.dir.join(format!("{source}-{compat}.cwasm"))
    }

    fn compile_to(&self, engine: &Engine, wasm: &[u8], path: &Path) -> Result<(), LoadError> {
        self.compiles.fetch_add(1, Ordering::SeqCst);
        let compiled = engine
            .precompile_component(wasm)
            .map_err(|e| LoadError::Compile(e.to_string()))?;
        let write = || -> std::io::Result<()> {
            create_private_dir(&self.dir)?;
            let tmp = path.with_extension(format!("tmp{}", std::process::id()));
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&compiled)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, path)
        };
        write().map_err(|e| LoadError::Io(format!("cannot write {}: {e}", path.display())))
    }
}

fn read_wasm(path: &Path) -> Result<Vec<u8>, LoadError> {
    let meta = fs::metadata(path).map_err(|e| LoadError::Io(e.to_string()))?;
    if meta.len() > MAX_WASM_BYTES {
        return Err(LoadError::TooLarge(meta.len()));
    }
    fs::read(path).map_err(|e| LoadError::Io(e.to_string()))
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
