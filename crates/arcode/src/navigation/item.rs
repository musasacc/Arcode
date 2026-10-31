//! Navigation targets and picker items, independent of where they came
//! from (a language server or the tree-sitter outline).

use std::ops::Range;
use std::path::PathBuf;

use arcode_buffer::Point;
use arcode_editor::Editor;
use arcode_lsp::PositionEncoding;
use arcode_lsp::lsp_types;
use arcode_syntax::{OutlineItem, OutlineKind};
use arcode_ui_kit::IconName;

use crate::lsp_bridge::convert::buffer_encoding;

/// A position in a file, in whatever coordinates its source used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavPos {
    /// An LSP range in the server's encoding; converted against the
    /// buffer once the file is open (it may hold unsaved edits).
    Lsp {
        /// The range.
        range: lsp_types::Range,
        /// The server's position encoding.
        encoding: PositionEncoding,
    },
    /// Buffer points.
    Points(Range<Point>),
}

/// Where a jump goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavTarget {
    /// The file.
    pub path: PathBuf,
    /// The place in it.
    pub pos: NavPos,
}

impl NavTarget {
    /// The target's range in `editor`'s buffer.
    pub fn range_in(&self, editor: &Editor) -> Range<Point> {
        match &self.pos {
            NavPos::Points(r) => r.clone(),
            NavPos::Lsp { range, encoding } => {
                let enc = buffer_encoding(*encoding);
                let start = editor.lsp_to_point(range.start.line, range.start.character, enc);
                let end = editor.lsp_to_point(range.end.line, range.end.character, enc);
                start..end
            }
        }
    }

    /// The zero-based line the target starts on.
    pub fn line(&self) -> usize {
        match &self.pos {
            NavPos::Points(r) => r.start.line,
            NavPos::Lsp { range, .. } => range.start.line as usize,
        }
    }
}

/// A short kind label (`fn`, `struct`, ...) and the syntax capture whose
/// theme color it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolTag {
    /// Shown before the name.
    pub label: &'static str,
    /// Theme syntax capture for its color.
    pub capture: &'static str,
}

impl SymbolTag {
    /// The Lucide icon of the kind in a picker row (docs/DESIGN.md 6.11).
    pub fn icon(self) -> IconName {
        match self.label {
            "fn" | "new" | "macro" => IconName::SquareFunction,
            "struct" | "enum" | "class" | "trait" | "interface" | "impl" | "type" | "array"
            | "obj" | "event" => IconName::Braces,
            "mod" => IconName::Box,
            "file" => IconName::File,
            "#" => IconName::FileText,
            _ => IconName::Variable,
        }
    }
}

const fn tag(label: &'static str, capture: &'static str) -> Option<SymbolTag> {
    Some(SymbolTag { label, capture })
}

/// The tag of an LSP symbol kind.
pub fn lsp_symbol_tag(kind: lsp_types::SymbolKind) -> Option<SymbolTag> {
    use lsp_types::SymbolKind as K;
    match kind {
        K::FILE => tag("file", "string"),
        K::MODULE | K::NAMESPACE | K::PACKAGE => tag("mod", "module"),
        K::CLASS => tag("class", "type"),
        K::METHOD => tag("fn", "function.method"),
        K::PROPERTY | K::FIELD => tag("field", "property"),
        K::CONSTRUCTOR => tag("new", "constructor"),
        K::ENUM => tag("enum", "type"),
        K::INTERFACE => tag("trait", "type"),
        K::FUNCTION => tag("fn", "function"),
        K::VARIABLE => tag("let", "variable"),
        K::CONSTANT => tag("const", "constant"),
        K::STRING => tag("str", "string"),
        K::NUMBER => tag("num", "number"),
        K::BOOLEAN => tag("bool", "boolean"),
        K::ARRAY => tag("array", "type"),
        K::OBJECT => tag("obj", "type"),
        K::KEY => tag("key", "property"),
        K::NULL => tag("null", "constant"),
        K::ENUM_MEMBER => tag("variant", "constant"),
        K::STRUCT => tag("struct", "type"),
        K::EVENT => tag("event", "type"),
        K::OPERATOR => tag("op", "operator"),
        K::TYPE_PARAMETER => tag("type", "type"),
        _ => None,
    }
}

