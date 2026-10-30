//! Routing keystrokes to interceptors before the editor's key bindings.
//!
//! GPUI dispatches bound actions before element key listeners, so a
//! listener on the editor would run too late for Vim or a completion list.
//! Instead one app-wide keystroke interceptor (installed by
//! [`crate::init`]) looks for the focused editor among those that have
//! handlers and offers it the keystroke; a handler returning `true` stops
//! propagation, which also suppresses text input for that key.

use gpui::{App, Entity, Global, KeystrokeEvent, Subscription, WeakEntity, Window};

use crate::Editor;

/// Editors that currently have key handlers.
#[derive(Default)]
struct Routing {
    editors: Vec<WeakEntity<Editor>>,
    _hook: Option<Subscription>,
}

impl Global for Routing {}

/// Installs the app-wide hook (idempotent).
pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<Routing>() {
        return;
    }
    let hook = cx.intercept_keystrokes(on_keystroke);
    cx.set_global(Routing {
        editors: Vec::new(),
        _hook: Some(hook),
    });
}

/// Registers `editor` as having key handlers.
pub(crate) fn register(editor: WeakEntity<Editor>, cx: &mut App) {
    if !cx.has_global::<Routing>() {
        init(cx);
    }
    let routing = cx.global_mut::<Routing>();
    routing.editors.retain(|e| e.upgrade().is_some());
    if !routing.editors.iter().any(|e| e == &editor) {
        routing.editors.push(editor);
    }
}

fn on_keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    let Some(editors) = cx.try_global::<Routing>().map(|r| r.editors.clone()) else {
        return;
    };
    let focused: Option<Entity<Editor>> = editors.iter().filter_map(|e| e.upgrade()).find(|e| {
        let editor = e.read(cx);
        editor.focus_handle.is_focused(window) && editor.marked.is_none()
    });
    let Some(editor) = focused else {
        return;
    };
    let handlers = {
        let editor = editor.read(cx);
        // Popovers are drawn only while their anchor line is laid out (see
        // `popover_elements`); before the first layout, assume they are.
        let layout = editor.layout.as_ref();
        editor
            .ext
            .key_handlers(|line| layout.is_none_or(|l| l.visible(line).is_some()))
    };
    for handler in handlers {
        if handler(&editor, &event.keystroke, window, cx) {
            cx.stop_propagation();
            return;
        }
    }
}
