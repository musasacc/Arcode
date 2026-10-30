//! Fixture-guest tests for plan steps 3-5 (docs/PLUGINS.md §13, §14).
//!
//! One test binary (wasmtime makes each one large). Needs the fixtures:
//! run `scripts/build-test-plugins.sh` first.

mod budgets;
mod common;
mod lifecycle;
mod permissions;
mod roundtrip;
