//! [`PanelView`]: one plugin panel drawn from its UI tree (docs/PLUGINS.md
//! §7). The plugin thread validated and applied the tree; this view only
//! maps every `widget` kind to GPUI elements and forwards events.
//!
//! - **Incremental.** A change arrives as an `Arc<PanelSnapshot>` plus the
//!   dirty/removed node ids; the view keeps per-node state for the dirty
//!   ones only and calls `cx.notify()` once. Nothing redraws while the
//!   plugin is quiet.
//! - **Uncontrolled inputs.** Text being edited, split ratios, the active
//!   tab and tree expansions live here (§7). An input is overwritten only
//!   when the plugin changed its `value`; every event carries the text of
//!   each input edited since the previous event.
//! - **Tables** page rows through `fetch-rows` around the viewport
//!   ([`table`]); **trees** are virtualized lists ([`tree`]).
//!
//! Element ids are `(kind, node-id)` inside the panel's own view.

mod cells;
mod code;
mod form;
mod layout;
mod select;
mod table;
mod table_scroll;
mod tree;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use arcode_plugin_host::bindings::ui::{Axis, Event, EventKind, Widget};
use arcode_plugin_host::bindings::{NodeId, PanelId};
use arcode_plugin_host::instance::Pending;
use arcode_plugin_host::ui_tree::{Changes, PanelSnapshot};
use arcode_plugin_host::{CallError, PluginId};
use arcode_ui_kit::{IconName, button, icon, theme};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, SharedString, Window, div,
    prelude::*, px,
};

use super::client::PluginClient;

/// What a panel reports to the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelViewEvent {
    /// A user-facing message (a refused event, a plugin error).
    Message(String),
}

/// One plugin panel.
pub struct PanelView {
    client: PluginClient,
    plugin: PluginId,
    panel: PanelId,
    plugin_name: SharedString,
    focus: FocusHandle,
    snapshot: Arc<PanelSnapshot>,
    /// "stopped: <reason>" after a crash, until the next view arrives.
    stopped: Option<(String, bool)>,
    /// Its instance was stopped by a reload or project switch: the panel
    /// id may belong to a new panel now, so nothing is sent any more.
    retired: bool,
    inputs: HashMap<NodeId, form::InputState>,
    /// Host-side overrides until the plugin redraws the node.
    tabs: HashMap<NodeId, u32>,
    checks: HashMap<NodeId, bool>,
    selects: HashMap<NodeId, u32>,
    open_select: Option<NodeId>,
    splits: HashMap<NodeId, layout::SplitState>,
    tables: HashMap<NodeId, table::TableState>,
    /// Tree keys toggled locally, waiting for the plugin's patch.
    tree_pending: HashSet<(NodeId, String)>,
    tree_selected: HashMap<NodeId, String>,
    /// The table keys go to (the one clicked last).
    key_table: Option<NodeId>,
    code: HashMap<NodeId, code::CodeState>,
}

impl EventEmitter<PanelViewEvent> for PanelView {}

impl Focusable for PanelView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PanelView {
    /// A panel showing `snapshot` (empty: a spinner until the plugin
    /// renders). Closing the view (dropping its last handle) tells the
    /// plugin with `view-closed`.
    pub fn new(
        client: PluginClient,
        plugin: PluginId,
        panel: PanelId,
        snapshot: Option<Arc<PanelSnapshot>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let plugin_name = client.plugin_name(&plugin).into();
        cx.on_release(|view: &mut PanelView, _| {
            let Some(m) = view.client.manager().filter(|_| !view.retired) else {
                return;
            };
            // Only a running instance that still has the panel hears about
            // it: telling a stopped one would start it.
            let open = m
                .instance(&view.plugin)
                .is_ok_and(|h| h.open_panels().iter().any(|(id, _)| *id == view.panel));
            if open {
                drop(m.close_panel(&view.plugin, view.panel));
            }
        })
        .detach();
        Self {
            client,
            plugin,
            panel,
            plugin_name,
            focus: cx.focus_handle(),
            snapshot: snapshot.unwrap_or_default(),
            stopped: None,
            retired: false,
            inputs: HashMap::new(),
            tabs: HashMap::new(),
            checks: HashMap::new(),
            selects: HashMap::new(),
            open_select: None,
            splits: HashMap::new(),
            tables: HashMap::new(),
            tree_pending: HashSet::new(),
            tree_selected: HashMap::new(),
            key_table: None,
            code: HashMap::new(),
        }
    }

