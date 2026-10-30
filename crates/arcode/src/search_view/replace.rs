//! Replace in a file and Replace all for the Search view.
//!
//! Files open in an editor are changed through the editor (one undo step
//! per file, the buffer stays unsaved); other files are rewritten on the
//! background executor with `arcode_search::replace_in_file`, which skips
//! non-UTF-8 and binary files and reports why.

use std::path::{Path, PathBuf};

use arcode_editor::Editor;
use arcode_search::{CancelToken, SearchError, SearchQuery, search_collect, search_text};
use gpui::{AppContext, Context, Entity, PromptLevel, Window};

use super::{ProjectSearch, ProjectSearchEvent};
use crate::find_bar::bulk::{MAX_EDITS, replace_all_edits};

/// Outcome of a replace run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceOutcome {
    /// Matches replaced.
    pub replacements: usize,
    /// Files changed (in an editor or on disk).
    pub files: usize,
    /// Files left alone, with the reason.
    pub skipped: Vec<(PathBuf, String)>,
}

impl ReplaceOutcome {
    /// A one-line summary such as `Replaced 3 occurrences in 2 files.`
    pub fn summary(&self, root: Option<&Path>) -> String {
        let plural = |n: usize, word: &str| {
            if n == 1 {
                format!("1 {word}")
            } else {
                format!("{n} {word}s")
            }
        };
        let mut text = format!(
            "Replaced {} in {}.",
            plural(self.replacements, "occurrence"),
            plural(self.files, "file")
        );
        if !self.skipped.is_empty() {
            let names: Vec<String> = self
                .skipped
                .iter()
                .take(3)
                .map(|(p, why)| {
                    let shown = root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p);
                    format!("{} ({why})", shown.display())
                })
                .collect();
            let more = self.skipped.len().saturating_sub(3);
            text.push_str(&format!(
                " Skipped {}: {}{}",
                plural(self.skipped.len(), "file"),
                names.join(", "),
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                }
            ));
        }
        text
    }
}

/// Why a disk replace skipped a file, for the user.
pub fn skip_reason(err: &SearchError) -> String {
    match err {
        SearchError::NotUtf8(_) => "not UTF-8".to_owned(),
        SearchError::Binary(_) => "binary".to_owned(),
        SearchError::Io { source, .. } => source.to_string(),
        other => other.to_string(),
    }
}

/// Matches of `query` in an open editor's buffer.
pub fn count_in_editor(editor: &Entity<Editor>, query: &SearchQuery, cx: &gpui::App) -> usize {
    let text = editor.read(cx).buffer().text().to_string();
    search_text(&text, query)
        .map(|lines| lines.iter().map(|l| l.match_count()).sum())
        .unwrap_or(0)
}

/// Replaces every match in an open editor as one undo step; returns the
/// number replaced.
pub fn replace_in_editor<T>(
    editor: &Entity<Editor>,
    query: &SearchQuery,
    replacement: &str,
    cx: &mut Context<T>,
) -> usize {
    let text = editor.read(cx).buffer().text().to_string();
    let Ok(bulk) = replace_all_edits(&text, query, replacement, MAX_EDITS) else {
        return 0;
    };
    let count = bulk.count;
    if !bulk.apply(editor, cx) {
        return 0;
    }
    count
}

impl ProjectSearch {
    pub(super) fn replace_query(&self, cx: &gpui::App) -> Option<(SearchQuery, String)> {
        let query = SearchQuery {
            max_results: 0,
            ..self.build_query(cx)
        };
        query.validate().ok()?;
        Some((query, self.replacement.read(cx).text().to_string()))
    }

