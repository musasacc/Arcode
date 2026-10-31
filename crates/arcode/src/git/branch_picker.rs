//! The branch picker: switch to a local branch or create a new one.
//! Opened from the branch in the title bar, the status bar and the Source
//! Control view. Uses the finder's key bindings (up/down/enter/escape).

use arcode_git::{Branch, Repo};
use arcode_ui_kit::{IconName, theme, truncated};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle,
    SharedString, Subscription, Task, Window, div, point, prelude::*, px,
};

use crate::finder::{Confirm, Dismiss, SelectNext, SelectPrev};
use crate::picker_frame::{
    LIST_PAD, count_status, empty_state, footer, input_row, list_height, offset_showing,
    picker_frame, picker_row, row_detail, row_icon, row_label,
};
use crate::text_field::{TextChanged, TextField};

const MAX_ROWS: usize = 10;

/// What the user chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchPickerEvent {
    /// Switch to this existing branch.
    Switch(String),
    /// Create this branch at `HEAD` and switch to it.
    Create(String),
    /// Closed without a choice.
    Dismissed,
}

/// One entry of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerItem {
    /// An existing branch (index into the branch list).
    Branch(usize),
    /// Create a branch with the query as its name.
    Create(String),
}

/// Entries for `query`: branches containing it (case-insensitive, the
/// current one first), then a create entry when no branch has that exact
/// name.
pub fn filter_items(branches: &[Branch], query: &str) -> Vec<PickerItem> {
    let query = query.trim();
    let lower = query.to_lowercase();
    let mut items: Vec<(bool, PickerItem)> = branches
        .iter()
        .enumerate()
        .filter(|(_, b)| b.name.to_lowercase().contains(&lower))
        .map(|(ix, b)| (!b.is_current, PickerItem::Branch(ix)))
        .collect();
    items.sort_by_key(|(not_current, _)| *not_current);
    let mut items: Vec<PickerItem> = items.into_iter().map(|(_, item)| item).collect();
    if !query.is_empty() && !branches.iter().any(|b| b.name == query) {
        items.push(PickerItem::Create(query.to_owned()));
    }
    items
}

/// The picker view.
pub struct BranchPicker {
    input: Entity<TextField>,
    branches: Vec<Branch>,
    items: Vec<PickerItem>,
    selected: usize,
    scroll: ScrollHandle,
    loading: bool,
    _load: Task<()>,
    _sub: Subscription,
}

impl EventEmitter<BranchPickerEvent> for BranchPicker {}

impl Focusable for BranchPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl BranchPicker {
    /// Lists the branches of `repo` (in the background).
    pub fn new(repo: Repo, cx: &mut Context<Self>) -> Self {
        let work = cx.background_spawn(async move { arcode_git::branches(&repo) });
        let load = cx.spawn(async move |this, cx| {
            let branches = work.await.unwrap_or_default();
            this.update(cx, |p, cx| p.set_branches(branches, cx)).ok();
        });
        Self::loading(load, cx)
    }

