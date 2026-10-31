//! The project's breakpoints: plain data, independent of any session.
//!
//! Lines are zero-based buffer lines; adapters get one-based lines. Ids are
//! stable for the lifetime of the book (used as gutter tags).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arcode_config::SavedBreakpoint;
use arcode_dap::{Breakpoint as DapBreakpoint, SourceBreakpoint};

/// One breakpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bp {
    /// Stable id (gutter tag).
    pub id: u64,
    /// The file.
    pub path: PathBuf,
    /// Zero-based line.
    pub line: usize,
    /// Unchecked breakpoints are kept but not sent.
    pub enabled: bool,
    /// Break only when this expression is true.
    pub condition: Option<String>,
    /// Break only after this many hits.
    pub hit_condition: Option<String>,
    /// Log this message instead of stopping.
    pub log_message: Option<String>,
    /// What the adapter said: `Some(false)` with a message when it could
    /// not set the breakpoint; `None` without a session.
    pub verified: Option<bool>,
    /// The adapter's message, if any.
    pub message: Option<String>,
}

/// What kind of extra a breakpoint has (for drawing and the edit dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpKind {
    /// Stops every time.
    Plain,
    /// Has a condition or hit count.
    Conditional,
    /// Logs instead of stopping.
    LogPoint,
}

impl Bp {
    /// The kind of breakpoint.
    pub fn kind(&self) -> BpKind {
        if self.log_message.is_some() {
            BpKind::LogPoint
        } else if self.condition.is_some() || self.hit_condition.is_some() {
            BpKind::Conditional
        } else {
            BpKind::Plain
        }
    }

    /// The adapter's form (one-based line).
    pub fn to_source(&self) -> SourceBreakpoint {
        SourceBreakpoint {
            line: self.line as u32 + 1,
            column: None,
            condition: self.condition.clone(),
            hit_condition: self.hit_condition.clone(),
            log_message: self.log_message.clone(),
        }
    }

