//! Requests from plugins that the workspace answers: consent prompts
//! (interim, `plugins::consent`), quick picks, editor writes (`apply-edits`,
//! `open-file`, `open-scratch`) and the plugin log tab.

use arcode_buffer::{Buffer, Point};
use arcode_editor::Editor;
use arcode_plugin_host::bindings::editor::EditorError;
use arcode_plugin_host::consent::{ConsentAnswer, ConsentRequest};
use arcode_ui_kit::IconName;
use gpui::{AppContext, Context, Entity, Focusable, PromptLevel, SharedString, Window};

use super::Workspace;
use crate::list_picker::{ListPicker, ListPickerEvent, PickItem};
use crate::pane::TabLabel;
use crate::plugins::bridge::EditorRequest;
use crate::plugins::consent;

impl Workspace {
    pub(super) fn plugin_consent(
        &mut self,
        request: ConsentRequest,
        reply: arcode_plugin_host::cancel::ReplySender<ConsentAnswer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(answer) = consent::scripted() {
            self.plugins
                .log(format!("{} -> {answer:?} (scripted)", request.message()));
            reply.send(answer);
            return;
        }
        let buttons = consent::buttons(&request.kind);
        let detail = consent::detail(&request);
        let answer = window.prompt(
            PromptLevel::Warning,
            &request.message(),
            Some(&detail),
            buttons,
            cx,
        );
        let kind = request.kind.clone();
        self.plugins.prompts_open += 1;
        cx.spawn(async move |this, cx| {
            let answer = match answer.await {
                Ok(ix) => consent::answer_for(&kind, ix),
                Err(_) => ConsentAnswer::Dismissed,
            };
            reply.send(answer);
            this.update(cx, |ws, _| {
                ws.plugins.prompts_open = ws.plugins.prompts_open.saturating_sub(1);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn plugin_quick_pick(
        &mut self,
        title: &str,
        items: Vec<String>,
        reply: arcode_plugin_host::cancel::ReplySender<Option<u32>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.plugins.quick_pick.is_some() {
            return; // one at a time; dropping `reply` dismisses this one
        }
        let items = items.into_iter().map(|i| PickItem::new(i, "")).collect();
        let title = title.to_owned();
        let picker = cx.new(|cx| ListPicker::new(title, IconName::Puzzle, items, cx));
        let sub = cx.subscribe_in(
            &picker,
            window,
            |ws, _, event: &ListPickerEvent, window, cx| {
                let picked = match event {
                    ListPickerEvent::Picked(ix) => Some(*ix as u32),
                    ListPickerEvent::Dismissed => None,
                    ListPickerEvent::Highlighted(_) => return,
                };
                ws.plugin_close_quick_pick(picked, window, cx);
            },
        );
        self.subs.set("plugin_quick_pick", sub);
        window.focus(&picker.focus_handle(cx));
        self.plugins.quick_pick = Some((picker, Some(reply)));
        cx.notify();
    }

    pub(super) fn plugin_close_quick_pick(
        &mut self,
        picked: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, reply)) = self.plugins.quick_pick.take() {
            if let (Some(reply), Some(ix)) = (reply, picked) {
                reply.send(Some(ix));
            }
            self.active_pane().read(cx).focus(window, cx);
            cx.notify();
        }
    }

    /// Adds the quick pick overlay to `root`.
    pub(super) fn render_plugin_overlay(
        &self,
        root: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::{InteractiveElement, MouseButton, ParentElement, Styled, div, px};
        let Some((picker, _)) = &self.plugins.quick_pick else {
            return root;
        };
        let c = arcode_ui_kit::theme(cx).colors.clone();
        let panel = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(picker.clone());
        root.child(
            div()
                .id("plugin-pick-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| ws.plugin_close_quick_pick(None, window, cx)),
                )
                .bg(c.background.opacity(0.35))
                .flex()
                .justify_center()
                .pt(px(crate::title_bar::TITLE_BAR_HEIGHT + 8.))
                .child(super::overlays::fade_in("plugin-pick-anim", panel, cx)),
        )
    }

    /// The editor of a plugin document id, if open.
    pub(super) fn plugin_editor_of(&self, doc: u64) -> Option<Entity<Editor>> {
        self.plugins
            .editors
            .values()
            .filter_map(|t| t.editor.upgrade())
            .find(|e| e.entity_id().as_u64() == doc)
    }

    /// Applies an editor write a plugin asked for (on the UI thread).
    pub(super) fn plugin_editor_request(
        &mut self,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match request {
            EditorRequest::ApplyEdits {
                doc,
                expected,
                edits,
                reply,
            } => {
                let Some(editor) = self.plugin_editor_of(doc) else {
                    let _ = reply.send(Err(EditorError::NoSuchDocument));
                    return;
                };
                let current = editor.read(cx).buffer().version();
                if current != expected {
                    let _ = reply.send(Err(EditorError::VersionMismatch(current)));
                    return;
                }
                let edits = edits
                    .into_iter()
                    .map(|e| {
                        let p = |p: arcode_plugin_host::bindings::Position| {
                            Point::new(p.line as usize, p.column as usize)
                        };
                        (p(e.range.start)..p(e.range.end), e.text)
                    })
                    .collect();
                let applied = editor.update(cx, |e, cx| e.apply_edits(edits, cx));
                let version = editor.read(cx).buffer().version();
                if reply.send(Ok(version)).is_err() && applied {
                    // The plugin stopped waiting and was told `denied`:
                    // the document must not change behind its back.
                    editor.update(cx, |e, cx| e.undo(cx));
                }
            }
            EditorRequest::OpenFile { path, at, reply } => {
                let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
                    let _ = reply.send(Err(EditorError::InvalidPath));
                    return;
                };
                // Answered first: nothing opens for a plugin that gave up.
                if reply.send(Ok(())).is_err() {
                    return;
                }
                let abs = root.join(&path);
                self.open_file(&abs, window, cx);
                if let (Some(at), Some(editor)) = (at, self.editor_for_path(&abs, cx)) {
                    let point = Point::new(at.line as usize, at.column as usize);
                    editor.update(cx, |e, cx| e.go_to(point, window, cx));
                }
            }
            EditorRequest::OpenScratch {
                title,
                language,
                text,
                reply,
            } => {
                let editor = cx.new(|cx| {
                    let mut e = Editor::new(Buffer::new(&text), cx);
                    if let Some(l) = arcode_syntax::language_by_id(&language) {
                        e.set_language(l, cx);
                    }
                    e
                });
                // The id is answered before the tab is shown: a plugin that
                // gave up waiting gets no tab.
                if reply.send(Ok(editor.entity_id().as_u64())).is_err() {
                    return;
                }
                let label = TabLabel::new(SharedString::from(title));
                self.active_pane().clone().update(cx, |p, cx| {
                    p.add_titled_editor(editor.clone(), label, window, cx)
                });
                cx.emit(super::WorkspaceEvent::EditorOpened(editor));
            }
        }
    }

    /// "Plugins: Show Log" in a read-only tab (replacing an open one).
    pub(super) fn show_plugin_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::plugins::log::{LOG_TAB_KEY, render};
        let text = {
            let manager = self.plugins.client.manager();
            render(manager.as_deref(), &self.plugins.host_log)
        };
        for pane in self.panes.clone() {
            if let Some(ix) = pane.read(cx).position_of_key(LOG_TAB_KEY) {
                pane.update(cx, |p, cx| p.close(ix, window, cx));
            }
        }
        let editor = cx.new(|cx| {
            let mut e = Editor::new(Buffer::new(&text), cx);
            e.set_read_only(true, cx);
            e
        });
        let label = TabLabel {
            title: "Plugin Log".into(),
            key: LOG_TAB_KEY.into(),
            tooltip: "Plugin log (reopen to refresh)".into(),
        };
        self.active_pane()
            .clone()
            .update(cx, |p, cx| p.add_titled_editor(editor, label, window, cx));
        cx.notify();
    }
}