    /// The owner.
    pub fn plugin(&self) -> &PluginId {
        &self.plugin
    }

    /// The panel id.
    pub fn panel(&self) -> PanelId {
        self.panel
    }

    /// The current view.
    pub fn snapshot(&self) -> &Arc<PanelSnapshot> {
        &self.snapshot
    }

    /// Applies a validated change from the plugin thread and redraws once.
    pub fn apply(
        &mut self,
        snapshot: Arc<PanelSnapshot>,
        changes: Changes,
        cx: &mut Context<Self>,
    ) {
        if self.retired {
            return;
        }
        let old = std::mem::replace(&mut self.snapshot, snapshot);
        self.stopped = None;
        let new = self.snapshot.clone();
        let gone = |id: &NodeId| !new.nodes.contains_key(id);
        if changes.full {
            self.inputs.retain(|id, _| !gone(id));
            self.splits.retain(|id, _| !gone(id));
            self.tables.retain(|id, _| !gone(id));
            self.code.retain(|id, _| !gone(id));
            self.tabs.retain(|id, _| !gone(id));
        } else {
            for id in &changes.removed {
                self.inputs.remove(id);
                self.splits.remove(id);
                self.tables.remove(id);
                self.code.remove(id);
                self.tabs.remove(id);
            }
        }
        let dirty: Vec<NodeId> = if changes.full {
            new.nodes.keys().copied().collect()
        } else {
            changes.dirty.clone()
        };
        for id in dirty {
            self.checks.remove(&id);
            self.selects.remove(&id);
            self.tree_pending.retain(|(n, _)| *n != id);
            match new.node(id).map(|n| &n.widget) {
                Some(Widget::Tabs(p)) => {
                    let was = old.node(id).and_then(|n| match &n.widget {
                        Widget::Tabs(o) => Some(o.active),
                        _ => None,
                    });
                    if was != Some(p.active) {
                        self.tabs.insert(id, p.active);
                    }
                }
                Some(Widget::Code(_) | Widget::Json(_)) => self.code_changed(id, cx),
                _ => {}
            }
        }
        for id in &changes.inputs {
            self.input_changed_by_plugin(*id, cx);
        }
        for change in &changes.tables {
            self.table_changed(change.table, change.row_count, change.reset);
        }
        cx.notify();
    }

    /// The plugin crashed; the panel shows why until it renders again.
    pub fn set_stopped(&mut self, reason: String, disabled: bool, cx: &mut Context<Self>) {
        self.stopped = Some((reason, disabled));
        cx.notify();
    }

    /// Detaches the view from its stopped instance for good (see
    /// `retired`); it keeps showing its last content under a banner.
    pub fn retire(&mut self, reason: String, cx: &mut Context<Self>) {
        self.retired = true;
        self.set_stopped(reason, false, cx);
    }

    /// Whether [`PanelView::retire`] was called.
    pub fn is_retired(&self) -> bool {
        self.retired
    }

    /// Sends a widget event with the text of every input edited since the
    /// previous event.
    fn send(&mut self, node: NodeId, kind: EventKind, cx: &mut Context<Self>) {
        if self.retired {
            return;
        }
        let values = self.take_values(cx);
        let event = Event { node, kind, values };
        let result = self
            .client
            .manager()
            .map(|m| m.ui_event(&self.plugin, self.panel, event));
        match result {
            Some(Ok(pending)) => self.await_call(pending, cx),
            Some(Err(e)) => cx.emit(PanelViewEvent::Message(format!(
                "{}: {e}",
                self.plugin_name
            ))),
            None => {}
        }
    }

