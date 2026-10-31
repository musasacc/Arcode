//! Status bar entries of the active file: indentation, encoding, line
//! ending and language. Clicking one opens its picker.

use arcode_buffer::LineEnding;
use arcode_editor::{Editor, IndentOverride};
use arcode_ui_kit::{Tooltip, theme};
use gpui::{AnyElement, AnyView, App, SharedString, Window, prelude::*};

use super::actions::{ChangeIndentation, ChangeLineEnding, SelectLanguageMode};

/// What the status bar shows about the active document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocStatus {
    /// `Spaces 4` or `Tab size 4`.
    pub indent: SharedString,
    /// `UTF-8` or `UTF-8 with BOM`.
    pub encoding: SharedString,
    /// `LF` or `CRLF`.
    pub line_ending: SharedString,
}

impl DocStatus {
    /// The status of `editor`.
    pub fn of(editor: &Editor, cx: &App) -> Self {
        Self {
            indent: indent_label(editor.indentation(cx)).into(),
            encoding: if editor.buffer().has_bom() {
                "UTF-8 with BOM"
            } else {
                "UTF-8"
            }
            .into(),
            line_ending: line_ending_label(editor.line_ending()).into(),
        }
    }
}

/// `Spaces 4` / `Tab size 4`.
pub fn indent_label(indent: IndentOverride) -> String {
    if indent.uses_spaces() {
        format!("Spaces {}", indent.size())
    } else {
        format!("Tab size {}", indent.tab_width)
    }
}

/// `LF` / `CRLF`.
pub fn line_ending_label(ending: LineEnding) -> &'static str {
    match ending {
        LineEnding::Lf => "LF",
        LineEnding::CrLf => "CRLF",
    }
}

/// A footer pill (docs/DESIGN.md 6.10); `strong` draws the text in
/// `text` (the language) instead of muted.
fn entry(
    id: &'static str,
    text: SharedString,
    tip: &'static str,
    action: Option<Box<dyn gpui::Action>>,
    strong: bool,
    cx: &App,
) -> AnyElement {
    let c = theme(cx).colors.clone();
    crate::status_bar::footer_item(id, action.is_some(), cx)
        .when(strong, |el| el.text_color(c.text))
        .child(text)
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tip, cx) })
        .when_some(action, |el, action| {
            el.on_click(move |_, window: &mut Window, cx| {
                window.dispatch_action(action.boxed_clone(), cx)
            })
        })
        .into_any_element()
}

/// The entries for `doc`, then the clickable language name.
pub fn render(
    doc: Option<&DocStatus>,
    language: Option<SharedString>,
    cx: &App,
) -> Vec<AnyElement> {
    let mut items = Vec::new();
    if let Some(doc) = doc {
        items.push(entry(
            "status-indent",
            doc.indent.clone(),
            "Select Indentation",
            Some(Box::new(ChangeIndentation)),
            false,
            cx,
        ));
        items.push(entry(
            "status-encoding",
            doc.encoding.clone(),
            "Encoding (Arcode reads and writes UTF-8)",
            None,
            false,
            cx,
        ));
        items.push(entry(
            "status-eol",
            doc.line_ending.clone(),
            "Select End of Line Sequence",
            Some(Box::new(ChangeLineEnding)),
            false,
            cx,
        ));
    }
    if let Some(language) = language {
        items.push(entry(
            "status-language",
            language,
            "Select Language Mode",
            Some(Box::new(SelectLanguageMode)),
            true,
            cx,
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(indent_label(IndentOverride::spaces(2)), "Spaces 2");
        assert_eq!(indent_label(IndentOverride::tabs(8)), "Tab size 8");
        assert_eq!(line_ending_label(LineEnding::CrLf), "CRLF");
    }
}
