//! The navigation picker: a modal list of symbols or references with a
//! filter field. References are grouped under their file; symbols are
//! ranked fuzzily (or, for workspace symbols, filtered by the server: the
//! query is reported and new items are set from outside).

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window,
};

use super::item::NavItem;
use crate::finder::{Confirm, Dismiss, SelectNext, SelectPrev};
use crate::text_field::{TextChanged, TextField};

/// Most rows ranked for a query.
const MAX_RESULTS: usize = 1000;

/// Events of a [`NavPicker`].
pub enum NavPickerEvent {
    /// The user chose an item.
    Confirmed(NavItem),
    /// The picker should close.
    Dismissed,
    /// The query changed (pickers whose server filters).
    QueryChanged(String),
}

/// How a picker lists its items.
#[derive(Debug, Clone, Default)]
pub struct PickerOptions {
    /// Placeholder of the filter field.
    pub placeholder: SharedString,
    /// Group items under a header per file (references).
    pub grouped: bool,
    /// The server filters: show items as given, report query changes.
    pub server_query: bool,
    /// Project root, to show paths relative to it.
    pub root: Option<PathBuf>,
}

/// A visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A file header with the number of its items.
    Header { path: PathBuf, count: usize },
    /// An item and the byte ranges of its label that matched the query.
    Item {
        ix: usize,
        matches: Vec<Range<usize>>,
    },
}

/// The picker modal.
pub struct NavPicker {
    pub(super) input: Entity<TextField>,
    pub(super) items: Vec<NavItem>,
    pub(super) options: PickerOptions,
    pub(super) loading: bool,
    pub(super) rows: Vec<Row>,
    pub(super) selected: usize,
    pub(super) scroll: UniformListScrollHandle,
    _sub: Subscription,
}

impl EventEmitter<NavPickerEvent> for NavPicker {}

