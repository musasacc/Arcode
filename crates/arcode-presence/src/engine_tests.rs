use std::sync::{Arc, Mutex};

use super::*;
use crate::activity::{Language, View};

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Connect(String),
    Set(Option<Value>),
    Close,
}

#[derive(Default)]
struct Fake {
    events: Vec<Event>,
    /// Discord is not running.
    absent: bool,
    /// The open connection broke.
    broken: bool,
    /// Discord refuses the application ID (close code 4000).
    refuse: bool,
    /// The test's clock, moved on by each handshake.
    clock: Option<Instant>,
    /// How long a handshake takes.
    handshake: Duration,
}

type Shared = Arc<Mutex<Fake>>;

struct FakeConnector(Shared);
struct FakeLink(Shared);

impl Connector for FakeConnector {
    fn connect(&mut self, application_id: &str) -> io::Result<Box<dyn Link>> {
        let mut fake = self.0.lock().unwrap();
        if let Some(clock) = fake.clock {
            fake.clock = Some(clock + fake.handshake);
        }
        if fake.absent {
            return Err(io::ErrorKind::NotFound.into());
        }
        if fake.refuse {
            fake.events.push(Event::Connect(application_id.into()));
            let closed = crate::client::Closed {
                code: Some(INVALID_CLIENT_ID),
                message: "Invalid Client ID".into(),
            };
            return Err(io::Error::new(io::ErrorKind::ConnectionRefused, closed));
        }
        fake.broken = false;
        fake.events.push(Event::Connect(application_id.into()));
        Ok(Box::new(FakeLink(self.0.clone())))
    }
}

impl Link for FakeLink {
    fn set_activity(&mut self, activity: Option<&Value>) -> io::Result<()> {
        let mut fake = self.0.lock().unwrap();
        if fake.broken {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        fake.events.push(Event::Set(activity.cloned()));
        Ok(())
    }

    fn check(&mut self) -> io::Result<()> {
        if self.0.lock().unwrap().broken {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(())
    }

    fn close(self: Box<Self>) {
        self.0.lock().unwrap().events.push(Event::Close);
    }
}

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

fn on(id: &str) -> Settings {
    Settings {
        enabled: true,
        show_file_name: true,
        show_project_name: false,
        show_elapsed: false,
        application_id: id.into(),
    }
}

fn editing(file: &str) -> Shown {
    Shown {
        view: View::Editing {
            file_name: file.into(),
            language: Language {
                key: "rust".into(),
                name: "Rust".into(),
            },
        },
        project: Some("app".into()),
        started: Some(1),
    }
}

fn engine() -> (Engine, Shared) {
    let shared = Shared::default();
    let engine = Engine::new(Box::new(FakeConnector(shared.clone())), Timing::default());
    (engine, shared)
}

fn take(shared: &Shared) -> Vec<Event> {
    std::mem::take(&mut shared.lock().unwrap().events)
}

fn details(event: &Event) -> Option<String> {
    match event {
        Event::Set(Some(a)) => a["details"].as_str().map(str::to_owned),
        _ => None,
    }
}

#[test]
fn updates_are_coalesced_to_one_per_window_ending_with_the_latest() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on("1"));
    e.show(editing("a.rs"));
    let wake = e.tick(t0, t0);
    let events = take(&shared);
    assert_eq!(events[0], Event::Connect("1".into()));
    assert_eq!(details(&events[1]).as_deref(), Some("Editing a.rs"));
    assert!(wake.is_some());
    // Three changes inside the window: nothing is sent yet.
    for (at, file) in [(1, "b.rs"), (4, "c.rs"), (9, "d.rs")] {
        e.show(editing(file));
        let wake = e.tick(t0 + secs(at), t0 + secs(at));
        assert_eq!(wake, Some(t0 + secs(15)), "wakes when the window ends");
        assert!(take(&shared).is_empty(), "nothing sent at {at} s");
    }
    // The window ends: only the latest state goes out.
    e.tick(t0 + secs(15), t0 + secs(9));
    let events = take(&shared);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(details(&events[0]).as_deref(), Some("Editing d.rs"));
    // The same state again sends nothing.
    e.show(editing("d.rs"));
    e.tick(t0 + secs(40), t0 + secs(40));
    assert!(take(&shared).is_empty());
    // A change long after the last update goes out at once.
    e.show(editing("e.rs"));
    e.tick(t0 + secs(41), t0 + secs(41));
    assert_eq!(details(&take(&shared)[0]).as_deref(), Some("Editing e.rs"));
}

