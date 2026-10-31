//! Words of the open buffers: the fallback source when no language server
//! has anything to offer.
//!
//! Each editor builds its [`WordIndex`] the first time completion needs it
//! (small buffers right away, larger ones on the background executor) and
//! keeps it in sync with every edit afterwards. Very large buffers are
//! never indexed.

use arcode_buffer::Edit;
use arcode_completion::{CompletionCandidate, WordIndex};
use gpui::{App, AppContext, Context, Entity, EntityId, Global, Task, WeakEntity};

use crate::Editor;

/// Buffers up to this size are indexed synchronously.
const SYNC_INDEX_BYTES: usize = 256 * 1024;
/// Buffers above this size are not indexed at all.
const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;

/// The word index of one editor.
#[derive(Default)]
pub(crate) enum BufferWords {
    /// Not needed yet.
    #[default]
    Unbuilt,
    /// Being built from a snapshot; edits made meanwhile are replayed.
    Building { pending: Vec<Edit>, _task: Task<()> },
    /// Up to date.
    Ready(WordIndex),
    /// The buffer is too large.
    Disabled,
}

/// Every live editor, so completion can offer words from all of them.
#[derive(Default)]
struct OpenEditors(Vec<WeakEntity<Editor>>);

impl Global for OpenEditors {}

/// Registers a new editor as a word source.
pub(crate) fn register_editor(editor: WeakEntity<Editor>, cx: &mut App) {
    let open = cx.default_global::<OpenEditors>();
    open.0.retain(|e| e.upgrade().is_some());
    open.0.push(editor);
}

/// Live editors other than `except`.
pub(crate) fn other_editors(except: EntityId, cx: &App) -> Vec<Entity<Editor>> {
    cx.try_global::<OpenEditors>()
        .map(|open| {
            open.0
                .iter()
                .filter_map(WeakEntity::upgrade)
                .filter(|e| e.entity_id() != except)
                .collect()
        })
        .unwrap_or_default()
}

impl Editor {
    /// Keeps the word index in sync with `edits`.
    pub(crate) fn words_after_edits(&mut self, edits: &[Edit]) {
        match &mut self.completion.words {
            BufferWords::Building { pending, .. } => pending.extend_from_slice(edits),
            BufferWords::Ready(index) => index.apply_edits(edits, &self.buffer),
            BufferWords::Unbuilt | BufferWords::Disabled => {}
        }
    }

    /// Starts building the word index if it was never needed before.
    fn ensure_word_index(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.completion.words, BufferWords::Unbuilt) {
            return;
        }
        let bytes = self.buffer.len_bytes();
        if self.large_file || bytes > MAX_INDEX_BYTES {
            self.completion.words = BufferWords::Disabled;
            return;
        }
        if bytes <= SYNC_INDEX_BYTES {
            let text = self.buffer.text().to_string();
            self.completion.words = BufferWords::Ready(WordIndex::from_text(&text));
            return;
        }
        let rope = self.buffer.text().clone();
        let build = cx.background_spawn(async move { WordIndex::from_text(&rope.to_string()) });
        let task = cx.spawn(async move |this, cx| {
            let mut index = build.await;
            this.update(cx, |editor, _| {
                if let BufferWords::Building { pending, .. } = &mut editor.completion.words {
                    let pending = std::mem::take(pending);
                    index.apply_edits(&pending, &editor.buffer);
                    editor.completion.words = BufferWords::Ready(index);
                }
            })
            .ok();
        });
        self.completion.words = BufferWords::Building {
            pending: Vec::new(),
            _task: task,
        };
    }

    /// Word candidates containing the chars of `needle` in order; `exclude`
    /// (the word being typed) is skipped when it occurs only once.
    pub(crate) fn word_candidates(
        &mut self,
        needle: &str,
        exclude: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Vec<CompletionCandidate> {
        self.ensure_word_index(cx);
        match &self.completion.words {
            BufferWords::Ready(index) => index.candidates_for(needle, exclude),
            _ => Vec::new(),
        }
    }
}
