//! Step 4: CPU budgets, cancellation, memory limits, crash backoff, log
//! cap and the idle ticker.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use arcode_plugin_host::bindings::editor::DocInfo;
use arcode_plugin_host::consent::DenyAll;
use arcode_plugin_host::plugin_log::{LOG_CAP_BYTES, LogSource};
use arcode_plugin_host::{Call, CallError, CallOutput, InstanceHandle, PluginOutput, Services};
use serde_json::json;

use crate::common::*;

fn plain(app: &App) -> Services {
    services(
        app,
        Arc::new(DenyAll),
        Arc::new(FakeEditor::default()),
        Arc::new(FakeHttp::default()),
    )
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn run(args: serde_json::Value) -> Call {
    Call::Command {
        command: "run".into(),
        args: args.to_string(),
    }
}

/// Runs one spin call next to a busy echo instance; returns the spin
/// result, its wall time and how many echo calls were answered meanwhile.
fn spin_once(
    spin: &InstanceHandle,
    echo: &InstanceHandle,
) -> (arcode_plugin_host::CallResult, Duration, u32) {
    let start = Instant::now();
    let pending = spin.dispatch(run(json!({})), true);
    let mut answers = 0;
    while pending.result.is_empty() {
        let t = Instant::now();
        assert_eq!(
            command(echo, "echo", json!({"op": "cancelled"}), false),
            Ok(CallOutput::Done)
        );
        assert!(t.elapsed() < ms(100), "echo answered in {:?}", t.elapsed());
        answers += 1;
        std::thread::sleep(ms(20));
    }
    let result = pending.wait();
    (result, start.elapsed(), answers)
}

#[test]
fn spin_traps_within_budget_while_another_instance_answers() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| {
        c.budgets.command = ms(300);
        c.crash.max_crashes = 10;
        c.crash.backoff = Duration::ZERO;
    });
    let spin = handle(&rt, fixture("spin"), plain(&app), None);
    let echo = handle(&rt, fixture("echo"), plain(&app), None);
    // Warm both up so compile and activation are not timed.
    assert_eq!(
        spin.dispatch(run(json!({"noop": true})), false).wait(),
        Ok(CallOutput::Done)
    );
    assert_eq!(
        command(&echo, "echo", json!({"op": "cancelled"}), false),
        Ok(CallOutput::Done)
    );

    // The budget counts the guest's run time, checked at each epoch tick,
    // so the trap comes at most about one (possibly late) tick after the
    // budget: macOS runners deliver 10 ms ticks ~25 ms apart. Wall time can
    // exceed that only when the machine is busy elsewhere (other test
    // binaries, builds): allow a few attempts for that, but every attempt
    // must trap on budget.
    let mut times = Vec::new();
    for _ in 0..3 {
        // Re-activate outside the timed window after the trap.
        spin.dispatch(run(json!({"noop": true})), false)
            .wait()
            .unwrap();
        let (result, elapsed, answers) = spin_once(&spin, &echo);
        assert_eq!(
            result,
            Err(CallError::Trapped("exceeded 300 ms CPU".into()))
        );
        assert!(elapsed >= ms(300), "trapped early: {elapsed:?}");
        assert!(answers >= 5, "echo kept answering ({answers} calls)");
        times.push(elapsed);
        if elapsed <= ms(400) {
            break;
        }
    }
    eprintln!("spin: wall time to trap {times:?}");
    assert!(
        times.iter().any(|t| *t <= ms(400)),
        "budget + 100 ms exceeded: {times:?}"
    );
    assert!(app.wait_for(|o| matches!(o, PluginOutput::Crashed { reason, disabled: false, .. } if reason == "exceeded 300 ms CPU")));
}