    /// Reports the call's failure, if any, once it is answered.
    fn await_call(&mut self, pending: Pending, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let Ok(Err(error)) = pending.result.recv().await else {
                return;
            };
            this.update(cx, |view, cx| view.report(error, cx)).ok();
        })
        .detach();
    }

    fn report(&mut self, error: CallError, cx: &mut Context<Self>) {
        match error {
            // Crashes arrive as `PluginOutput::Crashed`; the rest is noise.
            CallError::Trapped(_)
            | CallError::Superseded
            | CallError::Busy
            | CallError::Stopped => {}
            other => cx.emit(PanelViewEvent::Message(format!(
                "{}: {other}",
                self.plugin_name
            ))),
        }
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.client.manager() {
            let _ = m.restart(&self.plugin);
        }
        self.stopped = None;
        cx.notify();
    }

    /// Renders node `id`; `parent` is the axis of the enclosing stack (a
    /// divider in a row is vertical).
    fn render_node(
        &mut self,
        snap: &PanelSnapshot,
        id: NodeId,
        parent: Option<Axis>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(node) = snap.node(id) else {
            return div().into_any_element();
        };
        match &node.widget {
            Widget::Stack(p) => self.render_stack(snap, node, *p, window, cx),
            Widget::Split(p) => self.render_split(snap, node, *p, window, cx),
            Widget::Tabs(p) => self.render_tabs(snap, node, p, window, cx),
            Widget::Scroll => self.render_scroll(snap, node, window, cx),
            Widget::Text(p) => layout::text(p, parent, cx),
            Widget::Badge(p) => layout::badge(p, cx),
            Widget::Button(p) => self.render_button(id, p, cx),
            Widget::Input(p) => self.render_input(id, p, window, cx),
            Widget::Select(p) => self.render_select(id, p, cx),
            Widget::Checkbox(p) => self.render_checkbox(id, p, cx),
            Widget::KeyValues(pairs) => layout::key_values(pairs, cx),
            Widget::Table(p) => self.render_table(id, p, cx),
            Widget::Tree(items) => self.render_tree(id, items, cx),
            Widget::Code(p) => self.render_code(id, p, cx),
            Widget::Json(text) => self.render_json(id, text, cx),
            Widget::Spinner(text) => layout::spinner(text, cx),
            Widget::EmptyState((title, hint)) => layout::empty_state(title, hint, cx),
            Widget::Divider => layout::divider(parent, cx),
            Widget::Spacer => div().flex_1().into_any_element(),
        }
    }

    fn render_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (reason, disabled) = self.stopped.clone()?;
        let c = theme(cx).colors.clone();
        let text = format!("{} stopped: {reason}", self.plugin_name);
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .bg(c.error.opacity(0.12))
                .border_b_1()
                .border_color(c.border)
                .child(icon(IconName::CircleX).size(px(14.)).color(c.error))
                .child(div().flex_1().min_w_0().child(text))
                .when(disabled, |el| {
                    el.child(
                        button("plugin-restart", "Restart")
                            .icon(IconName::RotateCcw)
                            .on_click(cx.listener(|view, _, _, cx| view.restart(cx))),
                    )
                })
                .into_any_element(),
        )
    }
}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let snap = self.snapshot.clone();
        let body = if snap.root_node().is_some() {
            self.render_node(&snap, snap.root, None, window, cx)
        } else {
            layout::spinner("Loading…", cx)
        };
        div()
            .key_context("PluginPanel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|view, e: &gpui::KeyDownEvent, _, cx| {
                let (m, key) = (&e.keystroke.modifiers, e.keystroke.key.as_str());
                // Ctrl+Home / Ctrl+End (Cmd on macOS) jump like Home / End.
                let jump = m.secondary() && !m.shift && !m.alt && matches!(key, "home" | "end");
                if (!m.modified() || jump) && view.table_key(key, cx) {
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(c.background)
            .text_color(c.text)
            .text_sm()
            .children(self.render_banner(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests;
