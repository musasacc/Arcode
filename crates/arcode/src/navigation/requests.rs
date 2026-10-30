//! The navigation commands: each asks the language server bridge and
//! jumps or opens a picker; documents without a server use the
//! tree-sitter outline instead.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use arcode_buffer::Rope;
use arcode_editor::Editor;
use gpui::{AppContext, Context, Entity, SharedString, Window};

use super::item::{NavItem, NavTarget, outline_items};
use super::picker::{NavPicker, PickerOptions};
use super::preview::fill_previews;
use crate::lsp_bridge::navigation::GotoKind;
use crate::workspace::Workspace;

/// Typing pause before the workspace symbol query is sent.
const QUERY_DELAY: Duration = Duration::from_millis(120);

/// The outline items of `editor` (empty without a path or outline).
fn editor_outline(editor: &Entity<Editor>, cx: &gpui::App) -> Vec<NavItem> {
    let e = editor.read(cx);
    match e.path() {
        Some(path) => outline_items(&e.outline(), path, e.buffer()),
        None => Vec::new(),
    }
}

/// Without a server: the outline symbol named like the word at the cursor.
fn outline_definition(editor: &Entity<Editor>, cx: &gpui::App) -> Option<NavTarget> {
    let e = editor.read(cx);
    let cursor = e.cursor_position();
    let (word_range, word) = e.word_at(cursor)?;
    editor_outline(editor, cx)
        .into_iter()
        .find(|item| item.label == word && item.target.range_in(e) != word_range)
        .map(|item| item.target)
}

impl Workspace {
    fn root_path(&self) -> Option<PathBuf> {
        self.project().map(|p| p.root.clone())
    }

    /// Go to definition / type definition / implementation at the cursor.
    pub(super) fn goto(&mut self, kind: GotoKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let at = editor.read(cx).cursor_position();
        let request = self.lsp().read(cx).goto(editor.read(cx), at, kind);
        let Some(request) = request else {
            match outline_definition(&editor, cx).filter(|_| kind == GotoKind::Definition) {
                Some(target) => self.jump_to(&target, window, cx),
                None => self.show_toast(format!("No {} found", kind.noun()), cx),
            }
            return;
        };
        self.nav.task = Some(cx.spawn_in(window, async move |ws, cx| {
            let result = request.await;
            ws.update_in(cx, |ws, window, cx| match result {
                Ok(targets) if targets.is_empty() => {
                    ws.show_toast(format!("No {} found", kind.noun()), cx)
                }
                Ok(targets) if targets.len() == 1 => ws.jump_to(&targets[0], window, cx),
                Ok(targets) => {
                    let placeholder = format!("{} {}s", targets.len(), kind.noun());
                    let items = targets
                        .into_iter()
                        .map(|t| NavItem::new(String::new(), t))
                        .collect();
                    ws.show_locations(items, placeholder, window, cx);
                }
                Err(err) => ws.show_toast(format!("Cannot find the {}: {err}", kind.noun()), cx),
            })
            .ok();
        }));
    }

    /// Shows `items` (locations) grouped by file with preview lines.
    fn show_locations(
        &mut self,
        items: Vec<NavItem>,
        placeholder: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = self.show_nav_picker(
            PickerOptions {
                placeholder: placeholder.into(),
                grouped: true,
                server_query: false,
                root: self.root_path(),
            },
            window,
            cx,
        );
        self.load_previews(picker, items, cx);
    }

    /// Fills preview lines in the background, then shows the items.
    fn load_previews(
        &mut self,
        picker: Entity<NavPicker>,
        mut items: Vec<NavItem>,
        cx: &mut Context<Self>,
    ) {
        let open: HashMap<PathBuf, Rope> = self
            .editors(cx)
            .iter()
            .filter_map(|e| {
                let e = e.read(cx);
                Some((e.path()?.to_path_buf(), e.buffer().text().clone()))
            })
            .collect();
        let work = cx.background_spawn(async move {
            fill_previews(&mut items, &open);
            items
        });
        self.nav.preview_task = Some(cx.spawn(async move |_, cx| {
            let items = work.await;
            picker.update(cx, |p, cx| p.set_items(items, cx)).ok();
        }));
    }