#[test]
fn begin_task_raises_the_budget_only_in_a_gesture() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| c.budgets.command = ms(100));
    let mut plugin = fixture("spin");
    plugin.limits.task_seconds = 1;
    let spin = handle(&rt, plugin, plain(&app), None);
    spin.dispatch(run(json!({"noop": true})), false)
        .wait()
        .unwrap();

    let start = Instant::now();
    let r = spin.dispatch(run(json!({"task": true})), true).wait();
    assert_eq!(r, Err(CallError::Trapped("exceeded 1 s CPU".into())));
    assert!(
        start.elapsed() >= ms(1000) && start.elapsed() < ms(1500),
        "{:?}",
        start.elapsed()
    );
    assert!(
        spin.log()
            .lines()
            .iter()
            .any(|l| l.text == "begin-task=true")
    );
    assert!(
        app.wait_for(
            |o| matches!(o, PluginOutput::TaskStarted { title, .. } if title == "Spinning")
        )
    );
    assert!(app.wait_for(|o| matches!(o, PluginOutput::TaskFinished { .. })));

    let start = Instant::now();
    let r = spin.dispatch(run(json!({"task": true})), false).wait();
    assert_eq!(r, Err(CallError::Trapped("exceeded 100 ms CPU".into())));
    assert!(start.elapsed() < ms(600), "{:?}", start.elapsed());
    assert!(
        spin.log()
            .lines()
            .iter()
            .any(|l| l.text == "begin-task=false")
    );
}

#[test]
fn spin_cancellable_returns_early_on_cancel() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| c.budgets.command = Duration::from_secs(20));
    let h = handle(&rt, fixture("spin-cancellable"), plain(&app), None);
    let pending = h.dispatch(run(json!({})), true);
    std::thread::sleep(ms(300));
    assert!(pending.result.is_empty(), "still running before cancel");
    let cancelled_at = Instant::now();
    pending.cancel.cancel();
    assert_eq!(pending.wait(), Err(CallError::Plugin("cancelled".into())));
    eprintln!(
        "spin-cancellable: returned {:?} after cancel",
        cancelled_at.elapsed()
    );
    assert!(
        cancelled_at.elapsed() < ms(100),
        "{:?}",
        cancelled_at.elapsed()
    );
    assert!(
        h.log()
            .lines()
            .iter()
            .any(|l| l.text.starts_with("cancelled after"))
    );
    // Cooperative: the instance survives.
    assert_eq!(h.stats().instantiations.load(Ordering::SeqCst), 1);
    assert!(h.stats().loaded.load(Ordering::SeqCst));
}

#[test]
fn hard_cancel_traps_after_the_grace_period() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| {
        c.budgets.command = Duration::from_secs(20);
        c.budgets.cancel_grace = ms(200);
    });
    let h = handle(&rt, fixture("spin"), plain(&app), None);
    h.dispatch(run(json!({"noop": true})), false)
        .wait()
        .unwrap();
    let pending = h.dispatch(run(json!({})), true);
    std::thread::sleep(ms(100));
    let cancelled_at = Instant::now();
    pending.cancel.cancel();
    assert_eq!(
        pending.wait(),
        Err(CallError::Trapped("did not stop after cancel".into()))
    );
    let waited = cancelled_at.elapsed();
    eprintln!("hard cancel: trapped {waited:?} after cancel");
    assert!(waited >= ms(200) && waited < ms(300), "{waited:?}");
}

#[test]
fn lens_overruns_strike_out_lenses_only() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let h = handle(&rt, fixture("spin"), plain(&app), None);
    let doc = DocInfo {
        id: 1,
        path: None,
        language: "spin".into(),
        version: 1,
        line_count: 1,
    };
    for _ in 0..3 {
        let r = h.dispatch(Call::CodeLenses(doc.clone()), false).wait();
        assert_eq!(r, Err(CallError::Trapped("exceeded 100 ms CPU".into())));
    }
    let r = h.dispatch(Call::CodeLenses(doc), false).wait();
    assert_eq!(r, Err(CallError::LensesDisabled));
    // Overruns are not crashes: commands still work.
    assert_eq!(
        h.dispatch(run(json!({"noop": true})), false).wait(),
        Ok(CallOutput::Done)
    );
    assert!(!h.is_disabled());
}

