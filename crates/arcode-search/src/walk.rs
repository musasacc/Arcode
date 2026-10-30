//! Parallel, ignore-aware directory walking shared by search, replace and
//! TODO scanning. Results stream back to the calling thread over a bounded
//! channel so the consumer callback needs neither `Send` nor `Sync`.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use ignore::overrides::{Override, OverrideBuilder};
use ignore::{WalkBuilder, WalkState};

use crate::cancel::Halt;
use crate::error::{Result, SearchError};

/// Which files to visit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WalkConfig<'a> {
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_globs: &'a [String],
    pub exclude_globs: &'a [String],
    pub max_file_size: u64,
}

/// Counters from one walk.
#[derive(Debug, Default)]
pub(crate) struct WalkStats {
    pub files_visited: usize,
}

/// Build the include/exclude override matcher for `root`.
pub(crate) fn build_overrides(root: &Path, cfg: &WalkConfig<'_>) -> Result<Override> {
    let mut builder = OverrideBuilder::new(root);
    let mut add = |glob: &str, shown: &str| {
        builder
            .add(glob)
            .map(|_| ())
            .map_err(|e| SearchError::InvalidGlob {
                glob: shown.to_owned(),
                message: e.to_string(),
            })
    };
    for raw in cfg.include_globs {
        let glob = raw.trim().trim_end_matches(['/', '\\']);
        if glob.is_empty() {
            continue;
        }
        add(glob, raw)?;
        if !glob.ends_with("**") {
            // A bare directory include should cover its contents.
            add(&format!("{glob}/**"), raw)?;
        }
    }
    for raw in cfg.exclude_globs {
        let glob = raw.trim().trim_end_matches(['/', '\\']);
        if !glob.is_empty() {
            add(&format!("!{glob}"), raw)?;
        }
    }
    builder.build().map_err(|e| SearchError::InvalidGlob {
        glob: String::new(),
        message: e.to_string(),
    })
}

/// Make `root` absolute without resolving symlinks (keeps Windows paths
/// free of `\\?\` prefixes) and check it exists.
pub(crate) fn absolute_root(root: &Path) -> Result<PathBuf> {
    let abs = std::path::absolute(root).map_err(|e| SearchError::io(root, e))?;
    if !abs.exists() {
        return Err(SearchError::InvalidRoot(abs));
    }
    Ok(abs)
}

/// Path of `path` relative to `root` (the file name when `root` is the file).
pub(crate) fn relative_path(root: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(root) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_path_buf(),
        _ => path.file_name().map(PathBuf::from).unwrap_or_default(),
    }
}

/// Walk `root` in parallel. `make_visitor` is called once per worker thread;
/// each visitor is handed `(absolute, relative)` paths of regular files and
/// may produce an item. Items are passed to `consume` on the calling thread.
/// `consume` returning `Break` stops the walk. After the user cancels,
/// pending items are dropped.
pub(crate) fn walk<T, F, V, C>(
    root: &Path,
    cfg: &WalkConfig<'_>,
    halt: &Halt<'_>,
    mut make_visitor: F,
    mut consume: C,
) -> Result<WalkStats>
where
    T: Send,
    F: FnMut() -> V + Send,
    V: FnMut(&Path, &Path) -> Option<T> + Send,
    C: FnMut(T) -> ControlFlow<()>,
{
    let overrides = build_overrides(root, cfg)?;
    let mut builder = WalkBuilder::new(root);
    let ignores = cfg.respect_gitignore;
    builder
        .hidden(!cfg.include_hidden)
        .parents(ignores)
        .ignore(ignores)
        .git_ignore(ignores)
        .git_global(ignores)
        .git_exclude(ignores)
        .require_git(false)
        .follow_links(false)
        .max_filesize((cfg.max_file_size > 0).then_some(cfg.max_file_size))
        .overrides(overrides)
        .filter_entry(|e| e.file_name() != ".git");
    let walker = builder.build_parallel();

    let visited = AtomicUsize::new(0);
    let (tx, rx) = mpsc::sync_channel::<T>(256);
    std::thread::scope(|scope| {
        let visited = &visited;
        scope.spawn(move || {
            walker.run(|| {
                let tx = tx.clone();
                let mut visit = make_visitor();
                Box::new(move |entry| {
                    if halt.should_stop() {
                        return WalkState::Quit;
                    }
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(err) => {
                            log::debug!("search walk: {err}");
                            return WalkState::Continue;
                        }
                    };
                    if !entry.file_type().is_some_and(|t| t.is_file()) {
                        return WalkState::Continue;
                    }
                    visited.fetch_add(1, Ordering::Relaxed);
                    let relative = relative_path(root, entry.path());
                    let Some(item) = visit(entry.path(), &relative) else {
                        return WalkState::Continue;
                    };
                    if tx.send(item).is_err() {
                        WalkState::Quit
                    } else {
                        WalkState::Continue
                    }
                })
            });
        });
        for item in rx.iter() {
            if halt.is_cancelled() {
                break;
            }
            if consume(item).is_break() {
                halt.stop();
                break;
            }
        }
        // Dropping the receiver unblocks workers waiting to send.
        drop(rx);
    });
    Ok(WalkStats {
        files_visited: visited.into_inner(),
    })
}
