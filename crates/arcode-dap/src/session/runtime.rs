//! The crate's own tokio runtime, shared by all sessions.

use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

use crate::error::{DapError, Result};

static RUNTIME: OnceLock<std::result::Result<Runtime, String>> = OnceLock::new();

/// Handle to the shared runtime (two worker threads named `arcode-dap`),
/// created on first use and never shut down.
pub(crate) fn handle() -> Result<Handle> {
    let runtime = RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("arcode-dap")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    });
    match runtime {
        Ok(rt) => Ok(rt.handle().clone()),
        Err(e) => Err(DapError::Io(format!(
            "failed to create the arcode-dap runtime: {e}"
        ))),
    }
}
