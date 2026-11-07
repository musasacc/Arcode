//! The welcome screen shown when no project is open (docs/DESIGN.md
//! 6.15): the `</>` logo and the ways in (Open Folder, New Project, Clone
//! Repository) on the left with the recent projects under them, "Get
//! going" shortcuts on the right.
//!
//! Pinned projects come first. With more than [`list::FILTER_AFTER`]
//! projects a filter field sits at the top of the list; typing anywhere on
//! the screen goes into it, Up / Down / Enter pick and open a project and
//! Escape clears the filter. Whether each folder still exists is checked
//! once on the background executor when the list is first drawn (and when
//! it changes), never while drawing; a missing one is shown dimmed.

pub mod list;
mod menu;
mod render;
mod reveal;
mod row;

use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, KeyBinding, KeyDownEvent, Pixels,
    ScrollHandle, Subscription, Task, WeakEntity, Window, actions,
};

use crate::app_state;
use crate::text_field::{TextChanged, TextField};
use crate::workspace::{ToastLevel, Workspace};
pub use list::{relative_time, tilde_path};

actions!(
    welcome,
    [
        /// Selects the next project in the list.
        SelectNext,
        /// Selects the previous project in the list.
        SelectPrevious,
        /// Opens the selected project (else the first match).
        OpenSelected,
        /// Clears the filter, else the selection.
        ClearFilter,
    ]
);

/// Key context of the welcome screen.
pub const CONTEXT: &str = "Welcome";

/// Registers the list keys.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("up", SelectPrevious, ctx),
        KeyBinding::new("enter", OpenSelected, ctx),
        KeyBinding::new("escape", ClearFilter, ctx),
    ]);
}

/// The welcome screen's state.
pub struct Welcome {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    filter: Entity<TextField>,
    /// Index into the visible (filtered) rows.
    selected: Option<usize>,
    /// The row context menu, while open.
    menu: Option<menu::RowMenu>,
    /// The screen's scroll position.
    scroll: ScrollHandle,
    /// The selection moved by key: scroll its row into view when drawn.
    reveal: Rc<Cell<bool>>,
    /// Listed folders found missing by the last check.
    missing: HashSet<PathBuf>,
    /// The paths the last existence check covered.
    checked: Option<Vec<PathBuf>>,
    _check: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Focusable for Welcome {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Seconds since the Unix epoch now.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Welcome {
    /// Creates the welcome screen of `workspace`'s window.
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextField::new("Filter projects", cx));
        let sub = cx.subscribe(&filter, |welcome: &mut Self, _, _: &TextChanged, cx| {
            welcome.selected = None;
            cx.notify();
        });
        Self {
            workspace,
            focus_handle: cx.focus_handle(),
            filter,
            selected: None,
            menu: None,
            scroll: ScrollHandle::new(),
            reveal: Rc::default(),
            missing: HashSet::new(),
            checked: None,
            _check: None,
            _subs: vec![sub],
        }
    }

    /// Takes the keyboard while the welcome screen shows and focus is
    /// nowhere in particular (the window root, or a pane that is not
    /// drawn), so typing reaches the filter. `stray` lists those handles.
    pub fn claim_focus(&self, stray: &[FocusHandle], window: &mut Window, cx: &App) {
        let claim = match window.focused(cx) {
            None => true,
            Some(focused) => stray.contains(&focused),
        };
        if claim {
            window.focus(&self.focus_handle);
        }
    }

    /// Forgets the last existence check, so the next time the list is
    /// drawn the folders are checked again (the screen is shown anew,
    /// with an empty filter).
    pub fn recheck(&mut self, cx: &mut Context<Self>) {
        self.checked = None;
        self.menu = None;
        self.selected = None;
        self.clear_filter_text(cx);
        cx.notify();
    }

    /// Empties the filter field.
    fn clear_filter_text(&mut self, cx: &mut Context<Self>) {
        if !self.filter_text(cx).is_empty() {
            self.filter.update(cx, |f, cx| f.set_text("", cx));
        }
    }

    /// After the list got shorter: a filter that is no longer shown is
    /// emptied, so it cannot hide the projects left.
    fn list_shrunk(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.selected = None;
        if !self.shows_filter(cx) {
            self.clear_filter_text(cx);
        }
        cx.notify();
    }

    /// Every row in display order, and the indices the filter keeps.
    fn rows(&self, cx: &App) -> (Vec<list::Entry>, Vec<usize>) {
        let home = std::env::home_dir();
        let rows = list::entries(
            app_state::recent_projects(cx),
            &self.missing,
            home.as_deref(),
            now_secs(),
        );
        let query = if self.shows_filter(cx) {
            self.filter_text(cx)
        } else {
            ""
        };
        let visible = list::filter(&rows, query);
        (rows, visible)
    }

    /// The names of the rows shown, in order (tests).
    pub fn visible_names(&self, cx: &App) -> Vec<String> {
        let (rows, visible) = self.rows(cx);
        visible.into_iter().map(|i| rows[i].name.clone()).collect()
    }

