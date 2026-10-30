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
//!   (general patterns first, e.g. `function` before `method`). One pattern
//!   matching the same `@item` with different names lists each name.
//! - `(#set! level "n")` marks a heading of level `n`: the item spans from
//!   the heading to the next heading of level `n` or lower (a section), so
//!   sub-headings nest. `(#set! strip "atx_closing")` drops a Markdown
//!   closing `##` sequence from the name.

use std::ops::Range;

use ropey::Rope;

use crate::Syntax;
use crate::query_match::{matches, name_text, property};
use crate::rope_text::line_span;

mod headings;

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
    /// Display name, whitespace collapsed (e.g. `"main"`, `"Display for Point"`);
    /// string-literal names lose their quotes.
    pub name: String,
    /// What kind of symbol it is.
    pub kind: OutlineKind,
    /// Zero-based rope lines the symbol spans, end exclusive
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

/// An outline item found by the query, before dedup and depths.
struct Found {
    pattern: usize,
    /// Heading level for `(#set! level "n")` patterns (see [`headings`]).
    level: Option<u8>,
    item: OutlineItem,
}

impl Syntax {
    /// Lists the document's symbols in document order (parents before
    /// children), each with its nesting [`OutlineItem::depth`].
    ///
    /// Returns an empty `Vec` for languages without an outline query.
    /// Cost is linear in the file size (about 150 ms for 100k lines of Rust
    /// in release), so cache the result per parse and, for big files, call
    /// it off the UI thread. [`OutlineItem::range`] holds rope lines
    /// (ropey's line breaks, like [`Syntax::highlight_lines`]).
    /// `text` must be the text this tree was last parsed from.
    pub fn symbols(&self, text: &Rope) -> Vec<OutlineItem> {
        let Some(query) = self.language().outline_query() else {
            return Vec::new();
        };
        let mut found: Vec<Found> = Vec::new();
        for matched in matches(query, self.tree().root_node(), text) {
            let Some(item) = matched.node("item") else {
                continue;
            };
            let prop = |key| property(query, matched.pattern, key);
            let Some(kind) = prop("kind").and_then(OutlineKind::from_query_name) else {
                continue;
            };
            let names: Vec<_> = matched.nodes("name").collect();
            let (Some(first), Some(last)) = (names.first(), names.last()) else {
                continue;
            };
            let mut name = names
                .iter()
                .map(|node| name_text(text, *node))
                .collect::<Vec<_>>()
                .join(prop("join").unwrap_or(" "));
            if prop("strip") == Some("atx_closing") {
                name = headings::strip_atx_closing(&name).to_string();
            }
            if name.is_empty() {
                continue;
            }
            found.push(Found {
                pattern: matched.pattern,
                level: prop("level").and_then(|level| level.parse().ok()),
                item: OutlineItem {
                    name,
                    kind,
                    range: 0..0,
                    depth: 0,
                    byte_range: item.byte_range(),
                    name_range: first.start_byte()..last.end_byte(),
                },
            });
        }
        headings::extend_sections(&mut found, text.len_bytes());
        found.sort_by_key(|f| {
            let range = &f.item.byte_range;
            (
                range.start,
                std::cmp::Reverse(range.end),
                f.pattern,
                f.item.name_range.start,
            )
        });
        let mut items = dedup(found);
        assign_depths(&mut items);
        for item in &mut items {
            item.range = line_span(text, item.byte_range.clone());
        }
        items
    }
}

/// Removes duplicates of one `@item` node: a later pattern replaces every
/// earlier one; the same pattern keeps each distinct name (Go
/// `const a, b = 1, 2` yields `a` and `b`). `found` must be sorted.
fn dedup(found: Vec<Found>) -> Vec<OutlineItem> {
    let mut kept: Vec<Found> = Vec::with_capacity(found.len());
    for next in found {
        let same_node = |f: &Found| f.item.byte_range == next.item.byte_range;
        match kept.last() {
            Some(last) if same_node(last) && last.pattern != next.pattern => {
                while kept.last().is_some_and(same_node) {
                    kept.pop();
                }
            }
            Some(last) if same_node(last) && last.item.name_range == next.item.name_range => {
                continue;
            }
            _ => {}
        }
        kept.push(next);
    }
    kept.into_iter().map(|found| found.item).collect()
}

/// Sets each item's depth from range nesting. `items` must be sorted by
/// start ascending, end descending; items with the same range are
/// siblings.
fn assign_depths(items: &mut [OutlineItem]) {
    let mut open: Vec<Range<usize>> = Vec::new();
    for item in items {
        while open
            .last()
            .is_some_and(|r| r.end <= item.byte_range.start || *r == item.byte_range)
        {
            open.pop();
        }
        item.depth = open.len();
        open.push(item.byte_range.clone());
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
    fn same_range_items_are_siblings() {
        let mut items = vec![item(0..100), item(10..20), item(10..20), item(30..40)];
        assign_depths(&mut items);
        let depths: Vec<_> = items.iter().map(|i| i.depth).collect();
        assert_eq!(depths, [0, 1, 1, 1]);
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
