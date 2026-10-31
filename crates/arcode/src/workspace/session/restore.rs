//! Reopening a saved session: layout, panes and tabs, then each editor's
//! selections and scroll position.

use arcode_buffer::{Point, Selection, SelectionSet};
use arcode_config::{FileSession, LayoutSession, ProjectSession};
use arcode_editor::{Editor, ScrollAnchor};
use gpui::{Context, Entity, Window, px};

use super::{activity_from_key, tab_from_key};
use crate::bottom_panel;
use crate::sidebar;
use crate::workspace::Workspace;

impl Workspace {
    /// Restores `session` into the freshly opened project: sidebar and
    /// bottom panel, expanded folders, the panes with their tabs (files
    /// that no longer exist are skipped), and every editor's selections
    /// and scroll line.
    pub(crate) fn restore_session(
        &mut self,
        session: &ProjectSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.restoring = true;
        if let Some(layout) = &session.layout {
            self.apply_layout(layout, window, cx);
        }
        if let Some(tree) = &self.file_tree
            && !session.expanded_dirs.is_empty()
        {
            tree.update(cx, |tree, cx| {
                tree.set_expanded_dirs(&session.expanded_dirs, cx)
            });
        }
        let panes = session.pane_layout();
        let mut first = true;
        let mut active_pane = None;
        for (ix, pane) in panes.iter().enumerate() {
            let files: Vec<_> = pane.files.iter().filter(|f| f.is_file()).collect();
            if files.is_empty() {
                continue;
            }
            if !first {
                self.split_right(window, cx);
            }
            first = false;
            if ix == session.active_pane.min(panes.len() - 1) {
                active_pane = Some(self.active_pane);
            }
            for file in &files {
                self.open_file(file, window, cx);
                if let Some(editor) = self.active_editor(cx)
                    && let Some(state) = session.file(file)
                {
                    self.apply_file_state(&editor, &state, cx);
                }
            }
            let active_path = pane.files.get(pane.active);
            let pane_entity = self.active_pane().clone();
            if let Some(ix) = active_path.and_then(|p| pane_entity.read(cx).position_of(p, cx)) {
                pane_entity.update(cx, |pane, cx| pane.activate(ix, window, cx));
            }
        }
        if let Some(ix) = active_pane
            && ix < self.panes.len()
        {
            self.active_pane = ix;
            self.active_pane().read(cx).focus(window, cx);
            self.active_editor_changed(cx);
        }
        self.session.restoring = false;
        cx.notify();
    }

    fn apply_layout(
        &mut self,
        layout: &LayoutSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_visible = layout.sidebar_visible;
        self.sidebar_width = px(layout
            .sidebar_width
            .clamp(sidebar::MIN_WIDTH, sidebar::MAX_WIDTH));
        if let Some(activity) = activity_from_key(&layout.sidebar_view) {
            self.activity = activity;
            if self.sidebar_visible && activity == crate::activity_bar::Activity::Plugins {
                // The page checks which servers are installed when shown.
                self.langs.view.update(cx, |v, cx| v.shown(cx));
            }
        }
        let max_height =
            (f32::from(window.viewport_size().height) * 0.75).max(bottom_panel::MIN_HEIGHT);
        self.bottom_panel_height = px(layout.panel_height.clamp(
            bottom_panel::MIN_HEIGHT,
            max_height.max(bottom_panel::MIN_HEIGHT),
        ));
        // The panel is shown but not focused, and a terminal only starts
        // when the Terminal tab is actually looked at.
        self.bottom_panel_visible = layout.panel_visible;
        if let Some(tab) = tab_from_key(&layout.panel_tab) {
            self.bottom_panel
                .update(cx, |panel, cx| panel.set_active_tab(tab, cx));
        }
    }

    /// Puts the saved selections and scroll line into `editor`.
    pub(crate) fn apply_file_state(
        &mut self,
        editor: &Entity<Editor>,
        state: &FileSession,
        cx: &mut Context<Self>,
    ) {
        let scroll = state.scroll_line;
        editor.update(cx, |editor, cx| {
            let selections: Vec<Selection> = state
                .selections
                .iter()
                .map(|s| {
                    let anchor = editor.point_to_offset(Point::new(s.anchor.0, s.anchor.1));
                    let head = editor.point_to_offset(Point::new(s.head.0, s.head.1));
                    Selection::new(anchor, head)
                })
                .collect();
            if let Some(set) = SelectionSet::from_vec(selections, 0) {
                editor.set_selections(set, cx);
            }
            editor.scroll_to_line(scroll, ScrollAnchor::Top, cx);
        });
        self.session
            .restored_scroll
            .insert(editor.entity_id(), scroll);
    }
}
