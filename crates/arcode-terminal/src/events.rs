//! Terminal events and the alacritty [`EventListener`] that forwards them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoopSender, Msg};
use async_channel::Sender;

use crate::color::Palette;

/// Something the UI should react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// New content is available; take a snapshot and redraw. Coalesced:
    /// at most one is queued at a time.
    Wakeup,
    /// The program set the window title.
    Title(String),
    /// The program reset the window title.
    ResetTitle,
    /// The terminal bell rang.
    Bell,
    /// The terminal is finished (child exited); no more output will come.
    Exit,
    /// The child process exited with this code (`None` when killed by a
    /// signal or unknown).
    ChildExit(Option<i32>),
    /// The program asked to copy text to the clipboard (OSC 52).
    ClipboardStore(String),
}

/// Locks a std mutex, recovering the data if a panicking thread poisoned it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Bookkeeping for Wakeup coalescing: every event sent gets a sequence
/// number; since the channel is FIFO, the last `len()` sequence numbers
/// are still queued.
#[derive(Debug, Default)]
struct Coalesce {
    sent: u64,
    last_wakeup: Option<u64>,
}

/// State shared between the [`EventProxy`] (PTY thread) and the
/// [`crate::Terminal`] handle.
pub(crate) struct Shared {
    pub(crate) alive: AtomicBool,
    pub(crate) loop_sender: OnceLock<EventLoopSender>,
    pub(crate) palette: Mutex<Palette>,
    pub(crate) window_size: Mutex<WindowSize>,
    coalesce: Mutex<Coalesce>,
}

impl Shared {
    pub(crate) fn new(window_size: WindowSize) -> Self {
        Self {
            alive: AtomicBool::new(true),
            loop_sender: OnceLock::new(),
            palette: Mutex::new(Palette::default()),
            window_size: Mutex::new(window_size),
            coalesce: Mutex::new(Coalesce::default()),
        }
    }

    /// Sends bytes to the PTY; ignored once the event loop is gone.
    pub(crate) fn write_pty(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(sender) = self.loop_sender.get()
            && let Err(err) = sender.send(Msg::Input(bytes.into()))
        {
            log::debug!("terminal: dropping input, event loop is gone: {err}");
        }
    }
}

/// Forwards alacritty events to an async channel, coalescing wakeups and
/// answering terminal queries (DSR, OSC color and size requests) itself.
#[derive(Clone)]
pub(crate) struct EventProxy {
    tx: Sender<TerminalEvent>,
    shared: Arc<Shared>,
}

impl EventProxy {
    pub(crate) fn new(tx: Sender<TerminalEvent>, shared: Arc<Shared>) -> Self {
        Self { tx, shared }
    }

    /// Queues an event for the UI. Returns whether it was queued.
    fn push(&self, event: TerminalEvent) -> bool {
        let mut state = lock(&self.shared.coalesce);
        let is_wakeup = event == TerminalEvent::Wakeup;
        if is_wakeup
            && let Some(seq) = state.last_wakeup
            && seq + self.tx.len() as u64 >= state.sent
        {
            // A Wakeup is still queued; the UI will see the new content.
            return false;
        }
        if self.tx.try_send(event).is_err() {
            return false;
        }
        if is_wakeup {
            state.last_wakeup = Some(state.sent);
        }
        state.sent += 1;
        true
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                self.push(TerminalEvent::Wakeup);
            }
            Event::Title(title) => {
                self.push(TerminalEvent::Title(title));
            }
            Event::ResetTitle => {
                self.push(TerminalEvent::ResetTitle);
            }
            Event::Bell => {
                self.push(TerminalEvent::Bell);
            }
            Event::Exit => {
                self.shared.alive.store(false, Ordering::SeqCst);
                self.push(TerminalEvent::Exit);
            }
            Event::ChildExit(status) => {
                self.shared.alive.store(false, Ordering::SeqCst);
                self.push(TerminalEvent::ChildExit(status.code()));
            }
            Event::ClipboardStore(_, text) => {
                self.push(TerminalEvent::ClipboardStore(text));
            }
            Event::PtyWrite(text) => self.shared.write_pty(text.into_bytes()),
            Event::TextAreaSizeRequest(format) => {
                let size = *lock(&self.shared.window_size);
                self.shared.write_pty(format(size).into_bytes());
            }
            Event::ColorRequest(index, format) => {
                let rgb = lock(&self.shared.palette).table(index);
                self.shared.write_pty(format(rgb.into()).into_bytes());
            }
            // OSC 52 paste is disabled by the terminal config; cursor shape
            // and blinking changes are picked up by the next snapshot.
            Event::ClipboardLoad(..) | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::ExitStatus;

    fn proxy() -> (
        EventProxy,
        async_channel::Receiver<TerminalEvent>,
        Arc<Shared>,
    ) {
        let (tx, rx) = async_channel::unbounded();
        let size = WindowSize {
            num_lines: 24,
            num_cols: 80,
            cell_width: 8,
            cell_height: 16,
        };
        let shared = Arc::new(Shared::new(size));
        (EventProxy::new(tx, shared.clone()), rx, shared)
    }

    fn drain(rx: &async_channel::Receiver<TerminalEvent>) -> Vec<TerminalEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn wakeups_are_coalesced() {
        let (p, rx, _) = proxy();
        for _ in 0..100 {
            p.send_event(Event::Wakeup);
        }
        assert_eq!(drain(&rx), vec![TerminalEvent::Wakeup]);
        // Once consumed, the next wakeup goes through again.
        p.send_event(Event::Wakeup);
        p.send_event(Event::Wakeup);
        assert_eq!(drain(&rx), vec![TerminalEvent::Wakeup]);
    }

    #[test]
    fn wakeup_after_other_events() {
        let (p, rx, _) = proxy();
        p.send_event(Event::Wakeup);
        p.send_event(Event::Bell);
        p.send_event(Event::Wakeup);
        assert_eq!(rx.try_recv().ok(), Some(TerminalEvent::Wakeup));
        // The queued Bell must not hide the fact that the Wakeup is consumed.
        p.send_event(Event::Wakeup);
        p.send_event(Event::Title("t".into()));
        assert_eq!(
            drain(&rx),
            vec![
                TerminalEvent::Bell,
                TerminalEvent::Wakeup,
                TerminalEvent::Title("t".into())
            ]
        );
    }

    #[test]
    fn forwards_events() {
        let (p, rx, shared) = proxy();
        p.send_event(Event::Title("hi".into()));
        p.send_event(Event::ResetTitle);
        p.send_event(Event::Bell);
        p.send_event(Event::ClipboardStore(
            alacritty_terminal::term::ClipboardType::Clipboard,
            "copied".into(),
        ));
        p.send_event(Event::MouseCursorDirty);
        p.send_event(Event::PtyWrite("ignored without loop".into()));
        assert!(shared.alive.load(Ordering::SeqCst));
        p.send_event(Event::ChildExit(exit_status(3)));
        p.send_event(Event::Exit);
        assert!(!shared.alive.load(Ordering::SeqCst));
        assert_eq!(
            drain(&rx),
            vec![
                TerminalEvent::Title("hi".into()),
                TerminalEvent::ResetTitle,
                TerminalEvent::Bell,
                TerminalEvent::ClipboardStore("copied".into()),
                TerminalEvent::ChildExit(Some(3)),
                TerminalEvent::Exit,
            ]
        );
    }

    #[test]
    fn closed_receiver_is_fine() {
        let (p, rx, _) = proxy();
        drop(rx);
        p.send_event(Event::Wakeup);
        p.send_event(Event::Bell);
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
}
