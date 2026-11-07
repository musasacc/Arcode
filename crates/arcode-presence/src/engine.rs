//! The presence state machine run by the background thread: connects when
//! the presence is on, sends what should be shown at most once per
//! [`Timing::min_interval`] (always ending with the latest state), retries
//! quietly while Discord is not running, and clears the activity when
//! turned off. Time is passed in, so the rules are tested without waiting.

use std::io;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::activity::{Settings, Shown, activity};
use crate::client::{Closed, INVALID_CLIENT_ID};

/// Where the presence stands, for the Settings page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Off, or no application ID to use.
    #[default]
    Off,
    /// Not connected (Discord is not running); trying again.
    Waiting,
    /// Discord did not accept the application ID; trying again.
    Rejected,
    /// Discord shows the activity.
    Connected,
}

/// The intervals of the presence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// Shortest time between two activity updates. Arcode's own choice:
    /// Discord allows about five per 20 seconds.
    pub min_interval: Duration,
    /// Wait before connecting again after Discord was not there.
    pub retry: Duration,
    /// How often an idle connection is checked (pings answered, a closed
    /// Discord noticed).
    pub check: Duration,
    /// No input for this long shows "Idle".
    pub idle_after: Duration,
    /// How long the handshake waits for Discord's answer.
    pub answer_timeout: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_secs(15),
            retry: Duration::from_secs(30),
            check: Duration::from_secs(30),
            idle_after: Duration::from_secs(5 * 60),
            answer_timeout: Duration::from_secs(5),
        }
    }
}

/// One connection to Discord, after the handshake.
pub trait Link: Send {
    /// Sets (or with `None` clears) the activity.
    fn set_activity(&mut self, activity: Option<&Value>) -> io::Result<()>;
    /// Handles what arrived; an error means the connection is gone.
    fn check(&mut self) -> io::Result<()>;
    /// Ends the connection.
    fn close(self: Box<Self>);
}

/// Opens connections.
pub trait Connector: Send {
    /// Connects and does the handshake for `application_id`.
    fn connect(&mut self, application_id: &str) -> io::Result<Box<dyn Link>>;
}

/// A live connection and what it shows.
struct Live {
    link: Box<dyn Link>,
    application_id: String,
    since: Instant,
    /// The activity this connection shows; `None` before the first.
    shows: Option<Value>,
    next_check: Instant,
}

/// The presence state.
pub struct Engine {
    connector: Box<dyn Connector>,
    timing: Timing,
    settings: Settings,
    shown: Shown,
    idle: bool,
    live: Option<Live>,
    /// When the last activity update was sent, on any connection.
    last_sent: Option<Instant>,
    /// When to try connecting again; `None`: at once.
    next_connect: Option<Instant>,
    status: Status,
}

impl Engine {
    /// A disconnected engine.
    pub fn new(connector: Box<dyn Connector>, timing: Timing) -> Self {
        Self {
            connector,
            timing,
            settings: Settings::default(),
            shown: Shown::default(),
            idle: false,
            live: None,
            last_sent: None,
            next_connect: None,
            status: Status::Off,
        }
    }

    /// New settings, applied on the next [`Engine::tick`]. Turning the
    /// presence on or changing the application ID connects at once, even
    /// while an earlier attempt waits for its retry.
    pub fn configure(&mut self, settings: Settings) {
        if settings.active_application_id() != self.settings.active_application_id() {
            self.next_connect = None;
        }
        self.settings = settings;
    }

    /// What to show, sent on the next [`Engine::tick`] that may.
    pub fn show(&mut self, shown: Shown) {
        self.shown = shown;
    }

    /// Whether the last tick found no input for [`Timing::idle_after`].
    pub fn idle(&self) -> bool {
        self.idle
    }

    /// Whether a connection is open.
    pub fn connected(&self) -> bool {
        self.live.is_some()
    }

    /// Where the presence stands after the last tick.
    pub fn status(&self) -> Status {
        self.status
    }

    /// [`Engine::tick_with`] at a fixed time.
    pub fn tick(&mut self, now: Instant, last_input: Instant) -> Option<Instant> {
        self.tick_with(&|| now, last_input)
    }

