//! The theme editor in the workspace: its page tab, its close guard, its
//! requests, and reloading the themes when a theme file is saved in an
//! editor tab.

use std::path::Path;
use std::rc::Rc;

use arcode_config::Theme;
use arcode_editor::{Editor, EditorEvent};
use arcode_ui_kit::IconName;
use gpui::{App, AppContext, Context, Entity, Focusable, Window, WindowAppearance};

use super::Workspace;
use crate::pane::{PageTab, set_close_guard};
use crate::theme_editor::{PAGE_KEY, ThemeEditor, ThemeEditorEvent, configured_theme};
use crate::theme_sync;

impl Workspace {
    /// Shows the theme editor tab (activating it where it is open), or
    /// opens it on the theme in use.
    pub fn open_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_theme_editor_on(None, window, cx);
    }

    /// Shows the theme editor tab on `theme` (a theme just made; the open
    /// page asks first when it has unsaved edits), or, with `None`, as
    /// [`Workspace::open_theme_editor`].
    pub(super) fn open_theme_editor_on(
        &mut self,
        theme: Option<Theme>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project_menu_open = false;
        self.close_prefs_picker(true, window, cx);
        for (ix, pane) in self.panes.clone().into_iter().enumerate() {
            if let Some(page_ix) = pane.read(cx).page_position(PAGE_KEY) {
                self.active_pane = ix;
                pane.update(cx, |pane, cx| pane.activate_page(page_ix, window, cx));
                if let (Some(theme), Some(editor)) = (theme, self.prefs.theme_editor.clone()) {
                    editor.update(cx, |e, cx| e.start_from(theme, window, cx));
                }
                return;
            }
        }
        if let Some(old) = self.prefs.theme_editor.take() {
            set_close_guard(old.entity_id(), None, cx);
        }
        let theme = theme.unwrap_or_else(|| configured_theme(window, cx));
        let editor = cx.new(|cx| ThemeEditor::new(theme, cx));
        let sub = cx.subscribe_in(
            &editor,
            window,
            |ws, _, event: &ThemeEditorEvent, window, cx| ws.theme_editor_event(event, window, cx),
        );
        self.subs.set("theme_editor", sub);
        let weak = editor.downgrade();
        set_close_guard(
            editor.entity_id(),
            Some(Rc::new(move |window, cx| {
                weak.update(cx, |editor, cx| editor.confirm_close(window, cx))
                    .unwrap_or(true)
            })),
            cx,
        );
        self.prefs.theme_editor = Some(editor.clone());
        let tab = PageTab {
            key: PAGE_KEY.into(),
            title: "Theme Editor".into(),
            icon: IconName::Palette,
            view: editor.clone().into(),
            focus: editor.focus_handle(cx),
        };
        self.active_pane()
            .clone()
            .update(cx, |pane, cx| pane.add_page(tab, window, cx));
        cx.notify();
    }

    /// The theme editor, while its tab is open.
    pub fn theme_editor(&self) -> Option<&Entity<ThemeEditor>> {
        self.prefs.theme_editor.as_ref()
    }

    /// Whether the theme editor has changes no file holds yet.
    pub(crate) fn theme_edits_unsaved(&self, cx: &App) -> bool {
        self.prefs
            .theme_editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).has_unsaved_changes())
    }

    /// Drops the theme editor without asking (its tab is gone: the panes
    /// were replaced or the window closes) and shows the configured theme
    /// again.
    pub(crate) fn end_theme_editing(&mut self, appearance: WindowAppearance, cx: &mut App) {
        if let Some(editor) = self.prefs.theme_editor.take() {
            set_close_guard(editor.entity_id(), None, cx);
            theme_sync::restore(appearance, cx);
        }
    }

    fn theme_editor_event(
        &mut self,
        event: &ThemeEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ThemeEditorEvent::Close => {
                for pane in self.panes.clone() {
                    if let Some(ix) = pane.read(cx).page_position(PAGE_KEY) {
                        pane.update(cx, |pane, cx| pane.close_page_now(ix, window, cx));
                    }
                }
                if let Some(editor) = self.prefs.theme_editor.take() {
                    set_close_guard(editor.entity_id(), None, cx);
                    // The page showed the configured theme again, or the
                    // theme it saved and selected.
                    theme_sync::end_edit(cx);
                }
                cx.notify();
            }
            ThemeEditorEvent::OpenFile(path) => self.open_file(path, window, cx),
            ThemeEditorEvent::Toast(message) => self.show_toast(message.clone(), cx),
        }
    }

    /// A theme file (`*.toml` in the themes folder) saved in `editor`
    /// reloads the themes and shows the configured one from the new files.
    pub(super) fn watch_theme_file(
        &mut self,
        editor: &Entity<Editor>,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = crate::theme_editor::themes_dir(cx) else {
            return;
        };
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        let is_theme = path.parent() == Some(dir.as_path())
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
        if !is_theme {
            return;
        }
        let file = path.to_path_buf();
        let sub = cx.subscribe(editor, move |_, _, event: &EditorEvent, cx| {
            if *event != EditorEvent::Saved {
                return;
            }
            let reload = theme_sync::reload_and_restore(cx);
            let file = file.clone();
            cx.spawn(async move |this, cx| {
                let warnings = reload.await;
                this.update(cx, |ws, cx| {
                    // The page open now, not the one open when the file
                    // was opened.
                    if let Some(page) = ws.prefs.theme_editor.clone() {
                        page.update(cx, |page, cx| page.themes_reloaded(cx));
                    }
                    if let Some(message) = theme_sync::file_warning(&warnings, &file) {
                        ws.show_toast_as(message, super::ToastLevel::Warning, cx);
                    }
                })
                .ok();
            })
            .detach();
        });
        self.subs.tie(editor.downgrade().into(), sub);
    }
}