    /// An empty picker waiting for `load` to call [`Self::set_branches`].
    fn loading(load: Task<()>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextField::new("Switch to branch, or type a new name", cx));
        let sub = cx.subscribe(&input, |p, _, _: &TextChanged, cx| p.update_items(cx));
        Self {
            input,
            branches: Vec::new(),
            items: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            loading: true,
            _load: load,
            _sub: sub,
        }
    }

    fn set_branches(&mut self, branches: Vec<Branch>, cx: &mut Context<Self>) {
        self.branches = branches;
        self.loading = false;
        self.update_items(cx);
    }

    fn update_items(&mut self, cx: &mut Context<Self>) {
        self.items = filter_items(&self.branches, self.input.read(cx).text());
        self.selected = 0;
        self.reveal_selected();
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.items.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
            self.reveal_selected();
            cx.notify();
        }
    }

    /// The selected row (an index into the listed items).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The list's scroll offset (tests).
    pub fn scroll_offset(&self) -> f32 {
        f32::from(self.scroll.offset().y)
    }

    /// Scrolls the list so the selected row is in view.
    fn reveal_selected(&self) {
        let offset = f32::from(self.scroll.offset().y);
        let wanted = offset_showing(offset, self.selected, MAX_ROWS);
        if wanted != offset {
            self.scroll.set_offset(point(px(0.), px(wanted)));
        }
    }

    fn choose(&mut self, ix: usize, cx: &mut Context<Self>) {
        match self.items.get(ix) {
            Some(PickerItem::Branch(b)) => {
                let branch = &self.branches[*b];
                if branch.is_current {
                    cx.emit(BranchPickerEvent::Dismissed);
                } else {
                    cx.emit(BranchPickerEvent::Switch(branch.name.clone()));
                }
            }
            Some(PickerItem::Create(name)) => cx.emit(BranchPickerEvent::Create(name.clone())),
            None => {}
        }
    }

    fn render_item(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let selected = ix == self.selected;
        let (icon_name, label, detail): (IconName, SharedString, SharedString) =
            match &self.items[ix] {
                PickerItem::Branch(b) => {
                    let b = &self.branches[*b];
                    let mut detail = b.summary.clone();
                    if b.ahead + b.behind > 0 {
                        detail = format!("↑{} ↓{}  {detail}", b.ahead, b.behind);
                    }
                    let icon = if b.is_current {
                        IconName::Check
                    } else {
                        IconName::GitBranch
                    };
                    (icon, b.name.clone().into(), detail.into())
                }
                PickerItem::Create(name) => (
                    IconName::Plus,
                    format!("Create branch “{name}”").into(),
                    "from the current HEAD".into(),
                ),
            };
        let current =
            matches!(&self.items[ix], PickerItem::Branch(b) if self.branches[*b].is_current);
        let tint = current.then_some(c.accent);
        picker_row(("branch-item", ix), selected, cx)
            .child(row_icon(icon_name, selected, tint, cx))
            .child(row_label(truncated(label), !detail.is_empty()))
            .child(row_detail(truncated(detail), cx))
            .on_click(cx.listener(move |p, _, _, cx| p.choose(ix, cx)))
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = (0..self.items.len().min(200))
            .map(|ix| self.render_item(ix, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        let status = (!self.loading).then(|| {
            let shown = self
                .items
                .iter()
                .filter(|i| matches!(i, PickerItem::Branch(_)))
                .count();
            count_status(None, shown, self.branches.len())
        });
        picker_frame(px(520.), cx)
            .key_context("Finder")
            .on_action(cx.listener(|p, _: &SelectPrev, _, cx| p.step(-1, cx)))
            .on_action(cx.listener(|p, _: &SelectNext, _, cx| p.step(1, cx)))
            .on_action(cx.listener(|p, _: &Confirm, _, cx| p.choose(p.selected, cx)))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(BranchPickerEvent::Dismissed)))
            .child(input_row(
                IconName::GitBranch,
                None,
                self.input.clone(),
                Some("Branches".into()),
                cx,
            ))
            .when(empty, |el| {
                el.child(empty_state(
                    if self.loading {
                        "Loading branches…"
                    } else {
                        "No branches. Type a name to create one."
                    },
                    cx,
                ))
            })
            .when(!empty, |el| {
                el.child(
                    div()
                        .id("branch-list")
                        .track_scroll(&self.scroll)
                        .p(px(LIST_PAD))
                        .max_h(list_height(rows.len(), MAX_ROWS))
                        .overflow_y_scroll()
                        .children(rows),
                )
            })
            .child(footer("switch", status, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, current: bool) -> Branch {
        Branch {
            name: name.into(),
            is_current: current,
            commit: "abc".into(),
            summary: String::new(),
            upstream: None,
            ahead: 0,
            behind: 0,
            upstream_gone: false,
        }
    }

    #[test]
    fn filters_and_offers_creation() {
        let branches = [
            branch("feature", false),
            branch("main", true),
            branch("fix-ui", false),
        ];
        assert_eq!(
            filter_items(&branches, ""),
            [
                PickerItem::Branch(1),
                PickerItem::Branch(0),
                PickerItem::Branch(2)
            ]
        );
        assert_eq!(
            filter_items(&branches, "F"),
            [
                PickerItem::Branch(0),
                PickerItem::Branch(2),
                PickerItem::Create("F".into())
            ]
        );
        assert_eq!(filter_items(&branches, "main"), [PickerItem::Branch(1)]);
    }

    #[gpui::test]
    fn keyboard_selection_stays_in_view(cx: &mut gpui::TestAppContext) {
        use crate::picker_frame::ROW_H;
        crate::test_support::init(cx);
        let (picker, cx) = cx.add_window_view(|_, cx| {
            let mut p = BranchPicker::loading(Task::ready(()), cx);
            let branches = (0..16).map(|i| branch(&format!("b{i:02}"), i == 0));
            p.set_branches(branches.collect(), cx);
            p
        });
        cx.run_until_parked();
        // Ten rows show; the 13th comes in at the bottom, one row a time.
        for _ in 0..12 {
            picker.update(cx, |p, cx| p.step(1, cx));
            cx.run_until_parked();
        }
        picker.read_with(cx, |p, _| {
            assert_eq!(p.selected(), 12);
            assert_eq!(p.scroll_offset(), -3. * ROW_H);
        });
        // Back up past the top of the view, then round to the last row.
        for _ in 0..10 {
            picker.update(cx, |p, cx| p.step(-1, cx));
        }
        cx.run_until_parked();
        picker.read_with(cx, |p, _| assert_eq!(p.scroll_offset(), -2. * ROW_H));
        picker.update(cx, |p, cx| {
            p.step(-2, cx);
            p.step(-1, cx);
        });
        cx.run_until_parked();
        picker.read_with(cx, |p, _| {
            assert_eq!(p.selected(), 15);
            assert_eq!(p.scroll_offset(), -6. * ROW_H);
        });
    }
}
