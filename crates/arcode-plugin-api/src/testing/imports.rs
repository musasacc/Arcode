//! Fake implementations of every WIT import, with the exact signatures
//! wit-bindgen generates, so [`crate::sys`] can swap them in on native
//! targets. Behaviour mirrors the host where cheap (kv quota and key rules,
//! editor version checks, gesture-only `begin-task`).

/// Fake `arcode:plugin/host`.
pub mod host {
    use super::super::with;
    use crate::bindings::arcode::plugin::types::Level;

    /// Always "0.1.0".
    pub fn api_version() -> String {
        "0.1.0".to_owned()
    }
    /// Records the line in `FakeHost::log`.
    pub fn log(level: Level, message: &str) {
        with(|h| h.log.push((level, message.to_owned())));
    }
    /// Records the toast in `FakeHost::notifications`.
    pub fn notify(level: Level, message: &str) {
        with(|h| h.notifications.push((level, message.to_owned())));
    }
    /// Records the status item.
    pub fn set_status(text: Option<&str>, command: Option<&str>) {
        with(|h| h.status = Some((text.map(str::to_owned), command.map(str::to_owned))));
    }
    /// Pops the next scripted answer.
    pub fn quick_pick(title: &str, items: &[String]) -> Option<u32> {
        with(|h| {
            h.quick_picks.push(title.to_owned());
            let answer = if h.quick_pick_answers.is_empty() {
                None
            } else {
                h.quick_pick_answers.remove(0)
            };
            answer.filter(|&i| (i as usize) < items.len())
        })
    }
    /// `FakeHost::settings`, or `{}`.
    pub fn settings() -> String {
        with(|h| {
            if h.settings.is_empty() {
                "{}".to_owned()
            } else {
                h.settings.clone()
            }
        })
    }
    /// `FakeHost::cancelled`.
    pub fn cancelled() -> bool {
        with(|h| h.cancelled)
    }
    /// Succeeds only when `FakeHost::gesture` is set, like the host.
    pub fn begin_task(title: &str) -> bool {
        with(|h| {
            if h.gesture {
                h.tasks.push(title.to_owned());
            }
            h.gesture
        })
    }
}

/// Fake `arcode:plugin/kv`.
pub mod kv {
    use super::super::with;
    use crate::bindings::arcode::plugin::kv::{KvError, Scope};

    const QUOTA: usize = 1024 * 1024;

    fn valid_key(key: &str) -> bool {
        (1..=128).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
    }
    /// Reads a value.
    pub fn get(scope: Scope, key: &str) -> Option<Vec<u8>> {
        with(|h| h.kv.get(&(scope, key.to_owned())).cloned())
    }
    /// Writes a value, enforcing the key rules and the 1 MiB scope quota.
    pub fn set(scope: Scope, key: &str, value: &[u8]) -> Result<(), KvError> {
        if !valid_key(key) {
            return Err(KvError::InvalidKey);
        }
        with(|h| {
            let used: usize =
                h.kv.iter()
                    .filter(|((s, k), _)| *s == scope && k != key)
                    .map(|((_, k), v)| k.len() + v.len())
                    .sum();
            if used + key.len() + value.len() > QUOTA {
                return Err(KvError::QuotaExceeded);
            }
            h.kv.insert((scope, key.to_owned()), value.to_vec());
            Ok(())
        })
    }
    /// Deletes a value.
    pub fn delete(scope: Scope, key: &str) {
        with(|h| h.kv.remove(&(scope, key.to_owned())));
    }
    /// Keys of `scope` starting with `prefix`, sorted.
    pub fn keys(scope: Scope, prefix: &str) -> Vec<String> {
        with(|h| {
            h.kv.keys()
                .filter(|(s, k)| *s == scope && k.starts_with(prefix))
                .map(|(_, k)| k.clone())
                .collect()
        })
    }
}

/// Fake `arcode:plugin/workspace`.
pub mod workspace {
    use super::super::with;
    use crate::bindings::arcode::plugin::workspace::FsError;

    /// `FakeHost::root_name`.
    pub fn root_name() -> Option<String> {
        with(|h| h.root_name.clone())
    }
    /// `FakeHost::files`, capped at `limit` (and 10000). The glob is ignored.
    pub fn find_files(_glob: &str, limit: u32) -> Result<Vec<String>, FsError> {
        let limit = limit.min(10_000) as usize;
        with(|h| match &h.files_error {
            Some(e) => Err(e.clone()),
            None => Ok(h.files.iter().take(limit).cloned().collect()),
        })
    }
}

/// Fake `arcode:plugin/editor`.
pub mod editor {
    use super::super::{text, with};
    use crate::bindings::arcode::plugin::editor::{DocInfo, EditorError, TextEdit};
    use crate::bindings::arcode::plugin::types::{DocId, Position, Range};

