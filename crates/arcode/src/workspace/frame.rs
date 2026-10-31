//! The floating panels of the workbench (docs/DESIGN.md 6.1, 6.4): which
//! one wears the focus frame, and the resize arithmetic of the handles
//! that live in the canvas gaps between them. Also the debugger state the
//! title bar shows (the Paused chip and Continue).

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use arcode_ui_kit::metrics::{FOOTER_H, GAP, RAIL_W};
use gpui::{Context, Entity, FocusHandle, Pixels, SharedString, Subscription, Window};

use super::Workspace;
use crate::bottom_panel::BottomPanel;
use crate::debugger::Debugger;
use crate::status_bar::StatusBar;

/// One of the three floating panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FramedPanel {
    /// The sidebar (whichever view it shows).
    Sidebar,
    /// The editor panel with all its split panes.
    #[default]
    Editor,
    /// The bottom panel (terminal, problems, todo, debug).
    Bottom,
}

/// Where keyboard focus is, as seen by the panel wrappers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanelFocus {
    /// The sidebar wrapper contains the focused element.
    pub sidebar: bool,
    /// The editor panel contains it.
    pub editor: bool,
    /// The bottom panel contains it.
    pub bottom: bool,
}

/// The panel to frame. A panel holding focus wins; while focus is outside
/// every panel (an overlay, the palette, the workspace root) the previous
/// panel keeps the frame, so overlays never steal it. A frame on a panel
/// that has been hidden falls back to the editor: exactly one panel is
/// framed at a time.
pub fn framed_panel(
    previous: FramedPanel,
    focus: PanelFocus,
    sidebar_shown: bool,
    bottom_shown: bool,
) -> FramedPanel {
    if focus.sidebar && sidebar_shown {
        return FramedPanel::Sidebar;
    }
    if focus.bottom && bottom_shown {
        return FramedPanel::Bottom;
    }
    if focus.editor {
        return FramedPanel::Editor;
    }
    match previous {
        FramedPanel::Sidebar if !sidebar_shown => FramedPanel::Editor,
        FramedPanel::Bottom if !bottom_shown => FramedPanel::Editor,
        other => other,
    }
}

/// The sidebar width that puts the centre of the gap to its right under
/// the pointer at `x` (the sidebar panel starts at the rail's right edge).
pub fn sidebar_width_at(x: Pixels, min: Pixels, max: Pixels) -> Pixels {
    (x - RAIL_W - GAP / 2.).clamp(min, max)
}

/// The bottom panel height that puts the centre of the gap above it under
/// the pointer at `y`; the panel ends at the footer. At most 75 % of the
/// window, never below `min`.
pub fn bottom_height_at(y: Pixels, viewport_height: Pixels, min: Pixels) -> Pixels {
    let bottom = viewport_height - FOOTER_H;
    let max = (viewport_height * 0.75).max(min);
    (bottom - y - GAP / 2.).clamp(min, max)
}

/// Total width the sidebar takes in the body row: the panel plus the gap
/// to its right (both slide together, so nothing jumps while animating).
pub fn sidebar_slot(width: Pixels) -> Pixels {
    width + GAP
}

/// Total height the bottom panel takes in the editor column: the panel
/// plus the gap above it.
pub fn bottom_slot(height: Pixels) -> Pixels {
    height + GAP
}

/// The gap is also the hit area of a resize handle.
pub const HANDLE: Pixels = GAP;

/// Shell state of a workspace: the focus handles of the panel wrappers,
/// the framed panel and the paused location shown in the title bar.
pub struct Shell {
    /// Tracked by the sidebar panel.
    pub sidebar_focus: FocusHandle,
    /// Tracked by the editor panel.
    pub editor_focus: FocusHandle,
    /// Tracked by the bottom panel.
    pub bottom_focus: FocusHandle,
    /// The panel framed in the last render.
    pub framed: FramedPanel,
    /// `Some` while the debuggee is paused: the location (`main.rs:11`),
    /// empty when the adapter gave none.
    pub paused: Option<SharedString>,
    /// The bottom panel is hidden: the footer shows the last run then.
    pub panel_hidden: Rc<Cell<bool>>,
    _debugger: Subscription,
}

impl Shell {
    /// Creates the focus handles, follows `debugger`'s paused state (the
    /// workspace redraws only when that state changes, not for every
    /// console line) and gives the footer the last run of `bottom`'s
    /// terminals.
    pub fn new(
        debugger: &Entity<Debugger>,
        footer: &Entity<StatusBar>,
        bottom: &Entity<BottomPanel>,
        cx: &mut Context<Workspace>,
    ) -> Self {
        let panel_hidden = Rc::new(Cell::new(true));
        let terminals = bottom.read(cx).terminals().clone();
        let hidden = panel_hidden.clone();
        footer.update(cx, |bar, _| bar.set_run_source(terminals, hidden));
        Self {
            sidebar_focus: cx.focus_handle(),
            editor_focus: cx.focus_handle(),
            bottom_focus: cx.focus_handle(),
            framed: FramedPanel::Editor,
            paused: None,
            panel_hidden,
            _debugger: cx.observe(debugger, |ws, debugger, cx| {
                let paused = paused_readout(debugger.read(cx));
                if ws.shell.paused != paused {
                    ws.shell.paused = paused;
                    cx.notify();
                }
            }),
        }
    }

