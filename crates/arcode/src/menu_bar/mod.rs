//! The menu bar: File, Edit, Selection, View, Go, Run, Terminal, Git and
//! Help, in the title bar row after the logo (Linux and Windows). Every
//! item runs a palette command and shows its key binding. A click opens a
//! menu; while one is open, hovering another title opens that one. Alt
//! activates the bar and Alt+F (and the other underlined letters) opens a
//! menu, unless the focused view uses that key. In a narrow window, or with
//! `ui.menu_bar = false`, the menus collapse into one menu button.
//!
//! On macOS the same menus are the application's native menus
//! ([`native::menus`]) and the bar is not drawn.

pub mod alt_tap;
pub mod model;
pub mod native;
mod place;
mod resolve;
pub mod rows;
pub mod state;
mod view;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use arcode_config::SettingEdit;
use arcode_ui_kit::settings;
use gpui::{
    Action, App, Bounds, Context, FocusHandle, Focusable, KeyBinding, Pixels, ScrollHandle,
    Subscription, WeakEntity, Window, actions, px,
};

use crate::workspace::Workspace;
use alt_tap::AltTap;
use model::Facts;
use rows::RowKind;
use state::{NavKey, OpenMenu, Outcome};

pub use view::collapse_width;

actions!(
    menu_bar,
    [
        /// Activates the menu bar (a tap on Alt), or closes the open menu.
        ToggleMenuBar,
        /// Selects the previous item.
        SelectPrevious,
        /// Selects the next item.
        SelectNext,
        /// Opens the menu to the left, or closes a submenu.
        SelectLeft,
        /// Opens a submenu, or the menu to the right.
        SelectRight,
        /// Selects the first item.
        SelectFirst,
        /// Selects the last item.
        SelectLast,
        /// Runs the selected item.
        Confirm,
        /// Closes a submenu, or the menu.
        Dismiss,
        /// Stands in for a native menu item that does not apply right now.
        /// Nothing handles it, so the item is greyed.
        Unavailable,
    ]
);

/// Key context of the menu bar while a menu is open.
pub const CONTEXT: &str = "MenuBar";

/// Key bindings: the menu keys, and a tap on Alt to activate the bar
/// (Linux and Windows; macOS has native menus).
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("left", SelectLeft, ctx),
        KeyBinding::new("right", SelectRight, ctx),
        KeyBinding::new("home", SelectFirst, ctx),
        KeyBinding::new("pageup", SelectFirst, ctx),
        KeyBinding::new("end", SelectLast, ctx),
        KeyBinding::new("pagedown", SelectLast, ctx),
        KeyBinding::new("enter", Confirm, ctx),
        KeyBinding::new("space", Confirm, ctx),
        KeyBinding::new("escape", Dismiss, ctx),
    ]);
    if !cfg!(target_os = "macos") {
        cx.bind_keys([KeyBinding::new(
            "alt",
            ToggleMenuBar,
            Some(crate::actions::WORKSPACE_CONTEXT),
        )]);
    }
}

/// What the workspace tells the bar when a menu opens.
#[derive(Clone, Debug, Default)]
pub struct MenuFacts {
    pub facts: Facts,
    /// The open project, left out of the recent projects.
    pub project_root: Option<PathBuf>,
    /// The active editor: its key bindings are shown for editor commands
    /// when focus is elsewhere (the file tree, a panel).
    pub editor_focus: Option<FocusHandle>,
}

/// How a menu is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum How {
    /// Menu `title` of the bar; `keyboard` selects its first item and
    /// underlines the mnemonics.
    Title { title: usize, keyboard: bool },
    /// The bar only, its first title highlighted (a tap on Alt).
    Bar,
    /// The menu button's list of menus.
    List { keyboard: bool },
}

