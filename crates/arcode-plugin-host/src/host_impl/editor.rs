//! `arcode:plugin/editor`: reads come from rope snapshots (never the UI
//! thread); writes and opens are forwarded to the [`EditorBridge`].
//!
//! Access (§4): `editor = "none"` sees nothing; `"context"` sees the
//! document the call is about and documents of languages the plugin
//! contributes; `"all-open"` sees everything once granted (asked at enable)
//! and falls back to `"context"` otherwise.
//!
//! [`EditorBridge`]: crate::bridge::EditorBridge

use crate::bindings::arcode::plugin::editor::{self as wit, DocInfo, EditorError, TextEdit};
use crate::bindings::{DocId, Position, Range};
use crate::bridge::DocSnapshot;
use crate::consent::ConsentKind;
use crate::grants::{Capability, Decision};
use crate::manifest::EditorAccess;
use crate::state::HostState;
use crate::cancel::lock;

/// Longest scratch buffer text accepted (bytes).
const MAX_SCRATCH_BYTES: usize = 64 * 1024 * 1024;

fn info(s: &DocSnapshot) -> DocInfo {
    DocInfo {
        id: s.id,
        path: s.path.clone(),
        language: s.language.clone(),
        version: s.version,
        line_count: u32::try_from(s.rope.len_lines()).unwrap_or(u32::MAX),
    }
}

/// Char index of `p`, if it lies inside the document.
fn char_index(s: &DocSnapshot, p: Position) -> Option<usize> {
    let line = p.line as usize;
    if line >= s.rope.len_lines() {
        return None;
    }
    let line_len = s.rope.line(line).len_chars();
    let column = p.column as usize;
    (column <= line_len).then(|| s.rope.line_to_char(line) + column)
}

impl HostState {
    fn may_read(&self, snap: &DocSnapshot) -> bool {
        let m = &self.ctx.manifest;
        let in_context = || {
            Some(snap.id) == self.call.origin_doc
                || m.contributes.languages.iter().any(|l| l.id == snap.language)
                || m.contributes
                    .code_lenses
                    .iter()
                    .any(|c| c.languages.contains(&snap.language))
        };
        match m.capabilities.editor {
            EditorAccess::None => false,
            EditorAccess::Context => in_context(),
            EditorAccess::AllOpen => {
                let store = lock(&self.ctx.services.permissions.store);
                let granted = store.decide(
                    m.id(),
                    &Capability::EditorAllOpen,
                    &m.capabilities,
                    self.ctx.runtime.network_enabled(),
                ) == Decision::Allow;
                granted || in_context()
            }
        }
    }

    /// The snapshot of `doc` if this call may read it.
    fn readable(&self, doc: DocId) -> Result<DocSnapshot, EditorError> {
        if self.ctx.manifest.capabilities.editor == EditorAccess::None {
            return Err(EditorError::Denied);
        }
        let snap = self
            .ctx
            .services
            .editor
            .snapshot(doc)
            .ok_or(EditorError::NoSuchDocument)?;
        if self.may_read(&snap) {
            Ok(snap)
        } else {
            Err(EditorError::Denied)
        }
    }

    fn may_write(&self) -> bool {
        let ctx = &self.ctx;
        ctx.services.permissions.check(
            &ctx.manifest,
            ctx.project.as_ref().map(|p| p.name.as_str()),
            &Capability::EditorWrite,
            ConsentKind::EditorWrite,
            ctx.runtime.network_enabled(),
            &self.call.cancel,
        )
    }
}

/// A project-relative path with `/` separators and no `.`/`..` parts.
fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

impl wit::Host for HostState {
    fn current_document(&mut self) -> wasmtime::Result<Option<DocInfo>> {
        let doc = self.call.origin_doc.or_else(|| self.ctx.services.editor.focused());
        Ok(doc.and_then(|d| self.readable(d).ok()).map(|s| info(&s)))
    }

    fn document_info(&mut self, doc: DocId) -> wasmtime::Result<Result<DocInfo, EditorError>> {
        Ok(self.readable(doc).map(|s| info(&s)))
    }

    fn text(&mut self, doc: DocId, range: Option<Range>) -> wasmtime::Result<Result<String, EditorError>> {
        let snap = match self.readable(doc) {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let Some(range) = range else {
            return Ok(Ok(snap.rope.to_string()));
        };
        let (Some(start), Some(end)) = (char_index(&snap, range.start), char_index(&snap, range.end)) else {
            return Ok(Err(EditorError::InvalidRange));
        };
        if start > end {
            return Ok(Err(EditorError::InvalidRange));
        }
        Ok(Ok(snap.rope.slice(start..end).to_string()))
    }

    fn selections(&mut self, doc: DocId) -> wasmtime::Result<Result<Vec<Range>, EditorError>> {
        Ok(self.readable(doc).map(|s| s.selections))
    }

    fn apply_edits(
        &mut self,
        doc: DocId,
        expected_version: u64,
        edits: Vec<TextEdit>,
    ) -> wasmtime::Result<Result<u64, EditorError>> {
        if let Err(e) = self.readable(doc) {
            return Ok(Err(e));
        }
        if !self.may_write() {
            return Ok(Err(EditorError::Denied));
        }
        Ok(self.ctx.services.editor.apply_edits(doc, expected_version, edits))
    }

    fn open_file(&mut self, path: String, at: Option<Position>) -> wasmtime::Result<Result<(), EditorError>> {
        if self.ctx.project.is_none() {
            return Ok(Err(EditorError::Denied));
        }
        if !valid_relative_path(&path) {
            return Ok(Err(EditorError::InvalidPath));
        }
        Ok(self.ctx.services.editor.open_file(&path, at))
    }

    fn open_scratch(
        &mut self,
        title: String,
        language: String,
        text: String,
    ) -> wasmtime::Result<Result<DocId, EditorError>> {
        if text.len() > MAX_SCRATCH_BYTES {
            return Ok(Err(EditorError::InvalidRange));
        }
        let title = super::cap(title, 128);
        Ok(self.ctx.services.editor.open_scratch(&title, &language, &text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_only() {
        for ok in ["a", "src/main.rs", "a b/c.txt"] {
            assert!(valid_relative_path(ok), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a//b", "./a", "C:/x", "a\\b"] {
            assert!(!valid_relative_path(bad), "{bad}");
        }
    }

    #[test]
    fn positions_map_to_chars() {
        let snap = DocSnapshot {
            id: 1,
            path: None,
            language: "x".into(),
            version: 1,
            rope: ropey::Rope::from_str("héllo\nwörld"),
            selections: vec![],
        };
        let at = |line, column| char_index(&snap, Position { line, column });
        assert_eq!(at(0, 0), Some(0));
        assert_eq!(at(1, 2), Some(8));
        assert_eq!(at(1, 5), Some(11));
        assert_eq!(at(1, 6), None);
        assert_eq!(at(2, 0), None);
    }
}
