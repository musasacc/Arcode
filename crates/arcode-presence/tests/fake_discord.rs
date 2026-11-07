//! The presence against a fake Discord app listening on a Unix socket in a
//! temporary folder (passed in explicitly; no environment variables).
#![cfg(unix)]

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arcode_presence::frame::{Opcode, read_frame, write_frame};
use arcode_presence::{Language, Options, Presence, Settings, Shown, Timing, View};
use serde_json::{Value, json};

/// What the fake server saw.
#[derive(Debug)]
enum Seen {
    Connected,
    Frame(Opcode, Value),
    Hangup,
}

/// A fake Discord: answers the handshake with READY, optionally pings,
/// and reports every frame.
struct FakeDiscord {
    path: PathBuf,
    seen: Receiver<Seen>,
    stopping: Arc<AtomicBool>,
    /// The connection being served, to cut it when Discord "quits".
    current: Arc<Mutex<Option<UnixStream>>>,
    thread: Option<JoinHandle<()>>,
}

impl FakeDiscord {
    fn start(dir: &Path, ping: bool) -> Self {
        let path = dir.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bind");
        let (tx, seen) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let current = Arc::new(Mutex::new(None));
        let (stop, cur) = (stopping.clone(), current.clone());
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                *cur.lock().unwrap() = stream.try_clone().ok();
                if !serve(stream, &tx, ping) {
                    return;
                }
            }
        });
        Self {
            path,
            seen,
            stopping,
            current,
            thread: Some(thread),
        }
    }

    /// The next event, waiting up to five seconds.
    fn next(&self) -> Seen {
        self.seen
            .recv_timeout(Duration::from_secs(5))
            .expect("the fake Discord saw nothing")
    }

    /// The next frame of `opcode`, skipping connects and handshakes.
    fn frame(&self, opcode: Opcode) -> Value {
        loop {
            match self.next() {
                Seen::Frame(op, value) if op == opcode => return value,
                Seen::Connected | Seen::Frame(Opcode::Handshake, _) => {}
                other => panic!("expected {opcode:?}, got {other:?}"),
            }
        }
    }

    fn activity(&self) -> Value {
        let frame = self.frame(Opcode::Frame);
        assert_eq!(frame["cmd"], "SET_ACTIVITY");
        frame["args"]["activity"].clone()
    }

    /// Cuts the connection, stops listening and removes the socket
    /// (Discord quit).
    fn stop(mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(stream) = self.current.lock().unwrap().take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        // Wake the accept loop so its thread ends.
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("the fake Discord thread");
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Serves one connection; returns false once the server should stop.
fn serve(mut stream: UnixStream, tx: &Sender<Seen>, ping: bool) -> bool {
    if tx.send(Seen::Connected).is_err() {
        return false;
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    loop {
        let Ok((op, payload)) = read_frame(&mut stream) else {
            let _ = tx.send(Seen::Hangup);
            return true;
        };
        let value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
        if op == Opcode::Handshake {
            let ready = json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1}});
            let _ = write_frame(&mut stream, Opcode::Frame, ready.to_string().as_bytes());
            if ping {
                let _ = write_frame(&mut stream, Opcode::Ping, br#"{"beat":1}"#);
            }
        }
        if tx.send(Seen::Frame(op, value)).is_err() {
            return false;
        }
    }
}

fn fast() -> Timing {
    Timing {
        min_interval: Duration::from_millis(300),
        retry: Duration::from_millis(200),
        check: Duration::from_millis(50),
        idle_after: Duration::from_secs(3600),
        answer_timeout: Duration::from_secs(2),
    }
}

fn presence(dir: &Path) -> Presence {
    Presence::new(Options {
        socket_dirs: Some(vec![dir.to_path_buf()]),
        timing: fast(),
    })
}

fn on(id: &str) -> Settings {
    Settings {
        enabled: true,
        show_file_name: false,
        show_project_name: false,
        show_elapsed: true,
        application_id: id.into(),
    }
}

fn editing(file: &str, key: &str, name: &str) -> Shown {
    Shown {
        view: View::Editing {
            file_name: file.into(),
            language: Language {
                key: key.into(),
                name: name.into(),
            },
        },
        project: Some("hidden-project".into()),
        started: Some(1_700_000_000),
    }
}

/// Waits for the presence thread to finish its shutdown.
fn finish(mut p: Presence) {
    if let Some(done) = p.shutdown() {
        done.recv_timeout(Duration::from_secs(5))
            .expect("the thread stops");
    }
}