/// The menu bar of a window.
pub struct MenuBar {
    focus: FocusHandle,
    /// Where focus was when the menu opened: commands run there.
    return_focus: Option<FocusHandle>,
    /// Focused when there was no focus to return to.
    fallback_focus: FocusHandle,
    workspace: WeakEntity<Workspace>,
    open: Option<OpenMenu>,
    /// Drawn as the single menu button (narrow window or `ui.menu_bar`
    /// off).
    collapsed: bool,
    /// Where the titles (or the menu button) were drawn last, in window
    /// coordinates: the mouse over a title switches menus.
    titles: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    /// The scroll position of each open popup (a long menu in a short
    /// window scrolls).
    scroll: Vec<ScrollHandle>,
    /// Scroll the selected row of the popups from this level on into view
    /// at the next frame (after a key, or when a popup opens).
    reveal_from: Option<usize>,
    /// Whether Alt going up is a tap (see [`AltTap`]).
    pub(crate) alt: AltTap,
    _blur: Subscription,
}

impl Focusable for MenuBar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MenuBar {
    /// A closed menu bar for `workspace`, whose root focus handle is
    /// `fallback_focus`.
    pub fn new(
        workspace: WeakEntity<Workspace>,
        fallback_focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        // Focus moving anywhere else (a click into the editor) closes it.
        let blur = cx.on_blur(&focus, window, |bar, _, cx| {
            if bar.open.take().is_some() {
                bar.return_focus = None;
                cx.notify();
            }
        });
        MenuBar {
            focus,
            return_focus: None,
            fallback_focus,
            workspace,
            open: None,
            collapsed: false,
            titles: Rc::default(),
            scroll: Vec::new(),
            reveal_from: None,
            alt: AltTap::default(),
            _blur: blur,
        }
    }

    /// Whether a menu (or the bar) is active.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The open menu, for tests and the view.
    pub fn open_menu(&self) -> Option<&OpenMenu> {
        self.open.as_ref()
    }

    /// Whether the menus are collapsed into the menu button.
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// A tap on Alt: activates the bar (the menu button's list when
    /// collapsed), or closes what is open.
    pub fn toggle(&mut self, facts: MenuFacts, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_some() {
            self.close(window, cx);
        } else if self.collapsed {
            self.open_with(How::List { keyboard: true }, facts, window, cx);
        } else {
            self.open_with(How::Bar, facts, window, cx);
        }
    }

    /// Alt+`key`: opens the menu with that mnemonic. Returns whether one
    /// has it.
    pub fn open_mnemonic(
        &mut self,
        key: char,
        facts: MenuFacts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(title) = model::menu_for_mnemonic(key) else {
            return false;
        };
        self.open_with(
            How::Title {
                title,
                keyboard: true,
            },
            facts,
            window,
            cx,
        );
        true
    }

    fn open_with(
        &mut self,
        how: How,
        facts: MenuFacts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = &mut self.open {
            // Already open: the rows stay as built for where focus was.
            match how {
                How::Title { title, keyboard } if !open.collapsed => {
                    open.keyboard |= keyboard;
                    open.open_title(title, keyboard);
                }
                How::Title { title, .. } => open.open_title(title, true),
                How::Bar | How::List { .. } => {}
            }
            self.reveal_from = Some(0);
            cx.notify();
            return;
        }
        self.return_focus = window.focused(cx).filter(|f| *f != self.focus);
        let root = resolve::build_root(&facts, self.return_focus.as_ref(), window, cx);
        let menu = match how {
            How::Title { title, keyboard } => OpenMenu::new(root, self.collapsed, title, keyboard),
            How::Bar => OpenMenu::bar(root, 0),
            How::List { keyboard } => OpenMenu::list(root, keyboard),
        };
        self.open = Some(menu);
        self.reveal_from = Some(0);
        window.focus(&self.focus);
        cx.notify();
    }

    /// Opens (from the mouse) menu `title` of the bar, or closes it when it
    /// is the one open.
    fn click_title(&mut self, title: usize, window: &mut Window, cx: &mut Context<Self>) {
        let same = self
            .open
            .as_ref()
            .is_some_and(|o| !o.bar_only && !o.collapsed && o.title == title);
        if same {
            self.close(window, cx);
            return;
        }
        let facts = self.facts(cx);
        self.open_with(
            How::Title {
                title,
                keyboard: false,
            },
            facts,
            window,
            cx,
        );
    }

