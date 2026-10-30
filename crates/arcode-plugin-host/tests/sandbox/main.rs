//! Fixture-guest tests for plan steps 3-8 and the `PluginManager` facade
//! (docs/PLUGINS.md §13, §14), and the bundled plugins end to end (step 10:
//! `http_client`).
//!
//! One test binary (wasmtime makes each one large). Needs the fixtures:
//! run `scripts/build-test-plugins.sh` first, and `scripts/build-plugins.sh`
//! for the bundled-plugin tests.

mod budgets;
mod common;
mod convert;
mod http_client;
mod imports;
mod install;
mod lifecycle;
mod loopback;
mod manager;
mod network;
mod permissions;
mod roundtrip;
mod ui;