#[test]
fn handshake_then_activity_then_null_when_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let discord = FakeDiscord::start(dir.path(), false);
    let mut p = presence(dir.path());
    p.show(editing("main.rs", "rust", "Rust"));
    p.configure(on("123456789012345678"));
    let hello = discord.frame(Opcode::Handshake);
    assert_eq!(hello, json!({"v": 1, "client_id": "123456789012345678"}));
    let activity = discord.activity();
    assert_eq!(activity["details"], "Editing a Rust file");
    assert_eq!(activity["timestamps"]["start"], 1_700_000_000);
    assert_eq!(activity["assets"]["small_image"], "rust");
    let text = activity.to_string();
    assert!(!text.contains("main.rs") && !text.contains("hidden-project"));

    // A privacy switch applies at once (after the rate-limit window).
    p.configure(Settings {
        show_file_name: true,
        show_project_name: true,
        ..on("123456789012345678")
    });
    let activity = discord.activity();
    assert_eq!(activity["details"], "Editing main.rs");
    assert_eq!(activity["state"], "In hidden-project");

    // Turned off: a null activity, then the connection closes.
    p.configure(Settings {
        enabled: false,
        ..on("123456789012345678")
    });
    assert!(discord.activity().is_null());
    assert_eq!(discord.frame(Opcode::Close), json!({}));
    assert!(matches!(discord.next(), Seen::Hangup));
    finish(p);
    discord.stop();
}

#[test]
fn changes_inside_the_window_are_coalesced() {
    let dir = tempfile::tempdir().unwrap();
    let discord = FakeDiscord::start(dir.path(), false);
    let mut p = presence(dir.path());
    p.configure(Settings {
        show_file_name: true,
        ..on("1")
    });
    assert_eq!(discord.activity()["details"], "Choosing a project");
    let started = Instant::now();
    for file in ["a.rs", "b.py", "c.ts"] {
        p.show(editing(file, "rust", "Rust"));
    }
    // Only the latest, once the window has passed.
    assert_eq!(discord.activity()["details"], "Editing c.ts");
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(
        discord
            .seen
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "nothing else was sent"
    );
    finish(p);
    discord.stop();
}

#[test]
fn pings_are_answered_with_the_same_payload() {
    let dir = tempfile::tempdir().unwrap();
    let discord = FakeDiscord::start(dir.path(), true);
    let mut p = presence(dir.path());
    p.configure(on("1"));
    assert_eq!(discord.frame(Opcode::Pong), json!({"beat": 1}));
    finish(p);
    discord.stop();
}

#[test]
fn reconnects_and_resends_after_discord_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let discord = FakeDiscord::start(dir.path(), false);
    let mut p = presence(dir.path());
    p.show(editing("main.rs", "rust", "Rust"));
    p.configure(on("1"));
    assert_eq!(discord.activity()["details"], "Editing a Rust file");
    // Discord quits: the server's side of the connection closes.
    discord.stop();
    std::thread::sleep(Duration::from_millis(400));
    // Discord is back; the presence finds it and shows the activity again.
    let discord = FakeDiscord::start(dir.path(), false);
    let hello = discord.frame(Opcode::Handshake);
    assert_eq!(hello["client_id"], "1");
    assert_eq!(discord.activity()["details"], "Editing a Rust file");
    finish(p);
    discord.stop();
}

#[test]
fn quitting_clears_the_activity() {
    let dir = tempfile::tempdir().unwrap();
    let discord = FakeDiscord::start(dir.path(), false);
    let mut p = presence(dir.path());
    p.configure(on("1"));
    discord.activity();
    finish(p);
    assert!(discord.activity().is_null());
    discord.stop();
}

#[test]
fn no_application_id_makes_no_connection() {
    let dir = tempfile::tempdir().unwrap();
    let discord = FakeDiscord::start(dir.path(), false);
    let mut p = presence(dir.path());
    p.configure(on(""));
    if arcode_presence::DEFAULT_APPLICATION_ID.is_empty() {
        assert!(!p.is_running(), "no thread without an application ID");
        assert!(
            discord
                .seen
                .recv_timeout(Duration::from_millis(400))
                .is_err(),
            "nothing connected"
        );
    }
    // Off: nothing either.
    let mut off = presence(dir.path());
    off.configure(Settings {
        enabled: false,
        ..on("1")
    });
    assert!(!off.is_running());
    assert!(
        discord
            .seen
            .recv_timeout(Duration::from_millis(200))
            .is_err()
    );
    drop((p, off));
    discord.stop();
}

#[test]
fn absent_discord_is_quietly_retried() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = presence(dir.path());
    p.show(editing("main.rs", "rust", "Rust"));
    p.configure(on("1"));
    std::thread::sleep(Duration::from_millis(300));
    let discord = FakeDiscord::start(dir.path(), false);
    assert_eq!(discord.activity()["details"], "Editing a Rust file");
    finish(p);
    discord.stop();
}
