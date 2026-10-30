//! The [`Terminal`] handle: a PTY child plus its emulated screen.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use alacritty_terminal::event::OnResize;
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, Notifier};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use anyhow::{Context as _, Result, bail};
use async_channel::Receiver;

use crate::color::Palette;
use crate::events::{EventProxy, Shared, TerminalEvent, lock};
use crate::input::{encode_command, encode_paste};
use crate::options::TerminalOptions;
use crate::reaper::spawn_io_thread;
use crate::replies::spawn_responder;
use crate::selection::{self, GridPoint, SelectionKind};
use crate::size::window_size;
use crate::snapshot::{self, Snapshot};

/// A running terminal: the child process on a PTY, the VT state, and the
/// I/O thread connecting them. The child is terminated on drop (see
/// [`Terminal::shutdown`]); dropping never blocks.
pub struct Terminal {
    term: Arc<FairMutex<Term<EventProxy>>>,
    shared: Arc<Shared>,
    sender: EventLoopSender,
    /// Runs the event loop and closes the PTY afterwards; finishes once
    /// the child is gone.
    thread: Option<JoinHandle<()>>,
    child_pid: Option<u32>,
}

impl Terminal {
    /// Starts the program from `opts` on a new PTY and the I/O thread.
    ///
    /// Returns the handle and the event receiver; redraw on
    /// [`TerminalEvent::Wakeup`].
    pub fn spawn(opts: TerminalOptions) -> Result<(Terminal, Receiver<TerminalEvent>)> {
        if let Some(cwd) = &opts.cwd
            && !cwd.is_dir()
        {
            bail!("working directory {} is not a directory", cwd.display());
        }
        let (ws, size) = window_size(opts.cols, opts.rows, opts.cell_width, opts.cell_height);
        let (tx, rx) = async_channel::unbounded();
        let shared = Arc::new(Shared::new(ws));
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let proxy = EventProxy::new(tx, shared.clone(), reply_tx);

        let config = Config {
            scrolling_history: opts.scrollback,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, proxy.clone())));
        spawn_responder(reply_rx, Arc::downgrade(&term), shared.clone())
            .context("failed to start the terminal reply thread")?;

        let program = opts.program();
        let pty_options = pty_options(&opts, program.clone());
        let pty = tty::new(&pty_options, ws, 0)
            .with_context(|| format!("failed to start `{program}` in a terminal"))?;
        let child_pid = child_pid(&pty);

        let event_loop = EventLoop::new(term.clone(), proxy, pty, true, false)
            .context("failed to create the terminal event loop")?;
        let sender = event_loop.channel();
        // Set before the loop runs so terminal replies can be written.
        let _ = shared.loop_sender.set(sender.clone());
        let thread = spawn_io_thread(event_loop, shared.clone(), child_pid)
            .context("failed to start the terminal I/O thread")?;

        let terminal = Terminal {
            term,
            shared,
            sender,
            thread: Some(thread),
            child_pid,
        };
        Ok((terminal, rx))
    }

    /// Writes raw bytes to the child's input.
    pub fn write(&self, bytes: impl AsRef<[u8]>) {
        self.shared.write_pty(bytes.as_ref().to_vec());
    }

    /// Pastes text: newlines become `\r`, and the text is bracketed when
    /// the program enabled bracketed paste.
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        self.write(encode_paste(text, bracketed));
    }

    /// Types `cmd` followed by Enter.
    pub fn run_command(&self, cmd: &str) {
        self.write(encode_command(cmd));
    }

    /// Resizes the grid and the PTY. Sizes are clamped to at least 2x1
    /// cells; cell sizes are in pixels.
    pub fn resize(&self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        let (ws, size) = window_size(cols, rows, cell_w, cell_h);
        *lock(&self.shared.window_size) = ws;
        self.term.lock().resize(size);
        Notifier(self.sender.clone()).on_resize(ws);
    }

    /// Current grid size as `(cols, rows)`.
    pub fn size(&self) -> (u16, u16) {
        let ws = *lock(&self.shared.window_size);
        (ws.num_cols, ws.num_lines)
    }

    /// Scrolls the view; positive `delta` moves up into the history.
    pub fn scroll_lines(&self, delta: i32) {
        self.scroll(Scroll::Delta(delta));
    }

    /// Scrolls one screen up into the history.
    pub fn scroll_page_up(&self) {
        self.scroll(Scroll::PageUp);
    }

    /// Scrolls one screen down towards the live output.
    pub fn scroll_page_down(&self) {
        self.scroll(Scroll::PageDown);
    }

    /// Scrolls back to the live output.
    pub fn scroll_to_bottom(&self) {
        self.scroll(Scroll::Bottom);
    }

    fn scroll(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    /// Whether the child is still running and the I/O thread is alive.
    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
            && self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// The child's process id, if known.
    pub fn child_pid(&self) -> Option<u32> {
        self.child_pid
    }

    /// Stops the I/O thread and terminates the child: it is hung up
    /// (`SIGHUP` on Unix, the pseudo console is closed on Windows) and, on
    /// Unix, killed with `SIGKILL` if it is still running after
    /// [`crate::KILL_GRACE`]. All of that happens on the I/O thread; this
    /// call does not block and is safe to call more than once. No events
    /// follow, and [`Terminal::is_alive`] returns `false` from now on.
    pub fn shutdown(&self) {
        self.shared.alive.store(false, Ordering::SeqCst);
        let _ = self.sender.send(Msg::Shutdown);
    }

    /// Copies the visible screen with colors resolved against `palette`.
    /// Color queries from programs (OSC 4/10/11/12 `?`) report colors the
    /// program set at runtime, else this (most recent) palette.
    pub fn snapshot(&self, palette: &Palette) -> Snapshot {
        *lock(&self.shared.palette) = *palette;
        snapshot::build(&self.term.lock(), palette)
    }

    /// The visible screen as plain text, one line per row, trailing
    /// spaces trimmed.
    pub fn snapshot_text(&self) -> String {
        snapshot::text(&self.term.lock())
    }

    /// The terminal's current input modes (also part of [`Snapshot`]).
    pub fn mode(&self) -> crate::TermModeFlags {
        (*self.term.lock().mode()).into()
    }

    /// Starts a character-wise selection at `point` (snapshot coordinates).
    pub fn selection_start(&self, point: GridPoint) {
        self.selection_start_with(point, SelectionKind::Simple);
    }

    /// Starts a selection of the given kind at `point`.
    pub fn selection_start_with(&self, point: GridPoint, kind: SelectionKind) {
        selection::start(&mut self.term.lock(), point, kind);
    }

    /// Extends the selection to `point` (snapshot coordinates at the time
    /// of the call); both ends are inclusive. The start stays on the same
    /// text even if output scrolled since [`Terminal::selection_start`].
    /// Does nothing without a selection (none started, cleared, or
    /// discarded by the terminal because its text was erased).
    pub fn selection_update(&self, point: GridPoint) {
        selection::update(&mut self.term.lock(), point);
    }

    /// The selected text, if any.
    pub fn selection_text(&self) -> Option<String> {
        selection::text(&self.term.lock())
    }

    /// Removes the selection.
    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.shutdown();
        // Detach the I/O thread: it closes the PTY and terminates the
        // child itself, so the caller (the UI thread) never blocks here.
        drop(self.thread.take());
    }
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("child_pid", &self.child_pid)
            .field("alive", &self.is_alive())
            .finish_non_exhaustive()
    }
}

fn pty_options(opts: &TerminalOptions, program: String) -> tty::Options {
    #[cfg(windows)]
    let program = crate::options::quote_windows_program(&program);
    tty::Options {
        shell: Some(tty::Shell::new(program, opts.args.clone())),
        working_directory: opts.cwd.clone(),
        drain_on_exit: true,
        env: opts.environment(),
        // Quote arguments with the C runtime rules, like Rust's `Command`.
        #[cfg(windows)]
        escape_args: true,
    }
}

#[cfg(unix)]
fn child_pid(pty: &tty::Pty) -> Option<u32> {
    Some(pty.child().id())
}

#[cfg(windows)]
fn child_pid(pty: &tty::Pty) -> Option<u32> {
    pty.child_watcher().pid().map(|pid| pid.get())
}
