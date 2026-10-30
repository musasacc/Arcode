//! Code navigation in the workspace: go to definition / type definition /
//! implementation, find all references, document and workspace symbol
//! pickers and the back / forward history across files.
//!
//! Requests go through the [`crate::lsp_bridge::LspBridge`]; documents
//! without a server fall back to the tree-sitter outline. A target in
//! another file opens in the active pane (or activates the pane where it
//! is open) and scrolls there. Files outside the project (e.g. the Rust
//! standard library) open read-only.

pub mod history;
pub mod item;
pub mod picker;
mod picker_view;
pub mod preview;
mod requests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use arcode_editor::Editor;
use gpui::{
    AppContext, Context, Entity, Focusable, InteractiveElement, MouseButton, ParentElement,
    Stateful, Styled, Subscription, Task, WeakEntity, Window, div, px,
};

use crate::actions::{GoBack, GoForward, ShowDocumentSymbols, ShowWorkspaceSymbols};
use crate::lsp_bridge::navigation::GotoKind;
use crate::workspace::{Workspace, WorkspaceEvent};
use history::{History, NavLocation};
use item::NavTarget;
use picker::{NavPicker, NavPickerEvent, PickerOptions};

/// Navigation state of a workspace.
#[derive(Default)]
pub struct Navigator {
    history: History,
    /// The active editor, to record where the cursor was when another
    /// file becomes active.
    active: Option<WeakEntity<Editor>>,
    /// A switch to this file is our own jump (already recorded or a
    /// history step), not a new place.
    expect: Option<PathBuf>,
    picker: Option<Entity<NavPicker>>,
    picker_sub: Option<Subscription>,
    /// Request in flight (a newer one replaces it).
    task: Option<Task<()>>,
    /// Preview lines being loaded for the picker.
    preview_task: Option<Task<()>>,
}

impl Navigator {
    /// Subscribes the workspace to its own events to track file switches.
    pub fn track(cx: &mut Context<Workspace>) -> Subscription {
        cx.subscribe_self(
            |ws: &mut Workspace, event: &WorkspaceEvent, cx| match event {
                WorkspaceEvent::ActiveEditorChanged(editor) => {
                    ws.nav_active_changed(editor.as_ref(), cx)
                }
                WorkspaceEvent::ProjectChanged => {
                    ws.nav.history.clear();
                    ws.nav.active = None;
                }
                WorkspaceEvent::EditorOpened(_) => {}
            },
        )
    }

    /// The open picker (tests).
    pub fn picker(&self) -> Option<&Entity<NavPicker>> {
        self.picker.as_ref()
    }

    /// The history (tests).
    pub fn history(&self) -> &History {
        &self.history
    }
}

/// Where `editor`'s cursor is.
fn location_of(editor: &Entity<Editor>, cx: &gpui::App) -> Option<NavLocation> {
    let e = editor.read(cx);
    Some(NavLocation {
        path: e.path()?.to_path_buf(),
        point: e.cursor_position(),
    })
}