#[test]
fn at_most_one_update_per_fifteen_seconds_under_constant_changes() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on("1"));
    let mut sent_at = Vec::new();
    for ms in (0..60_000).step_by(250) {
        let now = t0 + Duration::from_millis(ms);
        e.show(editing(&format!("f{ms}.rs")));
        e.tick(now, now);
        for event in take(&shared) {
            if matches!(event, Event::Set(_)) {
                sent_at.push(ms);
            }
        }
    }
    assert_eq!(sent_at, [0, 15_000, 30_000, 45_000]);
}

#[test]
fn turning_off_clears_with_a_null_activity_and_hangs_up() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on("1"));
    e.show(editing("a.rs"));
    e.tick(t0, t0);
    take(&shared);
    e.configure(Settings {
        enabled: false,
        ..on("1")
    });
    assert_eq!(e.tick(t0 + secs(1), t0), None, "off: nothing more to do");
    assert_eq!(take(&shared), [Event::Set(None), Event::Close]);
    assert!(!e.connected());
    // Another application ID: hang up, connect with the new one.
    e.configure(on("1"));
    e.tick(t0 + secs(20), t0 + secs(20));
    e.configure(on("2"));
    e.tick(t0 + secs(21), t0 + secs(21));
    let events = take(&shared);
    assert!(events.contains(&Event::Connect("2".into())), "{events:?}");
    assert!(events.contains(&Event::Close), "{events:?}");
}

#[test]
fn no_application_id_never_connects() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on(""));
    e.show(editing("a.rs"));
    if crate::DEFAULT_APPLICATION_ID.is_empty() {
        assert_eq!(e.tick(t0, t0), None);
        assert!(take(&shared).is_empty());
    }
}

#[test]
fn absent_discord_is_retried_every_thirty_seconds() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    shared.lock().unwrap().absent = true;
    e.configure(on("1"));
    assert_eq!(e.tick(t0, t0), Some(t0 + secs(30)));
    assert_eq!(e.tick(t0 + secs(10), t0), Some(t0 + secs(30)), "not before");
    shared.lock().unwrap().absent = false;
    e.tick(t0 + secs(30), t0);
    assert!(e.connected());
    let events = take(&shared);
    assert_eq!(events[0], Event::Connect("1".into()));
    assert_eq!(details(&events[1]).as_deref(), Some("Choosing a project"));
}

#[test]
fn a_lost_connection_reconnects_and_resends() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on("1"));
    e.show(editing("a.rs"));
    e.tick(t0, t0);
    take(&shared);
    // Discord restarts after a while; the next check notices.
    shared.lock().unwrap().broken = true;
    e.tick(t0 + secs(60), t0 + secs(60));
    let events = take(&shared);
    assert_eq!(events[0], Event::Connect("1".into()), "{events:?}");
    assert_eq!(details(&events[1]).as_deref(), Some("Editing a.rs"));
    // A connection that breaks at once waits for the retry interval.
    shared.lock().unwrap().broken = true;
    e.show(editing("b.rs"));
    let wake = e.tick(t0 + secs(80), t0 + secs(80));
    assert_eq!(wake, Some(t0 + secs(110)));
    assert!(!e.connected());
    e.tick(t0 + secs(110), t0 + secs(110));
    let events = take(&shared);
    assert_eq!(details(&events[1]).as_deref(), Some("Editing b.rs"));
}

