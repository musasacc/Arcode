//! Adversarial review tests for `arcode-terminal`. Each test demonstrates
//! one defect found during review and is expected to FAIL until it is fixed.

use arcode_terminal::{Cell, Palette, Snapshot, TermModeFlags};

/// `Snapshot::row` returns `Option`, so out-of-range rows must give `None`;
/// instead `start + self.cols` overflows (panics in debug builds).
#[test]
fn snapshot_row_out_of_range_does_not_panic() {
    let p = Palette::default();
    let snap = Snapshot {
        cols: 2,
        rows: 1,
        cells: vec![Cell::blank(&p); 2],
        cursor: None,
        display_offset: 0,
        history_size: 0,
        mode: TermModeFlags::default(),
    };
    let row = usize::MAX / 2;
    let result = std::panic::catch_unwind(|| snap.row(row).is_none());
    assert!(
        matches!(result, Ok(true)),
        "Snapshot::row({row}) panicked instead of returning None"
    );
    assert_eq!(snap.row_text(row), "");
}

#[cfg(unix)]
mod pty {
    use std::time::{Duration, Instant};

    use arcode_terminal::{GridPoint, Terminal, TerminalEvent, TerminalOptions};
    use async_channel::Receiver;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn sh(script: &str) -> TerminalOptions {
        TerminalOptions::command("/bin/sh", ["-c", script])
    }

    fn wait_for_text(term: &Terminal, rx: &Receiver<TerminalEvent>, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            while rx.try_recv().is_ok() {}
            let text = term.snapshot_text();
            if text.contains(needle) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}; screen: {text:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// The selection anchor is stored as an alacritty grid `Point`, whose
    /// line numbers shift when output scrolls the screen. alacritty rotates
    /// its own `Term::selection`, but `Terminal` keeps the stale anchor and
    /// rebuilds the selection from it on `selection_update`, so a drag that
    /// continues while output arrives selects the wrong text.
    #[test]
    fn selection_anchor_follows_scrolling_output() {
        let script = "printf 'anchor-line\\n'; read x; i=0; \
                      while [ $i -lt 10 ]; do echo filler$i; i=$((i+1)); done; sleep 5";
        let (term, rx) = Terminal::spawn(TerminalOptions {
            rows: 5,
            cols: 40,
            ..sh(script)
        })
        .expect("spawn");
        wait_for_text(&term, &rx, "anchor-line");
        term.selection_start(GridPoint::new(0, 0));
        term.selection_update(GridPoint::new(0, 10));
        assert_eq!(term.selection_text().as_deref(), Some("anchor-line"));

        // More output scrolls "anchor-line" into the history.
        term.write("\r");
        wait_for_text(&term, &rx, "filler9");

        // The user keeps dragging to the bottom row.
        term.selection_update(GridPoint::new(4, 0));
        let text = term.selection_text().unwrap_or_default();
        assert!(
            text.starts_with("anchor-line"),
            "selection lost its anchor after output scrolled; got {text:?}"
        );
    }

    /// Color queries (OSC 10/11/4 `?`) are answered from the UI palette only,
    /// ignoring colors the program set at runtime, although snapshots honor
    /// those overrides. After `OSC 11;rgb:12/34/56`, querying OSC 11 must
    /// report that color, as xterm and alacritty do.
    #[test]
    fn color_query_reports_runtime_override() {
        // Reply: ESC ] 1 1 ; r g b : 1212/3434/5656 BEL = 24 bytes.
        let script = "stty -echo -icanon min 1; \
                      printf '\\033]11;rgb:12/34/56\\007\\033]11;?\\007'; \
                      r=$(dd bs=1 count=24 2>/dev/null); printf 'got:%s|' \"${r#?}\"; sleep 0.5";
        let (term, rx) = Terminal::spawn(sh(script)).expect("spawn");
        wait_for_text(&term, &rx, "got:");
        wait_for_text(&term, &rx, "|");
        let text = term.snapshot_text();
        assert!(
            text.contains("got:]11;rgb:1212/3434/5656"),
            "color query ignored the runtime override; screen: {text:?}"
        );
    }

    /// Spawns a child that ignores SIGHUP, optionally calls `shutdown()` and
    /// lets the PTY thread process it, drops the `Terminal` on a helper
    /// thread, and returns (did `drop` return within 2 s, did the child die
    /// within 5 s). Always SIGKILLs the child afterwards so nothing hangs.
    fn drop_hup_ignoring_child(shutdown_first: bool) -> (bool, bool) {
        let (term, rx) =
            Terminal::spawn(sh("trap '' HUP; echo ready; while :; do sleep 0.1; done"))
                .expect("spawn");
        wait_for_text(&term, &rx, "ready");
        let pid = term.child_pid().expect("pid").to_string();
        assert!(alive(&pid));
        if shutdown_first {
            term.shutdown();
            // Give the PTY thread time to see `Msg::Shutdown` and return.
            std::thread::sleep(Duration::from_millis(500));
        }

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(term);
            let _ = done_tx.send(());
        });
        let drop_returned = done_rx.recv_timeout(Duration::from_secs(2)).is_ok();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut child_died = false;
        while Instant::now() < deadline {
            if !alive(&pid) {
                child_died = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Clean up so neither the process nor a blocked thread leaks.
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid])
            .status();
        let _ = done_rx.recv_timeout(TIMEOUT);
        (drop_returned, child_died)
    }

    /// `Drop for Terminal` claims it never blocks the UI thread, but the
    /// PTY thread *returns* its `EventLoop` (which owns the `Pty`) through
    /// the `JoinHandle`. Once that thread has finished (after `shutdown()`,
    /// or by losing the race with `Msg::Shutdown` in `drop`), dropping the
    /// handle drops the `Pty` on the caller's thread, and `Pty::drop` blocks
    /// in `waitpid` until the child exits: forever for a child that ignores
    /// SIGHUP, freezing the UI.
    #[test]
    fn drop_after_shutdown_does_not_block_the_caller() {
        let (drop_returned, _) = drop_hup_ignoring_child(true);
        assert!(drop_returned, "dropping the Terminal blocked the caller");
    }

    /// Spec: "kill on Drop". Only SIGHUP is sent, so a child that ignores
    /// it survives (and whichever thread drops the `Pty` waits forever).
    #[test]
    fn drop_kills_child_that_ignores_sighup() {
        let (_, child_died) = drop_hup_ignoring_child(false);
        assert!(child_died, "child survived dropping the Terminal");
    }

    /// `shutdown()` is documented to hang up and terminate the child, but
    /// the PTY thread returns the `Pty` instead of dropping it, so nothing
    /// signals the child until the `Terminal` itself is dropped.
    #[test]
    fn shutdown_terminates_child() {
        let (term, rx) = Terminal::spawn(sh("echo ready; sleep 30; true")).expect("spawn");
        wait_for_text(&term, &rx, "ready");
        let pid = term.child_pid().expect("pid").to_string();
        term.shutdown();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut child_died = false;
        while Instant::now() < deadline {
            if !alive(&pid) {
                child_died = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(term);
        assert!(child_died, "child {pid} still running after shutdown()");
    }
}
