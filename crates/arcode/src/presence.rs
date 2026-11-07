//! Discord Rich Presence in the app (`[discord]`, docs/guide/discord.md).
//!
//! The `arcode-presence` crate does the work on its own thread; this module
//! tells it the settings and what the active window shows, and stamps the
//! time of input for "Idle". Nothing here redraws, polls or blocks: a
//! window reports when its project or active editor changes or when it is
//! activated, and the handle sends only changed values. The thread
//! reports back only when its [`Status`] changes (connecting, Discord
//! quitting, a refused application ID), for the Settings page.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use arcode_presence::{Language, Options, Presence, Settings, Shown, Status, View};
use gpui::{
    App, Context, DispatchPhase, Global, IntoElement, MouseDownEvent, ScrollWheelEvent, Styled,
    Subscription, Window, WindowId, canvas,
};

use crate::workspace::{Workspace, WorkspaceEvent};

/// The presence's status, updated only when it changes (the Settings
/// page observes it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DiscordStatus(pub Status);

impl Global for DiscordStatus {}

/// The app's presence and the window it describes.
struct DiscordPresence {
    presence: Presence,
    /// The window that reported last; another one takes over when it is
    /// activated.
    source: Option<WindowId>,
}

impl Global for DiscordPresence {}

/// Follows the settings and input from here on; the settings at startup
/// apply with [`start`]. The presence thread starts only once it is turned
/// on with an application ID.
pub fn init(cx: &mut App) {
    let mut presence = Presence::new(Options::default());
    let (status_tx, status_rx) = async_channel::unbounded();
    presence.on_status(move |status| {
        let _ = status_tx.try_send(status);
    });
    cx.set_global(DiscordStatus::default());
    cx.spawn(async move |cx| {
        while let Ok(status) = status_rx.recv().await {
            let status = std::iter::from_fn(|| status_rx.try_recv().ok())
                .last()
                .unwrap_or(status);
            if cx
                .update(|cx| cx.set_global(DiscordStatus(status)))
                .is_err()
            {
                return;
            }
        }
    })
    .detach();
    cx.set_global(DiscordPresence {
        presence,
        source: None,
    });
    cx.observe_global::<arcode_ui_kit::Settings>(|cx| {
        let settings = settings_of(&arcode_ui_kit::settings(cx).discord);
        cx.global_mut::<DiscordPresence>()
            .presence
            .configure(settings);
    })
    .detach();
    cx.intercept_keystrokes(|_, _, cx| touch(cx)).detach();
    cx.on_app_quit(|cx| {
        let done = cx
            .has_global::<DiscordPresence>()
            .then(|| cx.global_mut::<DiscordPresence>().presence.shutdown())
            .flatten();
        // GPUI gives quit tasks SHUTDOWN_TIMEOUT (100 ms); if the thread
        // is busy longer, the socket closes with the process and Discord
        // drops the activity anyway.
        cx.background_executor().spawn(async move {
            if let Some(done) = done {
                let _ = done.recv_timeout(gpui::SHUTDOWN_TIMEOUT);
            }
        })
    })
    .detach();
}

/// Applies the settings Arcode started with, once the startup windows
/// have reported what they show (their events come first), so a window
/// opening a project does not begin with "Choosing a project".
pub fn start(cx: &mut App) {
    cx.defer(|cx| {
        if !cx.has_global::<DiscordPresence>() {
            return;
        }
        let settings = settings_of(&arcode_ui_kit::settings(cx).discord);
        cx.global_mut::<DiscordPresence>()
            .presence
            .configure(settings);
    });
}

