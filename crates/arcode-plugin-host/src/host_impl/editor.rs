//! `arcode:plugin/editor`: reads come from rope snapshots (never the UI
//! thread); writes and opens are forwarded to the [`EditorBridge`].
//!
//! Access (§4): `editor = "none"` sees nothing; `"context"` sees the
//! document the call is about and documents of languages the plugin
//! contributes under `contributes.languages` (never a language the app has
//! built in, see `RuntimeConfig::builtin_languages`; lens languages alone
//! do not count); `"all-open"` sees everything once granted (asked at
//! enable) and falls back to `"context"` otherwise.
//!
//! `open-file` resolves the path inside the project and refuses symlinks
//! and anything that is not a regular file there, so a plugin with a write
//! grant cannot plant `/project/x -> ../secret` and have the app open it.
//!
//! [`EditorBridge`]: crate::bridge::EditorBridge

use crate::bindings::arcode::plugin::editor::{self as wit, DocInfo, EditorError, TextEdit};
use crate::bindings::{DocId, Position, Range};
use crate::bridge::DocSnapshot;
use crate::cancel::lock;
use crate::consent::ConsentKind;
use crate::grants::{Capability, Decision};
use crate::manifest::EditorAccess;
use crate::state::HostState;

/// Longest scratch buffer text accepted (bytes).
const MAX_SCRATCH_BYTES: usize = 64 * 1024 * 1024;
/// Most inserted text per `apply-edits` (bytes).
const MAX_EDIT_BYTES: usize = 64 * 1024 * 1024;

fn info(s: &DocSnapshot) -> DocInfo {
    DocInfo {
        id: s.id,
        path: s.path.clone(),
        language: s.language.clone(),
        version: s.version,
        line_count: u32::try_from(s.rope.len_lines()).unwrap_or(u32::MAX),
    }
}

impl HostState {
    fn may_read(&self, snap: &DocSnapshot) -> bool {
        let m = &self.ctx.manifest;
        let builtin = &self.ctx.runtime.config().builtin_languages;
        let in_context = || {
            Some(snap.id) == self.call.origin_doc
                || (!builtin.contains(&snap.language)
                    && m.contributes
                        .languages
                        .iter()
                        .any(|l| l.id == snap.language))
        };
        match m.capabilities.editor {
            EditorAccess::None => false,
            EditorAccess::Context => in_context(),
            EditorAccess::AllOpen => {
                let mut store = lock(&self.ctx.services.permissions.store);
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
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// True if `root/rel` is a regular file reached without following any
/// symlink (no component may be a link, the file itself included), so the
/// app opens exactly a file inside the project.
fn regular_file_inside(root: &std::path::Path, rel: &str) -> bool {
    let mut at = root.to_path_buf();
    let parts: Vec<&str> = rel.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        at.push(part);
        let Ok(meta) = std::fs::symlink_metadata(&at) else {
            return false;
        };
        let last = i + 1 == parts.len();
        let ok = if last {
            meta.file_type().is_file()
        } else {
            meta.file_type().is_dir()
        };
        if !ok {
            return false;
        }
    }
    true
}

impl wit::Host for HostState {
    fn current_document(&mut self) -> wasmtime::Result<Option<DocInfo>> {
        let doc = self
            .call
            .origin_doc
            .or_else(|| self.ctx.services.editor.focused());
        Ok(doc.and_then(|d| self.readable(d).ok()).map(|s| info(&s)))
    }

    fn document_info(&mut self, doc: DocId) -> wasmtime::Result<Result<DocInfo, EditorError>> {
        Ok(self.readable(doc).map(|s| info(&s)))
    }

    fn text(
        &mut self,
        doc: DocId,
        range: Option<Range>,
    ) -> wasmtime::Result<Result<String, EditorError>> {
        let snap = match self.readable(doc) {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let Some(range) = range else {
            return Ok(Ok(snap.rope.to_string()));
        };
        match snap.char_range(range) {
            Some(r) => Ok(Ok(snap.rope.slice(r).to_string())),
            None => Ok(Err(EditorError::InvalidRange)),
        }
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
        let snap = match self.readable(doc) {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        // Refuse what would fail anyway before asking the user anything.
        if snap.version != expected_version {
            return Ok(Err(EditorError::VersionMismatch(snap.version)));
        }
        let text_bytes: usize = edits.iter().map(|e| e.text.len()).sum();
        if text_bytes > MAX_EDIT_BYTES {
            return Ok(Err(EditorError::InvalidRange));
        }
        if let Err(e) = snap.resolve_edits(&edits) {
            return Ok(Err(e));
        }
        // The prompt and the UI-thread round trip are waits.
        Ok(self.waiting(|s| {
            if !s.may_write() {
                return Err(EditorError::Denied);
            }
            s.ctx
                .services
                .editor
                .apply_edits(doc, expected_version, edits)
        }))
    }

    fn open_file(
        &mut self,
        path: String,
        at: Option<Position>,
    ) -> wasmtime::Result<Result<(), EditorError>> {
        if self.ctx.project.is_none() {
            return Ok(Err(EditorError::Denied));
        }
        if !valid_relative_path(&path) {
            return Ok(Err(EditorError::InvalidPath));
        }
        let inside = self
            .ctx
            .project
            .as_ref()
            .is_some_and(|p| regular_file_inside(&p.root, &path));
        if !inside {
            return Ok(Err(EditorError::InvalidPath));
        }
        Ok(self.waiting(|s| s.ctx.services.editor.open_file(&path, at)))
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
        Ok(self.waiting(|s| s.ctx.services.editor.open_scratch(&title, &language, &text)))
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
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "C:/x",
            "a\\b",
        ] {
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
        let at = |line, column| snap.char_index(Position { line, column });
        assert_eq!(at(0, 0), Some(0));
        assert_eq!(at(1, 2), Some(8));
        assert_eq!(at(1, 5), Some(11));
        assert_eq!(at(1, 6), None);
        assert_eq!(at(2, 0), None);
    }
}
