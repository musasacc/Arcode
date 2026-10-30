//! Plan step 12 benchmark (docs/PLUGINS.md §13): activation from the
//! `.cwasm` cache and the resident memory of one idle instance, for the
//! bundled plugins in `target/wasm-plugins` (`scripts/build-plugins.sh`).
//!
//! ```sh
//! scripts/build-plugins.sh
//! cargo run --release -p arcode-plugin-host --example plugin_bench
//! ```
//!
//! Targets: activation from cache under 30 ms; RSS under 5 MB (HTTP) and
//! around 15 MB (DB). "Activation" is the first call into a fresh instance:
//! thread start, `.cwasm` deserialize, import check, instantiate,
//! `activate(restore)` and one trivial export (`view-closed`).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arcode_plugin_host::manifest::UserLimits;
use arcode_plugin_host::registry::load;
use arcode_plugin_host::{
    Call, InstanceHandle, InstanceSpec, PluginSource, RegisteredPlugin, Runtime, RuntimeConfig,
    Services,
};

const RUNS: usize = 20;

fn rss() -> usize {
    memory_stats::memory_stats().map_or(0, |m| m.physical_mem)
}

fn mb(bytes: isize) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn plugin(dir: &Path) -> Option<RegisteredPlugin> {
    let manifest = load(dir, PluginSource::Bundled).ok()?;
    let limits = manifest.limits.clamp(&UserLimits::default());
    Some(RegisteredPlugin {
        manifest,
        dir: dir.to_owned(),
        source: PluginSource::Bundled,
        limits,
        shadowed: Vec::new(),
    })
}

fn spec(plugin: &RegisteredPlugin) -> InstanceSpec {
    let (services, _outputs) = Services::headless();
    InstanceSpec {
        plugin: plugin.clone(),
        project: None,
        user_settings: serde_json::Map::new(),
        kv: arcode_plugin_host::host_impl::kv::KvFiles::none(),
        services,
        approved_sha256: None,
    }
}

/// One fresh instance answering one call; returns the time it took.
fn activate(runtime: &Arc<Runtime>, plugin: &RegisteredPlugin) -> (Duration, InstanceHandle) {
    let handle = InstanceHandle::new(runtime.clone(), spec(plugin));
    let started = Instant::now();
    let result = handle.dispatch(Call::ViewClosed(u32::MAX), false).wait();
    let took = started.elapsed();
    if let Err(e) = result {
        eprintln!("{}: {e:?}", plugin.id());
    }
    (took, handle)
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/wasm-plugins");
    let cache = std::env::temp_dir().join(format!("arcode-plugin-bench-{}", std::process::id()));
    let runtime = Arc::new(Runtime::new(RuntimeConfig::new(cache.clone())));
    let Ok(engine) = runtime.engine() else {
        eprintln!("cannot create the engine");
        return;
    };
    println!(
        "| plugin | precompile | activation from cache (min / median) | RSS of one instance |"
    );
    println!("|---|---|---|---|");
    for id in ["arcode.http-client", "arcode.db-viewer"] {
        let Some(plugin) = plugin(&root.join(id)) else {
            eprintln!("{id}: not built; run scripts/build-plugins.sh");
            continue;
        };
        let started = Instant::now();
        if let Err(e) = runtime.cache().prepare(engine, &plugin.entry_path()) {
            eprintln!("{id}: {e}");
            continue;
        }
        let compile = started.elapsed();
        // Warm up once (thread stacks, allocator), then measure.
        activate(&runtime, &plugin).1.shutdown();
        let mut times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let (took, handle) = activate(&runtime, &plugin);
            times.push(took);
            handle.shutdown();
        }
        times.sort();
        std::thread::sleep(Duration::from_millis(100));
        let before = rss();
        let (_, handle) = activate(&runtime, &plugin);
        std::thread::sleep(Duration::from_millis(100));
        let grew = rss() as isize - before as isize;
        handle.shutdown();
        println!(
            "| {id} | {compile:.0?} | {:.1?} / {:.1?} | {} |",
            times[0],
            times[RUNS / 2],
            mb(grew)
        );
    }
    let _ = std::fs::remove_dir_all(cache);
}
