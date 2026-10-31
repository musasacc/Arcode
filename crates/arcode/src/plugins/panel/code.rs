//! The `code` and `json` widgets: read-only [`Editor`]s, so long bodies are
//! virtualized and highlighted like any file (docs/PLUGINS.md §7). JSON is
//! parsed and pretty-printed on the background executor (serde_json's
//! nesting limit is 128); text over [`MAX_JSON_BYTES`] or invalid JSON is
//! shown as received.

use std::collections::hash_map::Entry;

use arcode_buffer::Buffer;
use arcode_editor::Editor;
use arcode_plugin_host::bindings::NodeId;
use arcode_plugin_host::bindings::ui::CodeProps;
use arcode_ui_kit::theme;
use gpui::{AnyElement, AppContext, Context, Entity, SharedString, Task, div, prelude::*, px};

use super::PanelView;
use super::layout::spinner;

/// Above this size JSON is shown as plain code (§7).
pub const MAX_JSON_BYTES: usize = 20 * 1024 * 1024;

/// One code or JSON node's view.
#[derive(Default)]
pub(super) struct CodeState {
    editor: Option<Entity<Editor>>,
    /// A note above the text ("not valid JSON: …").
    note: Option<String>,
    _parse: Option<Task<()>>,
}

/// Pretty-printed JSON, or why it is shown as received.
pub fn pretty_json(text: &str) -> Result<String, String> {
    if text.len() > MAX_JSON_BYTES {
        return Err("too large to format".to_owned());
    }
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

fn read_only_editor(text: &str, language: Option<&str>, cx: &mut gpui::App) -> Entity<Editor> {
    let language = language.and_then(arcode_syntax::language_by_id);
    cx.new(|cx| {
        let mut editor = Editor::new(Buffer::new(text), cx);
        if let Some(language) = language {
            editor.set_language(language, cx);
        }
        editor.set_read_only(true, cx);
        editor
    })
}

impl PanelView {
    /// The node's text changed: its editor is rebuilt on the next draw.
    pub(super) fn code_changed(&mut self, id: NodeId, _cx: &mut Context<Self>) {
        self.code.remove(&id);
    }

    fn code_element(&self, id: NodeId, cx: &mut Context<Self>) -> AnyElement {
        let c = theme(cx).colors.clone();
        let Some(state) = self.code.get(&id) else {
            return div().into_any_element();
        };
        match &state.editor {
            None => spinner("Formatting…", cx),
            Some(editor) => div()
                .size_full()
                .min_h(px(120.))
                .flex()
                .flex_col()
                .when_some(state.note.clone(), |el, note| {
                    el.child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .text_color(c.warning)
                            .child(SharedString::from(note)),
                    )
                })
                .child(div().flex_1().min_h_0().child(editor.clone()))
                .into_any_element(),
        }
    }

    pub(super) fn render_code(
        &mut self,
        id: NodeId,
        p: &CodeProps,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Entry::Vacant(slot) = self.code.entry(id) {
            slot.insert(CodeState {
                editor: Some(read_only_editor(&p.text, p.language.as_deref(), cx)),
                ..CodeState::default()
            });
        }
        self.code_element(id, cx)
    }

    pub(super) fn render_json(
        &mut self,
        id: NodeId,
        text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Entry::Vacant(slot) = self.code.entry(id) {
            let raw = text.to_owned();
            let parse = cx.background_spawn(async move {
                let pretty = pretty_json(&raw);
                (raw, pretty)
            });
            let task = cx.spawn(async move |this, cx| {
                let (raw, pretty) = parse.await;
                this.update(cx, |view, cx| {
                    let (text, language, note) = match pretty {
                        Ok(text) => (text, Some("json"), None),
                        Err(note) => (raw, None, Some(note)),
                    };
                    let editor = read_only_editor(&text, language, cx);
                    if let Some(state) = view.code.get_mut(&id) {
                        state.editor = Some(editor);
                        state.note = note;
                        cx.notify();
                    }
                })
                .ok();
            });
            slot.insert(CodeState {
                _parse: Some(task),
                ..CodeState::default()
            });
        }
        self.code_element(id, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_pretty_printed_or_explained() {
        assert_eq!(
            pretty_json("{\"a\":[1,2]}").unwrap(),
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
        assert!(
            pretty_json("{nope")
                .unwrap_err()
                .starts_with("not valid JSON")
        );
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(pretty_json(&deep).is_err(), "nesting is limited");
    }
}