    /// Does what is due now (`clock` tells the time; the last input was
    /// at `last_input`) and returns when to tick again; `None`: only when
    /// something changes.
    pub fn tick_with(
        &mut self,
        clock: &dyn Fn() -> Instant,
        last_input: Instant,
    ) -> Option<Instant> {
        let wake = self.step(clock, last_input);
        self.status = match (&self.live, self.settings.active_application_id()) {
            (_, None) => Status::Off,
            (Some(_), _) => Status::Connected,
            (None, _) if self.status == Status::Rejected => Status::Rejected,
            (None, _) => Status::Waiting,
        };
        wake
    }

    fn step(&mut self, clock: &dyn Fn() -> Instant, last_input: Instant) -> Option<Instant> {
        let mut now = clock();
        self.idle = now.saturating_duration_since(last_input) >= self.timing.idle_after;
        let wanted = self.settings.active_application_id().map(str::to_owned);
        // Off, or another application: clear the activity and hang up.
        if self
            .live
            .as_ref()
            .is_some_and(|live| wanted.as_deref() != Some(live.application_id.as_str()))
        {
            self.hang_up(now);
            self.next_connect = None;
        }
        let application_id = wanted?;
        if let Some(live) = &mut self.live
            && now >= live.next_check
        {
            match live.link.check() {
                Ok(()) => live.next_check = now + self.timing.check,
                Err(err) => self.lost(now, &err),
            }
        }
        if self.live.is_none() {
            if let Some(at) = self.next_connect
                && now < at
            {
                return Some(at);
            }
            let attempt = self.connector.connect(&application_id);
            // The handshake took time; the rate limit counts from now.
            now = clock();
            match attempt {
                Ok(link) => {
                    log::debug!("discord: connected");
                    self.live = Some(Live {
                        link,
                        application_id,
                        since: now,
                        shows: None,
                        next_check: now + self.timing.check,
                    });
                    self.next_connect = None;
                }
                Err(err) => {
                    let rejected = Closed::of(&err).and_then(|c| c.code) == Some(INVALID_CLIENT_ID);
                    self.status = if rejected {
                        Status::Rejected
                    } else {
                        Status::Waiting
                    };
                    log::debug!(
                        "discord: not connected ({err}); trying again in {} s",
                        self.timing.retry.as_secs()
                    );
                    let at = now + self.timing.retry;
                    self.next_connect = Some(at);
                    return Some(at);
                }
            }
        }
        let activity = activity(&self.shown, &self.settings, self.idle);
        let live = self.live.as_mut()?;
        let mut wake = live.next_check;
        if live.shows.as_ref() != Some(&activity) {
            let ready = self.last_sent.map_or(now, |t| t + self.timing.min_interval);
            if now >= ready {
                if let Err(err) = live.link.set_activity(Some(&activity)) {
                    self.lost(now, &err);
                    return self.next_connect;
                }
                live.shows = Some(activity);
                self.last_sent = Some(now);
            } else {
                // The latest state goes out when the window ends.
                wake = wake.min(ready);
            }
        }
        if !self.idle {
            wake = wake.min(last_input + self.timing.idle_after);
        }
        Some(wake)
    }

    /// Clears the activity and hangs up (Arcode quits).
    pub fn shutdown(&mut self, now: Instant) {
        self.hang_up(now);
    }

    /// Clears the activity (a null activity) and closes the connection.
    fn hang_up(&mut self, now: Instant) {
        if let Some(mut live) = self.live.take() {
            if let Err(err) = live.link.set_activity(None) {
                log::debug!("discord: clearing the activity failed: {err}");
            }
            self.last_sent = Some(now);
            live.link.close();
            log::debug!("discord: disconnected");
        }
    }

    /// The connection broke. A connection that lasted gets one attempt
    /// at once (Discord restarted); a short-lived one waits for
    /// [`Timing::retry`], so a failing Discord is never hammered.
    fn lost(&mut self, now: Instant, err: &io::Error) {
        let Some(live) = self.live.take() else {
            return;
        };
        log::debug!("discord: connection lost ({err})");
        let lasted = now.saturating_duration_since(live.since) >= self.timing.retry;
        self.next_connect = Some(if lasted { now } else { now + self.timing.retry });
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
