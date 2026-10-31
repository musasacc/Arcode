//! Tests of the run readout (`terminal_run.rs`).

use super::*;

#[test]
fn durations_read_like_instruments() {
    assert_eq!(format_duration(Duration::from_millis(920)), "0.92 s");
    assert_eq!(format_duration(Duration::from_millis(9_994)), "9.99 s");
    assert_eq!(format_duration(Duration::from_millis(12_340)), "12.3 s");
    assert_eq!(format_duration(Duration::from_secs(125)), "2 m 05 s");
}

#[test]
fn readouts_follow_the_status() {
    let d = Some(Duration::from_millis(920));
    let e = Duration::from_millis(4_100);
    let text = |status, code, d, watched| readout_text(status, code, d, e, watched, false);
    assert_eq!(
        text(RunStatus::Failed, Some(101), d, true),
        "exit 101 · 0.92 s"
    );
    assert_eq!(
        text(RunStatus::Succeeded, Some(0), d, true),
        "exit 0 · 0.92 s"
    );
    assert_eq!(text(RunStatus::Failed, None, d, true), "stopped · 0.92 s");
    assert_eq!(text(RunStatus::Running, None, None, true), "running · 4 s");
    assert_eq!(text(RunStatus::Ended, None, d, true), "done · 0.92 s");
    assert_eq!(text(RunStatus::Ended, None, None, true), "done");
    assert_eq!(text(RunStatus::Ended, None, None, false), "started");
    // Low-end mode only changes the running count; ended runs keep
    // their exact duration.
    assert_eq!(
        readout_text(RunStatus::Running, None, None, e, true, true),
        "running"
    );
    assert_eq!(
        readout_text(RunStatus::Failed, Some(101), d, e, true, true),
        "exit 101 · 0.92 s"
    );
}

#[test]
fn a_long_run_ticks_once_a_minute() {
    let at = |secs| running_text(Duration::from_secs(secs), false);
    assert_eq!(at(0), "running · 0 s");
    assert_eq!(at(59), "running · 59 s");
    assert_eq!(at(60), "running · 1 m");
    assert_eq!(at(119), "running · 1 m");
    assert_eq!(at(3_600), "running · 60 m");
    let coarse = |secs| running_text(Duration::from_secs(secs), true);
    assert_eq!(coarse(5), "running");
    assert_eq!(coarse(59), "running");
    assert_eq!(coarse(61), "running · 1 m");
    // How often the text changes over an hour: once a second for a
    // minute, then once a minute; low-end mode once a minute.
    let changes = |coarse: bool| {
        (1..3_600u64)
            .filter(|&s| {
                running_text(Duration::from_secs(s), coarse)
                    != running_text(Duration::from_secs(s - 1), coarse)
            })
            .count()
    };
    assert_eq!(changes(false), 59 + 59);
    assert_eq!(changes(true), 59);
}

#[test]
fn only_a_readout_on_screen_asks_for_a_redraw() {
    let r = run(false, None, true);
    // Never drawn (panel closed, Debug tab shown): never stale.
    assert!(!r.stale("running · 5 s"));
    // Drawn with the same text: nothing to do.
    *r.drawn.borrow_mut() = Some("running · 5 s".into());
    assert!(!r.stale("running · 5 s"));
    // Drawn with an older text: one redraw, then quiet until drawn
    // again.
    assert!(r.stale("running · 6 s"));
    assert!(!r.stale("running · 7 s"));
    *r.drawn.borrow_mut() = Some("running · 7 s".into());
    assert!(r.stale("running · 8 s"));
}

#[test]
fn polling_slows_down() {
    assert_eq!(poll_interval(Duration::ZERO), Duration::from_millis(100));
    assert_eq!(
        poll_interval(Duration::from_secs(5)),
        Duration::from_millis(250)
    );
    assert_eq!(
        poll_interval(Duration::from_secs(60)),
        Duration::from_secs(1)
    );
}

fn run(ended: bool, exit: Option<Option<i32>>, watched: bool) -> LastRun {
    LastRun {
        command: "cargo run".into(),
        started: Instant::now(),
        duration: None,
        ended,
        exit,
        watched,
        terminal: EntityId::from(1u64),
        drawn: RefCell::new(None),
        _watch: None,
    }
}

#[test]
fn status_from_the_exit() {
    assert_eq!(run(false, None, true).status(), RunStatus::Running);
    assert_eq!(
        run(true, Some(Some(0)), true).status(),
        RunStatus::Succeeded
    );
    assert_eq!(run(true, Some(Some(2)), true).status(), RunStatus::Failed);
    assert_eq!(run(true, Some(None), true).status(), RunStatus::Failed);
    assert_eq!(run(true, None, true).status(), RunStatus::Ended);
    // Not watchable (no way to see the end): reported as started.
    assert_eq!(run(false, None, false).status(), RunStatus::Ended);
    assert_eq!(run(false, None, false).readout(false), "started");
}
