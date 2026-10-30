//! Ordered replies to terminal queries (DSR, size and color requests).
//!
//! Color queries must report colors the program set at runtime, which live
//! in the [`Term`]. The PTY thread asks while it holds the terminal lock, so
//! the answer is produced on a small responder thread instead. All replies
//! go through that thread so they reach the program in the order asked
//! (programs often send a color query followed by a DA/DSR sentinel).

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Weak};

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Term, color};
use alacritty_terminal::vte::ansi;

use crate::color::Rgb;
use crate::events::{EventProxy, Shared, lock};

/// Formats a color reply for the color it is given.
pub(crate) type ColorFormatter = Arc<dyn Fn(ansi::Rgb) -> String + Send + Sync + 'static>;

/// One reply to write to the program, in order.
pub(crate) enum Reply {
    /// Bytes ready to write.
    Bytes(Vec<u8>),
    /// A color query for table index `index` (0..=255 or a named slot).
    Color(usize, ColorFormatter),
}

/// Sending half used by the [`EventProxy`].
pub(crate) type ReplySender = Sender<Reply>;

/// Answers replies in order until every sender is gone.
///
/// `lookup` returns the runtime color for a table index; `write` sends
/// bytes to the program.
pub(crate) fn respond(
    rx: Receiver<Reply>,
    mut lookup: impl FnMut(usize) -> Rgb,
    mut write: impl FnMut(Vec<u8>),
) {
    for reply in rx {
        match reply {
            Reply::Bytes(bytes) => write(bytes),
            Reply::Color(index, format) => write(format(lookup(index).into()).into_bytes()),
        }
    }
}

/// Starts the responder thread for one terminal. It holds the terminal
/// only weakly and exits once the terminal (and its senders) are gone.
pub(crate) fn spawn_responder(
    rx: Receiver<Reply>,
    term: Weak<FairMutex<Term<EventProxy>>>,
    shared: Arc<Shared>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("arcode-terminal-replies".into())
        .spawn(move || {
            let lookup = |index: usize| {
                let runtime = term
                    .upgrade()
                    .filter(|_| index < color::COUNT)
                    .and_then(|term| term.lock().colors()[index].map(Rgb::from));
                runtime.unwrap_or_else(|| lock(&shared.palette).table(index))
            };
            respond(rx, lookup, |bytes| shared.write_pty(bytes));
        })
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn formatter() -> ColorFormatter {
        Arc::new(|c| format!("{:02x}{:02x}{:02x}", c.r, c.g, c.b))
    }

    #[test]
    fn replies_keep_their_order() {
        let (tx, rx) = mpsc::channel();
        tx.send(Reply::Bytes(b"a".to_vec())).unwrap();
        tx.send(Reply::Color(257, formatter())).unwrap();
        tx.send(Reply::Bytes(b"b".to_vec())).unwrap();
        drop(tx);
        let mut out = Vec::new();
        let mut asked = Vec::new();
        respond(
            rx,
            |index| {
                asked.push(index);
                Rgb::new(0x12, 0x34, 0x56)
            },
            |bytes| out.push(String::from_utf8(bytes).unwrap()),
        );
        assert_eq!(asked, vec![257]);
        assert_eq!(out, vec!["a", "123456", "b"]);
    }

    #[test]
    fn stops_when_senders_are_gone() {
        let (tx, rx) = mpsc::channel::<Reply>();
        drop(tx);
        respond(rx, |_| Rgb::new(0, 0, 0), |_| panic!("nothing to write"));
    }
}
