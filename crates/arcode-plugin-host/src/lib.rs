//! Arcode's sandboxed WASM plugin host. No GPUI. See `docs/PLUGINS.md`.
//!
//! The host talks to the app only through plain data, channels and traits,
//! so it can be tested headless (and moved out of process later):
//! - the app implements [`bridge::EditorBridge`], [`bridge::ProjectIndex`],
//!   [`bridge::HttpTransport`] and [`consent::ConsentProvider`];
//! - plugins report to the app through [`output::PluginOutput`] on an
//!   `async-channel`;
//! - the app calls plugins through [`instance::InstanceHandle::dispatch`]
//!   and awaits the reply channel; the UI thread never waits on a plugin.
//!
//! Implemented so far (plan steps 1-5):
//! - [`manifest`], [`registry`], [`paths`]: manifests, discovery, locations;
//! - [`engine`], [`cache`], [`bindings`]: lazy engine, `.cwasm` cache, WIT;
//! - [`instance`], [`dispatch`]: instance thread, coalescing mailbox, idle
//!   unload, restart after traps;
//! - [`budget`], [`limits`]: epoch CPU budgets, memory caps, crash backoff;
//! - [`grants`], [`grant_store`], [`consent`], [`wasi`], [`imports_check`]:
//!   permissions, prompts, the WASI sandbox and the load-time import check.

pub mod bindings;
pub mod bridge;
pub mod budget;
pub mod cache;
pub mod cancel;
pub mod consent;
pub mod dispatch;
pub mod engine;
pub mod grant_store;
pub mod grants;
pub mod host_impl;
pub mod imports_check;
pub mod instance;
pub mod limits;
mod live;
pub mod manifest;
pub mod output;
pub mod paths;
pub mod plugin_log;
pub mod registry;
pub mod state;
pub mod wasi;

#[cfg(test)]
mod registry_tests;

pub use dispatch::{Call, CallError, CallOutput, CallResult};
pub use engine::{Runtime, RuntimeConfig};
pub use instance::{InstanceHandle, InstanceSpec, Pending};
pub use manifest::{Manifest, ManifestError, PluginId, PluginSource, QualifiedId};
pub use output::{OutputSender, PluginOutput};
pub use registry::{DiscoveryRoots, RegisteredPlugin, Registry};
pub use state::{ProjectContext, Services};

/// The WIT package version this host implements (`host.api-version`).
pub const API_VERSION: &str = "0.1.0";
