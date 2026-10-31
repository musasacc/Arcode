//! The bottom panel: tabs for Terminal, Problems, Todo and Debug.
//!
//! Terminal, Problems, Todo and Debug show [`TerminalPanel`],
//! [`ProblemsPanel`], [`TodoView`] and [`DebugPanel`].

use std::path::PathBuf;

use arcode_ui_kit::{IconName, icon, theme};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::debugger::panel::DebugPanel;
use crate::lsp_bridge::LspBridge;
use crate::problems::ProblemsPanel;
use crate::terminal_panel::TerminalPanel;
use crate::todo::{TodoStore, TodoView};

mod header;

/// Default panel height.
pub const DEFAULT_HEIGHT: f32 = 240.;
/// Smallest height when resizing.
pub const MIN_HEIGHT: f32 = 80.;

/// A bottom panel tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomTab {
    /// Integrated terminal.
    Terminal,
    /// Diagnostics list.
    Problems,
    /// TODO/FIXME list.
    Todo,
    /// Debug console.
    Debug,
}

impl BottomTab {
    /// Every tab in display order.
    pub const ALL: [BottomTab; 4] = [
        BottomTab::Terminal,
        BottomTab::Problems,
        BottomTab::Todo,
        BottomTab::Debug,
    ];

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            BottomTab::Terminal => "Terminal",
            BottomTab::Problems => "Problems",
            BottomTab::Todo => "Todo",
            BottomTab::Debug => "Debug",
        }
    }

    /// Tab icon.
    pub fn icon(self) -> IconName {
        match self {
            BottomTab::Terminal => IconName::Terminal,
            BottomTab::Problems => IconName::CircleAlert,
            BottomTab::Todo => IconName::ListTodo,
            BottomTab::Debug => IconName::Bug,
        }
    }
}

/// Events emitted by the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BottomPanelEvent {
    /// The active tab changed.
    TabChanged(BottomTab),
}

/// The bottom panel view.
pub struct BottomPanel {
    focus_handle: FocusHandle,
    active: BottomTab,
    terminals: Entity<TerminalPanel>,
    problems: Entity<ProblemsPanel>,
    /// The TODO list and its store (set by the workspace).
    todo: Option<(Entity<TodoStore>, Entity<TodoView>, Subscription)>,
    /// The Debug tab (set by the workspace).
    debug: Option<Entity<DebugPanel>>,
    /// Whether the workspace frames the panel (the ember mark on the
    /// active tab); `None` until told, then focus decides.
    framed: Option<bool>,
    /// The header is wide enough for the tab labels (measured when drawn).
    tab_labels: bool,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<BottomPanelEvent> for BottomPanel {}

impl Focusable for BottomPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BottomPanel {
    /// Creates the panel showing the Terminal tab; the Problems tab lists
    /// `bridge`'s diagnostics.
    pub fn new(bridge: Entity<LspBridge>, cx: &mut Context<Self>) -> Self {
        let terminals = cx.new(TerminalPanel::new);
        let problems = cx.new(|cx| ProblemsPanel::new(bridge, cx));
        let subscriptions = [
            cx.observe(&terminals, |_, _, cx| cx.notify()),
            cx.observe(&problems, |_, _, cx| cx.notify()),
        ];
        Self {
            focus_handle: cx.focus_handle(),
            active: BottomTab::Terminal,
            terminals,
            problems,
            todo: None,
            debug: None,
            framed: None,
            tab_labels: true,
            _subscriptions: subscriptions,
        }
    }

    /// The Terminal tab content.
    pub fn terminals(&self) -> &Entity<TerminalPanel> {
        &self.terminals
    }

    /// The Problems tab content.
    pub fn problems(&self) -> &Entity<ProblemsPanel> {
        &self.problems
    }

    /// Shows `view` (over `store`) in the Todo tab; the tab counts the
    /// store's items.
    pub fn set_todo(
        &mut self,
        store: Entity<TodoStore>,
        view: Entity<TodoView>,
        cx: &mut Context<Self>,
    ) {
        let sub = cx.observe(&store, |_, _, cx| cx.notify());
        self.todo = Some((store, view, sub));
        cx.notify();
    }

    /// Shows `panel` in the Debug tab.
    pub fn set_debug(&mut self, panel: Entity<DebugPanel>, cx: &mut Context<Self>) {
        self.debug = Some(panel);
        cx.notify();
    }

    /// Sets whether the panel wears the focus frame (read at the next
    /// render, which follows in the same frame: no notify).
    pub fn set_framed(&mut self, framed: bool) {
        self.framed = Some(framed);
    }

