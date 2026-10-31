//! Plugin entries of the status bar (docs/PLUGINS.md §3 `set-status`, §5
//! "Cancellation"): each plugin's status item, and a task chip
//! "title… [Cancel]" while a call that began a task is running. Clicking
//! the chip, or Esc, cancels the newest task.

use std::collections::BTreeMap;

use arcode_plugin_host::cancel::CancelToken;
use arcode_plugin_host::{PluginId, QualifiedId};
use arcode_ui_kit::{IconName, icon, theme};
use gpui::{Context, SharedString, Window, div, prelude::*, px};

use super::commands::RunPluginCommand;

/// A plugin call shown as a task.
#[derive(Debug, Clone)]
pub struct RunningTask {
    /// Owner.
    pub plugin: PluginId,
    /// The call.
    pub call: u64,
    /// Plain-text title from the plugin.
    pub title: SharedString,
    /// Cancels the call.
    pub cancel: CancelToken,
}

/// The plugin part of the status bar.
#[derive(Default)]
pub struct PluginStatus {
    items: BTreeMap<PluginId, (SharedString, Option<QualifiedId>)>,
    tasks: Vec<RunningTask>,
}

/// Longest status text shown (chars).
const MAX_STATUS_CHARS: usize = 60;

fn capped(text: &str) -> SharedString {
    let mut out: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_STATUS_CHARS)
        .collect();
    if text.chars().count() > MAX_STATUS_CHARS {
        out.push('…');
    }
    out.into()
}

impl PluginStatus {
    /// Sets or clears `plugin`'s status item.
    pub fn set_item(
        &mut self,
        plugin: PluginId,
        text: Option<String>,
        command: Option<QualifiedId>,
        cx: &mut Context<Self>,
    ) {
        match text {
            Some(text) => {
                self.items.insert(plugin, (capped(&text), command));
            }
            None => {
                self.items.remove(&plugin);
            }
        }
        cx.notify();
    }

    /// Drops every item and task of `plugin` (crash, unload, disable).
    pub fn forget_plugin(&mut self, plugin: &PluginId, cx: &mut Context<Self>) {
        self.items.remove(plugin);
        self.tasks.retain(|t| &t.plugin != plugin);
        cx.notify();
    }

    /// A task started.
    pub fn task_started(&mut self, task: RunningTask, cx: &mut Context<Self>) {
        self.tasks.push(RunningTask {
            title: capped(&task.title),
            ..task
        });
        cx.notify();
    }

    /// A task's call finished.
    pub fn task_finished(&mut self, call: u64, cx: &mut Context<Self>) {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.call != call);
        if before != self.tasks.len() {
            cx.notify();
        }
    }

    /// The running tasks, oldest first.
    pub fn tasks(&self) -> &[RunningTask] {
        &self.tasks
    }

    /// The status items.
    pub fn items(&self) -> impl Iterator<Item = (&PluginId, &SharedString)> {
        self.items.iter().map(|(p, (t, _))| (p, t))
    }

    /// Cancels the newest task that is not cancelled yet. Returns whether
    /// there was one (Esc is consumed only then).
    pub fn cancel_newest(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(task) = self.tasks.iter().rev().find(|t| !t.cancel.is_cancelled()) else {
            return false;
        };
        task.cancel.cancel();
        cx.notify();
        true
    }

    fn cancel_call(&mut self, call: u64, cx: &mut Context<Self>) {
        if let Some(task) = self.tasks.iter().find(|t| t.call == call) {
            task.cancel.cancel();
            cx.notify();
        }
    }
}

impl Render for PluginStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let mut row = div().flex().items_center().h_full();
        for task in &self.tasks {
            let call = task.call;
            let cancelling = task.cancel.is_cancelled();
            let label = if cancelling {
                format!("{}… cancelling", task.title)
            } else {
                format!("{}…", task.title)
            };
            row = row.child(
                div()
                    .id(SharedString::from(format!("plugin-task-{call}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_full()
                    .cursor_pointer()
                    .text_color(c.text)
                    .hover(|s| s.bg(c.hover))
                    .child(icon(IconName::Loader).size(px(12.)).color(c.accent))
                    .child(label)
                    .when(!cancelling, |el| {
                        el.child(
                            div()
                                .px_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(c.border)
                                .child("Cancel"),
                        )
                    })
                    .on_click(cx.listener(move |status, _, _, cx| status.cancel_call(call, cx))),
            );
        }
        for (plugin, (text, command)) in &self.items {
            let command = command.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("plugin-status-{plugin}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_full()
                    .child(icon(IconName::Puzzle).size(px(12.)).color(c.text_muted))
                    .child(text.clone())
                    .when_some(command, |el, command| {
                        el.cursor_pointer().hover(|s| s.bg(c.hover)).on_click(
                            move |_, window, cx| {
                                let id = command.as_str().to_owned().into();
                                window.dispatch_action(Box::new(RunPluginCommand { id }), cx);
                            },
                        )
                    }),
            );
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_is_one_capped_line() {
        assert_eq!(capped("HTTP env: dev"), "HTTP env: dev");
        assert_eq!(capped("a\nb"), "a b");
        let long = capped(&"y".repeat(100));
        assert_eq!(long.chars().count(), MAX_STATUS_CHARS + 1);
    }

    #[gpui::test]
    fn tasks_cancel_newest_first(cx: &mut gpui::TestAppContext) {
        let status = cx.new(|_| PluginStatus::default());
        let plugin = PluginId::parse("acme.tools").unwrap();
        let (a, b) = (CancelToken::new(), CancelToken::new());
        status.update(cx, |s, cx| {
            for (call, cancel) in [(1, a.clone()), (2, b.clone())] {
                s.task_started(
                    RunningTask {
                        plugin: plugin.clone(),
                        call,
                        title: "Running query".into(),
                        cancel,
                    },
                    cx,
                );
            }
            assert!(s.cancel_newest(cx));
            assert!(b.is_cancelled() && !a.is_cancelled());
            assert!(s.cancel_newest(cx));
            assert!(a.is_cancelled());
            assert!(!s.cancel_newest(cx));
            s.task_finished(2, cx);
            assert_eq!(s.tasks().len(), 1);
            s.forget_plugin(&plugin, cx);
            assert!(s.tasks().is_empty());
        });
    }
}
