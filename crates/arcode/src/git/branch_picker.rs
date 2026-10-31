//! The branch picker: switch to a local branch or create a new one.
//! Opened from the branch in the title bar, the status bar and the Source
//! Control view. Uses the finder's key bindings (up/down/enter/escape).

use arcode_git::{Branch, Repo};
use arcode_ui_kit::Elevated;
use arcode_ui_kit::{IconName, icon, kbd, theme};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};

use crate::finder::{Confirm, Dismiss, SelectNext, SelectPrev};
use crate::text_field::{TextChanged, TextField};

const ROW_HEIGHT: f32 = 30.;
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
        let input = cx.new(|cx| TextField::new("Switch to branch, or type a new name", cx));
        let sub = cx.subscribe(&input, |p, _, _: &TextChanged, cx| p.update_items(cx));
        let work = cx.background_spawn(async move { arcode_git::branches(&repo) });
        let load = cx.spawn(async move |this, cx| {
            let branches = work.await.unwrap_or_default();
            this.update(cx, |p, cx| {
                p.branches = branches;
                p.loading = false;
                p.update_items(cx);
            })
            .ok();
        });
        Self {
            input,
            branches: Vec::new(),
            items: Vec::new(),
            selected: 0,
            loading: true,
            _load: load,
            _sub: sub,
        }
    }

    fn update_items(&mut self, cx: &mut Context<Self>) {
        self.items = filter_items(&self.branches, self.input.read(cx).text());
        self.selected = 0;
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.items.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
            cx.notify();
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
        div()
            .id(("branch-item", ix))
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |el| el.bg(c.selection))
            .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
            .child(icon(icon_name).size(px(14.)).color(c.text_muted))
            .child(div().flex_none().whitespace_nowrap().child(label))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(detail),
            )
            .on_click(cx.listener(move |p, _, _, cx| p.choose(ix, cx)))
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let rows: Vec<_> = (0..self.items.len().min(200))
            .map(|ix| self.render_item(ix, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        div()
            .key_context("Finder")
            .on_action(cx.listener(|p, _: &SelectPrev, _, cx| p.step(-1, cx)))
            .on_action(cx.listener(|p, _: &SelectNext, _, cx| p.step(1, cx)))
            .on_action(cx.listener(|p, _: &Confirm, _, cx| p.choose(p.selected, cx)))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(BranchPickerEvent::Dismissed)))
            .w(px(520.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(40.))
                    .px_3()
                    .border_b_1()
                    .border_color(c.border)
                    .child(icon(IconName::GitBranch).size(px(16.)).color(c.text_muted))
                    .child(self.input.clone())
                    .child(kbd("Esc")),
            )
            .child(
                div()
                    .id("branch-list")
                    .p_1()
                    .max_h(px(ROW_HEIGHT * MAX_ROWS as f32 + 8.))
                    .overflow_y_scroll()
                    .when(empty, |el| {
                        el.child(div().px_3().py_2().text_color(c.text_muted).child(
                            if self.loading {
                                "Loading branches…"
                            } else {
                                "No branches. Type a name to create one."
                            },
                        ))
                    })
                    .children(rows),
            )
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
}
