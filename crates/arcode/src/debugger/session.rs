//! The state of a running debug session as the UI shows it: threads, the
//! selected thread's frames, scopes, loaded variables and watches.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use arcode_dap::{
    Capabilities, DebugSessionHandle, Scope, SessionState, StackFrame, Thread, Variable,
};
use gpui::Task;

use super::launch::LaunchChoice;

/// A node of the variables or watch tree, identified by the names from its
/// root (`"Locals/self/items"`). Expansion is kept by path, so it survives
/// steps (variable references change at every stop).
pub type NodePath = String;

/// Joins a parent path and a child name.
pub fn child_path(parent: &str, name: &str) -> NodePath {
    format!("{parent}/{name}")
}

/// What a watch expression evaluated to.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchValue {
    /// Not evaluated (no stopped frame).
    Pending,
    /// The value, its type and a reference to its children (0: none).
    Value {
        /// Display value.
        value: String,
        /// Type, if known.
        type_: Option<String>,
        /// Children reference.
        reference: i64,
    },
    /// The adapter's error.
    Error(String),
}

/// A running (or ended but still shown) session.
pub struct Session {
    /// The configuration's name.
    pub name: String,
    /// The debug adapter's name (`lldb-dap`, `gdb`).
    pub adapter: String,
    /// What was started (for restart).
    pub choice: LaunchChoice,
    /// The adapter handle.
    pub handle: DebugSessionHandle,
    /// Client-side state.
    pub state: SessionState,
    /// Adapter capabilities.
    pub capabilities: Capabilities,
    /// Known threads.
    pub threads: Vec<Thread>,
    /// The thread shown in the call stack.
    pub thread: Option<i64>,
    /// Frames of `thread`, innermost first.
    pub frames: Vec<StackFrame>,
    /// The selected frame id.
    pub frame: Option<i64>,
    /// Scopes of the selected frame.
    pub scopes: Vec<Scope>,
    /// Loaded children by variables reference.
    pub children: HashMap<i64, Vec<Variable>>,
    /// References being loaded.
    pub loading: HashSet<i64>,
    /// Adapter breakpoint id → our breakpoint id.
    pub adapter_ids: HashMap<i64, u64>,
    /// Ids of the breakpoints each file started with, in the order sent;
    /// the adapter's answer to that first set is matched with them. A file
    /// whose breakpoints were sent again since is removed.
    pub startup_ids: HashMap<PathBuf, Vec<u64>>,
    /// A continue or step request was sent and the program has not been
    /// reported running yet: further steps are dropped.
    pub stepping: bool,
    /// Increases at every stop; answers for older stops are ignored.
    pub generation: u64,
    /// The debuggee's exit code, once known.
    pub exit_code: Option<i64>,
    /// Pumps the adapter's events.
    pub _events: Task<()>,
}

impl Session {
    /// The thread to resume or step: the selected one, else the first.
    pub fn current_thread(&self) -> Option<i64> {
        self.thread
            .or_else(|| self.threads.first().map(|t| t.id))
            .or_else(|| self.handle.known_threads().first().map(|t| t.id))
    }

    /// Whether the debuggee is paused.
    pub fn is_stopped(&self) -> bool {
        self.state.is_stopped()
    }

    /// Whether the session ended.
    pub fn is_terminated(&self) -> bool {
        self.state == SessionState::Terminated
    }

    /// Forgets everything that belongs to the previous stop.
    pub fn forget_stop(&mut self) {
        self.generation += 1;
        self.frames.clear();
        self.frame = None;
        self.scopes.clear();
        self.children.clear();
        self.loading.clear();
    }

    /// The selected frame.
    pub fn selected_frame(&self) -> Option<&StackFrame> {
        let id = self.frame?;
        self.frames.iter().find(|f| f.id == id)
    }
}

/// Where a frame is, as a file and zero-based line.
pub fn frame_location(frame: &StackFrame) -> Option<(PathBuf, usize)> {
    let path = frame.source.as_ref()?.path.as_ref()?;
    Some((PathBuf::from(path), frame.line.saturating_sub(1) as usize))
}

/// The frame to select after a stop: the innermost one with a source file
/// (adapters often stop inside libraries without sources).
pub fn first_frame_with_source(frames: &[StackFrame]) -> Option<&StackFrame> {
    frames
        .iter()
        .find(|f| frame_location(f).is_some_and(|(p, _)| p.is_file()))
        .or_else(|| frames.iter().find(|f| frame_location(f).is_some()))
        .or_else(|| frames.first())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_dap::Source;

    fn frame(id: i64, path: Option<&str>, line: u32) -> StackFrame {
        StackFrame {
            id,
            name: format!("f{id}"),
            source: path.map(|p| Source {
                name: None,
                path: Some(p.into()),
                source_reference: None,
            }),
            line,
            column: 1,
            presentation_hint: None,
        }
    }

    #[test]
    fn the_first_frame_with_a_real_file_is_selected() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_string_lossy().into_owned();
        let frames = [
            frame(1, None, 0),
            frame(2, Some("/no/such/libc.c"), 10),
            frame(3, Some(&path), 7),
        ];
        assert_eq!(first_frame_with_source(&frames).unwrap().id, 3);
        assert_eq!(frame_location(&frames[2]), Some((PathBuf::from(&path), 6)));
        assert_eq!(first_frame_with_source(&frames[..2]).unwrap().id, 2);
        assert_eq!(first_frame_with_source(&frames[..1]).unwrap().id, 1);
        assert!(first_frame_with_source(&[]).is_none());
        assert_eq!(child_path("Locals", "x"), "Locals/x");
    }
}
