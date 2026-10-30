//! Pane management: subscriptions, splitting and closing.

use gpui::{AppContext, Context, Entity, Subscription, Window};

use super::Workspace;
use crate::pane::{Pane, PaneEvent};

impl Workspace {
    /// Subscribes the workspace to a pane's events.
    pub(super) fn subscribe_pane(
        pane: &Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(pane, window, |ws, pane, event: &PaneEvent, window, cx| {
            let ix = ws.panes.iter().position(|p| p == pane);
            match event {
                PaneEvent::Focused => {
                    if let Some(ix) = ix
                        && ix != ws.active_pane
                    {
                        ws.active_pane = ix;
                        ws.active_editor_changed(cx);
                    }
                }
                PaneEvent::ActiveItemChanged => {
                    if ix == Some(ws.active_pane) {
                        ws.active_editor_changed(cx);
                    } else {
                        cx.notify();
                    }
                }
                PaneEvent::CloseRequested => {
                    if let Some(ix) = ix {
                        ws.close_pane(ix, window, cx);
                    }
                }
            }
        })
    }

    /// Replaces all panes with one empty pane.
    pub(super) fn reset_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = cx.new(|cx| Pane::new(window, cx));
        let sub = Self::subscribe_pane(&pane, window, cx);
        self.subs.tie(pane.downgrade().into(), sub);
        self.panes = vec![pane];
        self.active_pane = 0;
        self.update_closable(cx);
        self.active_editor_changed(cx);
    }

    /// Adds an empty pane to the right of the active one and focuses it.
    pub fn split_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = cx.new(|cx| Pane::new(window, cx));
        let sub = Self::subscribe_pane(&pane, window, cx);
        self.subs.tie(pane.downgrade().into(), sub);
        let ix = (self.active_pane + 1).min(self.panes.len());
        self.panes.insert(ix, pane.clone());
        self.active_pane = ix;
        self.update_closable(cx);
        pane.read(cx).focus(window, cx);
        self.active_editor_changed(cx);
    }

    /// Closes the pane at `ix` (only when another pane remains).
    pub fn close_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.len() <= 1 || ix >= self.panes.len() {
            return;
        }
        if !self.panes[ix].read(cx).items().is_empty() {
            return;
        }
        self.panes.remove(ix);
        if self.active_pane >= ix && self.active_pane > 0 {
            self.active_pane -= 1;
        }
        self.update_closable(cx);
        self.active_pane().read(cx).focus(window, cx);
        self.active_editor_changed(cx);
    }

    fn update_closable(&mut self, cx: &mut Context<Self>) {
        let closable = self.panes.len() > 1;
        for pane in &self.panes {
            pane.update(cx, |pane, cx| pane.set_closable(closable, cx));
        }
    }
}
