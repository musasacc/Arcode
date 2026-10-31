//! The wasmtime engine and linker (§2). Created on first activation, never
//! at startup; [`Runtime`] holds them lazily.
//!
//! Only this module and [`crate::bindings`] name wasmtime's engine types,
//! so the engine stays swappable.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wasmtime::component::{HasSelf, Linker};
use wasmtime::{Config, Engine};

use crate::budget::{CallBudgets, TICK, Ticker};
use crate::cache::ComponentCache;
use crate::limits::{CrashPolicy, StoreCaps};
use crate::state::HostState;

/// Maximum guest stack (§2).
pub const MAX_WASM_STACK: usize = 512 * 1024;

/// The §2 engine configuration: epoch interruption, 512 KiB wasm stack,
/// copy-on-write memory init, the default on-demand allocator. Compilation
/// is serial and wasm threads are unavailable because those wasmtime
/// features are not compiled in (see PLUGINS.md Appendix B).
pub fn engine_config() -> Config {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.epoch_interruption(true);
    config.max_wasm_stack(MAX_WASM_STACK);
    config.memory_init_cow(true);
    config
}

/// Tunables of the runtime. Defaults are the §5 values; tests shorten them.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Where precompiled `.cwasm` files go (`<cache_dir>/arcode/plugins`).
    pub cache_dir: PathBuf,
    /// CPU budgets.
    pub budgets: CallBudgets,
    /// Epoch period.
    pub tick: Duration,
    /// An instance with no open panel unloads after this long without calls.
    pub idle_timeout: Duration,
    /// Trap handling.
    pub crash: CrashPolicy,
    /// Store caps besides memory.
    pub store: StoreCaps,
    /// Mailbox capacity per instance.
    pub mailbox: usize,
    /// Stack of each plugin thread.
    pub thread_stack: usize,
    /// Network requests allowed per user gesture.
    pub gesture_requests: u32,
    /// Language ids the app itself provides. A plugin that lists one under
    /// `contributes.languages` does not get `editor = "context"` read access
    /// to documents of that language (only to its origin document), so
    /// claiming `rust` is not an unprompted "all open Rust files". The app
    /// should set this from its syntax registry; the default is
    /// [`DEFAULT_BUILTIN_LANGUAGES`].
    pub builtin_languages: BTreeSet<String>,
}

/// Language ids treated as built in when the app sets none: Arcode's own
/// grammars plus common LSP language ids.
pub const DEFAULT_BUILTIN_LANGUAGES: &[&str] = &[
    "c",
    "clojure",
    "cpp",
    "csharp",
    "css",
    "dart",
    "diff",
    "dockerfile",
    "elixir",
    "erlang",
    "fsharp",
    "go",
    "graphql",
    "groovy",
    "haskell",
    "html",
    "ini",
    "java",
    "javascript",
    "javascriptreact",
    "json",
    "jsonc",
    "julia",
    "kotlin",
    "latex",
    "less",
    "lua",
    "makefile",
    "markdown",
    "nix",
    "objective-c",
    "ocaml",
    "perl",
    "php",
    "plaintext",
    "powershell",
    "python",
    "r",
    "ruby",
    "rust",
    "scala",
    "scss",
    "shellscript",
    "sql",
    "svelte",
    "swift",
    "text",
    "toml",
    "typescript",
    "typescriptreact",
    "vue",
    "xml",
    "yaml",
    "zig",
];

impl RuntimeConfig {
    /// The §5 defaults with the given cache directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            budgets: CallBudgets::default(),
            tick: TICK,
            idle_timeout: Duration::from_secs(600),
            crash: CrashPolicy::default(),
            store: StoreCaps::default(),
            mailbox: 64,
            thread_stack: 2 * 1024 * 1024,
            gesture_requests: 64,
            builtin_languages: DEFAULT_BUILTIN_LANGUAGES
                .iter()
                .map(|l| (*l).to_owned())
                .collect(),
        }
    }
}

struct Loaded {
    engine: Engine,
    linker: Linker<HostState>,
    ticker: Ticker,
}

/// Engine, linker, ticker and compiled-component cache shared by all
/// plugin instances. Cheap to create: nothing is built until
/// [`Runtime::engine`] is first called.
pub struct Runtime {
    config: RuntimeConfig,
    cache: ComponentCache,
    loaded: OnceLock<Result<Loaded, String>>,
    network_enabled: AtomicBool,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("config", &self.config)
            .field("engine_created", &self.engine_created())
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// A runtime; creates no engine yet.
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            cache: ComponentCache::new(config.cache_dir.clone()),
            config,
            loaded: OnceLock::new(),
            network_enabled: AtomicBool::new(true),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// The compiled-component cache.
    pub fn cache(&self) -> &ComponentCache {
        &self.cache
    }

    /// True once the engine exists (it is created on first activation).
    pub fn engine_created(&self) -> bool {
        self.loaded.get().is_some()
    }

    /// `settings.plugins.network`, the master switch for plugin networking.
    pub fn set_network_enabled(&self, enabled: bool) {
        self.network_enabled.store(enabled, Ordering::SeqCst);
    }

    /// See [`Runtime::set_network_enabled`].
    pub fn network_enabled(&self) -> bool {
        self.network_enabled.load(Ordering::SeqCst)
    }

    fn loaded(&self) -> Result<&Loaded, String> {
        self.loaded
            .get_or_init(|| load(&self.config).map_err(|e| format!("{e:#}")))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The engine, created on first use.
    pub fn engine(&self) -> Result<&Engine, String> {
        self.loaded().map(|l| &l.engine)
    }

    /// The linker with WASI and the plugin world.
    pub(crate) fn linker(&self) -> Result<&Linker<HostState>, String> {
        self.loaded().map(|l| &l.linker)
    }

    /// The epoch ticker.
    pub fn ticker(&self) -> Result<&Ticker, String> {
        self.loaded().map(|l| &l.ticker)
    }
}

fn load(config: &RuntimeConfig) -> wasmtime::Result<Loaded> {
    let engine = Engine::new(&engine_config())?;
    let mut linker = Linker::new(&engine);
    // WASI (sync): cli, clocks, random, io, filesystem, sockets. Sockets are
    // linked so std links, but the context denies every address.
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    // Cancellable sleeps and descriptor caps on top (`wasi_hooks`).
    crate::wasi_hooks::add_to_linker(&mut linker)?;
    crate::bindings::PluginWorld::add_to_linker::<HostState, HasSelf<HostState>>(
        &mut linker,
        |state| state,
    )?;
    let ticker = Ticker::start(engine.clone(), config.tick).map_err(wasmtime::Error::new)?;
    Ok(Loaded {
        engine,
        linker,
        ticker,
    })
}
