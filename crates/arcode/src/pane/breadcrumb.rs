//! The breadcrumb at the right end of a pane's tab strip (docs/DESIGN.md
//! 6.6): the active file's folders and name relative to the project, then
//! the symbols around the cursor (`src › main.rs › fn broken`).
//!
//! Symbols come from the editor's outline (a tree-sitter query). It is
//! cached per editor and buffer version and computed again a moment after
//! the last edit, never on every keystroke.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use arcode_editor::Editor;
use arcode_syntax::{OutlineItem, OutlineKind};
use arcode_ui_kit::metrics::{WEIGHT_MEDIUM, type_scale};
use arcode_ui_kit::{IconName, icon, theme};
use gpui::{AnyElement, Context, Entity, EntityId, SharedString, Task, div, prelude::*, px, relative};

use super::Pane;

/// How long after the last edit the outline is computed again.
const OUTLINE_DELAY: Duration = Duration::from_millis(400);
/// Files with more lines show no symbols (the outline is linear in size).
const MAX_OUTLINE_LINES: usize = 20_000;

/// One symbol of the outline, as the breadcrumb shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Zero-based lines it spans, end exclusive.
    pub lines: Range<usize>,
    /// `fn broken`, `impl Rect`, or the bare name outside Rust.
    pub label: String,
}

/// The cached outline of the pane's active editor.
#[derive(Default)]
pub(super) struct Crumbs {
    editor: Option<EntityId>,
    /// Buffer version the symbols were computed for.
    version: u64,
    /// Version a recomputation is scheduled for.
    scheduled: Option<u64>,
    symbols: Vec<Symbol>,
    pending: Option<Task<()>>,
}

