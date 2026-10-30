//! Arcode's sandboxed WASM plugin host. No GPUI. See `docs/PLUGINS.md`.
//!
//! The host talks to the app only through plain data, channels and traits,
//! so it can be tested headless (and moved out of process later).
//!
//! Implemented so far (plan steps 1-2):
//! - [`manifest`]: `plugin.toml` parsing, validation, limit clamping and
//!   derived activation events;
//! - [`registry`]: discovery (bundled < installed < dev) and contribution
//!   lookup, reading manifests only (no wasm, no engine at startup);
//! - [`paths`]: per-user plugin locations.

pub mod manifest;
pub mod paths;
pub mod registry;

#[cfg(test)]
mod registry_tests;

pub use manifest::{Manifest, ManifestError, PluginId, PluginSource, QualifiedId};
pub use registry::{DiscoveryRoots, RegisteredPlugin, Registry};
