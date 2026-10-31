//! [`TerminalView`]: the GPUI view owning a [`Terminal`].

use std::path::PathBuf;

use anyhow::Result;
use arcode_terminal::{Palette, Terminal, TerminalEvent, TerminalOptions};
use arcode_ui_kit::{settings, theme};
use async_channel::Receiver;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, CursorStyle, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    Point, Render, SharedString, Styled, Subscription, Task, Window, div,
};

use crate::actions;
use crate::colors::{palette_from_theme, rgb_to_hsla};
use crate::element::TerminalElement;
use crate::input::{KeyInput, classify_key};

/// Events emitted by a [`TerminalView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalViewEvent {
    /// The program changed (or reset) the title; read [`TerminalView::title`].
    TitleChanged,
    /// The shell exited; no more output will come.
    Exited,
    /// The terminal bell rang.
    Bell,
}

/// Geometry and modes of the last painted frame, for mouse and IME.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameInfo {
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub cols: usize,
    pub rows: usize,
    pub cursor: Option<(usize, usize)>,
    pub display_offset: usize,
}

/// A selection drag in progress.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drag {
    /// Where the button went down (grid point).
    pub anchor: arcode_terminal::GridPoint,
    /// The selection was started (only after the first move for single clicks).
    pub started: bool,
    /// Last point the selection was extended to.
    pub last: arcode_terminal::GridPoint,
}

/// An integrated terminal: renders the shell's screen and forwards input.
pub struct TerminalView {
    pub(crate) terminal: Terminal,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) palette: Palette,
    /// Grid and cell size last sent to the PTY: `(cols, rows, cell_w, cell_h)`.
    pub(crate) applied_size: Option<(u16, u16, u16, u16)>,
    pub(crate) frame: Option<FrameInfo>,
    /// IME composition text, drawn at the cursor.
    pub(crate) marked_text: Option<String>,
    pub(crate) drag: Option<Drag>,
    pub(crate) scroll_remainder: f32,
    title: Option<SharedString>,
    exited: bool,
    /// The program's exit code, once it exited (`None` if killed).
    exit_code: Option<i32>,
    _events: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Spawn options from the user's `[terminal]` settings.
fn spawn_options(cwd: Option<PathBuf>, cx: &App) -> TerminalOptions {
    let s = &settings(cx).terminal;
    TerminalOptions {
        shell: s.shell.clone(),
        args: s.args.clone(),
        cwd,
        scrollback: s.scrollback,
        ..TerminalOptions::default()
    }
}

