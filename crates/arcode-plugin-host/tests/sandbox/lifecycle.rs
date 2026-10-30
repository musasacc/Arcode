//! Step 3: compiled-artifact cache, idle thread, idle unload, deactivate.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use arcode_plugin_host::consent::DenyAll;
use arcode_plugin_host::{CallOutput, PluginOutput};
use serde_json::json;

use crate::common::*;

fn echo_services(app: &App) -> arcode_plugin_host::Services {
    services(
        app,
        Arc::new(DenyAll),
        Arc::new(FakeEditor::default()),
        Arc::new(FakeHttp::default()),
    )
}

#[test]
fn second_load_deserializes_without_compiling() {
    let cache = tempfile::tempdir().unwrap();
    let app = App::start(None);
    for round in 0..2 {
        let (rt, _unused) = runtime(|c| c.cache_dir = cache.path().join("cwasm"));
        assert!(!rt.engine_created(), "no engine before the first call");
        let h = handle(&rt, fixture("echo"), echo_services(&app), None);
        assert!(!h.thread_running(), "no thread before the first call");
        let r = command(&h, "echo", json!({"op": "api-version"}), false);
        assert_eq!(r, Ok(CallOutput::Done));
        let (compiles, hits) = (rt.cache().compiles(), rt.cache().cache_hits());
        if round == 0 {
            assert_eq!((compiles, hits), (1, 0), "first load compiles");
        } else {
            assert_eq!(
                (compiles, hits),
                (0, 1),
                "second load comes from the .cwasm"
            );
        }
    }
    let entries: Vec<_> = std::fs::read_dir(cache.path().join("cwasm"))
        .unwrap()
        .collect();
    assert_eq!(entries.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(cache.path().join("cwasm"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}

#[test]
fn corrupt_cache_entry_is_recompiled() {
    let (rt, _cache) = runtime(|_| {});
    let engine = rt.engine().unwrap();
    let wasm = fixture("echo").entry_path();
    let path = rt.cache().prepare(engine, &wasm).unwrap();
    assert_eq!(rt.cache().compiles(), 1);
    std::fs::write(&path, b"not a cwasm").unwrap();
    rt.cache().load(engine, &wasm).unwrap();
    assert_eq!(rt.cache().compiles(), 2);
    rt.cache().load(engine, &wasm).unwrap();
    assert_eq!((rt.cache().compiles(), rt.cache().cache_hits()), (2, 1));
}

/// Linux only: the scheduler state of our instance thread.
#[cfg(target_os = "linux")]
fn plugin_thread_states(prefix: &str) -> Vec<String> {
    let mut states = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
        let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        if comm.trim_end() == prefix {
            let stat = std::fs::read_to_string(task.path().join("stat")).unwrap_or_default();
            // "pid (comm) S ..." — the state follows the closing paren.
            if let Some(after) = stat.rsplit_once(')').map(|(_, a)| a.trim_start()) {
                states.push(after.chars().take(1).collect());
            }
        }
    }
    states
}

#[test]
fn idle_instance_thread_is_blocked_on_its_mailbox() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let h = handle(&rt, fixture("echo"), echo_services(&app), None);
    assert_eq!(
        command(&h, "echo", json!({"op": "cancelled"}), false),
        Ok(CallOutput::Done)
    );
    std::thread::sleep(Duration::from_millis(50));
    let wakeups = h.stats().wakeups.load(Ordering::SeqCst);
    let ticks = rt.ticker().unwrap().increments();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        h.stats().wakeups.load(Ordering::SeqCst),
        wakeups,
        "the thread did not wake up"
    );
    assert_eq!(
        rt.ticker().unwrap().increments(),
        ticks,
        "no epoch ticks while idle"
    );
    assert!(h.thread_running() && h.stats().loaded.load(Ordering::SeqCst));
    #[cfg(target_os = "linux")]
    {
        // Thread names are cut to 15 bytes by the kernel. Other tests run
        // echo instances in parallel, so ours is one of possibly several.
        let states = plugin_thread_states("plugin-test.ech");
        assert!(
            states.iter().any(|s| s == "S"),
            "a sleeping instance thread: {states:?}"
        );
        let tickers = plugin_thread_states("plugin-epoch");
        assert!(
            tickers.iter().any(|s| s == "S"),
            "a parked ticker: {tickers:?}"
        );
    }
}

#[test]
fn idle_unload_calls_deactivate_and_restarts_on_demand() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| c.idle_timeout = Duration::from_millis(200));
    let h = handle(&rt, fixture("echo"), echo_services(&app), None);
    assert_eq!(
        command(&h, "echo", json!({"op": "cancelled"}), false),
        Ok(CallOutput::Done)
    );
    let start = Instant::now();
    assert!(
        h.wait_stopped(Duration::from_secs(5)),
        "unloaded after the idle timeout"
    );
    assert!(start.elapsed() >= Duration::from_millis(150));
    assert_eq!(
        json_lines(&h).last().unwrap(),
        &json!({"export": "deactivate"})
    );
    assert!(!h.stats().loaded.load(Ordering::SeqCst));
    assert!(app.wait_for(|o| matches!(o, PluginOutput::Unloaded { .. })));

    // The next call starts a new thread and activates with `restore`.
    assert_eq!(
        command(&h, "echo", json!({"op": "cancelled"}), false),
        Ok(CallOutput::Done)
    );
    assert_eq!(h.stats().threads.load(Ordering::SeqCst), 2);
    assert_eq!(h.stats().instantiations.load(Ordering::SeqCst), 2);
    assert_eq!(
        last_with(&h, "export", "activate")["reason"],
        "Activation::Restore"
    );
}

#[test]
fn an_open_panel_keeps_the_instance_loaded() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| c.idle_timeout = Duration::from_millis(100));
    let h = handle(&rt, fixture("echo"), echo_services(&app), None);
    let (panel, p) = h.open_view("main", "{}");
    assert_eq!(p.wait(), Ok(CallOutput::Done));
    std::thread::sleep(Duration::from_millis(400));
    assert!(h.thread_running() && h.stats().loaded.load(Ordering::SeqCst));
    assert_eq!(h.close_view(panel).wait(), Ok(CallOutput::Done));
    assert!(h.wait_stopped(Duration::from_secs(5)));
}
