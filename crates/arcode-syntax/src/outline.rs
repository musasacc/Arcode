//! Outline: the symbols of a document (functions, types, modules, ...) for
//! breadcrumbs and the outline panel, found with per-language queries
//! (`queries/<lang>/outline.scm`).
//!
//! Query conventions:
//! - a pattern captures the whole symbol as `@item`, its name as `@name`
//!   and sets `(#set! kind "<kind>")` (see [`OutlineKind::from_query_name`]).
//! - several `@name` captures are joined with the pattern's
//!   `(#set! join "...")` value (default `" "`), e.g. Rust
//!   `impl Display for Point` becomes `Display for Point`.
//! - when several patterns match the same `@item` node, the later one wins
//!   (general patterns first, e.g. `function` before `method`).

use std::ops::Range;

use ropey::Rope;

use crate::Syntax;
use crate::query_match::{compact_text, matches, property};

/// What kind of symbol an [`OutlineItem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutlineKind {
    /// A free function.
    Function,
    /// A function inside a class, impl, trait or interface.
    Method,
    /// A constructor (`constructor()`, `__init__` is a [`OutlineKind::Method`]).
    Constructor,
    /// A struct or union.
    Struct,
    /// An enum.
    Enum,
    /// A Rust trait.
    Trait,
    /// A Rust `impl` block.
    Impl,
    /// A class.
    Class,
    /// An interface (TS, Go, Java).
    Interface,
    /// A module or namespace.
    Module,
    /// A constant or static.
    Const,
    /// A type alias or other named type.
    Type,
    /// A macro definition.
    Macro,
    /// A document heading (Markdown).
    Heading,
}

impl OutlineKind {
    /// Parses the `kind` property used in outline queries.
    pub fn from_query_name(name: &str) -> Option<OutlineKind> {
        Some(match name {
            "function" => OutlineKind::Function,
            "method" => OutlineKind::Method,
            "constructor" => OutlineKind::Constructor,
            "struct" => OutlineKind::Struct,
            "enum" => OutlineKind::Enum,
            "trait" => OutlineKind::Trait,
            "impl" => OutlineKind::Impl,
            "class" => OutlineKind::Class,
            "interface" => OutlineKind::Interface,
            "module" => OutlineKind::Module,
            "const" => OutlineKind::Const,
            "type" => OutlineKind::Type,
            "macro" => OutlineKind::Macro,
            "heading" => OutlineKind::Heading,
            _ => return None,
        })
    }
}

/// One symbol in the document outline.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutlineItem {
    /// Display name, whitespace collapsed (e.g. `"main"`, `"Display for Point"`).
    pub name: String,
    /// What kind of symbol it is.
    pub kind: OutlineKind,
    /// Zero-based lines the symbol spans, end exclusive
    /// (`start_line..last_line + 1`).
    pub range: Range<usize>,
    /// Nesting depth: the number of outline items that enclose this one.
    pub depth: usize,
    /// Byte range of the whole symbol.
    pub byte_range: Range<usize>,
    /// Byte range of the name (first to last `@name` capture), e.g. to put
    /// the cursor on the name when the item is picked.
    pub name_range: Range<usize>,
}

impl Syntax {
    /// Lists the document's symbols in document order (parents before
    /// children), each with its nesting [`OutlineItem::depth`].
    ///
    /// Returns an empty `Vec` for languages without an outline query.
    /// Cost is linear in the file size (about 150 ms for 100k lines of Rust
    /// in release), so cache the result per parse and, for big files, call
    /// it off the UI thread.
    /// `text` must be the text this tree was last parsed from.
    pub fn symbols(&self, text: &Rope) -> Vec<OutlineItem> {
        let Some(query) = self.language().outline_query() else {
            return Vec::new();
        };
        let mut found: Vec<(usize, OutlineItem)> = Vec::new();
        for matched in matches(query, self.tree().root_node(), text) {
            let Some(item) = matched.node("item") else {
                continue;
            };
            let Some(kind) =
                property(query, matched.pattern, "kind").and_then(OutlineKind::from_query_name)
            else {
                continue;
            };
            let names: Vec<_> = matched.nodes("name").collect();
            let (Some(first), Some(last)) = (names.first(), names.last()) else {
                continue;
            };
            let join = property(query, matched.pattern, "join").unwrap_or(" ");
            let name = names
                .iter()
                .map(|node| compact_text(text, *node))
                .collect::<Vec<_>>()
                .join(join);
            if name.is_empty() {
                continue;
            }
            let end = item.end_position();
            // A node ending at column 0 ends on the previous line's break.
            let last_line = if end.column == 0 && end.row > item.start_position().row {
                end.row - 1
            } else {
                end.row
            };
            found.push((
                matched.pattern,
                OutlineItem {
                    name,
                    kind,
                    range: item.start_position().row..last_line + 1,
                    depth: 0,
                    byte_range: item.byte_range(),
                    name_range: first.start_byte()..last.end_byte(),
                },
            ));
        }
        found.sort_by(|(pa, a), (pb, b)| {
            (a.byte_range.start, b.byte_range.end, pa).cmp(&(
                b.byte_range.start,
                a.byte_range.end,
                pb,
            ))
        });
        let mut items: Vec<OutlineItem> = Vec::with_capacity(found.len());
        for (_, next) in found {
            match items.last_mut() {
                Some(last) if last.byte_range == next.byte_range => *last = next,
                _ => items.push(next),
            }
        }
        assign_depths(&mut items);
        items
    }
}

/// Sets each item's depth from range nesting. `items` must be sorted by
/// start ascending, end descending, with no duplicate ranges.
fn assign_depths(items: &mut [OutlineItem]) {
    let mut open: Vec<usize> = Vec::new();
    for item in items {
        while open.last().is_some_and(|&end| end <= item.byte_range.start) {
            open.pop();
        }
        item.depth = open.len();
        open.push(item.byte_range.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(range: Range<usize>) -> OutlineItem {
        OutlineItem {
            name: "x".into(),
            kind: OutlineKind::Function,
            range: 0..1,
            depth: 0,
            byte_range: range,
            name_range: 0..0,
        }
    }

    #[test]
    fn depths_follow_nesting() {
        let mut items = vec![
            item(0..100),
            item(10..50),
            item(20..30),
            item(50..60),
            item(100..120),
        ];
        assign_depths(&mut items);
        let depths: Vec<_> = items.iter().map(|i| i.depth).collect();
        assert_eq!(depths, [0, 1, 2, 1, 0]);
    }

    #[test]
    fn kinds_parse() {
        assert_eq!(
            OutlineKind::from_query_name("impl"),
            Some(OutlineKind::Impl)
        );
        assert_eq!(
            OutlineKind::from_query_name("heading"),
            Some(OutlineKind::Heading)
        );
        assert_eq!(OutlineKind::from_query_name("nope"), None);
    }
}
