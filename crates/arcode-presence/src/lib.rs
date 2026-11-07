//! Discord Rich Presence for Arcode ("Playing Arcode — Editing main.rs").
//!
//! Arcode never contacts the network for this: it talks only to the
//! Discord app running on this machine, over its local IPC socket (a Unix
//! socket, or a named pipe on Windows), and Discord shows the activity to
//! your friends. Off by default.
//!
//! [`Presence`] is the handle the app keeps. All socket work happens on one
//! background thread, started the first time the presence is turned on
//! with an application ID; the handle only sends it small messages when
//! the settings or what is shown change, and [`Presence::touch`] (called
//! on input) is an atomic store. The thread sleeps until something is due:
//! a rate-limited update, a connection check or retry, or the start of
//! "Idle".
//!
//! - [`frame`]: the wire format.
//! - [`activity`]: what is shown and the privacy rules.
//! - [`engine`]: connection, rate limit, retries and idle, with time
//!   passed in.
//! - [`socket`]: where Discord listens on each platform.

pub mod activity;
pub mod client;
pub mod engine;
pub mod frame;
pub mod socket;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub use activity::{Language, Settings, Shown, View};
pub use engine::{Status, Timing};

use engine::{Connector, Engine, Link};

/// The Discord application Arcode's presence belongs to when
/// `discord.application_id` is empty. Its name is what Discord shows after
/// "Playing". Empty until the project has registered an "Arcode"
/// application; while both this and the setting are empty, turning the
/// presence on does nothing.
pub const DEFAULT_APPLICATION_ID: &str = "";

/// Where to look for Discord and how often to do things.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Folders holding the Discord socket (Linux and macOS). `None`: the
    /// runtime and temporary folders from the environment
    /// ([`socket::socket_dirs`]).
    pub socket_dirs: Option<Vec<PathBuf>>,
    /// Intervals.
    pub timing: Timing,
}

/// What the handle tells the thread.
enum Msg {
    Configure(Settings),
    Show(Shown),
    /// Input after "Idle" was shown.
    Wake,
    /// Clear the activity, hang up and stop.
    Shutdown,
}

/// Input time shared with the thread.
struct InputClock {
    base: Instant,
    /// Milliseconds after `base` of the last input.
    last: AtomicU64,
    /// The thread shows "Idle"; the next input wakes it.
    idle: AtomicBool,
}

impl InputClock {
    fn last_input(&self) -> Instant {
        self.base + Duration::from_millis(self.last.load(Ordering::Relaxed))
    }
}

struct Worker {
    tx: Sender<Msg>,
    clock: Arc<InputClock>,
    done: Option<Receiver<()>>,
}

/// Told on the presence thread when the [`Status`] changes.
type StatusListener = Box<dyn FnMut(Status) + Send>;

/// The app's handle on the presence.
pub struct Presence {
    options: Options,
    settings: Settings,
    shown: Shown,
    worker: Option<Worker>,
    on_status: Option<StatusListener>,
}

impl Presence {
    /// A presence that is off; no thread runs until it is turned on.
    pub fn new(options: Options) -> Self {
        Self {
            options,
            settings: Settings::default(),
            shown: Shown::default(),
            worker: None,
            on_status: None,
        }
    }

    /// Calls `listener` on the presence thread each time the [`Status`]
    /// changes (rarely: connecting, Discord quitting, a refused ID). Set
    /// it before the presence is first turned on.
    pub fn on_status(&mut self, listener: impl FnMut(Status) + Send + 'static) {
        self.on_status = Some(Box::new(listener));
    }

    /// Applies new settings: turning the presence on connects, turning it
    /// off clears the activity and hangs up.
    pub fn configure(&mut self, settings: Settings) {
        if settings == self.settings {
            return;
        }
        self.settings = settings.clone();
        if let Some(worker) = &self.worker {
            let _ = worker.tx.send(Msg::Configure(settings));
        } else if settings.active_application_id().is_some() {
            self.start();
        }
    }

    /// What to show now. Sends nothing when it did not change.
    pub fn show(&mut self, shown: Shown) {
        if shown == self.shown {
            return;
        }
        self.shown = shown.clone();
        if let Some(worker) = &self.worker {
            let _ = worker.tx.send(Msg::Show(shown));
        }
    }

