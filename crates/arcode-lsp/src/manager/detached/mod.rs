//! Rust files outside any Cargo project, such as `~/scratch/test.rs`.
//!
//! rust-analyzer finds no workspace for such a file: it reports "Failed to
//! discover workspace" and offers nothing for it. The instance a loose file
//! opens on is therefore given the file in its `detachedFiles` setting, and
//! rust-analyzer analyses it as a crate of its own with the standard
//! library: completion, hover and its own diagnostics work.
//!
//! The instance keeps the loose files open on it. When one is added, the
//! open ones that exist are sent as `workspace/didChangeConfiguration`
//! (rust-analyzer then asks for them with `workspace/configuration`) and,
//! once it has them, `rust-analyzer/reloadWorkspace`, since rust-analyzer
//! does not reload by itself when only its detached files changed. That
//! keeps the instance and what it loaded instead of restarting it.
//!
//! The list rust-analyzer has must only name files that exist: with a
//! missing one it rebuilds its workspace over and over, using a full core
//! and serving no file. When a file of the list is renamed or deleted the
//! manager therefore restarts the instance with the open files that exist
//! (see [`DetachedFiles::names_missing`]), or stops it when none is left.
//! A restart rather than a reload, because rust-analyzer 1.97 reports no
//! diagnostics for a file it only got through a reload. Closing a file
//! changes nothing by itself: rust-analyzer keeps it until the next change.
//!
//! rust-analyzer reads the whole folder of a detached file; a folder that
//! is too big for that is refused (see [`folder`]).
//!
//! `cargo check` on save is off for these instances unless the settings
//! choose: for a detached file it runs Cargo on it as a script, which needs
//! a nightly toolchain, and reports the failure after every save.

mod folder;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lsp_types::request::Request;
use serde_json::{Map, Value};

use super::config::ServerConfig;
use crate::client::LspClient;
pub(crate) use folder::FolderLimit;

/// Files that make a folder a project rust-analyzer finds on its own.
const PROJECT_FILES: [&str; 3] = ["Cargo.toml", "rust-project.json", ".rust-project.json"];

/// rust-analyzer's request to load its workspaces and detached files again.
enum ReloadWorkspace {}

impl Request for ReloadWorkspace {
    type Params = ();
    type Result = ();
    const METHOD: &'static str = "rust-analyzer/reloadWorkspace";
}

/// Whether `config` runs rust-analyzer: by its name, its command, or an
/// argument (`rustup run stable rust-analyzer`).
pub(crate) fn is_rust_analyzer(config: &ServerConfig) -> bool {
    let named = |s: &str| {
        Path::new(s)
            .file_stem()
            .is_some_and(|n| n == "rust-analyzer")
    };
    config.name == "rust-analyzer" || named(&config.command) || config.args.iter().any(|a| named(a))
}

/// Whether `file` is a loose file for `config`: the server is
/// rust-analyzer and no folder above the file holds a Cargo manifest or a
/// `rust-project.json`.
pub(crate) fn is_loose(config: &ServerConfig, file: &Path) -> bool {
    is_rust_analyzer(config)
        && file.parent().is_some_and(|dir| {
            !dir.ancestors()
                .any(|d| PROJECT_FILES.iter().any(|f| d.join(f).is_file()))
        })
}