    /// The Debug tab content, once set.
    pub fn debug(&self) -> Option<&Entity<DebugPanel>> {
        self.debug.as_ref()
    }

    /// Shows the Terminal tab and runs `command` in the terminal named
    /// `title` (a new one in `cwd` unless an idle one has that name).
    pub fn run_named(
        &mut self,
        title: &str,
        command: &str,
        cwd: Option<PathBuf>,
        on_started: Option<crate::terminal_panel::OnStarted>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_active_tab(BottomTab::Terminal, cx);
        self.terminals.update(cx, |t, cx| {
            t.run_in_named(title, command, cwd, on_started, window, cx)
        });
    }

    /// Sets the project root: terminals start there and problem paths are
    /// shown relative to it.
    pub fn set_project_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.terminals
            .update(cx, |t, cx| t.set_cwd(root.clone(), cx));
        self.problems.update(cx, |p, cx| p.set_root(root, cx));
    }

    /// The active tab.
    pub fn active_tab(&self) -> BottomTab {
        self.active
    }

    /// Switches to `tab`.
    pub fn set_active_tab(&mut self, tab: BottomTab, cx: &mut Context<Self>) {
        if self.active != tab {
            self.active = tab;
            cx.emit(BottomPanelEvent::TabChanged(tab));
            cx.notify();
        }
    }

    /// Shows the Terminal tab and runs `command` in the active terminal
    /// (starting one if needed), focusing it.
    pub fn run_command(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active_tab(BottomTab::Terminal, cx);
        self.terminals
            .update(cx, |t, cx| t.run_command(command, window, cx));
    }

    /// Shows the Terminal tab with a new terminal and focuses it.
    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active_tab(BottomTab::Terminal, cx);
        self.terminals.update(cx, |t, cx| {
            t.new_terminal(window, cx);
            t.focus(window, cx);
        });
    }

    /// Shows the Terminal tab with a new terminal started in `dir` and
    /// focuses it.
    pub fn new_terminal_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active_tab(BottomTab::Terminal, cx);
        self.terminals.update(cx, |t, cx| {
            t.new_terminal_in(Some(dir), window, cx);
            t.focus(window, cx);
        });
    }

    /// Focuses the active tab's content; the Terminal tab starts a
    /// terminal if none is running.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active {
            BottomTab::Terminal => self.terminals.update(cx, |t, cx| t.focus(window, cx)),
            BottomTab::Problems => window.focus(&self.problems.focus_handle(cx)),
            BottomTab::Todo => match &self.todo {
                Some((_, view, _)) => window.focus(&view.focus_handle(cx)),
                None => window.focus(&self.focus_handle),
            },
            BottomTab::Debug => match &self.debug {
                Some(debug) => debug.read(cx).focus_repl(window, cx),
                None => window.focus(&self.focus_handle),
            },
        }
    }

    /// The content of the active tab.
    fn render_content(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let tab = self.active;
        let message: SharedString = match tab {
            BottomTab::Terminal => {
                return div()
                    .flex_1()
                    .min_h_0()
                    .child(self.terminals.clone())
                    .into_any_element();
            }
            BottomTab::Problems => {
                return div()
                    .flex_1()
                    .min_h_0()
                    .child(self.problems.clone())
                    .into_any_element();
            }
            BottomTab::Todo => match &self.todo {
                Some((_, view, _)) => {
                    return div()
                        .flex_1()
                        .min_h_0()
                        .child(view.clone())
                        .into_any_element();
                }
                None => "TODO and FIXME comments will be listed here.".into(),
            },
            BottomTab::Debug => match &self.debug {
                Some(debug) => {
                    return div()
                        .flex_1()
                        .min_h_0()
                        .child(debug.clone())
                        .into_any_element();
                }
                None => "The debug console will appear here.".into(),
            },
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .text_sm()
            .text_color(c.text_muted)
            .child(icon(tab.icon()).size(px(24.)).color(c.text_disabled))
            .child(message)
            .into_any_element()
    }
}

impl Render for BottomPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let focused = self
            .framed
            .unwrap_or_else(|| self.focus_handle.contains_focused(window, cx));
        let header = self.render_header(focused, cx);
        // No fill: the panel's rounded fill shows through. The terminal
        // flows straight from the header; lists get a hairline under it.
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .when(self.active != BottomTab::Terminal, |el| {
                el.child(div().flex_none().h(px(1.)).bg(c.border))
            })
            .child(self.render_content(cx))
    }
}
