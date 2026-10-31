//! The language servers of the active file in the footer: one entry per
//! server, a 7 px state dot and its name (the state in words unless it is
//! ready); clicking one opens a small menu (restart the server, show its
//! log in a read-only tab). Clicking the entry again, a click elsewhere or
//! Escape closes it.

use arcode_ui_kit::{IconName, Tooltip, icon, list_row, popover, status_dot, theme, type_scale};
use gpui::{
    Context, Corner, Entity, FocusHandle, IntoElement, MouseButton, ParentElement, Pixels, Point,
    Render, SharedString, Styled, Subscription, WeakEntity, Window, anchored, deferred, div,
    prelude::*, px,
};

use super::LspBridge;
use super::servers::{EntryKind, ServerEntry, command_line};
use crate::workspace::Workspace;

/// Status bar entries of the bridge's servers for the active language.
pub struct LspStatusView {
    bridge: Entity<LspBridge>,
    workspace: WeakEntity<Workspace>,
    /// The server whose menu is open.
    menu: Option<SharedString>,
    /// The menu a mouse-down outside it just closed, and where: that same
    /// mouse-down on the menu's own entry must not open it again.
    closed_by: Option<(SharedString, Point<Pixels>)>,
    /// Focused while the menu is open, for Escape.
    focus: FocusHandle,
    /// What had focus before the menu opened; it gets it back.
    restore_focus: Option<FocusHandle>,
    _observe: Subscription,
}

impl LspStatusView {
    /// Shows `bridge`'s servers; logs open as tabs of `workspace`.
    pub fn new(
        bridge: Entity<LspBridge>,
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&bridge, |view: &mut Self, bridge, cx| {
            // Close a menu whose server is no longer listed.
            let listed = view.menu.as_ref().is_none_or(|name| {
                bridge
                    .read(cx)
                    .server_entries(cx)
                    .iter()
                    .any(|e| e.name == *name)
            });
            if !listed {
                view.menu = None;
            }
            cx.notify();
        });
        Self {
            bridge,
            workspace,
            menu: None,
            closed_by: None,
            focus: cx.focus_handle(),
            restore_focus: None,
            _observe: observe,
        }
    }

    /// The server whose menu is open.
    pub fn open_menu(&self) -> Option<&SharedString> {
        self.menu.as_ref()
    }

    /// Opens (or closes, when open) the menu of `server`.
    pub fn toggle_menu(
        &mut self,
        server: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.as_ref() == Some(&server) {
            self.close_menu(window, cx);
            return;
        }
        if self.menu.is_none() {
            self.restore_focus = window.focused(cx);
        }
        self.menu = Some(server);
        window.focus(&self.focus);
        cx.notify();
    }

    /// Closes the open menu, giving focus back.
    pub fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_none() {
            return;
        }
        if self.focus.is_focused(window)
            && let Some(previous) = self.restore_focus.take()
        {
            window.focus(&previous);
        }
        self.restore_focus = None;
        cx.notify();
    }

    /// A mouse-down on `server`'s entry: toggles its menu, unless the same
    /// mouse-down already closed it (the menu closes on any press outside).
    fn entry_pressed(
        &mut self,
        server: SharedString,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed_by.take() == Some((server.clone(), at)) {
            return;
        }
        self.toggle_menu(server, window, cx);
    }

    /// Restarts `server` (menu entry).
    pub fn restart(&mut self, server: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        self.bridge.update(cx, |b, cx| b.restart_server(server, cx));
    }

    /// Opens `server`'s log in a read-only tab (menu entry).
    pub fn show_log(&mut self, server: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
        let server = server.to_string();
        self.workspace
            .update(cx, |ws, cx| {
                super::log_tab::open_server_log(ws, &server, window, cx)
            })
            .ok();
    }

    fn render_entry(&self, entry: ServerEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let color = match entry.kind {
            EntryKind::Ready => c.success,
            EntryKind::Working | EntryKind::NotInstalled => c.warning,
            EntryKind::Stopped => c.text_disabled,
            EntryKind::Failed => c.error,
        };
        let open = self.menu.as_ref() == Some(&entry.name);
        let name = entry.name.clone();
        let tip = SharedString::from(format!("{}: {}", entry.name, entry.state));
        crate::status_bar::footer_item(
            SharedString::from(format!("lsp-server-{}", entry.name)),
            true,
            cx,
        )
        .debug_selector(|| format!("lsp-server-{}", entry.name))
        .relative()
        .when(open, |el| el.bg(c.hover))
        .child(status_dot(color))
        .child(entry.name.clone())
        .when(entry.kind != EntryKind::Ready, |el| {
            el.child(div().text_color(c.text_muted).child(entry.state.clone()))
        })
        .when(!open, |el| {
            el.tooltip(move |_, cx| Tooltip::text(tip.clone(), cx))
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                view.entry_pressed(name.clone(), event.position, window, cx);
            }),
        )
        .when(open, |el| el.child(self.render_menu(&entry.name, cx)))
    }

    fn render_menu(&self, server: &SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let command = command_line(server, cx);
        let row = |id: &'static str, label: &'static str, icon_name: IconName| {
            list_row(id, false, cx)
                .mx(px(0.))
                .h(px(26.))
                .gap(px(8.))
                .text_color(c.text)
                .child(icon(icon_name).size(px(14.)).color(c.text_muted))
                .child(div().flex_1().whitespace_nowrap().child(label))
        };
        let (restart, log) = (server.clone(), server.clone());
        let list = popover(cx)
            .id("lsp-server-menu")
            .min_w(px(220.))
            .text_size(type_scale(cx).body)
            .text_color(c.text)
            .occlude()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    view.close_menu(window, cx);
                }
            }))
            .on_mouse_down_out(
                cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                    if let Some(open) = view.menu.clone() {
                        view.closed_by = Some((open, event.position));
                    }
                    view.close_menu(window, cx);
                }),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .child(div().text_color(c.text).child(server.clone()))
                    .child(
                        div()
                            .text_size(type_scale(cx).meta)
                            .text_color(c.text_muted)
                            .child(command),
                    ),
            )
            .child(div().h(px(1.)).my(px(4.)).bg(c.border))
            .child(
                row("lsp-menu-restart", "Restart Server", IconName::Refresh).on_click(cx.listener(
                    move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.restart(&restart, window, cx);
                    },
                )),
            )
            .child(
                row("lsp-menu-log", "Show Server Log", IconName::FileText).on_click(cx.listener(
                    move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.show_log(&log, window, cx);
                    },
                )),
            );
        // Anchored at the entry's top-left corner, opening upwards.
        div().absolute().top_0().left_0().child(
            deferred(
                anchored()
                    .anchor(Corner::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(div().pb_1().child(list)),
            )
            .with_priority(1),
        )
    }
}

impl Render for LspStatusView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.bridge.read(cx).server_entries(cx);
        let mut row = div().flex().items_center().gap(px(4.)).h_full();
        for entry in entries {
            row = row.child(self.render_entry(entry, cx));
        }
        row
    }
}