    /// The filter's text.
    pub fn filter_text<'a>(&self, cx: &'a App) -> &'a str {
        self.filter.read(cx).text()
    }

    /// Whether the keyboard is in the filter field.
    pub fn filter_focused(&self, window: &Window, cx: &App) -> bool {
        self.filter.focus_handle(cx).is_focused(window)
    }

    /// Whether the filter field is shown.
    pub fn shows_filter(&self, cx: &App) -> bool {
        app_state::recent_projects(cx).len() > list::FILTER_AFTER
    }

    /// Replaces the filter's text (tests, scripting).
    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_text(text, cx));
        self.selected = None;
        cx.notify();
    }

    /// The selected row's project.
    pub fn selected_path(&self, cx: &App) -> Option<PathBuf> {
        let (rows, visible) = self.rows(cx);
        let ix = *visible.get(self.selected?)?;
        Some(rows[ix].path.clone())
    }

    /// How far the screen is scrolled down (tests).
    pub fn scroll_top(&self) -> Pixels {
        -self.scroll.offset().y
    }

    /// Whether the last check found `path` missing.
    pub fn is_missing(&self, path: &Path) -> bool {
        self.missing.contains(path)
    }

    /// Checks on the background executor which listed folders exist,
    /// unless the list is the one checked last.
    fn ensure_checked(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = app_state::recent_projects(cx)
            .iter()
            .map(|p| p.path.clone())
            .collect();
        if self.checked.as_ref() == Some(&paths) {
            return;
        }
        self.checked = Some(paths.clone());
        let check = cx.background_spawn(async move {
            paths
                .into_iter()
                .filter(|p| !p.is_dir())
                .collect::<HashSet<_>>()
        });
        self._check = Some(cx.spawn(async move |this, cx| {
            let missing = check.await;
            this.update(cx, |welcome, cx| {
                if welcome.missing != missing {
                    welcome.missing = missing;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Opens `path` here (or in a new window). A folder that is gone is
    /// marked missing and explained instead; one that came back opens.
    pub fn open(
        &mut self,
        path: PathBuf,
        new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        // Looked at again: a drive may have been mounted since the check.
        if !path.is_dir() {
            self.missing.insert(path.clone());
            let shown = tilde_path(&path, std::env::home_dir().as_deref());
            let message = format!("{shown} no longer exists.");
            self.workspace
                .update(cx, |ws, cx| {
                    ws.show_toast_as(message, ToastLevel::Warning, cx)
                })
                .ok();
            cx.notify();
            return;
        }
        if self.missing.remove(&path) {
            cx.notify();
        }
        if new_window {
            crate::windows::open_project_in_new_window(path, cx);
        } else {
            self.workspace
                .update(cx, |ws, cx| ws.open_project(path, window, cx))
                .ok();
        }
    }

    /// Pins `path` to the top, or unpins it.
    pub fn toggle_pin(&mut self, path: &Path, cx: &mut Context<Self>) {
        let pinned = app_state::recent_projects(cx)
            .iter()
            .any(|p| p.path == path && p.pinned);
        app_state::set_pinned(path, !pinned, cx);
        self.menu = None;
        self.selected = None;
        cx.notify();
    }

    /// Removes `path` from the list (the folder is left alone).
    pub fn remove(&mut self, path: &Path, cx: &mut Context<Self>) {
        app_state::remove_recent(path, cx);
        self.missing.remove(path);
        self.list_shrunk(cx);
    }

    /// Removes every project that is not pinned from the list.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        app_state::clear_recent(cx);
        self.list_shrunk(cx);
    }

    fn select_by(&mut self, delta: isize, cx: &mut Context<Self>) {
        let (_, visible) = self.rows(cx);
        self.selected = list::step(self.selected, visible.len(), delta);
        self.reveal.set(self.selected.is_some());
        self.menu = None;
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.select_by(1, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.select_by(-1, cx);
    }

    fn open_selected(&mut self, _: &OpenSelected, window: &mut Window, cx: &mut Context<Self>) {
        let (rows, visible) = self.rows(cx);
        let pick = match self.selected {
            Some(ix) => visible.get(ix).copied(),
            // With a filter, Enter opens the first match.
            None if !self.filter_text(cx).trim().is_empty() => visible.first().copied(),
            None => None,
        };
        match pick {
            Some(ix) => self.open(rows[ix].path.clone(), false, window, cx),
            None => cx.propagate(),
        }
    }

    fn clear_filter(&mut self, _: &ClearFilter, window: &mut Window, cx: &mut Context<Self>) {
        // An open menu closes first.
        if self.menu.take().is_none() {
            if !self.filter_text(cx).is_empty() {
                self.filter.update(cx, |f, cx| f.set_text("", cx));
            } else if self.selected.is_some() {
                self.selected = None;
            } else if self.filter.focus_handle(cx).is_focused(window) {
                window.focus(&self.focus_handle);
            } else {
                cx.propagate();
                return;
            }
        }
        cx.notify();
    }

    /// Typing a character while the filter is shown but not focused moves
    /// focus there and starts the query with it.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = &keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function || !self.shows_filter(cx) {
            return;
        }
        let focus = self.filter.focus_handle(cx);
        if focus.is_focused(window) {
            return;
        }
        let Some(text) = keystroke.key_char.as_deref() else {
            return;
        };
        if text.is_empty() || text.chars().any(char::is_control) {
            return;
        }
        let query = format!("{}{text}", self.filter_text(cx));
        self.filter.update(cx, |f, cx| f.set_text(&query, cx));
        window.focus(&focus);
        self.selected = None;
        cx.stop_propagation();
        cx.notify();
    }
}