    /// `FakeHost::current_document`.
    pub fn current_document() -> Option<DocInfo> {
        with(|h| {
            h.current_document
                .and_then(|id| h.documents.get(&id))
                .map(|d| d.info.clone())
        })
    }
    /// Info of a known document.
    pub fn document_info(doc: DocId) -> Result<DocInfo, EditorError> {
        with(|h| {
            h.documents
                .get(&doc)
                .map(|d| d.info.clone())
                .ok_or(EditorError::NoSuchDocument)
        })
    }
    /// Whole text, or the text in `range`.
    pub fn text(doc: DocId, range: Option<Range>) -> Result<String, EditorError> {
        with(|h| {
            let d = h.documents.get(&doc).ok_or(EditorError::NoSuchDocument)?;
            match range {
                None => Ok(d.text.clone()),
                Some(r) => text::byte_range(&d.text, r)
                    .map(|b| d.text[b].to_owned())
                    .ok_or(EditorError::InvalidRange),
            }
        })
    }
    /// `FakeDocument::selections`.
    pub fn selections(doc: DocId) -> Result<Vec<Range>, EditorError> {
        with(|h| {
            h.documents
                .get(&doc)
                .map(|d| d.selections.clone())
                .ok_or(EditorError::NoSuchDocument)
        })
    }
    /// Applies edits (ranges refer to the text before any edit), bumping the version.
    pub fn apply_edits(
        doc: DocId,
        expected_version: u64,
        edits: &[TextEdit],
    ) -> Result<u64, EditorError> {
        with(|h| {
            let d = h
                .documents
                .get_mut(&doc)
                .ok_or(EditorError::NoSuchDocument)?;
            if d.info.version != expected_version {
                return Err(EditorError::VersionMismatch(d.info.version));
            }
            let mut spans = Vec::with_capacity(edits.len());
            for e in edits {
                spans.push((
                    text::byte_range(&d.text, e.range).ok_or(EditorError::InvalidRange)?,
                    e.text.as_str(),
                ));
            }
            spans.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
            if spans.windows(2).any(|w| w[1].0.end > w[0].0.start) {
                return Err(EditorError::InvalidRange);
            }
            for (range, new) in spans {
                d.text.replace_range(range, new);
            }
            d.info.version += 1;
            d.info.line_count = text::line_count(&d.text);
            Ok(d.info.version)
        })
    }
    /// Records the call; fails for absolute paths or `..`, like the host.
    pub fn open_file(path: &str, at: Option<Position>) -> Result<(), EditorError> {
        if path.starts_with('/') || path.split(['/', '\\']).any(|c| c == "..") {
            return Err(EditorError::InvalidPath);
        }
        with(|h| h.opened_files.push((path.to_owned(), at)));
        Ok(())
    }
    /// Creates a new untitled document.
    pub fn open_scratch(title: &str, language: &str, text: &str) -> Result<DocId, EditorError> {
        with(|h| {
            h.scratches
                .push((title.to_owned(), language.to_owned(), text.to_owned()));
            Ok(h.add_document(None, language, text))
        })
    }
}

/// Fake `arcode:plugin/http`.
pub mod http {
    use super::super::with;
    use crate::bindings::arcode::plugin::http::{HttpError, Request, Response};

    /// Records the request and asks `FakeHost::http` (default: `denied`).
    pub fn send(req: &Request) -> Result<Response, HttpError> {
        // Take the responder out so it may itself use the fake host.
        let responder = with(|h| {
            h.requests.push(req.clone());
            h.http.take()
        });
        let Some(mut responder) = responder else {
            return Err(HttpError::Denied);
        };
        let result = responder(req);
        with(|h| {
            h.http.get_or_insert(responder);
        });
        result
    }
}

/// Fake `arcode:plugin/ui`.
pub mod ui {
    use super::super::{FakePanel, with};
    use crate::bindings::arcode::plugin::types::PanelId;
    use crate::bindings::arcode::plugin::ui::{Location, Patch, UiError, View};
    use crate::ui::FlatView;

    /// Focuses the open panel of `contribution` or opens a new one.
    pub fn open_panel(
        contribution: &str,
        title: &str,
        _location: Option<Location>,
    ) -> Result<PanelId, UiError> {
        if contribution.is_empty() {
            return Err(UiError::UnknownContribution);
        }
        with(|h| {
            if let Some((&id, _)) = h
                .panels
                .iter()
                .find(|(_, p)| p.contribution == contribution && !p.closed)
            {
                return Ok(id);
            }
            let id = h.panels.keys().next_back().map_or(1, |last| last + 1);
            let panel = FakePanel {
                contribution: contribution.to_owned(),
                title: title.to_owned(),
                ..FakePanel::default()
            };
            h.panels.insert(id, panel);
            Ok(id)
        })
    }
    /// Replaces the panel's view.
    pub fn render(panel: PanelId, view: &View) -> Result<(), UiError> {
        with(|h| {
            let p = h
                .panels
                .get_mut(&panel)
                .filter(|p| !p.closed)
                .ok_or(UiError::NoSuchPanel)?;
            p.view = FlatView::from_view(view);
            p.renders += 1;
            Ok(())
        })
    }
    /// Applies patches with the SDK's reference semantics.
    pub fn apply_patches(panel: PanelId, patches: &[Patch]) -> Result<(), UiError> {
        with(|h| {
            let p = h
                .panels
                .get_mut(&panel)
                .filter(|p| !p.closed)
                .ok_or(UiError::NoSuchPanel)?;
            crate::ui::apply(&mut p.view, patches);
            p.patch_calls.push(patches.to_vec());
            Ok(())
        })
    }
    /// Sets the panel title.
    pub fn set_title(panel: PanelId, title: &str) {
        with(|h| {
            h.panels
                .get_mut(&panel)
                .into_iter()
                .for_each(|p| p.title = title.to_owned())
        });
    }
    /// Marks the panel closed.
    pub fn close_panel(panel: PanelId) {
        with(|h| {
            h.panels
                .get_mut(&panel)
                .into_iter()
                .for_each(|p| p.closed = true)
        });
    }
}
