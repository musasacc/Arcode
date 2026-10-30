//! The Terminal tab: any number of integrated terminals, started lazily in
//! the project root, with a small tab strip to switch, add and close them.

use std::path::PathBuf;

use arcode_terminal_view::{TerminalView, TerminalViewEvent};
use arcode_ui_kit::{IconName, Tooltip, button, icon, icon_button, theme};
use gpui::{
    App, Context, Entity, EntityId, FocusHandle, Focusable, SharedString, Subscription, Window,
    div, prelude::*, px,
};

use crate::actions::{NewTerminal, shortcut_label};

/// Longest terminal title shown in a tab.
const MAX_TITLE_CHARS: usize = 24;

/// The terminals of one workspace.
pub struct TerminalPanel {
    terminals: Vec<Entity<TerminalView>>,
    active: usize,
    cwd: Option<PathBuf>,
    /// Why the last terminal could not be started.
    error: Option<SharedString>,
    focus_handle: FocusHandle,
    subscriptions: Vec<(EntityId, Subscription)>,
}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TerminalPanel {
    /// Creates an empty panel; terminals start on first use.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            terminals: Vec::new(),
            active: 0,
            cwd: None,
            error: None,
            focus_handle: cx.focus_handle(),
            subscriptions: Vec::new(),
        }
    }

    /// Sets the folder new terminals start in. Changing it closes the
    /// running terminals (they belong to the previous project).
    pub fn set_cwd(&mut self, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.cwd != cwd {
            self.cwd = cwd;
            self.terminals.clear();
            self.subscriptions.clear();
            self.active = 0;
            self.error = None;
            cx.notify();
        }
    }

    /// The running terminals in tab order.
    pub fn terminals(&self) -> &[Entity<TerminalView>] {
        &self.terminals
    }

    /// The terminal shown.
    pub fn active_terminal(&self) -> Option<&Entity<TerminalView>> {
        self.terminals.get(self.active)
    }

    /// The active terminal, starting one if none is running.
    pub fn ensure_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        match self.active_terminal() {
            Some(terminal) => Some(terminal.clone()),
            None => self.new_terminal(window, cx),
        }
    }

    /// Starts a new terminal in the project root and activates it.
    pub fn new_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let terminal = match TerminalView::spawn(self.cwd.clone(), window, cx) {
            Ok(terminal) => terminal,
            Err(err) => {
                log::error!("cannot start a terminal: {err:#}");
                self.error = Some(format!("Cannot start a terminal: {err:#}").into());
                cx.notify();
                return None;
            }
        };
        let sub =
            cx.subscribe_in(
                &terminal,
                window,
                |panel, terminal, event, window, cx| match event {
                    TerminalViewEvent::Exited => {
                        let had_focus = terminal.focus_handle(cx).contains_focused(window, cx);
                        panel.remove(terminal.entity_id(), cx);
                        if had_focus {
                            panel.focus_existing(window, cx);
                        }
                    }
                    TerminalViewEvent::TitleChanged => cx.notify(),
                    TerminalViewEvent::Bell => {}
                },
            );
        self.subscriptions.push((terminal.entity_id(), sub));
        self.terminals.push(terminal.clone());
        self.active = self.terminals.len() - 1;
        self.error = None;
        cx.notify();
        Some(terminal)
    }

    /// Activates the terminal at `ix`.
    pub fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.terminals.len() {
            self.active = ix;
            self.focus(window, cx);
            cx.notify();
        }
    }

    /// Closes the terminal at `ix` (its shell is terminated).
    pub fn close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.terminals.get(ix) {
            let had_focus = terminal.focus_handle(cx).contains_focused(window, cx);
            self.remove(terminal.entity_id(), cx);
            if had_focus {
                self.focus_existing(window, cx);
            }
        }
    }

    fn remove(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(ix) = self.terminals.iter().position(|t| t.entity_id() == id) else {
            return;
        };
        self.terminals.remove(ix);
        self.subscriptions.retain(|(sid, _)| *sid != id);
        if self.active > ix || self.active >= self.terminals.len() {
            self.active = self.active.saturating_sub(1);
        }
        cx.notify();
    }

    /// Focuses the active terminal, starting one if needed.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.ensure_terminal(window, cx) {
            window.focus(&terminal.focus_handle(cx));
        } else {
            window.focus(&self.focus_handle);
        }
    }

    /// Focuses the active terminal without starting one.
    fn focus_existing(&self, window: &mut Window, cx: &App) {
        match self.active_terminal() {
            Some(terminal) => window.focus(&terminal.focus_handle(cx)),
            None => window.focus(&self.focus_handle),
        }
    }

    /// Runs `command` in the active terminal (started if needed) and
    /// focuses it.
    pub fn run_command(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.ensure_terminal(window, cx) {
            terminal.read(cx).run_command(command);
            window.focus(&terminal.focus_handle(cx));
        }
    }
}