impl Workspace {
    /// Registers the navigation action handlers on the root element.
    pub(crate) fn register_nav_actions<E: InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(
            cx.listener(|ws, _: &arcode_editor::GoToDefinition, window, cx| {
                ws.goto(GotoKind::Definition, window, cx)
            }),
        )
        .on_action(
            cx.listener(|ws, _: &arcode_editor::GoToTypeDefinition, window, cx| {
                ws.goto(GotoKind::TypeDefinition, window, cx)
            }),
        )
        .on_action(
            cx.listener(|ws, _: &arcode_editor::GoToImplementation, window, cx| {
                ws.goto(GotoKind::Implementation, window, cx)
            }),
        )
        .on_action(
            cx.listener(|ws, _: &arcode_editor::FindAllReferences, window, cx| {
                ws.find_references(window, cx)
            }),
        )
        .on_action(
            cx.listener(|ws, _: &ShowDocumentSymbols, window, cx| ws.document_symbols(window, cx)),
        )
        .on_action(
            cx.listener(|ws, _: &ShowWorkspaceSymbols, window, cx| {
                ws.workspace_symbols(window, cx)
            }),
        )
        .on_action(cx.listener(|ws, _: &GoBack, window, cx| ws.go_back(window, cx)))
        .on_action(cx.listener(|ws, _: &GoForward, window, cx| ws.go_forward(window, cx)))
    }

    fn nav_active_changed(&mut self, editor: Option<&Entity<Editor>>, cx: &mut Context<Self>) {
        let previous = self.nav.active.take().and_then(|w| w.upgrade());
        self.nav.active = editor.map(Entity::downgrade);
        let new_path = editor.and_then(|e| e.read(cx).path().map(Path::to_path_buf));
        if new_path.is_some() && self.nav.expect == new_path {
            self.nav.expect = None;
            return;
        }
        if let Some(from) = previous.and_then(|p| location_of(&p, cx))
            && Some(&from.path) != new_path.as_ref()
        {
            self.nav.history.record(from);
        }
    }

    /// Where the cursor of the active editor is.
    fn current_location(&self, cx: &gpui::App) -> Option<NavLocation> {
        location_of(&self.active_editor(cx)?, cx)
    }

    /// Jumps to `target`, recording the current place for Back.
    pub fn jump_to(&mut self, target: &NavTarget, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.current_location(cx);
        if let Some(from) = from.clone() {
            self.nav.history.record(from);
        }
        self.open_target(target, from.map(|f| f.path), window, cx);
    }

    /// Opens `target` without recording it. `from` is the file the jump
    /// started in (decides read-only when no project is open).
    fn open_target(
        &mut self,
        target: &NavTarget,
        from: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let path = std::fs::canonicalize(&target.path).unwrap_or_else(|_| target.path.clone());
        let was_open = self
            .editors(cx)
            .iter()
            .any(|e| e.read(cx).path() == Some(path.as_path()));
        if from.as_ref() != Some(&path) {
            self.nav.expect = Some(path.clone());
        }
        let outside = !was_open && self.is_outside_project(&path, from.as_deref());
        if outside {
            // A library source belongs to the project's language server.
            self.lsp().read(cx).attach_to_workspace(&path);
        }
        self.open_file(&path, window, cx);
        let Some(editor) = self.active_editor(cx) else {
            self.nav.expect = None;
            return false;
        };
        if editor.read(cx).path() != Some(path.as_path()) {
            self.nav.expect = None;
            return false;
        }
        if outside {
            editor.update(cx, |e, cx| e.set_read_only(true, cx));
            self.show_toast("Opened read-only: the file is outside the project", cx);
        }
        let range = target.range_in(editor.read(cx));
        editor.update(cx, |e, cx| e.go_to(range.start, window, cx));
        true
    }

    /// Whether `path` lies outside the project (or, without a project,
    /// outside the folder of the file the jump started in).
    fn is_outside_project(&self, path: &Path, from: Option<&Path>) -> bool {
        match (self.project(), from.and_then(Path::parent)) {
            (Some(project), _) => !path.starts_with(&project.root),
            (None, Some(dir)) => !path.starts_with(dir),
            (None, None) => false,
        }
    }

    fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.current_location(cx);
        if let Some(target) = self.nav.history.back(current) {
            self.open_location(target, window, cx);
        }
    }

    fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.current_location(cx);
        if let Some(target) = self.nav.history.forward(current) {
            self.open_location(target, window, cx);
        }
    }

    fn open_location(
        &mut self,
        location: NavLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = self.current_location(cx).map(|l| l.path);
        let target = NavTarget {
            path: location.path,
            pos: item::NavPos::Points(location.point..location.point),
        };
        self.open_target(&target, from, window, cx);
    }

    /// Opens `picker` as the navigation overlay.
    fn show_nav_picker(
        &mut self,
        options: PickerOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<NavPicker> {
        let picker = cx.new(|cx| NavPicker::new(options, cx));
        let sub = cx.subscribe_in(
            &picker,
            window,
            |ws, _, event: &NavPickerEvent, window, cx| match event {
                NavPickerEvent::Confirmed(item) => {
                    let target = item.target.clone();
                    ws.close_nav_picker(window, cx);
                    ws.jump_to(&target, window, cx);
                }
                NavPickerEvent::Dismissed => ws.close_nav_picker(window, cx),
                NavPickerEvent::QueryChanged(query) => {
                    ws.query_workspace_symbols(query.clone(), cx)
                }
            },
        );
        window.focus(&picker.focus_handle(cx));
        self.nav.picker = Some(picker.clone());
        self.nav.picker_sub = Some(sub);
        cx.notify();
        picker
    }

    fn close_nav_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.nav.task = None;
        self.nav.preview_task = None;
        self.nav.picker_sub = None;
        if self.nav.picker.take().is_some() {
            self.active_pane().read(cx).focus(window, cx);
            cx.notify();
        }
    }

    /// Adds the navigation picker overlay as the last child of `root`.
    pub(crate) fn render_nav_overlay(
        &self,
        root: Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let Some(picker) = &self.nav.picker else {
            return root;
        };
        let c = arcode_ui_kit::theme(cx).colors.clone();
        let panel = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(picker.clone());
        root.child(
            div()
                .id("nav-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| ws.close_nav_picker(window, cx)),
                )
                .bg(c.background.opacity(0.35))
                .flex()
                .justify_center()
                .pt(px(crate::title_bar::TITLE_BAR_HEIGHT + 8.))
                .child(fade_in(panel, cx)),
        )
    }
}

/// A short fade-in when animations are on.
fn fade_in(el: gpui::Div, cx: &gpui::App) -> gpui::AnyElement {
    use gpui::{Animation, AnimationExt, IntoElement};
    match arcode_ui_kit::animation_duration(cx) {
        Some(duration) => el
            .with_animation(
                "nav-anim",
                Animation::new(duration).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(t),
            )
            .into_any_element(),
        None => el.into_any_element(),
    }
}
