//! Public methods for the extension points in [`crate::ext`].

use gpui::{Context, SharedString};

use crate::Editor;
use crate::ext::{
    BackgroundHighlight, CodeLens, CursorShape, EditorPopover, GutterDecoration, GutterLane,
    HighlightRange, InterceptorId, KeyInterceptor, VirtualText, VirtualTextPosition,
};

impl Editor {
    /// Replaces the gutter decorations of set `set` (e.g. `"git"`).
    pub fn set_gutter_decorations(
        &mut self,
        set: impl Into<SharedString>,
        mut marks: Vec<GutterDecoration>,
        cx: &mut Context<Self>,
    ) {
        marks.sort_by_key(|m| m.line);
        replace(&mut self.ext.gutter, set.into(), marks);
        cx.notify();
    }

    /// The gutter decorations of `set`.
    pub fn gutter_decorations(&self, set: &str) -> &[GutterDecoration] {
        self.ext.gutter.get(set).map_or(&[], Vec::as_slice)
    }

    /// Reserves (or releases) space for `lane` even when it has no marks,
    /// e.g. the breakpoint lane in debuggable files.
    pub fn set_gutter_lane_visible(
        &mut self,
        lane: GutterLane,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = if visible {
            self.ext.lanes_shown.insert(lane)
        } else {
            self.ext.lanes_shown.remove(&lane)
        };
        if changed {
            cx.notify();
        }
    }

    /// Replaces the background highlights of `set`.
    pub fn set_background_highlights(
        &mut self,
        set: impl Into<SharedString>,
        mut highlights: Vec<BackgroundHighlight>,
        cx: &mut Context<Self>,
    ) {
        highlights.sort_by_key(|h| match &h.range {
            HighlightRange::Lines(r) => (r.start, 0),
            HighlightRange::Text(r) => (r.start.line, r.start.column),
        });
        replace(&mut self.ext.highlights, set.into(), highlights);
        cx.notify();
    }

    /// The background highlights of `set`.
    pub fn background_highlights(&self, set: &str) -> &[BackgroundHighlight] {
        self.ext.highlights.get(set).map_or(&[], Vec::as_slice)
    }

    /// Replaces the virtual text (inlay hints, inline blame) of `set`.
    pub fn set_virtual_text(
        &mut self,
        set: impl Into<SharedString>,
        mut texts: Vec<VirtualText>,
        cx: &mut Context<Self>,
    ) {
        texts.sort_by_key(|v| match v.position {
            VirtualTextPosition::EndOfLine(l) => (l, usize::MAX),
            VirtualTextPosition::Inline(p) => (p.line, p.column),
        });
        replace(&mut self.ext.virtual_text, set.into(), texts);
        cx.notify();
    }

    /// The virtual text of `set`.
    pub fn virtual_text(&self, set: &str) -> &[VirtualText] {
        self.ext.virtual_text.get(set).map_or(&[], Vec::as_slice)
    }

    /// Replaces the code lenses of `set`.
    pub fn set_code_lenses(
        &mut self,
        set: impl Into<SharedString>,
        mut lenses: Vec<CodeLens>,
        cx: &mut Context<Self>,
    ) {
        lenses.sort_by_key(|l| l.line);
        replace(&mut self.ext.lenses, set.into(), lenses);
        cx.notify();
    }

    /// The code lenses of `set`.
    pub fn code_lenses(&self, set: &str) -> &[CodeLens] {
        self.ext.lenses.get(set).map_or(&[], Vec::as_slice)
    }

    /// Removes every decoration (gutter, highlights, virtual text, lenses)
    /// and the popover registered under `set`.
    pub fn clear_decorations(&mut self, set: &str, cx: &mut Context<Self>) {
        let mut changed = self.ext.gutter.remove(set).is_some();
        changed |= self.ext.highlights.remove(set).is_some();
        changed |= self.ext.virtual_text.remove(set).is_some();
        changed |= self.ext.lenses.remove(set).is_some();
        let before = self.ext.popovers.len();
        self.ext.popovers.retain(|(key, _)| key.as_ref() != set);
        changed |= before != self.ext.popovers.len();
        if changed {
            cx.notify();
        }
    }

    /// Shows `popover` under `key`, replacing any popover with that key.
    pub fn show_popover(
        &mut self,
        key: impl Into<SharedString>,
        popover: EditorPopover,
        cx: &mut Context<Self>,
    ) {
        let key = key.into();
        if popover.on_key.is_some() {
            crate::ext::keys::register(cx.entity().downgrade(), cx);
        }
        self.ext.popovers.retain(|(k, _)| *k != key);
        self.ext.popovers.push((key, popover));
        cx.notify();
    }

    /// Hides the popover `key`. Returns whether one was shown.
    pub fn hide_popover(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let before = self.ext.popovers.len();
        self.ext.popovers.retain(|(k, _)| k.as_ref() != key);
        let removed = before != self.ext.popovers.len();
        if removed {
            cx.notify();
        }
        removed
    }

    /// Whether popover `key` is shown.
    pub fn has_popover(&self, key: &str) -> bool {
        self.ext.popovers.iter().any(|(k, _)| k.as_ref() == key)
    }

    /// The popover `key`, if shown (its anchor follows edits).
    pub fn popover(&self, key: &str) -> Option<&EditorPopover> {
        self.ext
            .popovers
            .iter()
            .find(|(k, _)| k.as_ref() == key)
            .map(|(_, p)| p)
    }

    /// Installs `interceptor`, which sees keystrokes before the editor's
    /// bindings while the editor is focused. Higher `priority` runs first;
    /// popover key handlers always run before installed interceptors.
    pub fn add_key_interceptor(
        &mut self,
        priority: i32,
        interceptor: KeyInterceptor,
        cx: &mut Context<Self>,
    ) -> InterceptorId {
        let id = InterceptorId(self.ext.next_id());
        self.ext.interceptors.push((id, priority, interceptor));
        crate::ext::keys::register(cx.entity().downgrade(), cx);
        id
    }

    /// Removes an interceptor. Returns whether it was installed.
    pub fn remove_key_interceptor(&mut self, id: InterceptorId) -> bool {
        let before = self.ext.interceptors.len();
        self.ext.interceptors.retain(|(i, _, _)| *i != id);
        before != self.ext.interceptors.len()
    }

    /// Sets the cursor shape (e.g. a block in Vim normal mode).
    pub fn set_cursor_shape(&mut self, shape: CursorShape, cx: &mut Context<Self>) {
        if self.ext.cursor_shape != shape {
            self.ext.cursor_shape = shape;
            cx.notify();
        }
    }

    /// The current cursor shape.
    pub fn cursor_shape(&self) -> CursorShape {
        self.ext.cursor_shape
    }
}

/// Stores `items` under `key`, removing the entry when empty.
fn replace<T>(
    map: &mut std::collections::BTreeMap<SharedString, Vec<T>>,
    key: SharedString,
    items: Vec<T>,
) {
    if items.is_empty() {
        map.remove(&key);
    } else {
        map.insert(key, items);
    }
}
