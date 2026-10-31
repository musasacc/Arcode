//! UI-agnostic integrated terminal for Arcode.
//!
//! [`Terminal::spawn`] starts a program on a PTY (Unix pty or Windows
//! ConPTY via `alacritty_terminal`) and runs the I/O loop on its own
//! thread. The UI listens for [`TerminalEvent::Wakeup`] and then takes a
//! [`Snapshot`] with fully resolved colors to draw. Keyboard input is
//! encoded with [`keys::encode_key`].

mod color;
mod events;
#[cfg(unix)]
mod hangup;
mod input;
pub mod keys;
mod options;
mod reaper;
mod replies;
#[cfg(all(test, unix))]
mod review_reaper;
mod selection;
mod size;
mod snapshot;
mod terminal;

pub use color::{DIM_FACTOR, Palette, Rgb};
pub use events::TerminalEvent;
pub use input::{PASTE_END, PASTE_START, encode_command, encode_paste};
pub use keys::{KeyMods, encode_key};
pub use options::{TerminalOptions, default_shell, find_in_path};
pub use reaper::KILL_GRACE;
pub use selection::{GridPoint, SelectionKind};
pub use size::{MIN_COLS, MIN_ROWS};
pub use snapshot::{Cell, CursorShape, CursorState, Snapshot, TermModeFlags};
pub use terminal::Terminal;