    /// Opens (from the mouse) the menu button's list, or closes it.
    fn click_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_some() {
            self.close(window, cx);
            return;
        }
        let facts = self.facts(cx);
        self.open_with(How::List { keyboard: false }, facts, window, cx);
    }

    fn facts(&self, cx: &App) -> MenuFacts {
        self.workspace
            .upgrade()
            .map(|ws| ws.read(cx).menu_facts(cx))
            .unwrap_or_default()
    }

    /// Closes the menu and gives focus back.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.take().is_none() {
            return;
        }
        let focus = self
            .return_focus
            .take()
            .unwrap_or_else(|| self.fallback_focus.clone());
        if self.focus.is_focused(window) {
            window.focus(&focus);
        }
        cx.notify();
    }

    fn nav(&mut self, key: NavKey, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        let outcome = open.key(key);
        self.reveal_from = Some(0);
        self.apply(outcome, window, cx);
    }

    fn apply(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        match outcome {
            Outcome::Moved => cx.notify(),
            Outcome::Close => self.close(window, cx),
            Outcome::Pick { level, row } => self.pick(level, row, window, cx),
        }
    }

    /// Runs row `row` of popup `level` after closing the menu.
    fn pick(&mut self, level: usize, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.open.as_ref().and_then(|o| o.row(level, row)) else {
            return;
        };
        if !row.selectable() {
            return;
        }
        enum Then {
            Run(Box<dyn Action>),
            Recent(PathBuf),
            Theme(String),
        }
        let then = match &row.kind {
            RowKind::Run(action) => Then::Run(action.boxed_clone()),
            RowKind::OpenRecent(path) => Then::Recent(path.clone()),
            RowKind::SetTheme(value) => Then::Theme(value.to_string()),
            RowKind::Sub(_) | RowKind::Separator => return,
        };
        self.close(window, cx);
        let workspace = self.workspace.clone();
        match then {
            // Focus is back where the menu opened, so the action runs there.
            Then::Run(action) => window.dispatch_action(action, cx),
            Then::Recent(path) => window.defer(cx, move |window, cx| {
                workspace
                    .update(cx, |ws, cx| ws.open_project(path, window, cx))
                    .ok();
            }),
            Then::Theme(value) => {
                let edits = vec![SettingEdit::set("ui.theme", value)];
                crate::prefs::store::write_and_report(edits, workspace, cx);
            }
        }
    }

    /// A letter or digit while a menu is open (with Alt: the menu with that
    /// mnemonic).
    fn on_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &event.keystroke;
        let m = ks.modifiers;
        if m.control || m.platform || m.function || self.open.is_none() {
            return;
        }
        let mut chars = ks.key.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return;
        };
        if !ch.is_ascii_alphanumeric() {
            return;
        }
        let ch = ch.to_ascii_lowercase();
        cx.stop_propagation();
        if m.alt {
            if let Some(title) = model::menu_for_mnemonic(ch)
                && let Some(open) = &mut self.open
            {
                open.keyboard = true;
                open.open_title(title, true);
                self.reveal_from = Some(0);
                cx.notify();
            }
            return;
        }
        if let Some(open) = &mut self.open {
            open.keyboard = true;
        }
        self.nav(NavKey::Char(ch), window, cx);
    }
}

/// Builds the native menus and installs them (macOS).
pub fn install_native_menus(cx: &mut App) {
    let facts = Facts::default();
    cx.set_global(NativeFacts(facts));
    let menus = native::menus(facts, cx);
    cx.set_menus(menus);
}

/// The facts the installed native menus were built for.
struct NativeFacts(Facts);

impl gpui::Global for NativeFacts {}

/// Rebuilds the native menus when the facts of the active window changed,
/// so items that do not apply (Step Over without a debug session) are
/// greyed (macOS).
pub fn sync_native_menus(facts: Facts, cx: &mut App) {
    if cx
        .try_global::<NativeFacts>()
        .is_some_and(|installed| installed.0 == facts)
    {
        return;
    }
    cx.set_global(NativeFacts(facts));
    cx.defer(move |cx| {
        let menus = native::menus(facts, cx);
        cx.set_menus(menus);
    });
}

/// Whether the menus fold into the menu button: `ui.menu_bar` is off or
/// the window is narrower than [`collapse_width`] at the UI size.
pub fn folds(window: &Window, cx: &App) -> bool {
    let ui = &settings(cx).ui;
    !ui.menu_bar || window.viewport_size().width < px(collapse_width(ui.font_size))
}