/// The parts of `path` before the symbols: its folders and name relative
/// to `root`, or the parent folder and name for files outside the project.
pub fn path_parts(root: Option<&Path>, path: &Path) -> Vec<String> {
    let names = |p: &Path| -> Vec<String> {
        p.components()
            .filter_map(|c| match c {
                std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    if let Some(rel) = root.and_then(|root| path.strip_prefix(root).ok()) {
        let parts = names(rel);
        if !parts.is_empty() {
            return parts;
        }
    }
    let all = names(path);
    let skip = all.len().saturating_sub(2);
    all.into_iter().skip(skip).collect()
}

/// The labels of the symbols enclosing `line`, outermost first.
pub fn symbol_chain(symbols: &[Symbol], line: usize) -> Vec<&str> {
    symbols
        .iter()
        .filter(|s| s.lines.contains(&line))
        .map(|s| s.label.as_str())
        .collect()
}

/// How a symbol reads in the breadcrumb: Rust items keep their keyword
/// (`fn main`, `impl Rect`), other languages show the name.
pub fn symbol_label(kind: OutlineKind, name: &str, rust: bool) -> String {
    if !rust {
        return name.to_string();
    }
    let keyword = match kind {
        OutlineKind::Function | OutlineKind::Method | OutlineKind::Constructor => "fn",
        OutlineKind::Struct => "struct",
        OutlineKind::Enum => "enum",
        OutlineKind::Trait => "trait",
        OutlineKind::Impl => "impl",
        OutlineKind::Module => "mod",
        OutlineKind::Const => "const",
        OutlineKind::Type => "type",
        OutlineKind::Macro => "macro",
        _ => return name.to_string(),
    };
    format!("{keyword} {name}")
}

/// The breadcrumb symbols of an outline.
pub fn symbols_of(items: &[OutlineItem], rust: bool) -> Vec<Symbol> {
    items
        .iter()
        .map(|item| Symbol {
            lines: item.range.clone(),
            label: symbol_label(item.kind, &item.name, rust),
        })
        .collect()
}

fn outline_of(editor: &Editor) -> Vec<Symbol> {
    if editor.is_large_file() || editor.line_count() > MAX_OUTLINE_LINES {
        return Vec::new();
    }
    symbols_of(&editor.outline(), editor.language().id == "rust")
}

impl Pane {
    /// Sets the project root the breadcrumb paths are relative to.
    pub fn set_project_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root != root {
            self.root = root;
            cx.notify();
        }
    }

    /// Brings the outline cache up to date with `editor`: computed at once
    /// for a newly shown editor, a moment after the last edit otherwise.
    fn sync_crumbs(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        let version = editor.read(cx).buffer().version();
        if self.crumbs.editor != Some(id) {
            self.crumbs = Crumbs {
                editor: Some(id),
                version,
                scheduled: None,
                symbols: outline_of(editor.read(cx)),
                pending: None,
            };
            return;
        }
        if version == self.crumbs.version || self.crumbs.scheduled == Some(version) {
            return;
        }
        self.crumbs.scheduled = Some(version);
        let editor = editor.downgrade();
        self.crumbs.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(OUTLINE_DELAY).await;
            this.update(cx, |pane, cx| {
                let Some(editor) = editor.upgrade() else {
                    return;
                };
                if pane.crumbs.editor == Some(editor.entity_id()) {
                    let e = editor.read(cx);
                    pane.crumbs.version = e.buffer().version();
                    pane.crumbs.symbols = outline_of(e);
                    pane.crumbs.scheduled = None;
                    pane.crumbs.pending = None;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The breadcrumb of the active editor (nothing for pages, diff tabs
    /// and files without a path).
    pub(super) fn render_breadcrumb(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.active_editor()?.clone();
        if self.titles.contains_key(&editor.entity_id()) {
            return None;
        }
        self.sync_crumbs(&editor, cx);
        let e = editor.read(cx);
        let path = e.path()?.to_path_buf();
        let line = e.cursor_position().line;
        let mut parts: Vec<SharedString> = path_parts(self.root.as_deref(), &path)
            .into_iter()
            .map(SharedString::from)
            .collect();
        parts.extend(
            symbol_chain(&self.crumbs.symbols, line)
                .into_iter()
                .map(|s| SharedString::from(s.to_string())),
        );
        let c = theme(cx).colors.clone();
        let last = parts.len().saturating_sub(1);
        let mut row = div()
            .id("breadcrumb")
            .flex()
            .flex_shrink()
            .min_w_0()
            .max_w(relative(0.5))
            .overflow_hidden()
            // Overflow goes to the left: the symbol name stays readable.
            .justify_end()
            .items_center()
            .gap(px(5.))
            .ml(px(8.))
            .text_size(type_scale(cx).meta)
            .text_color(c.text_muted)
            .whitespace_nowrap();
        for (ix, part) in parts.into_iter().enumerate() {
            if ix > 0 {
                row = row.child(
                    div().flex_none().child(
                        icon(IconName::ChevronRight)
                            .size(px(12.))
                            .color(c.text_disabled),
                    ),
                );
            }
            row = row.child(div().flex_none().when(ix == last, |el| {
                el.text_color(c.text).font_weight(WEIGHT_MEDIUM)
            }).child(part));
        }
        let tooltip = SharedString::from(path.display().to_string());
        Some(
            row.tooltip(move |_, cx| arcode_ui_kit::Tooltip::text(tooltip.clone(), cx))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(lines: Range<usize>, label: &str) -> Symbol {
        Symbol {
            lines,
            label: label.to_string(),
        }
    }

    #[test]
    fn paths_are_relative_to_the_project() {
        let root = Path::new("/home/me/demo");
        assert_eq!(
            path_parts(Some(root), Path::new("/home/me/demo/src/main.rs")),
            ["src", "main.rs"]
        );
        assert_eq!(
            path_parts(Some(root), Path::new("/home/me/demo/Cargo.toml")),
            ["Cargo.toml"]
        );
        // Outside the project: the parent folder and the name.
        assert_eq!(
            path_parts(Some(root), Path::new("/usr/lib/rust/src/lib.rs")),
            ["src", "lib.rs"]
        );
        assert_eq!(path_parts(None, Path::new("/a/b/c/d.rs")), ["c", "d.rs"]);
        assert_eq!(path_parts(None, Path::new("/d.rs")), ["d.rs"]);
    }

    #[test]
    fn the_chain_lists_enclosing_symbols_outermost_first() {
        let symbols = [
            sym(0..3, "struct Rect"),
            sym(5..20, "impl Rect"),
            sym(6..9, "fn area"),
            sym(10..14, "fn perimeter"),
        ];
        assert_eq!(symbol_chain(&symbols, 7), ["impl Rect", "fn area"]);
        assert_eq!(symbol_chain(&symbols, 9), ["impl Rect"]);
        assert_eq!(symbol_chain(&symbols, 2), ["struct Rect"]);
        assert!(symbol_chain(&symbols, 4).is_empty());
        assert!(symbol_chain(&symbols, 20).is_empty());
    }

    #[test]
    fn rust_symbols_keep_their_keyword() {
        assert_eq!(symbol_label(OutlineKind::Function, "main", true), "fn main");
        assert_eq!(symbol_label(OutlineKind::Method, "area", true), "fn area");
        assert_eq!(symbol_label(OutlineKind::Impl, "Rect", true), "impl Rect");
        assert_eq!(
            symbol_label(OutlineKind::Impl, "Display for Rect", true),
            "impl Display for Rect"
        );
        assert_eq!(symbol_label(OutlineKind::Struct, "Rect", true), "struct Rect");
        assert_eq!(symbol_label(OutlineKind::Function, "main", false), "main");
        assert_eq!(symbol_label(OutlineKind::Class, "Shape", false), "Shape");
        assert_eq!(symbol_label(OutlineKind::Heading, "Intro", true), "Intro");
    }

    #[test]
    fn outline_items_become_symbols() {
        let item = |name: &str, kind, range: Range<usize>| OutlineItem {
            name: name.to_string(),
            kind,
            range,
            depth: 0,
            byte_range: 0..0,
            name_range: 0..0,
        };
        let items = [
            item("main", OutlineKind::Function, 5..10),
            item("Rect", OutlineKind::Struct, 0..4),
        ];
        assert_eq!(
            symbols_of(&items, true),
            [sym(5..10, "fn main"), sym(0..4, "struct Rect")]
        );
        assert_eq!(symbols_of(&items, false)[0].label, "main");
    }
}
