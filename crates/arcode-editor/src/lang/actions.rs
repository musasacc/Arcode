//! Code actions: the light bulb on the cursor line (asked 250 ms after the
//! cursor stops), the menu (Ctrl+. for every action, "Refactor..." for
//! refactorings only, or a click on the bulb) and applying the picked one
//! through the provider.

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_ui_kit::settings;
use gpui::{Context, Task};

use super::action_rows::{ActionGroup, action_group, action_rows};
use super::{CODE_ACTIONS_POPOVER, CodeAction};
use crate::Editor;
use crate::menu::OnPick;

/// How long the cursor must rest before the light bulb is asked for.
const BULB_DELAY: Duration = Duration::from_millis(250);

/// Code actions of one editor.
#[derive(Default)]
pub(crate) struct ActionsState {
    /// Line of the light bulb and whether a quick fix is among the
    /// actions (drawn in the warning color).
    pub bulb: Option<(usize, bool)>,
    /// The last answer: buffer version, range and actions.
    cached: Option<(u64, Range<Point>, Rc<Vec<CodeAction>>)>,
    seq: u64,
    bulb_task: Option<Task<()>>,
    menu_task: Option<Task<()>>,
}

impl Editor {
    /// The range code actions are asked for: the primary selection.
    fn action_range(&self) -> Range<Point> {
        let sel = self.selections.primary();
        let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        self.buffer.char_to_point(a)..self.buffer.char_to_point(b)
    }

    fn cursor_line(&self) -> usize {
        self.buffer.char_to_line(self.selections.primary().head)
    }

    fn cached_actions(&self) -> Option<Rc<Vec<CodeAction>>> {
        let (version, range, actions) = self.lang.actions.cached.as_ref()?;
        (*version == self.buffer.version() && *range == self.action_range())
            .then(|| actions.clone())
    }

    /// The line showing the light bulb, if any.
    pub fn lightbulb_line(&self) -> Option<usize> {
        self.lang.actions.bulb.map(|(line, _)| line)
    }

    fn set_bulb(&mut self, bulb: Option<(usize, bool)>, cx: &mut Context<Self>) {
        if self.lang.actions.bulb != bulb {
            self.lang.actions.bulb = bulb;
            cx.notify();
        }
    }

    /// After a cursor move or edit: keeps the bulb while the cursor stays on
    /// its line and asks for the actions at the cursor once it rests.
    pub(crate) fn bulb_schedule(&mut self, cx: &mut Context<Self>) {
        let wanted = self.lang.provider.is_some()
            && !self.read_only
            && self.selections.len() == 1
            && settings(cx).editor.code_action_lightbulb;
        self.lang.actions.seq += 1;
        if !wanted {
            self.lang.actions.bulb_task = None;
            self.set_bulb(None, cx);
            return;
        }
        if let Some(actions) = self.cached_actions() {
            let bulb = bulb_for(&actions, self.cursor_line());
            self.set_bulb(bulb, cx);
            return;
        }
        if self
            .lang
            .actions
            .bulb
            .is_some_and(|(line, _)| line != self.cursor_line())
        {
            self.set_bulb(None, cx);
        }
        let seq = self.lang.actions.seq;
        self.lang.actions.bulb_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BULB_DELAY).await;
            let Ok(Some(request)) = this.update(cx, |e, cx| e.request_actions(cx)) else {
                return;
            };
            let answer = request.await;
            this.update(cx, |e, cx| {
                if e.lang.actions.seq != seq {
                    return;
                }
                let bulb = answer.and_then(|actions| bulb_for(&actions, e.cursor_line()));
                e.set_bulb(bulb, cx);
            })
            .ok();
        }));
    }

    /// Asks the provider for the actions at the cursor; the answer is
    /// cached for this version and range.
    fn request_actions(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Task<Option<Rc<Vec<CodeAction>>>>> {
        let provider = self.lang.provider.clone()?;
        let range = self.action_range();
        let version = self.buffer.version();
        let request = provider.code_actions(self, range.clone(), cx)?;
        Some(cx.spawn(async move |this, cx| {
            let actions = match request.await {
                Ok(actions) => Rc::new(actions),
                Err(err) => {
                    log::debug!("code actions failed: {err}");
                    return None;
                }
            };
            this.update(cx, |e, _| {
                if e.buffer.version() != version {
                    return None;
                }
                e.lang.actions.cached = Some((version, range, actions.clone()));
                Some(actions)
            })
            .ok()
            .flatten()
        }))
    }

    /// Opens the code action menu at the cursor (Ctrl+.): every action, or
    /// only refactorings.
    pub fn show_code_actions(&mut self, refactor_only: bool, cx: &mut Context<Self>) {
        if self.lang.provider.is_none() {
            self.show_notice("No code actions available", cx);
            return;
        }
        if let Some(actions) = self.cached_actions() {
            self.open_actions_menu(actions, refactor_only, cx);
            return;
        }
        let Some(request) = self.request_actions(cx) else {
            self.show_notice("No code actions available", cx);
            return;
        };
        self.lang.actions.menu_task = Some(cx.spawn(async move |this, cx| {
            let actions = request.await;
            this.update(cx, |e, cx| match actions {
                Some(actions) => e.open_actions_menu(actions, refactor_only, cx),
                None => e.show_notice("No code actions available", cx),
            })
            .ok();
        }));
    }

    fn open_actions_menu(
        &mut self,
        actions: Rc<Vec<CodeAction>>,
        refactor_only: bool,
        cx: &mut Context<Self>,
    ) {
        let (rows, targets) = action_rows(&actions, refactor_only);
        if rows.is_empty() {
            let what = if refactor_only {
                "No refactorings available"
            } else {
                "No code actions available"
            };
            self.show_notice(what, cx);
            return;
        }
        let on_pick: OnPick = Rc::new(move |editor, row, _, cx| {
            let Some(ix) = targets.get(row).copied().flatten() else {
                return;
            };
            let action = actions[ix].clone();
            editor.update(cx, |e, cx| e.apply_code_action(action, cx));
        });
        let anchor = self.offset_to_point(self.selections.primary().head);
        self.show_menu(CODE_ACTIONS_POPOVER, anchor, rows, on_pick, cx);
    }

    /// Applies `action` through the provider.
    pub fn apply_code_action(&mut self, action: CodeAction, cx: &mut Context<Self>) {
        let Some(provider) = self.lang.provider.clone() else {
            return;
        };
        self.lang.actions.cached = None;
        let title = action.title.clone();
        let Some(task) = provider.apply_code_action(self, action, cx) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            if let Err(err) = task.await {
                log::warn!("code action {title:?} failed: {err}");
                this.update(cx, |e, cx| e.show_notice(format!("{title}: {err}"), cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Labels of the open code action or context menu (`[Group]` headers,
    /// disabled entries in parentheses, the selected one after `> `).
    pub fn menu_labels(&self, cx: &gpui::App) -> Option<Vec<String>> {
        [CODE_ACTIONS_POPOVER, super::CONTEXT_MENU_POPOVER]
            .iter()
            .find_map(|key| self.menu_view(key))
            .map(|menu| menu.read(cx).labels())
    }
}

/// The bulb for `actions` on `line`: shown when one can be applied.
fn bulb_for(actions: &[CodeAction], line: usize) -> Option<(usize, bool)> {
    let usable = actions.iter().filter(|a| a.disabled.is_none());
    let mut any = false;
    let mut quick = false;
    for action in usable {
        any = true;
        quick |= action_group(action.kind.as_deref()) == ActionGroup::QuickFix;
    }
    any.then_some((line, quick))
}
