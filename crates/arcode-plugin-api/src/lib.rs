//! Arcode plugin SDK (guest side), API 0.1. See `docs/PLUGINS.md`.
//!
//! A plugin is a `wasm32-wasip2` component. Implement [`Plugin`] for a
//! `Default` type and export it:
//!
//! ```ignore
//! use arcode_plugin::prelude::*;
//!
//! #[derive(Default)]
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn command(&mut self, id: &str, _args: Json) -> Result<()> {
//!         host::notify(host::Level::Info, &format!("ran {id}"));
//!         Ok(())
//!     }
//! }
//! arcode_plugin::export_plugin!(Hello);
//! ```
//!
//! The same crate builds natively, where every import is answered by the
//! fakes in [`testing`], so plugin logic is unit-tested with `cargo test`.
//! The WIT contract lives in `wit/arcode-plugin.wit`; the host's
//! `bindgen!` reads the same file.

pub mod bindings;

mod args;
mod imports;
mod pager;
mod plugin;
mod sys;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;
pub mod ui;

pub use args::{Error, Json, Result};
pub use bindings::exports::arcode::plugin::guest::{Activation, CodeLens};
pub use imports::{editor, host, http, kv, workspace};
pub use pager::TablePager;
pub use plugin::Plugin;

/// The API version this SDK was built against (the WIT package version).
pub const API_VERSION: &str = "0.1.0";

/// Everything a typical plugin needs.
pub mod prelude {
    pub use crate::editor::{self, DocInfo};
    pub use crate::ui::{Event, EventKind, NodeId, Panel, PanelId, RowPage};
    pub use crate::{Activation, CodeLens, Error, Json, Plugin, Result, TablePager};
    pub use crate::{host, http, kv, workspace};
}

#[doc(hidden)]
pub mod __private {
    pub use crate::plugin::Adapter;
}
