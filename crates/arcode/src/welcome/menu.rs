//! The context menu of a recent project row (docs/DESIGN.md 6.12).

use std::path::PathBuf;

use arcode_ui_kit::{IconName, icon, menu_row, menu_separator, popover, theme, ui_font_family};
use gpui::{
    AnyElement, ClipboardItem, Context, Pixels, Point, anchored, deferred, div, prelude::*, px,
};

use super::Welcome;
use crate::app_state;

/// The row the menu is for and where it opened.
pub(super) struct RowMenu {
    pub path: PathBuf,
    pub position: Point<Pixels>,
}

/// What a menu entry does.
#[derive(Clone, Copy)]
enum Entry {
    Open,
    OpenInNewWindow,
    TogglePin,
    CopyPath,
    Remove,
    Clear,
}

impl Welcome {
    /// Opens the menu of the row for `path` at `position`.
    pub(super) fn open_menu(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(RowMenu { path, position });
        cx.notify();
    }

    fn menu_action(&mut self, entry: Entry, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        let path = menu.path;
        match entry {
            Entry::Open => self.open(path, false, window, cx),
            Entry::OpenInNewWindow => self.open(path, true, window, cx),
            Entry::TogglePin => self.toggle_pin(&path, cx),
            Entry::CopyPath => {
                cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()))
            }
            Entry::Remove => self.remove(&path, cx),
            Entry::Clear => self.clear(cx),
        }
        cx.notify();
    }

    /// The open menu, drawn over everything.
    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let c = theme(cx).colors.clone();
        let recent = app_state::recent_projects(cx);
        let pinned = recent.iter().any(|p| p.path == menu.path && p.pinned);
        let any_unpinned = recent.iter().any(|p| !p.pinned);
        let pin = if pinned {
            ("Unpin", IconName::PinOff)
        } else {
            ("Pin to Top", IconName::Pin)
        };
        let mut groups: Vec<Vec<(&'static str, IconName, Entry)>> = vec![
            vec![
                ("Open", IconName::FolderOpen, Entry::Open),
                (
                    "Open in New Window",
                    IconName::ExternalLink,
                    Entry::OpenInNewWindow,
                ),
            ],
            vec![
                (pin.0, pin.1, Entry::TogglePin),
                ("Copy Path", IconName::Copy, Entry::CopyPath),
            ],
            vec![("Remove from Recent Projects", IconName::X, Entry::Remove)],
        ];
        if any_unpinned {
            groups[2].push(("Clear Recent Projects", IconName::Trash, Entry::Clear));
        }
        let mut list = popover(cx)
            .id("welcome-menu")
            .min_w(px(220.))
            .font_family(ui_font_family(cx))
            .occlude()
            .on_mouse_down_out(cx.listener(|welcome, _, _, cx| {
                welcome.menu = None;
                cx.notify();
            }));
        let mut ix: usize = 0;
        for (g, group) in groups.into_iter().enumerate() {
            if g > 0 {
                list = list.child(menu_separator(cx));
            }
            for (label, glyph, entry) in group {
                list = list.child(
                    menu_row(("welcome-menu-item", ix), false, cx)
                        .child(icon(glyph).size(px(14.)).color(c.text_muted))
                        .child(div().flex_1().whitespace_nowrap().child(label))
                        .on_click(cx.listener(move |welcome, _, window, cx| {
                            welcome.menu_action(entry, window, cx)
                        })),
                );
                ix += 1;
            }
        }
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
