//! Inlay hint refreshes reach only editors that are shown.

mod common;

use common::lang_fake::*;
use common::*;
use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div,
};

use arcode_editor::Editor;

/// Shows its editor only while `shown` (like a tab that is not active).
struct Host {
    editor: Entity<Editor>,
    shown: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let root = div().size_full();
        if self.shown {
            root.child(self.editor.clone())
        } else {
            root
        }
    }
}

#[gpui::test]
fn a_refresh_asks_hidden_editors_only_when_shown(cx: &mut TestAppContext) {
    // Sets up the globals and gives us a services log for an editor.
    let (first, cx) = open(cx, CODE);
    let log = install_services(&first, false, cx);
    settle(cx);
    let (host, cx) = cx.add_window_view(|_, cx| {
        let editor = cx.new(|cx| Editor::new(arcode_buffer::Buffer::new(CODE), cx));
        Host {
            editor,
            shown: true,
        }
    });
    let editor = host.read_with(cx, |h, _| h.editor.clone());
    let services_log = install_services(&editor, false, cx);
    settle(cx);
    assert_eq!(services_log.borrow().hint_requests, 1, "shown: asked once");
    assert_eq!(editor.read_with(cx, |e, _| e.inlay_hint_labels()).len(), 2);

    // Hidden, a refresh asks nothing and keeps the hints.
    host.update(cx, |h, cx| {
        h.shown = false;
        cx.notify();
    });
    settle(cx);
    editor.update(cx, |e, cx| e.refresh_inlay_hints(cx));
    settle(cx);
    assert_eq!(services_log.borrow().hint_requests, 1);
    assert_eq!(editor.read_with(cx, |e, _| e.inlay_hint_labels()).len(), 2);

    // Shown again, it asks at once.
    host.update(cx, |h, cx| {
        h.shown = true;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(services_log.borrow().hint_requests, 2);
    drop(log);
}