#[cfg(test)]
thread_local! {
    static TOUCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How often input was recorded on this thread (tests).
#[cfg(test)]
pub(crate) fn touches() -> usize {
    TOUCHES.with(std::cell::Cell::get)
}

/// Records input for "Idle" (an atomic store).
pub fn touch(cx: &App) {
    #[cfg(test)]
    TOUCHES.with(|t| t.set(t.get() + 1));
    if let Some(global) = cx.try_global::<DiscordPresence>() {
        global.presence.touch();
    }
}

/// An empty layer that records scrolling and clicks for "Idle" in the
/// capture phase, so it sees them although editors stop them.
pub fn input_watcher() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|_: &ScrollWheelEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    touch(cx);
                }
            });
            window.on_mouse_event(|_: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    touch(cx);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// The presence's status as last reported.
pub(crate) fn status(cx: &App) -> Status {
    cx.try_global::<DiscordStatus>()
        .map_or(Status::Off, |s| s.0)
}

/// The presence settings of `[discord]`.
pub fn settings_of(d: &arcode_config::DiscordSettings) -> Settings {
    Settings {
        enabled: d.enabled,
        show_file_name: d.show_file_name,
        show_project_name: d.show_project_name,
        show_elapsed: d.show_elapsed,
        application_id: d.application_id.clone(),
    }
}

/// When the workspace's current project was opened.
#[derive(Default)]
struct Opened {
    root: Option<PathBuf>,
    at: Option<u64>,
}

impl Opened {
    /// The opening time of `root`, starting the clock when it changed.
    fn of(&mut self, root: Option<&Path>) -> Option<u64> {
        if self.root.as_deref() != root {
            self.root = root.map(Path::to_path_buf);
            self.at = root.map(|_| unix_now());
        }
        self.at
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Reports the workspace's view when its project or active editor
/// changes, or when its window is activated.
pub fn track(window: &mut Window, cx: &mut Context<Workspace>) -> Vec<Subscription> {
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Opened::default()));
    let on_event = opened.clone();
    let events = cx.subscribe_in(
        &cx.entity(),
        window,
        move |ws, _, event: &WorkspaceEvent, window, cx| {
            if !matches!(event, WorkspaceEvent::EditorOpened(_)) {
                report(ws, &mut on_event.borrow_mut(), false, window, cx);
            }
        },
    );
    let activation = cx.observe_window_activation(window, move |ws, window, cx| {
        if window.is_window_active() {
            report(ws, &mut opened.borrow_mut(), true, window, cx);
        }
    });
    vec![events, activation]
}

/// Sends what `ws` shows, if its window is the one the presence follows.
fn report(ws: &Workspace, opened: &mut Opened, activated: bool, window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let shown = shown(ws, opened, cx);
    let Some(global) = cx.try_global::<DiscordPresence>() else {
        return;
    };
    let follows = activated || global.source.is_none_or(|source| source == id);
    if !follows {
        return;
    }
    let global = cx.global_mut::<DiscordPresence>();
    global.source = Some(id);
    global.presence.show(shown);
}

/// What the workspace shows: never a path, only the active editor's file
/// name and the project folder's name.
fn shown(ws: &Workspace, opened: &mut Opened, cx: &App) -> Shown {
    let project = ws.project();
    let started = opened.of(project.map(|p| p.root.as_path()));
    let view = match (project, ws.active_editor(cx)) {
        (_, Some(editor)) => {
            let editor = editor.read(cx);
            let language = editor.language();
            View::Editing {
                file_name: editor
                    .path()
                    .and_then(Path::file_name)
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                language: Language {
                    key: language.id.to_lowercase(),
                    name: language.name.to_owned(),
                },
            }
        }
        (Some(_), None) => View::Browsing,
        (None, None) => View::Choosing,
    };
    Shown {
        view,
        project: project.map(|p| p.name.clone()),
        started,
    }
}

/// Installs a presence that never starts (tests of what is shown).
#[cfg(test)]
pub(crate) fn init_for_tests(cx: &mut App) {
    cx.set_global(DiscordPresence {
        presence: Presence::new(Options::default()),
        source: None,
    });
}

/// What the presence would show now (tests).
#[cfg(test)]
pub(crate) fn current(cx: &App) -> Shown {
    cx.global::<DiscordPresence>().presence.shown().clone()
}
