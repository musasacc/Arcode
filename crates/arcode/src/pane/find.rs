//! The pane's find bar: shown above the active editor, following it when
//! another tab becomes active.

use gpui::{AppContext, Context, Entity, Window};

use super::Pane;
use crate::find_bar::{FindBar, FindBarEvent};

impl Pane {
    /// The find bar, if it was ever opened.
    pub fn find_bar(&self) -> Option<&Entity<FindBar>> {
        self.find.as_ref().map(|(bar, _)| bar)
    }

    /// Whether the find bar is shown.
    pub fn find_open(&self) -> bool {
        self.find_open && self.active_editor().is_some()
    }

    /// Shows the find bar on the active editor (with the replace row when
    /// `replace`) and focuses its query.
    pub fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor().cloned() else {
            return;
        };
        let bar = match &self.find {
            Some((bar, _)) => bar.clone(),
            None => {
                let bar = cx.new(FindBar::new);
                let sub = cx.subscribe_in(&bar, window, |pane, _, event, window, cx| {
                    let FindBarEvent::Closed = event;
                    pane.find_open = false;
                    pane.focus(window, cx);
                    cx.notify();
                });
                self.find = Some((bar.clone(), sub));
                bar
            }
        };
        self.find_open = true;
        bar.update(cx, |bar, cx| {
            bar.open(Some(editor), replace, cx);
            bar.focus_query(window, cx);
        });
        cx.notify();
    }

    /// F3 / Shift+F3 from the editor: steps through the find bar's matches
    /// (showing the bar again if it was closed with a query).
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some((bar, _)) = &self.find else {
            return;
        };
        let bar = bar.clone();
        if !self.find_open {
            if bar.read(cx).query_text(cx).is_empty() {
                return;
            }
            self.find_open = true;
            let editor = self.active_editor().cloned();
            bar.update(cx, |bar, cx| {
                bar.set_editor(editor, cx);
                bar.reshow(cx);
            });
            cx.notify();
        }
        bar.update(cx, |bar, cx| bar.go(forward, cx));
    }

    /// Points an open find bar at the active editor.
    pub(super) fn sync_find_target(&mut self, cx: &mut Context<Self>) {
        if let Some((bar, _)) = &self.find
            && self.find_open
        {
            let editor = self.active_editor().cloned();
            bar.update(cx, |bar, cx| bar.set_editor(editor, cx));
        }
    }

    /// Hides the find bar (and its highlights) if it is shown.
    pub(super) fn close_find(&mut self, cx: &mut Context<Self>) {
        if self.find_open
            && let Some((bar, _)) = &self.find
        {
            self.find_open = false;
            bar.update(cx, |bar, cx| bar.close(cx));
            cx.notify();
        }
    }
}
