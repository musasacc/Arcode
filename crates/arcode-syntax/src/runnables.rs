//! Runnables: `main` functions, tests, test modules and benchmarks found
//! with per-language queries (`queries/<lang>/runnables.scm`).
//!
//! Query conventions:
//! - a runnable pattern captures the item as `@run`, its name as `@name`
//!   (string quotes are stripped) and sets `(#set! kind "<kind>")` with
//!   kind `main`, `test`, `test_module` or `bench`. Several `@name`
//!   captures are joined with `(#set! join "...")` (default `" "`). A
//!   pattern without `@name` may set a fixed `(#set! name "...")`.
//! - a scope pattern captures `@scope` and `@name` (a Rust `mod`, a Python
//!   test class, a JS `describe(...)` block). Every scope strictly enclosing
//!   a runnable contributes to its [`RunnableItem::scope_path`]. One pattern
//!   may capture both `@scope` and `@run` (a `describe` block is both).
//! - an item preceded by attributes (Rust `#[test] fn`) is matched through
//!   the attribute: the pattern captures it as `@anchor` (no `@run`, no
//!   `@name`) and sets `target`, `skip` and optionally `require`; the item
//!   is the first sibling after the attribute run (see `src/anchor.rs`) and
//!   its name is the item's `name` field.
//! - when several patterns match the same `@run` node, the later one wins.
//! - `@name` captures of string literals keep their inner whitespace and lose
//!   their quotes; other names have whitespace runs collapsed.

use std::ops::Range;

use ropey::Rope;

use crate::Syntax;
use crate::anchor::{self, Anchor};
use crate::query_match::{matches, name_text, property, strictly_contains};
use crate::rope_text::line_of_byte;

/// What a [`RunnableItem`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunnableKind {
    /// A program entry point (`fn main`, `if __name__ == "__main__":`).
    Main,
    /// A single test function or test case.
    Test,
    /// A group of tests (`#[cfg(test)] mod tests`, `class TestFoo`,
    /// `describe("...")`).
    TestModule,
    /// A benchmark (`#[bench]`, `func BenchmarkXxx`).
    Bench,
}

impl RunnableKind {
    /// Parses the `kind` property used in runnables queries.
    pub fn from_query_name(name: &str) -> Option<RunnableKind> {
        match name {
            "main" => Some(RunnableKind::Main),
            "test" => Some(RunnableKind::Test),
            "test_module" => Some(RunnableKind::TestModule),
            "bench" => Some(RunnableKind::Bench),
            _ => None,
        }
    }
}

/// Something the user can run from the gutter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunnableItem {
    /// Zero-based line of the item (the `fn`/`def`/`func`/call line; for
    /// Rust, the line after the attributes).
    pub line: usize,
    /// What kind of runnable this is.
    pub kind: RunnableKind,
    /// The item's name: function name, test string, `"main"` or `"__main__"`.
    pub name: String,
    /// Names of enclosing scopes, outermost first: Rust `mod` names, Python
    /// test class, JS/TS `describe` titles. Empty at top level.
    pub scope_path: Vec<String>,
}

impl RunnableItem {
    /// `scope_path` and `name` joined by `separator`, e.g.
    /// `tests::parser::parses` with `"::"` (a Rust test filter).
    pub fn qualified_name(&self, separator: &str) -> String {
        let mut parts: Vec<&str> = self.scope_path.iter().map(String::as_str).collect();
        parts.push(&self.name);
        parts.join(separator)
    }
}

/// A runnable found by the query, before scopes are assigned.
struct Found {
    range: Range<usize>,
    pattern: usize,
    item: RunnableItem,
}

/// A runnable whose node is the item after an `@anchor` (see [`anchor`](crate::anchor)).
struct Pending {
    pattern: usize,
    kind: RunnableKind,
    fixed_name: Option<String>,
}

