//! The snippet library: built-in snippets plus the user's `snippets.toml`
//! from the config directory, loaded on the background executor at startup
//! and again whenever that file is saved in an editor.

use std::path::Path;
use std::sync::Arc;

use arcode_completion::{SnippetLibrary, load_snippets};
use gpui::{App, Global};

use super::CompletionItem;

/// The active snippet library.
struct SnippetStore {
    library: Arc<SnippetLibrary>,
    /// Problem with the user file, if any.
    error: Option<String>,
}

impl Global for SnippetStore {}

/// Installs the built-in snippets and loads the user file in the
/// background (idempotent).
pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<SnippetStore>() {
        return;
    }
    cx.set_global(SnippetStore {
        library: Arc::new(SnippetLibrary::builtin()),
        error: None,
    });
    reload_user_snippets(cx);
}

/// Reads `snippets.toml` from the config directory again (in the
/// background) and replaces the user snippets. A missing file means none;
/// an invalid one keeps the built-ins and is logged.
pub fn reload_user_snippets(cx: &mut App) {
    let Some(path) = arcode_config::paths::snippets_file() else {
        return;
    };
    let read = cx
        .background_executor()
        .spawn(async move { std::fs::read_to_string(&path).ok() });
    cx.spawn(async move |cx| {
        let text = read.await;
        cx.update(|cx| {
            if let Err(err) = load_user_snippets(text.as_deref().unwrap_or(""), cx) {
                log::warn!("snippets.toml: {err:#}");
            }
        })
        .ok();
    })
    .detach();
}

/// Replaces the user snippets with those parsed from `toml_text`
/// (see `arcode_completion::load_snippets` for the format). Returns how
/// many were loaded; on error the built-ins stay and the error is kept.
pub fn load_user_snippets(toml_text: &str, cx: &mut App) -> anyhow::Result<usize> {
    let mut library = SnippetLibrary::builtin();
    let result = load_snippets(toml_text);
    let (count, error) = match &result {
        Ok(defs) => {
            library.add(defs.iter().cloned());
            (defs.len(), None)
        }
        Err(err) => (0, Some(format!("{err:#}"))),
    };
    cx.set_global(SnippetStore {
        library: Arc::new(library),
        error,
    });
    result.map(|_| count)
}

/// The problem with the user's snippet file, if loading it failed.
pub fn user_snippets_error(cx: &App) -> Option<String> {
    cx.try_global::<SnippetStore>()?.error.clone()
}

/// Whether `path` is the user's snippet file.
pub(crate) fn is_snippets_file(path: &Path) -> bool {
    arcode_config::paths::snippets_file().is_some_and(|p| p == path)
}

/// Snippet items for `language_id`.
pub(crate) fn snippet_items(language_id: &str, cx: &App) -> Vec<CompletionItem> {
    let Some(store) = cx.try_global::<SnippetStore>() else {
        return Vec::new();
    };
    store
        .library
        .snippets_for(&language_id.to_lowercase())
        .into_iter()
        .map(CompletionItem::new)
        .collect()
}
