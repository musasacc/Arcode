//! Host imports, as plain functions grouped by WIT interface.
//!
//! These are the generated bindings in wasm and the [`crate::testing`]
//! fakes natively; plugin code calls them the same way in both.

/// `arcode:plugin/host`: log, notify, status item, quick pick, settings,
/// cancellation and tasks.
pub mod host {
    pub use crate::bindings::arcode::plugin::types::Level;
    pub use crate::sys::host::{
        api_version, begin_task, cancelled, log, notify, quick_pick, set_status, settings,
    };

    /// The plugin's settings (manifest defaults merged with user values),
    /// deserialized into `T`.
    pub fn settings_as<T: serde::de::DeserializeOwned>() -> crate::Result<T> {
        crate::Json::from(settings()).parse()
    }
}

/// `arcode:plugin/kv`: small persistent key-value storage per scope.
pub mod kv {
    pub use crate::bindings::arcode::plugin::kv::{KvError, Scope};
    pub use crate::sys::kv::{delete, get, keys, set};
}

/// `arcode:plugin/workspace`: project name and indexed file search. File
/// contents are read with `std::fs` under `/project` (WASI).
pub mod workspace {
    pub use crate::bindings::arcode::plugin::workspace::FsError;
    pub use crate::sys::workspace::{find_files, root_name};

    /// Where the project is preopened inside the sandbox.
    pub const PROJECT_DIR: &str = "/project";
}

/// `arcode:plugin/editor`: documents, selections and edits.
pub mod editor {
    pub use crate::bindings::arcode::plugin::editor::{DocInfo, EditorError, TextEdit};
    pub use crate::bindings::arcode::plugin::types::{DocId, Position, Range};
    pub use crate::sys::editor::{
        apply_edits, current_document, document_info, open_file, open_scratch, selections, text,
    };
}

/// `arcode:plugin/http`: user-initiated HTTP with per-host consent.
pub mod http {
    pub use crate::bindings::arcode::plugin::http::{HttpError, Request, Response, Timing};
    pub use crate::sys::http::send;
}
