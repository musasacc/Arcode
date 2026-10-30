//! The bottom panel: tabs for Terminal, Problems, Todo and Debug.
//!
//! In this step every tab shows placeholder content. The terminal and
//! problems wiring plugs in by giving [`BottomPanel`] real content for
//! [`BottomTab::Terminal`] and [`BottomTab::Problems`] in
//! [`BottomPanel::render_content`].

use arcode_ui_kit::{IconName, icon, icon_button, theme};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, SharedString, Window, div,
    prelude::*, px,
};

use crate::actions::{ToggleBottomPanel, shortcut_label};

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
    /// A run command requested before the terminal exists (placeholder).
    pending_command: Option<SharedString>,
}

impl EventEmitter<BottomPanelEvent> for BottomPanel {}

impl Focusable for BottomPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BottomPanel {
    /// Creates the panel showing the Terminal tab.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            active: BottomTab::Terminal,
            pending_command: None,
        }
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

    /// Runs `command` in the terminal. Until the terminal is wired in, the
    /// command is only shown in the Terminal tab.
    pub fn run_command(&mut self, command: &str, cx: &mut Context<Self>) {
        self.pending_command = Some(SharedString::from(command.to_string()));
        self.set_active_tab(BottomTab::Terminal, cx);
        cx.notify();
    }

    /// Focuses the panel's content.
    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        div()
            .flex()
            .h_full()
            .items_end()
            .gap_1()
            .children(BottomTab::ALL.into_iter().map(|tab| {
                let active = tab == self.active;
                div()
                    .id(tab.label())
                    .relative()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .h_full()
                    .px_2()
                    .cursor_pointer()
                    .text_color(if active { c.text } else { c.text_muted })
                    .hover(|s| s.text_color(c.text))
                    .child(icon(tab.icon()).size(px(13.)).color(if active {
                        c.text
                    } else {
                        c.text_muted
                    }))
                    .child(tab.label())
                    .when(active, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_1()
                                .right_1()
                                .bottom_0()
                                .h(px(2.))
                                .bg(c.accent),
                        )
                    })
                    .on_click(cx.listener(move |panel, _, _, cx| panel.set_active_tab(tab, cx)))
            }))
    }

    /// The content of the active tab (placeholders in this step).
    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let tab = self.active;
        let message: SharedString = match tab {
            BottomTab::Terminal => match &self.pending_command {
                Some(cmd) => format!("Will run `{cmd}` once the terminal is connected.").into(),
                None => "The integrated terminal will appear here.".into(),
            },
            BottomTab::Problems => "No problems have been detected in the workspace.".into(),
            BottomTab::Todo => "TODO and FIXME comments will be listed here.".into(),
            BottomTab::Debug => "The debug console will appear here.".into(),
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
    }
}

impl Render for BottomPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(32.))
            .pl_2()
            .pr_1()
            .bg(c.panel_header)
            .border_b_1()
            .border_color(c.border)
            .text_xs()
            .child(self.render_tabs(cx))
            .child(
                icon_button("close-panel", IconName::X)
                    .size(px(24.))
                    .tooltip(format!("Close panel ({})", shortcut_label("secondary-j")))
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ToggleBottomPanel), cx)
                    }),
            );
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(c.surface)
            .child(header)
            .child(self.render_content(cx))
    }
}
