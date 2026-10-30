//! The custom GPUI element that lays out and paints the editor.

use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent, Style, Window,
    relative,
};

use crate::Editor;
use crate::layout::LayoutSnapshot;
use crate::paint::{Decorations, decorations};

/// Paints an [`Editor`]: gutter, text, selections, cursors, diagnostics and
/// scrollbar. Only visible lines are shaped and painted.
pub(crate) struct EditorElement {
    editor: Entity<Editor>,
}

impl EditorElement {
    pub(crate) fn new(editor: Entity<Editor>) -> Self {
        EditorElement { editor }
    }
}

/// State carried from prepaint to paint.
pub(crate) struct PrepaintState {
    layout: Rc<LayoutSnapshot>,
    decorations: Decorations,
    hitbox: Hitbox,
    text_hitbox: Hitbox,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (layout, decorations) = self.editor.update(cx, |editor, cx| {
            let layout = editor.layout(bounds, window, cx);
            let decorations = decorations(editor, &layout, window, cx);
            (layout, decorations)
        });
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let text_hitbox = window.insert_hitbox(layout.text_bounds, HitboxBehavior::Normal);
        PrepaintState {
            layout,
            decorations,
            hitbox,
            text_hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let over_scrollbar = prepaint
            .layout
            .scrollbar
            .is_some_and(|b| b.track.contains(&window.mouse_position()));
        if !over_scrollbar {
            window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);
        }
        self.register_mouse_listeners(&prepaint.hitbox, window);
        let layout = prepaint.layout.clone();
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            prepaint.decorations.paint(&layout, window, cx);
        });
    }
}

impl EditorElement {
    fn register_mouse_listeners(&self, hitbox: &Hitbox, window: &mut Window) {
        let editor = self.editor.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hb.is_hovered(window) {
                editor.update(cx, |ed, cx| ed.mouse_down(e, window, cx));
                cx.stop_propagation();
            }
        });
        let editor = self.editor.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                let hovered = hb.is_hovered(window);
                editor.update(cx, |ed, cx| ed.mouse_move(e, hovered, window, cx));
            }
        });
        let editor = self.editor.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble {
                editor.update(cx, |ed, cx| ed.mouse_up(e, cx));
            }
        });
        let editor = self.editor.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hb.should_handle_scroll(window) {
                editor.update(cx, |ed, cx| ed.scroll_wheel(e, cx));
                cx.stop_propagation();
            }
        });
    }
}
