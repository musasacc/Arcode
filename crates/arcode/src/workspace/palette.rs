//! The command palette in the workspace: opening it where focus is, running
//! the chosen command back there, and switching to and from the file
//! finder with the `>` prefix.

use gpui::{AppContext, Context, Div, Focusable, MouseButton, Stateful, Window, div, prelude::*};

use super::Workspace;
use super::overlays::fade_in;
use crate::actions::{ToggleCommandPalette, ToggleFinder};
use crate::app_state;
use crate::commands::palette::{CommandPalette, PaletteEvent, capture_entries};
use crate::picker_frame::picker_layer;

impl Workspace {
    /// The open command palette.
    pub fn palette(&self) -> Option<&gpui::Entity<CommandPalette>> {
        self.palette.as_ref()
    }

    pub(super) fn toggle_palette(
        &mut self,
        _: &ToggleCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        } else {
            self.open_palette("", window, cx);
        }
    }

    /// Opens the palette with `query`. The commands listed are those
    /// available where focus is now (the active editor when focus is in an
    /// overlay or nowhere).
    pub fn open_palette(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        // One overlay at a time; a theme or font preview is undone.
        self.close_prefs_picker(true, window, cx);
        if self.finder.take().is_some() {
            self.active_pane().read(cx).focus(window, cx);
        }
        if window.focused(cx).is_none() {
            self.active_pane().read(cx).focus(window, cx);
        }
        let return_focus = window.focused(cx);
        let recent = app_state::state(cx).recent_commands.clone();
        let entries = capture_entries(&recent, window, cx);
        let palette = cx.new(|cx| CommandPalette::new(entries, query, return_focus, cx));
        let sub = cx.subscribe_in(&palette, window, |ws, palette, event, window, cx| {
            let return_focus = palette.read(cx).return_focus().cloned();
            match event {
                PaletteEvent::Run { label, action } => {
                    ws.palette = None;
                    app_state::touch_recent_command(label, cx);
                    match return_focus {
                        Some(focus) => window.focus(&focus),
                        None => ws.active_pane().read(cx).focus(window, cx),
                    }
                    window.dispatch_action(action.boxed_clone(), cx);
                    cx.notify();
                }
                PaletteEvent::Dismissed => ws.close_palette(window, cx),
                PaletteEvent::SwitchToFiles(query) => {
                    let query = query.clone();
                    ws.close_palette(window, cx);
                    ws.toggle_finder(&ToggleFinder, window, cx);
                    if let Some(finder) = &ws.finder {
                        finder.update(cx, |f, cx| f.set_query(&query, cx));
                    }
                }
            }
        });
        self.subs.set("palette", sub);
        window.focus(&palette.focus_handle(cx));
        self.palette = Some(palette);
        cx.notify();
    }

    /// Closes the palette, giving focus back.
    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            match palette.read(cx).return_focus().cloned() {
                Some(focus) => window.focus(&focus),
                None => self.active_pane().read(cx).focus(window, cx),
            }
            cx.notify();
        }
    }

    /// Adds the palette overlay to `root`.
    pub(super) fn render_palette_overlay(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some(palette) = &self.palette else {
            return root;
        };
        let panel = div()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(palette.clone());
        root.child(
            picker_layer("palette-backdrop")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| ws.close_palette(window, cx)),
                )
                .child(fade_in("palette-anim", panel, cx)),
        )
    }
}