#[test]
fn alloc_bomb_hits_the_limit_and_the_host_survives() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let plugin = fixture("alloc-bomb");
    assert_eq!(plugin.limits.memory_bytes, 16 * 1024 * 1024);
    let h = handle(&rt, plugin, plain(&app), None);

    // Fallible allocation: the guest sees the failure inside its sandbox.
    assert_eq!(
        h.dispatch(run(json!({"mode": "try"})), false).wait(),
        Ok(CallOutput::Done)
    );
    let report = last_with(&h, "op", "try");
    let reached = report["reached-mib"].as_u64().unwrap();
    eprintln!("alloc-bomb: {report}");
    assert!((8..16).contains(&reached), "{report}");

    // Infallible allocation: the guest aborts, the host reports OOM.
    let r = h.dispatch(run(json!({"mode": "abort"})), false).wait();
    assert_eq!(
        r,
        Err(CallError::Trapped("out of memory (limit 16 MiB)".into()))
    );
    let peak = h.stats().peak_memory.load(Ordering::SeqCst);
    assert!(peak <= 16 * 1024 * 1024, "peak {peak}");

    // The host is fine and the plugin restarts on the next call.
    assert_eq!(
        h.dispatch(run(json!({"mode": "try"})), false).wait(),
        Ok(CallOutput::Done)
    );
    assert_eq!(h.stats().instantiations.load(Ordering::SeqCst), 2);
}

#[test]
fn crash_backs_off_then_disables() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|c| c.crash.backoff = ms(300));
    let h = handle(&rt, fixture("crash"), plain(&app), None);
    let crash = || h.dispatch(run(json!({"crash": true})), false).wait();
    let ok = || h.dispatch(run(json!({})), false).wait();

    let r = crash();
    assert!(
        matches!(&r, Err(CallError::Trapped(m)) if m.starts_with("crashed")),
        "{r:?}"
    );
    assert!(
        h.log()
            .lines()
            .iter()
            .any(|l| l.source == LogSource::Stderr && l.text.contains("asked to crash"))
    );
    // First trap: restart at once, with `restore`.
    assert_eq!(ok(), Ok(CallOutput::Done));
    assert!(
        h.log()
            .lines()
            .iter()
            .any(|l| l.text == "activate Activation::Restore")
    );

    // Second trap: back off.
    assert!(matches!(crash(), Err(CallError::Trapped(_))));
    assert!(matches!(ok(), Err(CallError::Backoff(d)) if d <= ms(300)));
    std::thread::sleep(ms(350));
    assert_eq!(ok(), Ok(CallOutput::Done));

    // Third trap within ten minutes: disabled for the session.
    assert!(matches!(crash(), Err(CallError::Trapped(_))));
    assert_eq!(ok(), Err(CallError::Disabled));
    assert!(h.is_disabled());
    assert!(app.wait_for(|o| matches!(o, PluginOutput::Crashed { disabled: true, .. })));
    assert_eq!(h.stats().instantiations.load(Ordering::SeqCst), 3);
}

#[test]
fn stdout_flood_hits_the_log_cap() {
    let _serial = timing_lock();
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let h = handle(&rt, fixture("stdout-flood"), plain(&app), None);
    assert_eq!(
        h.dispatch(run(json!({"mib": 4})), false).wait(),
        Ok(CallOutput::Done)
    );
    let log = h.log();
    assert!(log.bytes() <= LOG_CAP_BYTES, "{} bytes kept", log.bytes());
    assert!(
        log.dropped_lines() > 3000,
        "{} lines dropped",
        log.dropped_lines()
    );
    let lines = log.lines();
    assert_eq!(lines.last().map(|l| l.text.as_str()), Some("done"));
    assert!(
        lines
            .iter()
            .any(|l| l.source == LogSource::Stderr && l.text == "stderr line")
    );
    // The newest stdout line survived, the oldest did not.
    assert!(lines.iter().any(|l| l.text.starts_with("00004095 ")));
    assert!(!lines.iter().any(|l| l.text.starts_with("00000000 ")));
}

#[test]
fn ticker_is_silent_without_calls_in_flight() {
    let app = App::start(None);
    let (rt, _cache) = runtime(|_| {});
    let h = handle(&rt, fixture("echo"), plain(&app), None);
    assert_eq!(
        command(&h, "echo", json!({"op": "cancelled"}), false),
        Ok(CallOutput::Done)
    );
    let ticker = rt.ticker().unwrap();
    std::thread::sleep(ms(50));
    assert_eq!(ticker.in_flight(), 0);
    let before = ticker.increments();
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(ticker.increments(), before, "zero increments over 1 s");
}