#[test]
fn idle_after_five_minutes_without_input() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on("1"));
    e.show(editing("a.rs"));
    let wake = e.tick(t0, t0);
    assert_eq!(
        wake,
        Some(t0 + secs(30)),
        "the connection check comes first"
    );
    let wake = e.tick(t0 + secs(290), t0);
    assert_eq!(wake, Some(t0 + secs(300)), "wakes when idle starts");
    e.tick(t0 + secs(300), t0);
    assert!(e.idle());
    let events = take(&shared);
    assert_eq!(details(events.last().unwrap()).as_deref(), Some("Idle"));
    // Input again: back to the file.
    e.tick(t0 + secs(400), t0 + secs(400));
    assert!(!e.idle());
    assert_eq!(details(&take(&shared)[0]).as_deref(), Some("Editing a.rs"));
}

#[test]
fn shutdown_clears_the_activity() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    e.configure(on("1"));
    e.tick(t0, t0);
    take(&shared);
    e.shutdown(t0 + secs(1));
    assert_eq!(take(&shared), [Event::Set(None), Event::Close]);
}

#[test]
fn a_new_application_id_or_turning_on_again_connects_without_waiting_for_the_retry() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    // Discord refuses a mistyped ID; the next attempt would be in 30 s.
    shared.lock().unwrap().refuse = true;
    e.configure(on("999"));
    assert_eq!(e.tick(t0, t0), Some(t0 + secs(30)));
    assert_eq!(e.status(), Status::Rejected);
    assert_eq!(take(&shared), [Event::Connect("999".into())]);
    // The corrected ID is tried at once.
    shared.lock().unwrap().refuse = false;
    e.configure(on("111"));
    e.tick(t0 + secs(3), t0 + secs(3));
    assert!(e.connected());
    assert_eq!(e.status(), Status::Connected);
    assert_eq!(take(&shared)[0], Event::Connect("111".into()));

    // Discord is not running: a retry is pending.
    let (mut e, shared) = engine();
    shared.lock().unwrap().absent = true;
    e.configure(on("1"));
    assert_eq!(e.tick(t0, t0), Some(t0 + secs(30)));
    assert_eq!(e.status(), Status::Waiting);
    // Discord starts; turning presence off and on connects at once.
    shared.lock().unwrap().absent = false;
    e.configure(Settings {
        enabled: false,
        ..on("1")
    });
    assert_eq!(e.tick(t0 + secs(4), t0 + secs(4)), None);
    assert_eq!(e.status(), Status::Off);
    e.configure(on("1"));
    e.tick(t0 + secs(5), t0 + secs(5));
    assert!(e.connected(), "connected at 5 s, not at 30 s");
    assert_eq!(take(&shared)[0], Event::Connect("1".into()));
}

#[test]
fn a_privacy_switch_does_not_skip_the_retry_wait() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    shared.lock().unwrap().absent = true;
    e.configure(on("1"));
    e.tick(t0, t0);
    e.configure(Settings {
        show_project_name: true,
        ..on("1")
    });
    assert_eq!(e.tick(t0 + secs(1), t0), Some(t0 + secs(30)));
}

#[test]
fn the_rate_limit_counts_from_after_the_handshake() {
    let (mut e, shared) = engine();
    let t0 = Instant::now();
    {
        let mut fake = shared.lock().unwrap();
        fake.clock = Some(t0);
        fake.handshake = Duration::from_millis(400);
    }
    let clock = || shared.lock().unwrap().clock.unwrap();
    e.configure(on("1"));
    e.show(editing("a.rs"));
    e.tick_with(&clock, t0);
    assert_eq!(take(&shared).len(), 2, "connected and sent");
    // The first update went out at 0.4 s: the next is due at 15.4 s.
    e.show(editing("b.rs"));
    shared.lock().unwrap().clock = Some(t0 + secs(15));
    let wake = e.tick_with(&clock, t0 + secs(15));
    assert!(take(&shared).is_empty(), "not 15 s after the frame yet");
    assert_eq!(wake, Some(t0 + Duration::from_millis(15_400)));
}