    /// A one-line description for lists and tooltips.
    pub fn detail(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(c) = &self.condition {
            parts.push(format!("when {c}"));
        }
        if let Some(h) = &self.hit_condition {
            parts.push(format!("hits {h}"));
        }
        if let Some(l) = &self.log_message {
            parts.push(format!("log \"{l}\""));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// Every breakpoint of a project.
#[derive(Debug, Default, Clone)]
pub struct BreakpointBook {
    items: Vec<Bp>,
    next_id: u64,
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

impl BreakpointBook {
    /// A book from saved breakpoints.
    pub fn from_saved(saved: &[SavedBreakpoint]) -> Self {
        let mut book = Self::default();
        for s in saved {
            let id = book.add(&s.path, s.line);
            if let Some(bp) = book.get_mut(id) {
                bp.enabled = s.enabled;
                bp.condition = non_empty(s.condition.clone());
                bp.hit_condition = non_empty(s.hit_condition.clone());
                bp.log_message = non_empty(s.log_message.clone());
            }
        }
        book
    }

    /// The breakpoints to save.
    pub fn to_saved(&self) -> Vec<SavedBreakpoint> {
        self.items
            .iter()
            .map(|b| SavedBreakpoint {
                path: b.path.clone(),
                line: b.line,
                enabled: b.enabled,
                condition: b.condition.clone(),
                hit_condition: b.hit_condition.clone(),
                log_message: b.log_message.clone(),
            })
            .collect()
    }

    /// All breakpoints, sorted by path and line.
    pub fn all(&self) -> Vec<&Bp> {
        let mut all: Vec<&Bp> = self.items.iter().collect();
        all.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        all
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The breakpoints of `path`.
    pub fn in_file<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a Bp> + 'a {
        self.items.iter().filter(move |b| b.path == path)
    }

    /// Files that have breakpoints.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.items.iter().map(|b| b.path.clone()).collect();
        files.sort();
        files.dedup();
        files
    }

    /// The breakpoint at `path:line`.
    pub fn at(&self, path: &Path, line: usize) -> Option<&Bp> {
        self.items.iter().find(|b| b.path == path && b.line == line)
    }

    /// A breakpoint by id.
    pub fn get(&self, id: u64) -> Option<&Bp> {
        self.items.iter().find(|b| b.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Bp> {
        self.items.iter_mut().find(|b| b.id == id)
    }

    /// Adds a plain breakpoint (or returns the existing one's id).
    pub fn add(&mut self, path: &Path, line: usize) -> u64 {
        if let Some(bp) = self.at(path, line) {
            return bp.id;
        }
        self.next_id += 1;
        self.items.push(Bp {
            id: self.next_id,
            path: path.to_path_buf(),
            line,
            enabled: true,
            condition: None,
            hit_condition: None,
            log_message: None,
            verified: None,
            message: None,
        });
        self.next_id
    }

    /// Adds a breakpoint at `path:line`, or removes the one there. Returns
    /// whether one exists afterwards.
    pub fn toggle(&mut self, path: &Path, line: usize) -> bool {
        match self.at(path, line).map(|b| b.id) {
            Some(id) => {
                self.remove(id);
                false
            }
            None => {
                self.add(path, line);
                true
            }
        }
    }

    /// Removes a breakpoint; returns its file.
    pub fn remove(&mut self, id: u64) -> Option<PathBuf> {
        let ix = self.items.iter().position(|b| b.id == id)?;
        Some(self.items.remove(ix).path)
    }

    /// Removes every breakpoint; returns the files that had some.
    pub fn clear(&mut self) -> Vec<PathBuf> {
        let files = self.files();
        self.items.clear();
        files
    }

    /// Checks or unchecks a breakpoint; returns its file when it changed.
    pub fn set_enabled(&mut self, id: u64, enabled: bool) -> Option<PathBuf> {
        let bp = self.get_mut(id)?;
        (bp.enabled != enabled).then(|| {
            bp.enabled = enabled;
            bp.path.clone()
        })
    }

    /// Sets the condition, hit count and log message of the breakpoint at
    /// `path:line` (created if needed; empty strings clear a field).
    pub fn set_options(
        &mut self,
        path: &Path,
        line: usize,
        condition: Option<String>,
        hit_condition: Option<String>,
        log_message: Option<String>,
    ) -> u64 {
        let id = self.add(path, line);
        if let Some(bp) = self.get_mut(id) {
            bp.condition = non_empty(condition);
            bp.hit_condition = non_empty(hit_condition);
            bp.log_message = non_empty(log_message);
            bp.enabled = true;
        }
        id
    }

    /// The enabled breakpoints of `path` in the adapter's form, with their
    /// ids in the same order.
    pub fn source_breakpoints(&self, path: &Path) -> (Vec<u64>, Vec<SourceBreakpoint>) {
        let mut items: Vec<&Bp> = self.in_file(path).filter(|b| b.enabled).collect();
        items.sort_by_key(|b| b.line);
        (
            items.iter().map(|b| b.id).collect(),
            items.iter().map(|b| b.to_source()).collect(),
        )
    }

    /// Every file's enabled breakpoints, for starting a session.
    pub fn all_source_breakpoints(&self) -> BTreeMap<PathBuf, Vec<SourceBreakpoint>> {
        self.files()
            .into_iter()
            .filter_map(|path| {
                let (_, list) = self.source_breakpoints(&path);
                (!list.is_empty()).then_some((path, list))
            })
            .collect()
    }

    /// Takes the lines an edit moved breakpoints to (`(id, line)` for every
    /// breakpoint of `path` whose line still exists). Breakpoints of `path`
    /// that are not listed were deleted with their line and are removed.
    /// Returns whether anything changed.
    pub fn relocate(&mut self, path: &Path, drawn: &[(u64, usize)]) -> bool {
        let before = self.items.clone();
        self.items
            .retain(|b| b.path != path || drawn.iter().any(|(id, _)| *id == b.id));
        for &(id, line) in drawn {
            if let Some(bp) = self.get_mut(id) {
                bp.line = line;
            }
        }
        // Two breakpoints moved onto one line: keep the first.
        let mut seen = std::collections::HashSet::new();
        self.items.retain(|b| seen.insert((b.path.clone(), b.line)));
        self.items != before
    }

    /// Applies the adapter's answer for `ids` (same order as sent).
    pub fn apply_verification(&mut self, ids: &[u64], answer: &[DapBreakpoint]) {
        for (id, bp) in ids.iter().zip(answer) {
            if let Some(item) = self.get_mut(*id) {
                item.verified = Some(bp.verified);
                item.message = bp.message.clone();
            }
        }
    }

    /// Forgets what adapters said (the session ended).
    pub fn clear_verification(&mut self) {
        for bp in &mut self.items {
            bp.verified = None;
            bp.message = None;
        }
    }

    /// Follows a renamed file or folder.
    pub fn rename(&mut self, from: &Path, to: &Path) -> bool {
        let mut changed = false;
        for bp in &mut self.items {
            if let Ok(rest) = bp.path.strip_prefix(from) {
                bp.path = if rest.as_os_str().is_empty() {
                    to.to_path_buf()
                } else {
                    to.join(rest)
                };
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
#[path = "breakpoints_tests.rs"]
mod tests;
