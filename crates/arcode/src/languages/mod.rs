//! Language packs and per-file language settings.
//!
//! - [`bundle`]: what a language pack is made of (grammar, comment tokens,
//!   file extensions, language servers with their install commands,
//!   snippets, run commands, file associations), joined from the
//!   UI-agnostic crates.
//! - [`view::LanguagesView`]: the Languages page in the Plugins activity,
//!   with an enable switch per pack (`[language.<id>].enabled`).
//! - [`resolve`]: `[files] associations` on top of the built-in extensions.
//! - [`indent`]: guessing and converting indentation.
//! - [`status_items`]: the status bar entries (indentation, encoding, line
//!   ending, language) whose clicks open the pickers.
//!
//! The pickers and the package banner are wired in `workspace/languages.rs`
//! and `workspace/packages.rs`.

pub mod actions;
pub mod bundle;
pub mod indent;
pub mod resolve;
pub mod status_items;
pub mod view;

use std::collections::HashMap;

use arcode_buffer::LineEnding;
use arcode_editor::{Editor, IndentOverride};
use arcode_project::MissingPackages;
use arcode_syntax::Language;
use gpui::{Entity, EntityId, Task, WeakEntity};

use crate::list_picker::ListPicker;

/// A choice of the indentation picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentChoice {
    /// Use this indentation for the file.
    Set(IndentOverride),
    /// Guess from the content again.
    Detect,
    /// Back to the language settings.
    Reset,
    /// Rewrite leading tabs as spaces.
    ToSpaces,
    /// Rewrite leading spaces as tabs.
    ToTabs,
}

/// A choice of the language picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageChoice {
    /// Back to the language the file name gives.
    Auto,
    /// Pick a language to always use for files like this one.
    Associate,
    /// This language.
    Language(&'static Language),
}

/// What the open picker chooses.
pub enum PickPurpose {
    /// The language of `editor` (or, with `associate`, the language
    /// written to `[files] associations` for that pattern).
    Language {
        /// The editor.
        editor: WeakEntity<Editor>,
        /// The rows.
        choices: Vec<LanguageChoice>,
        /// The pattern being associated (second step).
        associate: Option<String>,
    },
    /// The line ending of `editor`.
    LineEnding {
        /// The editor.
        editor: WeakEntity<Editor>,
        /// The rows.
        choices: Vec<LineEnding>,
    },
    /// The indentation of `editor`.
    Indentation {
        /// The editor.
        editor: WeakEntity<Editor>,
        /// The rows.
        choices: Vec<IndentChoice>,
    },
}

/// The workspace's language and package state.
pub struct LangParts {
    /// The Languages page.
    pub view: Entity<view::LanguagesView>,
    /// The open picker and what it is for.
    pub picker: Option<(Entity<ListPicker>, PickPurpose)>,
    /// Editors whose language was picked by hand, with that language
    /// (associations and renames no longer change it).
    pub manual: HashMap<EntityId, &'static Language>,
    /// The package banner.
    pub banner: PackageBanner,
}

/// The "packages are not installed" banner.
#[derive(Default)]
pub struct PackageBanner {
    /// Manifests with missing dependencies (empty: no banner).
    pub found: Vec<MissingPackages>,
    /// Closed for this session (Dismiss, or Install was clicked).
    pub dismissed: bool,
    /// The pending check.
    pub task: Option<Task<()>>,
}

impl PackageBanner {
    /// Whether the banner is shown.
    pub fn visible(&self) -> bool {
        !self.dismissed && !self.found.is_empty()
    }
}