    /// Works out the framed panel for this frame (from the last frame's
    /// focus, so only focus changes move the frame) and remembers it.
    pub fn update_frame(
        &mut self,
        sidebar_shown: bool,
        bottom_shown: bool,
        window: &Window,
        cx: &gpui::App,
    ) -> FramedPanel {
        self.panel_hidden.set(!bottom_shown);
        let focus = PanelFocus {
            sidebar: self.sidebar_focus.contains_focused(window, cx),
            editor: self.editor_focus.contains_focused(window, cx),
            bottom: self.bottom_focus.contains_focused(window, cx),
        };
        self.framed = framed_panel(self.framed, focus, sidebar_shown, bottom_shown);
        self.framed
    }
}

impl Workspace {
    /// The debugger's paused location while it is paused (see
    /// [`Shell::paused`]).
    pub fn paused_readout(&self) -> Option<&SharedString> {
        self.shell.paused.as_ref()
    }

    /// Changed files of the active repository by kind, for the title bar
    /// and the rail badge.
    pub fn git_counts(&self, cx: &gpui::App) -> crate::title_bar::GitCounts {
        let store = self.git.store.read(cx);
        crate::title_bar::GitCounts::of(&store.snapshot().entries)
    }

    /// Changed files of the active repository (the rail badge).
    pub(super) fn git_change_count(&self, cx: &gpui::App) -> usize {
        self.git_counts(cx).total()
    }

    /// Whether the active pane shows the Settings page (the title bar's
    /// gear turns `accent` then).
    pub fn settings_tab_active(&self, cx: &gpui::App) -> bool {
        self.active_pane()
            .read(cx)
            .active_page()
            .is_some_and(|page| page.key.as_ref() == crate::settings_ui::PAGE_KEY)
    }
}

/// The paused readout of `debugger`: `Some("main.rs:11")` while its
/// session is stopped, `Some("")` when stopped without a location.
fn paused_readout(debugger: &Debugger) -> Option<SharedString> {
    let stopped = debugger
        .session()
        .is_some_and(|s| s.is_stopped() && !s.is_terminated());
    stopped.then(|| {
        debugger
            .stopped_at()
            .map(|(path, line)| location_label(path, *line))
            .unwrap_or_default()
            .into()
    })
}

/// `main.rs:11` for a zero-based line.
pub fn location_label(path: &Path, line: usize) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    format!("{name}:{}", line + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    const NONE: PanelFocus = PanelFocus {
        sidebar: false,
        editor: false,
        bottom: false,
    };

    #[test]
    fn the_focused_panel_is_framed() {
        let at = |sidebar, editor, bottom| PanelFocus {
            sidebar,
            editor,
            bottom,
        };
        let prev = FramedPanel::Editor;
        assert_eq!(
            framed_panel(prev, at(true, false, false), true, true),
            FramedPanel::Sidebar
        );
        assert_eq!(
            framed_panel(prev, at(false, false, true), true, true),
            FramedPanel::Bottom
        );
        assert_eq!(
            framed_panel(FramedPanel::Bottom, at(false, true, false), true, true),
            FramedPanel::Editor
        );
    }

    #[test]
    fn overlays_do_not_steal_the_frame() {
        // Focus in the palette (outside every panel): the last panel stays.
        for prev in [
            FramedPanel::Sidebar,
            FramedPanel::Editor,
            FramedPanel::Bottom,
        ] {
            assert_eq!(framed_panel(prev, NONE, true, true), prev);
        }
    }

    #[test]
    fn hidden_panels_hand_the_frame_to_the_editor() {
        assert_eq!(
            framed_panel(FramedPanel::Sidebar, NONE, false, true),
            FramedPanel::Editor
        );
        assert_eq!(
            framed_panel(FramedPanel::Bottom, NONE, true, false),
            FramedPanel::Editor
        );
        // Stale focus inside a panel that is sliding closed does not count.
        let focus = PanelFocus {
            bottom: true,
            ..NONE
        };
        assert_eq!(
            framed_panel(FramedPanel::Bottom, focus, true, false),
            FramedPanel::Editor
        );
    }

    #[test]
    fn sidebar_resize_follows_the_gap_centre() {
        let (min, max) = (px(160.), px(600.));
        // Pointer in the middle of the gap right of a 240 px sidebar.
        let x = RAIL_W + px(240.) + GAP / 2.;
        assert_eq!(sidebar_width_at(x, min, max), px(240.));
        assert_eq!(sidebar_width_at(px(10.), min, max), min);
        assert_eq!(sidebar_width_at(px(5000.), min, max), max);
    }

    #[test]
    fn bottom_resize_follows_the_gap_centre_and_stops_at_the_footer() {
        let min = px(80.);
        let viewport = px(800.);
        // Gap centre above a 228 px panel that ends at the footer.
        let y = viewport - FOOTER_H - px(228.) - GAP / 2.;
        assert_eq!(bottom_height_at(y, viewport, min), px(228.));
        assert_eq!(bottom_height_at(px(0.), viewport, min), px(600.));
        assert_eq!(bottom_height_at(px(790.), viewport, min), min);
        // A tiny window still allows the minimum.
        assert_eq!(bottom_height_at(px(0.), px(90.), min), min);
    }

    #[test]
    fn paused_location_is_one_based() {
        let path = Path::new("/p/src/main.rs");
        assert_eq!(location_label(path, 10), "main.rs:11");
        assert_eq!(location_label(Path::new("/"), 0), "/:1");
    }

    #[test]
    fn slots_include_the_gap() {
        assert_eq!(sidebar_slot(px(240.)), px(246.));
        assert_eq!(bottom_slot(px(228.)), px(234.));
        assert_eq!(HANDLE, GAP);
    }
}