    /// Records input (a key press, a click). Cheap enough for every event.
    pub fn touch(&self) {
        let Some(worker) = &self.worker else {
            return;
        };
        let clock = &worker.clock;
        let ms = clock.base.elapsed().as_millis();
        clock
            .last
            .store(u64::try_from(ms).unwrap_or(u64::MAX), Ordering::Relaxed);
        if clock.idle.swap(false, Ordering::Relaxed) {
            let _ = worker.tx.send(Msg::Wake);
        }
    }

    /// What is shown now (the last value given to [`Presence::show`]).
    pub fn shown(&self) -> &Shown {
        &self.shown
    }

    /// Whether the background thread was started.
    pub fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    /// Clears the activity and stops the thread. Returns a receiver that
    /// gets a message once that is done (wait on it with a timeout).
    pub fn shutdown(&mut self) -> Option<Receiver<()>> {
        let mut worker = self.worker.take()?;
        let _ = worker.tx.send(Msg::Shutdown);
        worker.done.take()
    }

    fn start(&mut self) {
        let dirs = self
            .options
            .socket_dirs
            .clone()
            .unwrap_or_else(|| socket::socket_dirs(|name| std::env::var(name).ok()));
        let timing = self.options.timing;
        let connector = SocketConnector {
            dirs,
            answer_timeout: timing.answer_timeout,
        };
        let mut engine = Engine::new(Box::new(connector), timing);
        engine.configure(self.settings.clone());
        engine.show(self.shown.clone());
        let clock = Arc::new(InputClock {
            base: Instant::now(),
            last: AtomicU64::new(0),
            idle: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let thread_clock = clock.clone();
        let on_status = self.on_status.take();
        let spawned = std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || {
                run(engine, &rx, &thread_clock, on_status);
                let _ = done_tx.send(());
            });
        match spawned {
            Ok(_) => {
                self.worker = Some(Worker {
                    tx,
                    clock,
                    done: Some(done),
                })
            }
            Err(err) => log::debug!("discord: cannot start the presence thread: {err}"),
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        // The thread clears the activity on its own; nobody waits.
        let _ = self.shutdown();
    }
}

/// The thread: waits for a message or the next due time, then ticks.
fn run(
    mut engine: Engine,
    rx: &Receiver<Msg>,
    clock: &InputClock,
    mut on_status: Option<StatusListener>,
) {
    // Tick at once: the presence was just turned on.
    let mut wake = Some(Instant::now());
    let mut status = Status::Off;
    loop {
        let first = match wake {
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => Some(Msg::Shutdown),
            },
            None => Some(rx.recv().unwrap_or(Msg::Shutdown)),
        };
        // Apply everything waiting at once: only the latest state matters.
        for msg in first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()))
        {
            match msg {
                Msg::Configure(settings) => engine.configure(settings),
                Msg::Show(shown) => engine.show(shown),
                Msg::Wake => {}
                Msg::Shutdown => {
                    engine.shutdown(Instant::now());
                    return;
                }
            }
        }
        wake = engine.tick_with(&Instant::now, clock.last_input());
        clock.idle.store(engine.idle(), Ordering::Relaxed);
        if engine.status() != status {
            status = engine.status();
            if let Some(listener) = &mut on_status {
                listener(status);
            }
        }
    }
}

/// Connects to the Discord app over its local socket.
struct SocketConnector {
    dirs: Vec<PathBuf>,
    answer_timeout: Duration,
}

impl Connector for SocketConnector {
    fn connect(&mut self, application_id: &str) -> std::io::Result<Box<dyn Link>> {
        let stream = socket::connect(&self.dirs)?;
        let client = client::Client::handshake(stream, application_id, self.answer_timeout)?;
        Ok(Box::new(client))
    }
}

impl<S: socket::IpcStream> Link for client::Client<S> {
    fn set_activity(&mut self, activity: Option<&serde_json::Value>) -> std::io::Result<()> {
        client::Client::set_activity(self, activity)
    }

    fn check(&mut self) -> std::io::Result<()> {
        client::Client::check(self)
    }

    fn close(self: Box<Self>) {
        client::Client::close(*self)
    }
}