/// The tag of a tree-sitter outline kind.
pub fn outline_tag(kind: OutlineKind) -> Option<SymbolTag> {
    match kind {
        OutlineKind::Function => tag("fn", "function"),
        OutlineKind::Method => tag("fn", "function.method"),
        OutlineKind::Constructor => tag("new", "constructor"),
        OutlineKind::Struct => tag("struct", "type"),
        OutlineKind::Enum => tag("enum", "type"),
        OutlineKind::Trait => tag("trait", "type"),
        OutlineKind::Impl => tag("impl", "keyword"),
        OutlineKind::Class => tag("class", "type"),
        OutlineKind::Interface => tag("interface", "type"),
        OutlineKind::Module => tag("mod", "module"),
        OutlineKind::Const => tag("const", "constant"),
        OutlineKind::Type => tag("type", "type"),
        OutlineKind::Macro => tag("macro", "function.macro"),
        OutlineKind::Heading => tag("#", "keyword"),
        _ => None,
    }
}

/// One row of a navigation picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    /// The symbol name, or the preview line of a reference.
    pub label: String,
    /// Muted text after the label (container, detail, file).
    pub detail: Option<String>,
    /// Kind of symbol, if it is one.
    pub kind: Option<SymbolTag>,
    /// Nesting depth (document symbols).
    pub depth: usize,
    /// Byte range of `label` to emphasize (the reference in a preview).
    pub highlight: Option<Range<usize>>,
    /// Where choosing the row goes.
    pub target: NavTarget,
}

impl NavItem {
    /// An item with only a label and a target.
    pub fn new(label: impl Into<String>, target: NavTarget) -> Self {
        NavItem {
            label: label.into(),
            detail: None,
            kind: None,
            depth: 0,
            highlight: None,
            target,
        }
    }
}

/// Picker items for a tree-sitter outline of the file at `path`. Points
/// are converted with `buffer` (the outline's byte offsets refer to it).
pub fn outline_items(
    items: &[OutlineItem],
    path: &std::path::Path,
    buffer: &arcode_buffer::Buffer,
) -> Vec<NavItem> {
    items
        .iter()
        .map(|item| {
            let start = buffer.char_to_point(buffer.byte_to_char(item.name_range.start));
            let end = buffer.char_to_point(buffer.byte_to_char(item.name_range.end));
            NavItem {
                kind: outline_tag(item.kind),
                depth: item.depth,
                ..NavItem::new(
                    item.name.clone(),
                    NavTarget {
                        path: path.to_path_buf(),
                        pos: NavPos::Points(start..end),
                    },
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_items_point_at_names() {
        let buffer = arcode_buffer::Buffer::new("fn main() {}\nstruct Point;\n");
        let language = arcode_syntax::language_by_id("rust").unwrap();
        let syntax = arcode_syntax::Syntax::new(language, buffer.text()).unwrap();
        let outline = syntax.symbols(buffer.text());
        let items = outline_items(&outline, std::path::Path::new("/p/a.rs"), &buffer);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "main");
        assert_eq!(items[0].kind.unwrap().label, "fn");
        assert_eq!(
            items[1].target.pos,
            NavPos::Points(Point::new(1, 7)..Point::new(1, 12))
        );
        assert_eq!(items[1].target.line(), 1);
    }

    #[test]
    fn every_common_kind_has_a_tag() {
        use lsp_types::SymbolKind as K;
        for k in [
            K::FUNCTION,
            K::STRUCT,
            K::ENUM,
            K::MODULE,
            K::CONSTANT,
            K::FIELD,
        ] {
            assert!(lsp_symbol_tag(k).is_some(), "{k:?}");
        }
        assert_eq!(outline_tag(OutlineKind::Trait).unwrap().label, "trait");
    }

    #[test]
    fn kinds_have_icons() {
        use lsp_types::SymbolKind as K;
        let icon = |k| lsp_symbol_tag(k).unwrap().icon();
        assert_eq!(icon(K::FUNCTION), IconName::SquareFunction);
        assert_eq!(icon(K::METHOD), IconName::SquareFunction);
        assert_eq!(icon(K::STRUCT), IconName::Braces);
        assert_eq!(icon(K::MODULE), IconName::Box);
        assert_eq!(icon(K::FIELD), IconName::Variable);
        assert_eq!(icon(K::CONSTANT), IconName::Variable);
        let outline = |k| outline_tag(k).unwrap().icon();
        assert_eq!(outline(OutlineKind::Impl), IconName::Braces);
        assert_eq!(outline(OutlineKind::Macro), IconName::SquareFunction);
        assert_eq!(outline(OutlineKind::Heading), IconName::FileText);
    }
}
