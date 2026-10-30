//! Arcode's sandboxed WASM plugin host. No GPUI. See `docs/PLUGINS.md` and
//! this crate's `README.md` (the app integration guide).
//!
//! The app uses [`PluginManager`]: it implements the traits in [`bridge`]
//! and [`consent`], drains [`PluginOutput`]s from an `async-channel`, and
//! calls plugins through methods that return a [`Pending`] reply channel at
//! once. The UI thread never waits on a plugin.
//!
//! Modules by plan step:
//! - [`manifest`], [`registry`], [`paths`] (2): manifests, discovery;
//! - [`engine`], [`cache`], [`bindings`], [`instance`], [`dispatch`] (3):
//!   lazy engine, `.cwasm` cache, instance thread, coalescing mailbox;
//! - [`budget`], [`limits`] (4): CPU budgets, memory caps, crash backoff;
//! - [`grants`], [`grant_store`], [`consent`], [`wasi`], [`imports_check`]
//!   (5): permissions, prompts, the WASI sandbox, the import check;
//! - [`host_impl`], [`index`], [`memory_editor`] (6): the imports, a
//!   `.gitignore`-aware file index, an in-memory editor;
//! - [`net`] (7): the ureq transport and the network log;
//! - [`ui_tree`], [`rows`] (8): panel trees and table row caching;
//! - [`manager`]: the facade.

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
pub mod index;
pub mod install;
pub mod instance;
mod kv_file;
pub mod limits;
mod live;
pub mod manager;
pub mod manifest;
pub mod memory_editor;
pub mod net;
pub mod output;
pub mod paths;
pub mod plugin_log;
pub mod plugin_state;
pub mod registry;
pub mod rows;
pub mod state;
pub mod ui_tree;
pub mod wasi;
mod wasi_hooks;

#[cfg(test)]
mod registry_tests;

pub use dispatch::{Call, CallError, CallOutput, CallResult};
pub use engine::{Runtime, RuntimeConfig};
pub use instance::{InstanceHandle, InstanceSpec, Pending};
pub use manager::{AppServices, ManagerConfig, ManagerError, PluginManager};
pub use manifest::{Manifest, ManifestError, PluginId, PluginSource, QualifiedId};
pub use output::{OutputSender, PluginOutput};
pub use registry::{DiscoveryRoots, RegisteredPlugin, Registry};
pub use state::{ProjectContext, Services};

/// The WIT package version this host implements (`host.api-version`).
pub const API_VERSION: &str = "0.1.0";
