//! A misbehaving program on the Discord socket (flooding frames, or
//! stopping in the middle of one) never holds the presence thread past
//! its timeout.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arcode_presence::client::Client;
use arcode_presence::frame::{Opcode, encode};
use arcode_presence::{Options, Presence, Settings, Timing, socket};

/// Accepts connections on `discord-ipc-0` in `dir` and runs `serve` on
/// each until stopped.
struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: std::path::PathBuf,
}

impl Server {
    fn start(dir: &Path, serve: fn(UnixStream, &AtomicBool)) -> Self {
        let path = dir.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = stream else { return };
                let flag = flag.clone();
                std::thread::spawn(move || serve(stream, &flag));
            }
        });
        Self {
            stop,
            thread: Some(thread),
            path,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Pings as fast as it can, never READY; drains what the client writes.
fn flood(mut stream: UnixStream, stop: &AtomicBool) {
    let mut reader = stream.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while matches!(reader.read(&mut buf), Ok(n) if n > 0) {}
    });
    let ping = encode(Opcode::Ping, br#"{"n":1}"#);
    while !stop.load(Ordering::SeqCst) && stream.write_all(&ping).is_ok() {}
}

/// Sends three bytes of a frame header and goes quiet.
fn half_frame(mut stream: UnixStream, stop: &AtomicBool) {
    let _ = stream.write_all(&[1, 0, 0]);
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn handshake_takes(serve: fn(UnixStream, &AtomicBool)) -> Duration {
    let dir = tempfile::tempdir().unwrap();
    let _server = Server::start(dir.path(), serve);
    let stream = socket::connect(&[dir.path().to_path_buf()]).unwrap();
    let started = Instant::now();
    let result = Client::handshake(stream, "1", Duration::from_millis(200));
    assert!(result.is_err(), "no READY was sent");
    started.elapsed()
}

#[test]
fn a_flood_without_ready_ends_the_handshake_at_its_timeout() {
    let took = handshake_takes(flood);
    assert!(took < Duration::from_millis(1000), "{took:?}");
}

#[test]
fn half_a_frame_ends_the_handshake_at_its_timeout() {
    let took = handshake_takes(half_frame);
    assert!(took < Duration::from_millis(1000), "{took:?}");
}

#[test]
fn a_flooding_discord_does_not_hold_the_presence_thread() {
    let dir = tempfile::tempdir().unwrap();
    let _server = Server::start(dir.path(), flood);
    let mut p = Presence::new(Options {
        socket_dirs: Some(vec![dir.path().to_path_buf()]),
        timing: Timing {
            answer_timeout: Duration::from_millis(200),
            ..Timing::default()
        },
    });
    p.configure(Settings {
        enabled: true,
        application_id: "1".into(),
        ..Settings::default()
    });
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    let done = p.shutdown().expect("the thread ran");
    done.recv_timeout(Duration::from_secs(2))
        .expect("the thread got to its messages");
    assert!(started.elapsed() < Duration::from_secs(1));
}