/// The terminal tab strip shown in the bottom panel header.
pub fn render_strip(panel: &Entity<TerminalPanel>, cx: &App) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let p = panel.read(cx);
    let many = p.terminals.len() > 1;
    let tabs = p.terminals.iter().enumerate().map(|(ix, terminal)| {
        let active = ix == p.active;
        let full_title = terminal.read(cx).title();
        let title = truncate(&short_title(&full_title), MAX_TITLE_CHARS);
        let select = panel.downgrade();
        let close = panel.downgrade();
        div()
            .id(("terminal-tab", ix))
            .flex()
            .items_center()
            .gap_1()
            .h(px(22.))
            .pl_1p5()
            .pr_0p5()
            .rounded_sm()
            .cursor_pointer()
            .text_color(if active { c.text } else { c.text_muted })
            .when(active && many, |el| el.bg(c.surface_raised))
            .hover(|s| s.bg(c.hover))
            .child(icon(IconName::Terminal).size(px(12.)).color(c.text_muted))
            .child(title)
            .child(
                icon_button(("close-terminal", ix), IconName::X)
                    .size(px(16.))
                    .tooltip("Kill terminal")
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        close.update(cx, |p, cx| p.close(ix, window, cx)).ok();
                    }),
            )
            .tooltip(move |_, cx| Tooltip::text(full_title.clone(), cx))
            .on_click(move |_, window, cx| {
                select.update(cx, |p, cx| p.activate(ix, window, cx)).ok();
            })
    });
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .min_w_0()
        .overflow_hidden()
        .children(tabs)
        .child(
            icon_button("new-terminal", IconName::Plus)
                .size(px(22.))
                .tooltip(format!(
                    "New terminal ({})",
                    shortcut_label("secondary-shift-`")
                ))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTerminal), cx)),
        )
}

/// A shell title like `user@host: ~/code/app` shortened to `app`; other
/// titles (e.g. a running program) are kept.
fn short_title(title: &str) -> String {
    let title = title.trim();
    let path = title
        .split_once(": ")
        .map_or(title, |(_, rest)| rest)
        .trim();
    if path.starts_with('/') || path.starts_with('~') || path.contains(":\\") {
        let trimmed = path.trim_end_matches(['/', '\\']);
        let name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
        if !name.is_empty() {
            return name.to_string();
        }
        return path.to_string();
    }
    title.to_string()
}

fn truncate(text: &str, max: usize) -> SharedString {
    if text.chars().count() <= max {
        return SharedString::from(text.to_string());
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…").into()
}

impl Render for TerminalPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let content = match self.active_terminal() {
            Some(terminal) => div()
                .size_full()
                .pl_2()
                .pt_1()
                .child(terminal.clone())
                .into_any_element(),
            None => {
                let message: SharedString = self
                    .error
                    .clone()
                    .unwrap_or_else(|| "No terminal is running.".into());
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_sm()
                    .text_color(c.text_muted)
                    .child(
                        icon(IconName::Terminal)
                            .size(px(24.))
                            .color(c.text_disabled),
                    )
                    .child(message)
                    .child(
                        button("start-terminal", "New Terminal")
                            .icon(IconName::Plus)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewTerminal), cx)
                            }),
                    )
                    .into_any_element()
            }
        };
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(c.background)
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::{short_title, truncate};

    #[test]
    fn shell_titles_are_shortened() {
        assert_eq!(short_title("root@vm: /tmp/x/m1demo"), "m1demo");
        assert_eq!(short_title("me@host: ~"), "~");
        assert_eq!(short_title("me@host: ~/code/app/"), "app");
        assert_eq!(short_title("/"), "/");
        assert_eq!(short_title("C:\\Users\\me\\proj"), "proj");
        assert_eq!(short_title("vim main.rs"), "vim main.rs");
        assert_eq!(short_title("Terminal"), "Terminal");
    }

    #[test]
    fn long_titles_are_truncated() {
        assert_eq!(truncate("bash", 24).as_ref(), "bash");
        assert_eq!(truncate("abcdef", 4).as_ref(), "abc…");
    }
}