    /// Find all references of the symbol at the cursor.
    pub(super) fn find_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let (at, word) = {
            let e = editor.read(cx);
            let at = e.cursor_position();
            (at, e.word_at(at).map(|(_, w)| w))
        };
        let Some(request) = self.lsp().read(cx).references(editor.read(cx), at) else {
            self.show_toast("No language server offers references for this file", cx);
            return;
        };
        let placeholder = match &word {
            Some(w) => format!("References to {w}"),
            None => "References".to_string(),
        };
        let picker = self.show_nav_picker(
            PickerOptions {
                placeholder: placeholder.into(),
                grouped: true,
                server_query: false,
                root: self.root_path(),
            },
            window,
            cx,
        );
        self.nav.task = Some(cx.spawn(async move |ws, cx| {
            let result = request.await;
            ws.update(cx, |ws, cx| match result {
                Ok(items) => ws.load_previews(picker, items, cx),
                Err(err) => {
                    picker.update(cx, |p, cx| p.set_items(Vec::new(), cx));
                    ws.show_toast(format!("Cannot find references: {err}"), cx);
                }
            })
            .ok();
        }));
    }

    /// The symbols of the active document (outline without a server).
    pub(super) fn document_symbols(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            self.show_toast("Open a file to list its symbols", cx);
            return;
        };
        let title = editor.read(cx).title();
        let request = self.lsp().read(cx).document_symbols(editor.read(cx));
        let picker = self.show_nav_picker(
            PickerOptions {
                placeholder: SharedString::from(format!("Go to symbol in {title}")),
                ..Default::default()
            },
            window,
            cx,
        );
        let Some(request) = request else {
            let items = editor_outline(&editor, cx);
            picker.update(cx, |p, cx| p.set_items(items, cx));
            return;
        };
        self.nav.task = Some(cx.spawn(async move |_, cx| {
            let items = request.await.unwrap_or_default();
            cx.update(|cx| {
                let items = if items.is_empty() {
                    editor_outline(&editor, cx)
                } else {
                    items
                };
                picker.update(cx, |p, cx| p.set_items(items, cx));
            })
            .ok();
        }));
    }

    /// Symbols of the whole workspace from the servers; without one, the
    /// outlines of the open files.
    pub(super) fn workspace_symbols(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx);
        let has_server = {
            let e = editor.as_ref().map(|e| e.read(cx));
            self.lsp().read(cx).workspace_symbol_path(e).is_some()
        };
        let picker = self.show_nav_picker(
            PickerOptions {
                placeholder: "Go to symbol in workspace".into(),
                grouped: false,
                server_query: has_server,
                root: self.root_path(),
            },
            window,
            cx,
        );
        if has_server {
            self.query_workspace_symbols(String::new(), cx);
            return;
        }
        let items: Vec<NavItem> = self
            .editors(cx)
            .iter()
            .flat_map(|e| {
                let file = e.read(cx).title().to_string();
                editor_outline(e, cx).into_iter().map(move |mut item| {
                    item.detail = Some(file.clone());
                    item
                })
            })
            .collect();
        picker.update(cx, |p, cx| p.set_items(items, cx));
    }

    /// Sends the workspace symbol query after a short pause.
    pub(super) fn query_workspace_symbols(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(picker) = self.nav.picker.clone() else {
            return;
        };
        let editor = self.active_editor(cx);
        self.nav.task = Some(cx.spawn(async move |ws, cx| {
            if !query.is_empty() {
                cx.background_executor().timer(QUERY_DELAY).await;
            }
            let request = ws.update(cx, |ws, cx| {
                let e = editor.as_ref().map(|e| e.read(cx));
                ws.lsp().read(cx).workspace_symbols(e, query)
            });
            let Ok(Some(request)) = request else {
                return;
            };
            let items = request.await.unwrap_or_default();
            picker.update(cx, |p, cx| p.set_items(items, cx)).ok();
        }));
    }
}
