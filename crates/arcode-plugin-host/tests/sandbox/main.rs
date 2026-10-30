//! Fixture-guest tests for plan steps 3-8 and the `PluginManager` facade
//! (docs/PLUGINS.md §13, §14).
//!
//! One test binary (wasmtime makes each one large). Needs the fixtures:
//! run `scripts/build-test-plugins.sh` first.

mod budgets;
mod common;
mod convert;
mod imports;
mod lifecycle;
mod loopback;
mod manager;
mod network;
mod permissions;
mod roundtrip;
mod ui;
