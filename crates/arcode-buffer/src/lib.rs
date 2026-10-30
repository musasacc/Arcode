//! Rope-backed text buffer for the Arcode editor.
//!
//! The crate is UI-agnostic. It provides:
//!
//! - [`Buffer`]: a [`ropey::Rope`] with file I/O, line-ending handling,
//!   versioning, dirty tracking and undo/redo ([`Buffer::edit`]).
//! - [`Edit`]: a record of one applied change with char, byte and
//!   tree-sitter style point coordinates, plus its LSP view.
//! - Coordinate conversion ([`Point`], [`TsPoint`], [`PositionEncoding`]).
//! - Multi-cursor selections ([`Selection`], [`SelectionSet`]).
//! - Cursor [`movement`], editing [`ops`] and [`search`].
//!
//! All buffer positions are **char indices** into the rope. Text is kept
//! with `\n` line endings in memory; the original [`LineEnding`] is restored
//! on save.
//!
//! ```
//! use arcode_buffer::{Buffer, SelectionSet, ops};
//!
//! let mut buffer = Buffer::new("fn main() {}\r\n");
//! let mut selections = SelectionSet::single(11); // between the braces
//! let cfg = ops::OpsConfig::default();
//! let edits = ops::newline_and_indent(&mut buffer, &mut selections, &cfg);
//! assert_eq!(buffer.to_string(), "fn main() {\n    \n}\n");
//! assert_eq!(edits[0].start_byte, 11);
//! assert!(buffer.is_dirty());
//! buffer.undo();
//! assert!(!buffer.is_dirty());
//! ```

mod buffer;
mod changes;
mod coords;
mod edit;
mod history;
mod io;
mod line_ending;
pub mod movement;
pub mod ops;
pub mod search;
mod selection;
mod text_util;

pub use buffer::Buffer;
pub use coords::{Point, PositionEncoding, TsPoint};
pub use edit::{Edit, LspRange};
pub use history::{UndoGroup, UndoResult};
pub use line_ending::LineEnding;
pub use ropey::{Rope, RopeSlice};
pub use selection::{Selection, SelectionSet};