    /// Replaces every match of `query` by `replacement` in `paths`: open
    /// files through their editors, the others on disk in the background.
    /// Then searches again. The query and replacement are passed in (not
    /// read from the fields) so a confirmed replace writes exactly what the
    /// confirmation described.
    pub(super) fn replace_files(
        &mut self,
        paths: Vec<PathBuf>,
        query: SearchQuery,
        replacement: String,
        cx: &mut Context<Self>,
    ) {
        let mut outcome = ReplaceOutcome::default();
        let mut on_disk = Vec::new();
        for path in paths {
            match self.open_editor(&path, cx) {
                Some(editor) if editor.read(cx).is_read_only() => {
                    outcome.skipped.push((path, "read-only".to_owned()));
                }
                Some(editor) => {
                    let n = replace_in_editor(&editor, &query, &replacement, cx);
                    if n > 0 {
                        outcome.replacements += n;
                        outcome.files += 1;
                    }
                }
                None => on_disk.push(path),
            }
        }
        self.replacing = true;
        cx.notify();
        let work = cx.background_spawn(async move {
            for path in on_disk {
                match arcode_search::replace_in_file(&path, &query, &replacement, None) {
                    Ok(0) => {}
                    Ok(n) => {
                        outcome.replacements += n;
                        outcome.files += 1;
                    }
                    Err(err) => outcome.skipped.push((path, skip_reason(&err))),
                }
            }
            outcome
        });
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            this.update(cx, |view, cx| {
                view.replacing = false;
                let text = outcome.summary(view.root.as_deref());
                view.notice = Some(text.clone().into());
                cx.emit(ProjectSearchEvent::Message(text));
                view.run(true, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Replaces every match in the listed file at index `file_ix`.
    pub fn replace_in_file(&mut self, file_ix: usize, cx: &mut Context<Self>) {
        let path = self.results.files().get(file_ix).map(|f| f.path.clone());
        if let (Some(path), Some((query, replacement))) = (path, self.replace_query(cx)) {
            self.replace_files(vec![path], query, replacement, cx);
        }
    }

    /// Counts every match (not just the listed ones), asks for
    /// confirmation with the counts, then replaces them all.
    pub fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(root), Some((query, replacement))) = (self.root.clone(), self.replace_query(cx))
        else {
            return;
        };
        if self.replacing {
            return;
        }
        self.replacing = true;
        cx.notify();
        let count_query = query.clone();
        let counting = cx.background_spawn(async move {
            search_collect(&root, &count_query, &CancelToken::new()).map(|(files, _)| files)
        });
        cx.spawn_in(window, async move |this, cx| {
            let files = counting.await.unwrap_or_default();
            let Ok((paths, total)) = this.update(cx, |view, cx| {
                let mut total = 0;
                let mut paths: Vec<PathBuf> = files
                    .into_iter()
                    .map(|f| {
                        total += match view.open_editor(&f.path, cx) {
                            Some(editor) => count_in_editor(&editor, &query, cx),
                            None => f.match_count(),
                        };
                        f.path
                    })
                    .collect();
                // Unsaved buffers whose text matches while their file does not.
                for (path, n) in view.unsaved_only_counts(&paths, &query, cx) {
                    total += n;
                    paths.push(path);
                }
                (paths, total)
            }) else {
                return;
            };
            if total == 0 {
                this.update(cx, |view, cx| {
                    view.replacing = false;
                    view.notice = Some("Nothing to replace.".into());
                    cx.notify();
                })
                .ok();
                return;
            }
            let message = format!(
                "Replace {total} occurrence{} in {} file{} with “{replacement}”?",
                if total == 1 { "" } else { "s" },
                paths.len(),
                if paths.len() == 1 { "" } else { "s" },
            );
            let detail = "Files open in an editor are changed there and can be undone. \
                          Other files are written directly; files that are not UTF-8 are skipped.";
            let answer = this.update_in(cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    &message,
                    Some(detail),
                    &["Replace All", "Cancel"],
                    cx,
                )
            });
            let confirmed = match answer {
                Ok(answer) => answer.await.ok() == Some(0),
                Err(_) => false,
            };
            this.update(cx, |view, cx| {
                view.replacing = false;
                if confirmed {
                    view.replace_files(paths, query, replacement, cx);
                } else {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_name_skipped_files() {
        let mut o = ReplaceOutcome {
            replacements: 1,
            files: 1,
            skipped: Vec::new(),
        };
        assert_eq!(o.summary(None), "Replaced 1 occurrence in 1 file.");
        o.replacements = 5;
        o.files = 2;
        o.skipped = vec![(PathBuf::from("/p/latin1.txt"), "not UTF-8".into())];
        assert_eq!(
            o.summary(Some(Path::new("/p"))),
            "Replaced 5 occurrences in 2 files. Skipped 1 file: latin1.txt (not UTF-8)"
        );
    }
}
