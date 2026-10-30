//! Everything a plugin sends to the app (§5 "PluginOutput").
//!
//! The host never touches the UI: plugin threads push [`PluginOutput`]s into
//! an `async-channel` that a GPUI task drains (the same pattern as the LSP
//! client). Requests that need an answer carry a [`ReplySender`]; the plugin
//! thread blocks on it, the UI thread never waits on a plugin.

use crate::bindings::ui::{Location, Patch, View};
use crate::bindings::{Level, PanelId};
use crate::cancel::ReplySender;
use crate::manifest::{PluginId, QualifiedId};

/// One message from a plugin to the app.
#[derive(Debug)]
pub enum PluginOutput {
    /// `host.notify` (already rate-limited and length-capped).
    Notify {
        /// Sender.
        plugin: PluginId,
        /// Severity.
        level: Level,
        /// Plain text.
        message: String,
    },
    /// `host.set-status`; `text: None` clears the item.
    Status {
        /// Sender.
        plugin: PluginId,
        /// Item text.
        text: Option<String>,
        /// Contributed command run (as a gesture) on click.
        command: Option<QualifiedId>,
    },
    /// `host.quick-pick`: answer with the chosen index, or drop the sender.
    QuickPick {
        /// Sender.
        plugin: PluginId,
        /// Plain-text title.
        title: String,
        /// Plain-text items.
        items: Vec<String>,
        /// Where the answer goes.
        reply: ReplySender<Option<u32>>,
    },
    /// A panel was opened (by `ui.open-panel` or for an opener).
    PanelOpened {
        /// Owner.
        plugin: PluginId,
        /// Host-allocated panel id (unique per plugin).
        panel: PanelId,
        /// The namespaced panel or opener contribution.
        contribution: QualifiedId,
        /// Title.
        title: String,
        /// Requested location; `None` uses the manifest's.
        location: Option<Location>,
    },
    /// A full view for a panel (`ui.render`).
    Render {
        /// Owner.
        plugin: PluginId,
        /// Target panel.
        panel: PanelId,
        /// The new tree.
        view: View,
    },
    /// Patches for a panel (`ui.apply-patches`).
    Patches {
        /// Owner.
        plugin: PluginId,
        /// Target panel.
        panel: PanelId,
        /// Patches, in order.
        patches: Vec<Patch>,
    },
    /// `ui.set-title`.
    PanelTitle {
        /// Owner.
        plugin: PluginId,
        /// Target panel.
        panel: PanelId,
        /// New title.
        title: String,
    },
    /// `ui.close-panel`.
    PanelClosed {
        /// Owner.
        plugin: PluginId,
        /// Closed panel.
        panel: PanelId,
    },
    /// `host.begin-task` succeeded: show "title… [Cancel]".
    TaskStarted {
        /// Owner.
        plugin: PluginId,
        /// The call that became a task.
        call: u64,
        /// Plain-text title.
        title: String,
    },
    /// The task's call finished (any outcome).
    TaskFinished {
        /// Owner.
        plugin: PluginId,
        /// The call.
        call: u64,
    },
    /// The instance trapped and was dropped (§5 "Crashes").
    Crashed {
        /// Owner.
        plugin: PluginId,
        /// "exceeded 2 s CPU", "out of memory", ...
        reason: String,
        /// True once the plugin is disabled for the session.
        disabled: bool,
    },
    /// The instance was unloaded after being idle.
    Unloaded {
        /// Owner.
        plugin: PluginId,
    },
}

/// The sending side of the output channel, shared by every plugin.
#[derive(Debug, Clone)]
pub struct OutputSender {
    tx: async_channel::Sender<PluginOutput>,
}

impl OutputSender {
    /// Wraps a channel sender.
    pub fn new(tx: async_channel::Sender<PluginOutput>) -> Self {
        Self { tx }
    }

    /// An unbounded channel; the app drains the receiver.
    pub fn channel() -> (Self, async_channel::Receiver<PluginOutput>) {
        let (tx, rx) = async_channel::unbounded();
        (Self { tx }, rx)
    }

    /// Sends without blocking. A closed channel (app shutting down) is
    /// ignored; a dropped `QuickPick` then reads as "dismissed".
    pub fn send(&self, output: PluginOutput) {
        let _ = self.tx.try_send(output);
    }
}
