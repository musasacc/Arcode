//! Pane management: subscriptions, splitting and closing.

use std::path::Path;

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
                    // Tabs opened, closed or switched: remember it soon.
                    ws.schedule_session_save(cx);
                    ws.end_hidden_settings_preview(window, cx);
                    if ix == Some(ws.active_pane) {
                        ws.active_editor_changed(cx);
                    } else {
                        cx.notify();
                    }
                }
                PaneEvent::ItemClosed(editor) => ws.remember_closed_editor(editor, cx),
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
        self.end_hidden_settings_preview(window, cx);
        self.update_closable(cx);
        self.active_editor_changed(cx);
    }

    /// Activates the tab of `path` if it is open. Opened in another pane
    /// while the active pane is empty (a fresh split), the tab moves here:
    /// the user asked to see the file in this pane.
    pub(super) fn activate_open_file(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let found = self
            .panes
            .iter()
            .enumerate()
            .find_map(|(pane_ix, pane)| Some((pane_ix, pane.read(cx).position_of(path, cx)?)));
        let Some((pane_ix, ix)) = found else {
            return false;
        };
        if pane_ix != self.active_pane && !self.active_pane().read(cx).has_tabs() {
            self.move_tab_to_active(pane_ix, ix, window, cx);
        } else {
            self.active_pane = pane_ix;
            self.panes[pane_ix].update(cx, |pane, cx| pane.activate(ix, window, cx));
        }
        true
    }

    /// Moves tab `ix` of pane `from` into the active pane.
    fn move_tab_to_active(
        &mut self,
        from: usize,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((editor, label)) = self.panes[from].update(cx, |p, cx| p.detach(ix, window, cx))
        else {
            return;
        };
        let to = self.active_pane().clone();
        to.update(cx, |p, cx| p.attach(editor, label, window, cx));
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

    /// Split Right (`secondary-\`): like [`Self::split_right`], and the active tab
    /// moves into the new pane when its pane has others to keep showing,
    /// so the split does not start empty.
    pub(super) fn split_active_tab_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.active_pane;
        let moved = {
            let pane = self.active_pane().read(cx);
            pane.active_index().filter(|_| pane.items().len() > 1)
        };
        self.split_right(window, cx);
        if let Some(tab) = moved {
            self.move_tab_to_active(source, tab, window, cx);
            self.active_editor_changed(cx);
        }
    }

    /// Closes the pane at `ix` (only when another pane remains).
    pub fn close_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.len() <= 1 || ix >= self.panes.len() {
            return;
        }
        if self.panes[ix].read(cx).has_tabs() {
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

#[cfg(test)]
#[path = "panes_tests.rs"]
mod tests;