impl TerminalView {
    /// Spawns the configured shell (`settings.terminal`) in `cwd` and
    /// builds the view. Requires the ui-kit settings and theme globals.
    ///
    /// Use inside `cx.new` only when failure is handled before; usually
    /// [`TerminalView::spawn`] is more convenient.
    pub fn new(cwd: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Result<Self> {
        let (terminal, events) = Terminal::spawn(spawn_options(cwd, cx))?;
        Ok(Self::with_terminal(terminal, events, window, cx))
    }

    /// Spawns the configured shell and returns the view entity, or the
    /// spawn error (e.g. missing shell or bad working directory).
    pub fn spawn(cwd: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Result<Entity<Self>> {
        let (terminal, events) = Terminal::spawn(spawn_options(cwd, cx))?;
        Ok(cx.new(|cx| Self::with_terminal(terminal, events, window, cx)))
    }

    /// Builds the view around an already spawned terminal and its event
    /// receiver (as returned by [`Terminal::spawn`]).
    pub fn with_terminal(
        terminal: Terminal,
        events: Receiver<TerminalEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |view, cx| view.on_terminal_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let subscriptions = vec![
            cx.on_focus(&focus_handle, window, |_, _, cx| cx.notify()),
            cx.on_blur(&focus_handle, window, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
        ];
        Self {
            terminal,
            focus_handle,
            palette: palette_from_theme(&theme(cx).source.terminal),
            applied_size: None,
            frame: None,
            marked_text: None,
            drag: None,
            scroll_remainder: 0.,
            title: None,
            exited: false,
            exit_code: None,
            _events: task,
            _subscriptions: subscriptions,
        }
    }

    /// Types `command` followed by Enter.
    pub fn run_command(&self, command: &str) {
        self.terminal.scroll_to_bottom();
        self.terminal.run_command(command);
    }

    /// The title set by the program, or `"Terminal"`.
    pub fn title(&self) -> SharedString {
        self.title
            .clone()
            .unwrap_or_else(|| SharedString::new_static("Terminal"))
    }

    /// Whether the shell is still running.
    pub fn is_alive(&self) -> bool {
        !self.exited && self.terminal.is_alive()
    }

    /// The exit code of the program, once it exited with one (`None`
    /// while it runs, or when it was killed by a signal).
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// The underlying terminal (for size, text snapshots, selection, ...).
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    fn on_terminal_event(&mut self, event: TerminalEvent, cx: &mut Context<Self>) {
        match event {
            // Coalesced: GPUI draws the latest snapshot on the next frame.
            TerminalEvent::Wakeup => cx.notify(),
            TerminalEvent::Title(title) => {
                self.title = (!title.trim().is_empty()).then(|| title.into());
                cx.emit(TerminalViewEvent::TitleChanged);
            }
            TerminalEvent::ResetTitle => {
                self.title = None;
                cx.emit(TerminalViewEvent::TitleChanged);
            }
            TerminalEvent::Bell => cx.emit(TerminalViewEvent::Bell),
            TerminalEvent::Exit | TerminalEvent::ChildExit(_) => {
                // The child's code comes first, then `Exit`.
                if let TerminalEvent::ChildExit(code) = event {
                    self.exit_code = self.exit_code.or(code);
                }
                if !self.exited {
                    self.exited = true;
                    cx.emit(TerminalViewEvent::Exited);
                    cx.notify();
                }
            }
            TerminalEvent::ClipboardStore(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }

    /// Sends user input to the program, jumping back to the live output.
    pub(crate) fn write_input(&mut self, bytes: impl AsRef<[u8]>, cx: &mut Context<Self>) {
        if self.frame.is_some_and(|f| f.display_offset > 0) {
            self.terminal.scroll_to_bottom();
            cx.notify();
        }
        self.terminal.write(bytes);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let app_cursor = self.terminal.mode().app_cursor;
        let option_composes = cfg!(target_os = "macos");
        match classify_key(
            &ks.key,
            ks.key_char.as_deref(),
            ks.modifiers,
            app_cursor,
            option_composes,
        ) {
            KeyInput::Bytes(bytes) => {
                self.write_input(bytes, cx);
                cx.stop_propagation();
            }
            // Text arrives through the input handler (see `ime.rs`).
            KeyInput::Text | KeyInput::Ignore => {}
        }
    }

    fn copy(&mut self, _: &actions::Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.terminal.selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &actions::Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.terminal.scroll_to_bottom();
            self.terminal.paste(&text);
            cx.notify();
        }
    }

    fn clear(&mut self, _: &actions::Clear, _: &mut Window, cx: &mut Context<Self>) {
        // arcode-terminal has no API to drop the scrollback; Ctrl+L makes
        // the shell clear the screen and redraw its prompt.
        self.terminal.clear_selection();
        self.write_input([0x0c], cx);
    }

    fn scroll_page_up(
        &mut self,
        _: &actions::ScrollPageUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal.scroll_page_up();
        cx.notify();
    }

    fn scroll_page_down(
        &mut self,
        _: &actions::ScrollPageDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal.scroll_page_down();
        cx.notify();
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme(cx).source.terminal;
        self.palette = palette_from_theme(colors);
        let focused = self.focus_handle.is_focused(window) && window.is_window_active();
        div()
            .id("terminal-view")
            .key_context("Terminal")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_hidden()
            .p_1()
            .bg(rgb_to_hsla(self.palette.background))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::scroll_page_up))
            .on_action(cx.listener(Self::scroll_page_down))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(TerminalElement::new(cx.entity(), focused))
    }
}
