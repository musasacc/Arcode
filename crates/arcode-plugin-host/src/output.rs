//! Everything a plugin sends to the app (§5 "PluginOutput").
//!
//! The host never touches the UI: plugin threads push [`PluginOutput`]s into
//! an `async-channel` that a GPUI task drains (the same pattern as the LSP
//! client). Requests that need an answer carry a [`ReplySender`]; the plugin
//! thread blocks on it, the UI thread never waits on a plugin.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bindings::ui::Location;
use crate::bindings::{Level, PanelId};
use crate::cancel::{CancelToken, ReplySender};
use crate::consent::{ConsentAnswer, ConsentRequest};
use crate::manifest::{PluginId, QualifiedId};
use crate::ui_tree::{Changes, PanelSnapshot};

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
    /// A consent prompt (only with [`crate::consent::ChannelConsent`], the
    /// manager's default): show host chrome built from `request` (its
    /// [`ConsentRequest::message`]) and answer, or drop `reply` to dismiss.
    /// The plugin's thread waits; the UI thread never does.
    Consent {
        /// What is asked; host-derived data, plugin text quoted.
        request: ConsentRequest,
        /// Where the answer goes.
        reply: ReplySender<ConsentAnswer>,
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
    /// A panel's view changed (`ui.render` or `ui.apply-patches`, already
    /// validated and applied on the plugin thread). Redraw the `changes`;
    /// element ids are `(panel, node-id)`.
    PanelView {
        /// Owner.
        plugin: PluginId,
        /// Target panel.
        panel: PanelId,
        /// The whole view after the change (shared, immutable).
        snapshot: Arc<PanelSnapshot>,
        /// What changed: dirty and removed nodes, inputs the plugin set,
        /// tables whose row cache must be reset or resized.
        changes: Changes,
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
    /// `host.begin-task` succeeded: show "title… \[Cancel\]".
    TaskStarted {
        /// Owner.
        plugin: PluginId,
        /// The call that became a task.
        call: u64,
        /// Plain-text title.
        title: String,
        /// The \[Cancel\] button: cancels the call (cooperatively, then hard
        /// after the grace period).
        cancel: CancelToken,
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
    /// An installed plugin's `plugin.wasm` is not the one the user approved
    /// (§3): it does not run. Ask the user, then call
    /// `PluginManager::set_enabled(plugin, true)` (approves the new file)
    /// or `false`.
    NeedsApproval {
        /// The plugin.
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

/// Shortest time between two `Status` outputs of one plugin.
pub const STATUS_INTERVAL: Duration = Duration::from_millis(100);

/// Coalesces `host.set-status` so a plugin cannot queue outputs without
/// bound: at most one `Status` per [`STATUS_INTERVAL`] is sent at once; a
/// newer one in between replaces the waiting one, which is sent when the
/// call returns ([`StatusThrottle::flush`]). The app always ends up with
/// the latest status.
#[derive(Debug, Default)]
pub(crate) struct StatusThrottle {
    last_sent: Option<Instant>,
    pending: Option<PluginOutput>,
}

impl StatusThrottle {
    /// `status` to send now, or `None` if it waits for the next flush.
    pub fn offer(&mut self, status: PluginOutput, now: Instant) -> Option<PluginOutput> {
        if self
            .last_sent
            .is_some_and(|t| now.duration_since(t) < STATUS_INTERVAL)
        {
            self.pending = Some(status);
            return None;
        }
        self.last_sent = Some(now);
        self.pending = None;
        Some(status)
    }

    /// The status that is still waiting, if any.
    pub fn flush(&mut self, now: Instant) -> Option<PluginOutput> {
        let pending = self.pending.take()?;
        self.last_sent = Some(now);
        Some(pending)
    }
}
