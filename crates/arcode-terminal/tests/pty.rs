//! End-to-end tests against a real PTY running `/bin/sh` (Unix only).
#![cfg(unix)]

use std::time::{Duration, Instant};

use arcode_terminal::{
    GridPoint, KeyMods, Palette, Terminal, TerminalEvent, TerminalOptions, encode_key,
};
use async_channel::Receiver;

const TIMEOUT: Duration = Duration::from_secs(10);

fn sh(script: &str) -> TerminalOptions {
    TerminalOptions::command("/bin/sh", ["-c", script])
}

fn interactive() -> TerminalOptions {
    TerminalOptions {
        shell: Some("/bin/sh".into()),
        env: vec![("PS1".into(), "$ ".into()), ("ENV".into(), String::new())],
        ..Default::default()
    }
}

fn spawn(opts: TerminalOptions) -> (Terminal, Receiver<TerminalEvent>) {
    Terminal::spawn(opts).expect("spawn terminal")
}

/// Polls events until `done` returns true; panics after [`TIMEOUT`].
fn wait_until(
    rx: &Receiver<TerminalEvent>,
    events: &mut Vec<TerminalEvent>,
    what: &str,
    mut done: impl FnMut(&[TerminalEvent]) -> bool,
) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        if done(events) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; events: {events:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_text(
    term: &Terminal,
    rx: &Receiver<TerminalEvent>,
    needle: &str,
) -> Vec<TerminalEvent> {
    let mut events = Vec::new();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        let text = term.snapshot_text();
        if text.contains(needle) {
            return events;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {needle:?}; screen: {text:?}; events: {events:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// An interactive `/bin/sh` that has printed its first prompt, so input
/// typed from now on is not discarded by its terminal setup.
fn spawn_ready_shell() -> (Terminal, Receiver<TerminalEvent>) {
    let (term, rx) = spawn(interactive());
    wait_for_text(&term, &rx, "$");
    (term, rx)
}

fn wait_for_exit(rx: &Receiver<TerminalEvent>) -> Vec<TerminalEvent> {
    let mut events = Vec::new();
    wait_until(rx, &mut events, "exit", |evs| {
        evs.contains(&TerminalEvent::Exit)
    });
    events
}

#[test]
fn prints_output() {
    let (term, rx) = spawn(sh("printf 'hello arcode'; sleep 0.3"));
    let events = wait_for_text(&term, &rx, "hello arcode");
    assert!(events.contains(&TerminalEvent::Wakeup) || term.snapshot_text().contains("hello"));
    let snap = term.snapshot(&Palette::default());
    assert_eq!((snap.cols, snap.rows), (80, 24));
    assert_eq!(snap.row_text(0), "hello arcode");
    assert_eq!(snap.cursor.map(|c| (c.row, c.col)), Some((0, 12)));
    assert!(term.child_pid().is_some());
}

#[test]
fn interactive_shell_runs_command() {
    let (term, rx) = spawn_ready_shell();
    assert!(term.is_alive());
    term.write("echo $((6*7))\r");
    wait_for_text(&term, &rx, "\n42");
    term.run_command("echo done-$((1+1))");
    wait_for_text(&term, &rx, "done-2");
    term.run_command("exit 0");
    wait_for_exit(&rx);
}

#[test]
fn keys_reach_the_shell() {
    let (term, rx) = spawn_ready_shell();
    // Type a command, cancel it with ctrl+c, then run another one.
    term.write("echo nope");
    wait_for_text(&term, &rx, "echo nope");
    term.write(encode_key("c", KeyMods::CTRL, false).unwrap());
    let mut events = Vec::new();
    wait_until(&rx, &mut events, "second prompt", |_| {
        term.snapshot_text()
            .lines()
            .filter(|l| l.starts_with('$'))
            .count()
            >= 2
    });
    for key in ["e", "c", "h", "o", "space", "k", "e", "y", "s"] {
        term.write(encode_key(key, KeyMods::NONE, false).unwrap());
    }
    term.write(encode_key("enter", KeyMods::NONE, false).unwrap());
    let deadline = Instant::now() + TIMEOUT;
    while !term.snapshot_text().lines().any(|l| l.trim() == "keys") {
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(
            Instant::now() < deadline,
            "timed out: {:?} {events:?}",
            term.snapshot_text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!term.snapshot_text().lines().any(|l| l.trim() == "nope"));
}

#[test]
fn resize_updates_grid_and_pty() {
    let (term, rx) = spawn_ready_shell();
    term.resize(100, 30, 9, 18);
    assert_eq!(term.size(), (100, 30));
    let snap = term.snapshot(&Palette::default());
    assert_eq!((snap.cols, snap.rows, snap.cells.len()), (100, 30, 3000));
    term.run_command("stty size");
    wait_for_text(&term, &rx, "30 100");
    // Degenerate sizes are clamped instead of panicking.
    term.resize(0, 0, 0, 0);
    let snap = term.snapshot(&Palette::default());
    assert_eq!((snap.cols, snap.rows), (2, 1));
}

#[test]
fn exit_reports_status() {
    let (term, rx) = spawn(sh("exit 3"));
    let events = wait_for_exit(&rx);
    assert!(
        events.contains(&TerminalEvent::ChildExit(Some(3))),
        "{events:?}"
    );
    let deadline = Instant::now() + TIMEOUT;
    while term.is_alive() {
        assert!(Instant::now() < deadline, "terminal still alive");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Writing after exit is harmless.
    term.write("ignored");
    term.resize(40, 10, 8, 16);
}

#[test]
fn output_before_exit_is_drained() {
    let (term, rx) = spawn(sh("printf 'last words'"));
    wait_for_exit(&rx);
    assert!(term.snapshot_text().contains("last words"));
}

#[test]
fn title_and_bell_events() {
    let (_term, rx) = spawn(sh(
        "printf '\\033]0;my title\\007'; printf '\\007'; sleep 0.3",
    ));
    let mut events = Vec::new();
    wait_until(&rx, &mut events, "title and bell", |evs| {
        evs.contains(&TerminalEvent::Title("my title".into())) && evs.contains(&TerminalEvent::Bell)
    });
}

#[test]
fn osc52_clipboard_store() {
    // "copied" in base64 is "Y29waWVk".
    let (_term, rx) = spawn(sh("printf '\\033]52;c;Y29waWVk\\007'; sleep 0.3"));
    let mut events = Vec::new();
    wait_until(&rx, &mut events, "clipboard", |evs| {
        evs.contains(&TerminalEvent::ClipboardStore("copied".into()))
    });
}

#[test]
fn answers_cursor_position_report() {
    let script = "stty -echo -icanon min 1; printf '\\033[6n'; \
                  r=$(dd bs=1 count=6 2>/dev/null); printf 'got:%s' \"${r#?}\"; sleep 0.3";
    let (term, rx) = spawn(sh(script));
    wait_for_text(&term, &rx, "got:[1;1R");
}

#[test]
fn cwd_and_env_are_applied() {
    let dir = std::env::temp_dir().canonicalize().unwrap();
    let opts = TerminalOptions {
        cwd: Some(dir.clone()),
        env: vec![("ARCODE_TEST_VAR".into(), "xyzzy".into())],
        ..sh("pwd; echo \"var=$ARCODE_TEST_VAR term=$TERM color=$COLORTERM\"; sleep 0.3")
    };
    let (term, rx) = spawn(opts);
    wait_for_text(&term, &rx, "var=xyzzy term=xterm-256color color=truecolor");
    assert!(term.snapshot_text().contains(dir.to_str().unwrap()));
}

#[test]
fn bad_cwd_is_an_error() {
    let opts = TerminalOptions {
        cwd: Some("/definitely/not/here".into()),
        ..sh("true")
    };
    assert!(Terminal::spawn(opts).is_err());
}

#[test]
fn missing_program_is_an_error() {
    let opts = TerminalOptions::command("/definitely/not/a/shell", Vec::<String>::new());
    assert!(Terminal::spawn(opts).is_err());
}

#[test]
fn scrollback_and_scrolling() {
    let script = "i=0; while [ $i -lt 60 ]; do echo line$i; i=$((i+1)); done; sleep 5";
    let (term, rx) = spawn(TerminalOptions {
        rows: 10,
        ..sh(script)
    });
    wait_for_text(&term, &rx, "line59");
    term.scroll_lines(5);
    let snap = term.snapshot(&Palette::default());
    assert_eq!(snap.display_offset, 5);
    assert!(snap.history_size >= 50);
    assert!(snap.cursor.is_none());
    term.scroll_page_up();
    assert!(term.snapshot(&Palette::default()).display_offset > 5);
    term.scroll_page_down();
    term.scroll_to_bottom();
    let snap = term.snapshot(&Palette::default());
    assert_eq!(snap.display_offset, 0);
    assert!(snap.cursor.is_some());
}

#[test]
fn selection_copy() {
    let (term, rx) = spawn(sh("printf 'hello arcode'; sleep 0.5"));
    wait_for_text(&term, &rx, "hello arcode");
    assert_eq!(term.selection_text(), None);
    term.selection_start(GridPoint::new(0, 6));
    term.selection_update(GridPoint::new(0, 11));
    assert_eq!(term.selection_text().as_deref(), Some("arcode"));
    let snap = term.snapshot(&Palette::default());
    assert!(snap.cell(0, 6).unwrap().selected && !snap.cell(0, 5).unwrap().selected);
    term.clear_selection();
    assert_eq!(term.selection_text(), None);
    // Updating without a started selection does nothing.
    term.selection_update(GridPoint::new(0, 3));
    assert_eq!(term.selection_text(), None);
}

#[test]
fn bracketed_paste_mode_is_reported() {
    let (term, rx) = spawn(sh("printf '\\033[?2004hready'; sleep 1"));
    wait_for_text(&term, &rx, "ready");
    assert!(term.mode().bracketed_paste);
    assert!(term.snapshot(&Palette::default()).mode.bracketed_paste);
    term.paste("text\n");
}

#[test]
fn wakeups_do_not_flood() {
    let script =
        "i=0; while [ $i -lt 2000 ]; do echo line$i; i=$((i+1)); done; printf END; sleep 2";
    let (term, rx) = spawn(sh(script));
    let deadline = Instant::now() + TIMEOUT;
    while !term.snapshot_text().contains("END") {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Nothing was consumed while the output streamed in.
    let wakeups = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|e| *e == TerminalEvent::Wakeup)
        .count();
    assert_eq!(wakeups, 1);
}

#[test]
fn drop_terminates_child() {
    let (term, _rx) = spawn(sh("sleep 30"));
    let pid = term.child_pid().unwrap().to_string();
    let alive = |pid: &str| {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    assert!(alive(&pid));
    drop(term);
    let deadline = Instant::now() + TIMEOUT;
    while alive(&pid) {
        assert!(Instant::now() < deadline, "child {pid} survived drop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn shutdown_stops_terminal() {
    let (term, rx) = spawn(interactive());
    term.shutdown();
    assert!(!term.is_alive());
    term.shutdown();
    term.write("echo after\r");
    drop(rx);
}