impl Focusable for NavPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl NavPicker {
    /// A picker showing "Loading…" until [`NavPicker::set_items`].
    pub fn new(options: PickerOptions, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextField::new(options.placeholder.clone(), cx));
        let sub = cx.subscribe(&input, |picker, input, _: &TextChanged, cx| {
            if picker.options.server_query {
                let query = input.read(cx).text().to_string();
                picker.loading = true;
                cx.emit(NavPickerEvent::QueryChanged(query));
            }
            picker.refilter(cx);
        });
        NavPicker {
            input,
            items: Vec::new(),
            options,
            loading: true,
            rows: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            _sub: sub,
        }
    }

    /// Replaces the items and stops showing "Loading…".
    pub fn set_items(&mut self, items: Vec<NavItem>, cx: &mut Context<Self>) {
        self.items = items;
        self.loading = false;
        self.refilter(cx);
    }

    /// The items shown, in row order (tests).
    pub fn visible_labels(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Item { ix, .. } => Some(self.items[*ix].label.clone()),
                Row::Header { .. } => None,
            })
            .collect()
    }

    /// Whether the items arrived.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_string();
        self.rows = build_rows(&self.items, &query, &self.options);
        self.selected = self.first_item_from(0, true).unwrap_or(0);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// The nearest item row from `ix` going forward or back.
    fn first_item_from(&self, ix: usize, forward: bool) -> Option<usize> {
        let is_item = |i: &usize| matches!(self.rows.get(*i), Some(Row::Item { .. }));
        if forward {
            (ix..self.rows.len()).find(is_item)
        } else {
            (0..=ix.min(self.rows.len().saturating_sub(1)))
                .rev()
                .find(is_item)
        }
    }

    pub(super) fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if matches!(self.rows.get(ix), Some(Row::Item { .. })) {
            let from = self.selected;
            self.selected = ix;
            // Going up to a file's first item also shows its header.
            let header_above = ix
                .checked_sub(1)
                .filter(|h| matches!(self.rows.get(*h), Some(Row::Header { .. })));
            let target = if ix < from {
                header_above.unwrap_or(ix)
            } else {
                ix
            };
            self.scroll
                .scroll_to_item(target, crate::picker_frame::scroll_strategy(from, ix));
            cx.notify();
        }
    }

    pub(super) fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        let prev = self
            .selected
            .checked_sub(1)
            .and_then(|i| self.first_item_from(i, false))
            .or_else(|| self.first_item_from(self.rows.len().saturating_sub(1), false));
        if let Some(ix) = prev {
            self.select(ix, cx);
        }
    }

    pub(super) fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let next = self
            .first_item_from(self.selected + 1, true)
            .or_else(|| self.first_item_from(0, true));
        if let Some(ix) = next {
            self.select(ix, cx);
        }
    }

    pub(super) fn confirm_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(Row::Item { ix, .. }) = self.rows.get(ix) {
            cx.emit(NavPickerEvent::Confirmed(self.items[*ix].clone()));
        }
    }

    pub(super) fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.confirm_row(self.selected, cx);
    }

    pub(super) fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(NavPickerEvent::Dismissed);
    }

    /// `path` relative to the project root when inside it.
    pub(super) fn display_path(&self, path: &Path) -> String {
        self.options
            .root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

/// The rows for `query`: grouped pickers keep their order and match the
/// label or file name (case-insensitive); others rank labels fuzzily.
pub fn build_rows(items: &[NavItem], query: &str, options: &PickerOptions) -> Vec<Row> {
    let query = query.trim();
    let ranked: Vec<(usize, Vec<Range<usize>>)> = if options.server_query || query.is_empty() {
        (0..items.len()).map(|ix| (ix, Vec::new())).collect()
    } else if options.grouped {
        let needle = query.to_lowercase();
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                let file = item.target.path.file_name().unwrap_or_default();
                item.label.to_lowercase().contains(&needle)
                    || file.to_string_lossy().to_lowercase().contains(&needle)
            })
            .map(|(ix, _)| (ix, Vec::new()))
            .collect()
    } else {
        let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
        crate::fuzzy::rank(&labels, query, MAX_RESULTS)
            .into_iter()
            .map(|m| (m.index, m.highlights))
            .collect()
    };
    let mut rows = Vec::with_capacity(ranked.len() + 8);
    let mut header: Option<usize> = None;
    for (ix, matches) in ranked {
        if options.grouped {
            let path = &items[ix].target.path;
            let same = header
                .is_some_and(|h| matches!(&rows[h], Row::Header { path: p, .. } if p == path));
            if !same {
                header = Some(rows.len());
                rows.push(Row::Header {
                    path: path.clone(),
                    count: 0,
                });
            }
            if let Some(Row::Header { count, .. }) = header.and_then(|h| rows.get_mut(h)) {
                *count += 1;
            }
        }
        rows.push(Row::Item { ix, matches });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::item::{NavPos, NavTarget};
    use arcode_buffer::Point;

    fn item(label: &str, file: &str, line: usize) -> NavItem {
        NavItem::new(
            label,
            NavTarget {
                path: PathBuf::from(file),
                pos: NavPos::Points(Point::new(line, 0)..Point::new(line, 1)),
            },
        )
    }

    #[test]
    fn grouped_rows_have_one_header_per_file() {
        let items = vec![
            item("let a = helper();", "a.rs", 1),
            item("helper()", "a.rs", 5),
            item("fn helper() {}", "b.rs", 2),
        ];
        let options = PickerOptions {
            grouped: true,
            ..Default::default()
        };
        let rows = build_rows(&items, "", &options);
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[0],
            Row::Header {
                path: "a.rs".into(),
                count: 2
            }
        );
        let rows = build_rows(&items, "FN ", &options);
        assert_eq!(rows.len(), 2, "{rows:?}");
        let rows = build_rows(&items, "b.rs", &options);
        assert!(matches!(&rows[0], Row::Header { count: 1, .. }));
    }

    #[test]
    fn symbols_rank_fuzzily_with_match_ranges() {
        let items = vec![item("main", "a.rs", 0), item("make_point", "a.rs", 3)];
        let rows = build_rows(&items, "mkp", &PickerOptions::default());
        assert_eq!(rows.len(), 1);
        let Row::Item { ix, matches } = &rows[0] else {
            panic!("item row");
        };
        assert_eq!(*ix, 1);
        assert!(!matches.is_empty());
        let server = PickerOptions {
            server_query: true,
            ..Default::default()
        };
        assert_eq!(build_rows(&items, "zzz", &server).len(), 2);
    }
}
