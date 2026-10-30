//! Host-side bindings generated from `crates/arcode-plugin-api/wit` (§6).
//!
//! Together with [`crate::engine`] this is the only place that names
//! wasmtime's component types. The generated records and variants are plain
//! data, so the rest of the host (and the app) uses them directly through the
//! re-exports below instead of mirroring every WIT type by hand.

wasmtime::component::bindgen!({
    path: "../arcode-plugin-api/wit",
    world: "plugin",
    // Every import may trap (e.g. a host bug or a poisoned store); the
    // functions return `wasmtime::Result<T>`.
    imports: { default: trappable },
    // Plain-data comparisons in tests and in the app's diffing.
    additional_derives: [PartialEq],
});

/// The generated world type (`world plugin`), renamed so it does not clash
/// with the many other things called "plugin".
pub use self::Plugin as PluginWorld;

/// `arcode:plugin/types`
pub use self::arcode::plugin::types::{DocId, Level, NodeId, PanelId, Position, Range};

/// `arcode:plugin/editor`
pub mod editor {
    pub use super::arcode::plugin::editor::{DocInfo, EditorError, TextEdit};
}

/// `arcode:plugin/http`
pub mod http {
    pub use super::arcode::plugin::http::{HttpError, Request, Response, Timing};
}

/// `arcode:plugin/kv`
pub mod kv {
    pub use super::arcode::plugin::kv::{KvError, Scope};
}

/// `arcode:plugin/workspace`
pub mod workspace {
    pub use super::arcode::plugin::workspace::FsError;
}

/// `arcode:plugin/ui`
pub mod ui {
    pub use super::arcode::plugin::ui::{
        Axis, BadgeProps, ButtonProps, CheckboxProps, Cell, CodeProps, Column, Event, EventKind,
        InputProps, Location, Node, Patch, RowPage, SelectProps, SplitProps, StackProps, TableProps,
        TabsProps, TextProps, Tone, TreeItem, UiError, View, Widget,
    };
}

/// `arcode:plugin/guest` (the exports).
pub mod guest {
    pub use super::exports::arcode::plugin::guest::{Activation, CodeLens};
}