impl Syntax {
    /// Finds runnable items (mains, tests, test modules, benchmarks) in the
    /// whole document, in document order.
    ///
    /// Returns an empty `Vec` for languages without a runnables query.
    /// Cost is linear in the file size (about 150 ms for 100k lines of Rust
    /// in release), so cache the result per parse and, for big files, call
    /// it off the UI thread. [`RunnableItem::line`] is a rope line (ropey's
    /// line breaks, like [`Syntax::highlight_lines`]).
    /// `text` must be the text this tree was last parsed from.
    pub fn runnables(&self, text: &Rope) -> Vec<RunnableItem> {
        let Some(query) = self.language().runnables_query() else {
            return Vec::new();
        };
        let root = self.tree().root_node();
        let mut found: Vec<Found> = Vec::new();
        let mut scopes: Vec<(Range<usize>, String)> = Vec::new();
        let mut anchors: Vec<Anchor<'_>> = Vec::new();
        let mut pending: Vec<Pending> = Vec::new();
        for matched in matches(query, root, text) {
            let parts: Vec<String> = matched.nodes("name").map(|n| name_text(text, n)).collect();
            let join = property(query, matched.pattern, "join").unwrap_or(" ");
            let name = (!parts.is_empty()).then(|| parts.join(join));
            if let Some(scope) = matched.node("scope")
                && let Some(name) = &name
            {
                scopes.push((scope.byte_range(), name.clone()));
            }
            let Some(kind) =
                property(query, matched.pattern, "kind").and_then(RunnableKind::from_query_name)
            else {
                continue;
            };
            let fixed = property(query, matched.pattern, "name").map(str::to_string);
            if let Some(anchor) = matched.node("anchor") {
                anchors.push(Anchor {
                    node: anchor,
                    pattern: matched.pattern,
                });
                pending.push(Pending {
                    pattern: matched.pattern,
                    kind,
                    fixed_name: name.or(fixed),
                });
                continue;
            }
            let (Some(run), Some(name)) = (matched.node("run"), name.or(fixed)) else {
                continue;
            };
            found.push(new_found(text, run, matched.pattern, kind, name));
        }
        let targets = anchor::resolve(query, root, &anchors);
        for (target, pending) in targets.into_iter().zip(pending) {
            let Some(run) = target else {
                continue;
            };
            let name = pending.fixed_name.or_else(|| {
                run.child_by_field_name("name")
                    .map(|node| name_text(text, node))
            });
            if let Some(name) = name {
                found.push(new_found(text, run, pending.pattern, pending.kind, name));
            }
        }
        found.sort_by(|a, b| {
            (a.range.start, b.range.end, a.pattern).cmp(&(b.range.start, a.range.end, b.pattern))
        });
        // Same node matched by several patterns: keep the last pattern.
        let mut items: Vec<Found> = Vec::with_capacity(found.len());
        for next in found {
            match items.last_mut() {
                Some(last) if last.range == next.range => *last = next,
                _ => items.push(next),
            }
        }
        assign_scopes(&mut items, scopes);
        items.into_iter().map(|found| found.item).collect()
    }
}

fn new_found(
    text: &Rope,
    run: tree_sitter::Node<'_>,
    pattern: usize,
    kind: RunnableKind,
    name: String,
) -> Found {
    Found {
        range: run.byte_range(),
        pattern,
        item: RunnableItem {
            line: line_of_byte(text, run.start_byte()),
            kind,
            name,
            scope_path: Vec::new(),
        },
    }
}

/// Sets each item's `scope_path` to the scopes strictly enclosing it, in
/// one stack walk. `items` must be sorted by start ascending, end
/// descending. Scopes with the same range (one node matched by several
/// scope patterns) count once.
fn assign_scopes(items: &mut [Found], mut scopes: Vec<(Range<usize>, String)>) {
    scopes.sort_by_key(|(range, _)| (range.start, std::cmp::Reverse(range.end)));
    scopes.dedup_by(|next, prev| next.0 == prev.0);
    let mut scopes = scopes.into_iter().peekable();
    let mut open: Vec<(Range<usize>, String)> = Vec::new();
    for found in items {
        let key = (found.range.start, std::cmp::Reverse(found.range.end));
        while let Some(scope) = scopes.next_if(|(r, _)| (r.start, std::cmp::Reverse(r.end)) < key) {
            while open.last().is_some_and(|(r, _)| !contains(r, &scope.0)) {
                open.pop();
            }
            open.push(scope);
        }
        while open
            .last()
            .is_some_and(|(r, _)| !strictly_contains(r, &found.range))
        {
            open.pop();
        }
        found.item.scope_path = open.iter().map(|(_, name)| name.clone()).collect();
    }
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_parse() {
        assert_eq!(
            RunnableKind::from_query_name("main"),
            Some(RunnableKind::Main)
        );
        assert_eq!(
            RunnableKind::from_query_name("test"),
            Some(RunnableKind::Test)
        );
        assert_eq!(
            RunnableKind::from_query_name("test_module"),
            Some(RunnableKind::TestModule)
        );
        assert_eq!(
            RunnableKind::from_query_name("bench"),
            Some(RunnableKind::Bench)
        );
        assert_eq!(RunnableKind::from_query_name("other"), None);
    }

    #[test]
    fn qualified_name_joins_scopes() {
        let item = RunnableItem {
            line: 0,
            kind: RunnableKind::Test,
            name: "parses".into(),
            scope_path: vec!["tests".into(), "parser".into()],
        };
        assert_eq!(item.qualified_name("::"), "tests::parser::parses");
        let top = RunnableItem {
            scope_path: Vec::new(),
            ..item
        };
        assert_eq!(top.qualified_name("::"), "parses");
    }
}