/// `options` with `files` as rust-analyzer's `detachedFiles` and, unless
/// the options set it, `checkOnSave` off.
pub(crate) fn with_detached_files(options: Option<Value>, files: &[PathBuf]) -> Value {
    let mut map = match options {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let files = files
        .iter()
        .map(|f| Value::String(f.to_string_lossy().into_owned()))
        .collect();
    map.insert("detachedFiles".into(), Value::Array(files));
    map.entry("checkOnSave").or_insert(Value::Bool(false));
    Value::Object(map)
}

/// How an instance starts.
#[derive(Debug, PartialEq)]
pub(crate) enum Launch {
    /// With the configured initialization options (not a loose-file
    /// instance).
    Configured,
    /// With these initialization options.
    Detached(Value),
    /// Not at all: none of its loose files can be analysed.
    Nothing,
}

/// What a running instance must do after a file was added.
#[derive(Debug, PartialEq)]
pub(crate) enum Update {
    Unchanged,
    /// Take these settings (see [`send_settings`]).
    Send(Value),
}

/// What [`DetachedFiles::launch`] or [`DetachedFiles::update`] decided,
/// with the folders found too big to analyse (each reported once).
#[derive(Debug, PartialEq)]
pub(crate) struct Outcome<T> {
    pub(crate) action: T,
    pub(crate) refused: Vec<PathBuf>,
}

/// The loose files of one server instance.
pub(crate) struct DetachedFiles {
    /// The configured initialization options.
    base: Option<Value>,
    limit: FolderLimit,
    state: Mutex<Files>,
}

struct Files {
    /// Created for a loose file.
    loose: bool,
    /// The loose files open on the instance.
    open: Vec<PathBuf>,
    /// The list the server has; `None` until it starts.
    sent: Option<Vec<PathBuf>>,
    /// Never started, for want of a file to analyse.
    retired: bool,
    /// Folder -> small enough to analyse; see [`folder`].
    folders: HashMap<PathBuf, bool>,
}

/// The files to send, from the open files as they were.
struct Wanted {
    open: Vec<PathBuf>,
    files: Vec<PathBuf>,
    refused: Vec<PathBuf>,
}

/// What [`DetachedFiles::add`] did.
#[derive(Debug, PartialEq)]
pub(crate) enum Added {
    New,
    Known,
    /// Nothing: the instance is retired, start another.
    Retired,
}

impl DetachedFiles {
    /// An instance's files: `first` is the loose file it is started for,
    /// `base` the configured initialization options.
    pub(crate) fn new(base: Option<Value>, limit: FolderLimit, first: Option<&Path>) -> Self {
        let files = Files {
            loose: first.is_some(),
            open: first.into_iter().map(Path::to_path_buf).collect(),
            sent: None,
            retired: false,
            folders: HashMap::new(),
        };
        DetachedFiles {
            base,
            limit,
            state: Mutex::new(files),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Files> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the instance serves loose files.
    pub(crate) fn is_loose(&self) -> bool {
        self.lock().loose
    }

    /// Whether the instance did not start because none of its loose files
    /// could be analysed.
    pub(crate) fn is_retired(&self) -> bool {
        self.lock().retired
    }

    /// Whether the list the server has names a file that is gone: the
    /// manager then restarts or stops it. Reads the file system for a few
    /// files.
    pub(crate) fn names_missing(&self) -> bool {
        let sent = self.lock().sent.clone();
        sent.is_some_and(|sent| sent.iter().any(|f| !f.is_file()))
    }

    /// A loose file was opened on the instance. A new one must then be
    /// sent to the server (see [`DetachedFiles::update`]).
    pub(crate) fn add(&self, file: &Path) -> Added {
        let mut state = self.lock();
        if state.retired {
            return Added::Retired;
        }
        state.loose = true;
        if state.open.iter().any(|f| f == file) {
            return Added::Known;
        }
        state.open.push(file.to_path_buf());
        Added::New
    }

    /// `path`, or every file below it, was closed, renamed or deleted.
    /// Returns whether the instance serves loose files.
    pub(crate) fn remove(&self, path: &Path) -> bool {
        let mut state = self.lock();
        state.open.retain(|f| !f.starts_with(path));
        state.loose
    }

    /// Marks the server as starting and decides how. Reads the file system:
    /// call it off the UI thread.
    pub(crate) fn launch(&self) -> Outcome<Launch> {
        let mut refused = Vec::new();
        loop {
            if !self.is_loose() {
                self.lock().sent = Some(Vec::new());
                return Outcome {
                    action: Launch::Configured,
                    refused,
                };
            }
            let wanted = self.wanted();
            refused.extend(wanted.refused);
            let mut state = self.lock();
            if state.open != wanted.open {
                continue; // a file was added meanwhile
            }
            let action = if wanted.files.is_empty() {
                state.retired = true;
                Launch::Nothing
            } else {
                Launch::Detached(with_detached_files(self.base.clone(), &wanted.files))
            };
            state.sent = Some(wanted.files);
            return Outcome { action, refused };
        }
    }

    /// What the running server must hear after a file was added; see the
    /// module docs. A missing file of its list is dropped too, should one
    /// get here before the manager restarted the instance. Reads the file
    /// system: call it off the UI thread.
    pub(crate) fn update(&self) -> Outcome<Update> {
        let sent = {
            let state = self.lock();
            state.sent.clone().filter(|_| state.loose)
        };
        let Some(sent) = sent else {
            // Not started yet: the launch takes the files as they are then.
            return Outcome {
                action: Update::Unchanged,
                refused: Vec::new(),
            };
        };
        let Wanted { files, refused, .. } = self.wanted();
        let stale = sent.iter().any(|f| !f.is_file());
        let new = files.iter().any(|f| !sent.contains(f));
        if files.is_empty() || !(stale || new) {
            return Outcome {
                action: Update::Unchanged,
                refused,
            };
        }
        let action = Update::Send(with_detached_files(self.base.clone(), &files));
        self.lock().sent = Some(files);
        Outcome { action, refused }
    }

    /// The open files that exist in folders small enough to analyse, and
    /// the folders newly found too big. Counts folders without the lock.
    fn wanted(&self) -> Wanted {
        let (open, mut verdicts) = {
            let state = self.lock();
            (state.open.clone(), state.folders.clone())
        };
        let mut refused = Vec::new();
        let existing: Vec<&PathBuf> = open.iter().filter(|f| f.is_file()).collect();
        for file in &existing {
            let Some(dir) = file.parent() else { continue };
            if !verdicts.contains_key(dir) {
                let small = folder::small_enough(dir, self.limit);
                if !small {
                    refused.push(dir.to_path_buf());
                }
                verdicts.insert(dir.to_path_buf(), small);
            }
        }
        let files = existing
            .into_iter()
            .filter(|f| f.parent().is_some_and(|d| verdicts[d]))
            .cloned()
            .collect();
        self.lock().folders = verdicts;
        Wanted {
            open,
            files,
            refused,
        }
    }
}

/// What the user sees about a loose file in `dir`, a folder too big to
/// analyse.
pub(crate) fn refused_message(dir: &Path) -> String {
    format!(
        "Rust files in {} are not analysed: they are outside any project and \
         rust-analyzer would read every Rust file under that folder, which is \
         too many. Move the file into a folder of its own, or make the folder \
         a project with `cargo init`.",
        dir.display()
    )
}

/// Gives the running `client` the settings `options`, then has it reload
/// its workspaces once it asked for them; see the module docs.
pub(crate) fn send_settings(client: &LspClient, options: Value) {
    let pulled = client.change_settings(options);
    let client = client.clone();
    tokio::spawn(async move {
        if !pulled.await {
            log::debug!("[{}] did not ask for the new settings", client.name());
        }
        if let Err(err) = client.request::<ReloadWorkspace>(()).await {
            log::debug!("[{}] {}: {err}", client.name(), ReloadWorkspace::METHOD);
        }
    });
}
