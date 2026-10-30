//! The per-session request queue.
//!
//! Handle calls are pushed here synchronously and dispatched one by one:
//! each job waits for the phase it needs and then enqueues its request on
//! the connection before the next job runs. Requests therefore reach the
//! adapter in call order (only enqueueing is serialized; responses are
//! still awaited concurrently).

use std::sync::Arc;

use tokio::sync::mpsc;

use super::core::Core;
use crate::connection::Connection;
use crate::error::{DapError, Result};

/// Runs synchronously on the dispatcher with the connection (or the reason
/// there is none); it must enqueue its requests before returning.
pub(crate) type Work = Box<dyn FnOnce(&Arc<Core>, Result<Arc<Connection>>) + Send>;

struct Job {
    /// Wait for [`super::core::Phase::Configured`] instead of `Ready`.
    configured: bool,
    work: Work,
}

/// Sending side of a session's queue; dropping it ends the dispatcher
/// once the queued jobs are done.
pub(crate) struct Queue(mpsc::UnboundedSender<Job>);

impl Queue {
    /// Creates the queue and starts its dispatcher on the session runtime
    /// (without a runtime every job fails at once).
    pub(crate) fn spawn(core: &Arc<Core>) -> Queue {
        let (tx, rx) = mpsc::unbounded_channel();
        if let Some(rt) = &core.runtime {
            rt.spawn(dispatch(core.clone(), rx));
        }
        Queue(tx)
    }

    /// Queues `work` behind every job pushed before it.
    pub(crate) fn push(&self, core: &Arc<Core>, configured: bool, work: Work) {
        if let Err(mpsc::error::SendError(job)) = self.0.send(Job { configured, work }) {
            (job.work)(
                core,
                Err(DapError::SessionEnded("runtime shut down".into())),
            );
        }
    }
}

async fn dispatch(core: Arc<Core>, mut rx: mpsc::UnboundedReceiver<Job>) {
    while let Some(job) = rx.recv().await {
        let conn = core.connection(job.configured).await;
        (job.work)(&core, conn);
    }
}
