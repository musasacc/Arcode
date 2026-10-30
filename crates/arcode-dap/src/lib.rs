//! Debug Adapter Protocol client for Arcode.
//!
//! - [`types`]: hand-written serde types for the DAP subset Arcode uses.
//! - [`transport`]: `Content-Length` framing (same as LSP).
//! - [`DebugSession::start`]: spawns or connects to an adapter (stdio or
//!   TCP) and runs `initialize` → `launch`/`attach` → `initialized` →
//!   breakpoints → `configurationDone` in the background. It returns a
//!   [`DebugSessionHandle`] and an [`EventReceiver`] of [`DebugEvent`]s.
//! - [`adapters`]: known adapters (codelldb, lldb-dap, gdb, debugpy, delve)
//!   and launch templates; pure data plus a `PATH` check.
//!
//! Threading follows `arcode-lsp`: the crate owns a small tokio runtime,
//! UI-facing methods never block, request futures resolve through oneshot
//! channels (await them on any executor) and fail instead of hanging when
//! the adapter dies. The crate does not depend on GPUI and performs no
//! network access beyond connecting to a configured (normally local)
//! adapter port.

#![warn(missing_docs)]

pub mod adapters;
mod config;
mod connection;
mod error;
mod event;
mod process;
mod resolve;
mod session;
pub mod transport;
pub mod types;

pub use adapters::{
    KnownAdapter, LaunchOptions, adapter_for_language, adapters_for_language, find_adapter,
};
pub use config::{
    AdapterCommand, AdapterSpec, AdapterTransport, DEFAULT_CONNECT_TIMEOUT, DebugConfig,
    DebugRequest, PORT_PLACEHOLDER,
};
pub use error::{DapError, Result};
pub use event::{DebugEvent, EventReceiver, RunInTerminalRequest, SessionState};
pub use resolve::find_executable;
pub use session::{
    DebugSession, DebugSessionHandle, INITIALIZE_TIMEOUT, REQUEST_TIMEOUT, STOP_TIMEOUT,
};
pub use types::{
    Breakpoint, Capabilities, EvaluateContext, EvaluateResponseBody, FunctionBreakpoint, Scope,
    Source, SourceBreakpoint, StackFrame, Thread, Variable,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_thread_safe() {
        fn check<T: Send + Sync>() {}
        check::<DebugSessionHandle>();
        check::<DebugEvent>();
        check::<EventReceiver>();
        check::<RunInTerminalRequest>();
        check::<DapError>();
    }
}
