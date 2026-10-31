//! `arcode:plugin/host`.

use std::time::{Duration, Instant};

use crate::bindings::Level;
use crate::bindings::arcode::plugin::host;
use crate::cancel::{lock, oneshot};
use crate::output::PluginOutput;
use crate::state::HostState;

use super::cap;

/// `host.notify` calls allowed per [`NOTIFY_WINDOW`].
const NOTIFY_LIMIT: usize = 5;
const NOTIFY_WINDOW: Duration = Duration::from_secs(60);
/// Longest notification or status text.
const MAX_MESSAGE_BYTES: usize = 1024;
/// Longest single `host.log` message.
const MAX_LOG_BYTES: usize = 16 * 1024;
/// Quick-pick caps.
const MAX_PICK_ITEMS: usize = 10_000;
const MAX_PICK_ITEM_BYTES: usize = 512;

impl host::Host for HostState {
    fn api_version(&mut self) -> wasmtime::Result<String> {
        Ok(crate::API_VERSION.to_owned())
    }

    fn log(&mut self, level: Level, message: String) -> wasmtime::Result<()> {
        self.ctx
            .shared
            .log
            .push(level, &cap(message, MAX_LOG_BYTES));
        Ok(())
    }

    fn notify(&mut self, level: Level, message: String) -> wasmtime::Result<()> {
        let now = Instant::now();
        {
            let mut times = lock(&self.ctx.shared.notify_times);
            while times
                .front()
                .is_some_and(|t| now.duration_since(*t) >= NOTIFY_WINDOW)
            {
                times.pop_front();
            }
            if times.len() >= NOTIFY_LIMIT {
                self.ctx.shared.log.push(
                    Level::Warn,
                    "notification dropped (rate limit: 5 per minute)",
                );
                return Ok(());
            }
            times.push_back(now);
        }
        self.ctx.output(PluginOutput::Notify {
            plugin: self.ctx.manifest.id().clone(),
            level,
            message: cap(message, MAX_MESSAGE_BYTES),
        });
        Ok(())
    }

    fn set_status(
        &mut self,
        text: Option<String>,
        command: Option<String>,
    ) -> wasmtime::Result<()> {
        let manifest = &self.ctx.manifest;
        // Only a contributed command may be bound to the item.
        let command = command
            .filter(|c| manifest.contributes.commands.iter().any(|k| &k.id == c))
            .map(|c| manifest.id().qualify(&c));
        let status = PluginOutput::Status {
            plugin: manifest.id().clone(),
            text: text.map(|t| cap(t, MAX_MESSAGE_BYTES)),
            command,
        };
        // Coalesced: a flood of updates sends at most one per 100 ms, and
        // the latest one when the call returns.
        let now = lock(&self.ctx.shared.status).offer(status, Instant::now());
        if let Some(status) = now {
            self.ctx.output(status);
        }
        Ok(())
    }

    fn quick_pick(&mut self, title: String, items: Vec<String>) -> wasmtime::Result<Option<u32>> {
        if self.call.cancel.is_cancelled() {
            return Ok(None);
        }
        let items: Vec<String> = items
            .into_iter()
            .take(MAX_PICK_ITEMS)
            .map(|i| cap(i, MAX_PICK_ITEM_BYTES))
            .collect();
        let count = items.len();
        let (reply, answer) = oneshot();
        self.ctx.output(PluginOutput::QuickPick {
            plugin: self.ctx.manifest.id().clone(),
            title: cap(title, MAX_MESSAGE_BYTES),
            items,
            reply,
        });
        // A wait for the user: not charged to the CPU budget.
        let cancel = self.call.cancel.clone();
        let picked = self.waiting(|_| answer.wait(&cancel, None).ok().flatten());
        Ok(picked.filter(|i| (*i as usize) < count))
    }

    fn settings(&mut self) -> wasmtime::Result<String> {
        Ok(lock(&self.ctx.shared.settings).clone())
    }

    fn cancelled(&mut self) -> wasmtime::Result<bool> {
        Ok(self.call.cancel.is_cancelled())
    }

    fn begin_task(&mut self, title: String) -> wasmtime::Result<bool> {
        if !self.call.gesture {
            return Ok(false);
        }
        let task = Duration::from_secs(u64::from(self.ctx.limits.task_seconds));
        self.call.budget.raise_to(task);
        if !self.call.task_started {
            self.call.task_started = true;
            self.ctx.output(PluginOutput::TaskStarted {
                plugin: self.ctx.manifest.id().clone(),
                call: self.call.id,
                title: cap(title, MAX_MESSAGE_BYTES),
                cancel: self.call.cancel.clone(),
            });
        }
        Ok(true)
    }
}
