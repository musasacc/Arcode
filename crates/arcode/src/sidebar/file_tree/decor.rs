//! What the tree shows next to names (docs/DESIGN.md 6.5): the git status
//! letter of a file, a dot on folders that hold changes, and the error
//! count of a file. Built from the git store and the diagnostics only when
//! they change, so drawing a row is two map lookups.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use arcode_git::{FileStatus, StatusCode};
use arcode_ui_kit::ThemeColors;
use gpui::{Context, Entity, Hsla};

use super::FileTree;
use crate::git::{GitEvent, GitStore};
use crate::lsp_bridge::LspBridge;

/// The git state of a file, as one letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitMark {
    Modified,
    Added,
    Untracked,
    Deleted,
    Conflicted,
}

impl GitMark {
    /// The mark of a status entry: a conflict wins, then the work tree's
    /// change, then the index's. `None` for clean or ignored entries.
    pub(crate) fn of(status: &FileStatus) -> Option<Self> {
        if status.is_conflicted() {
            return Some(Self::Conflicted);
        }
        let code = match status.worktree {
            StatusCode::Unmodified | StatusCode::Ignored => status.index,
            code => code,
        };
        match code {
            StatusCode::Unmodified | StatusCode::Ignored => None,
            StatusCode::Untracked => Some(Self::Untracked),
            StatusCode::Added => Some(Self::Added),
            StatusCode::Deleted => Some(Self::Deleted),
            StatusCode::Unmerged => Some(Self::Conflicted),
            _ => Some(Self::Modified),
        }
    }

    /// The letter shown at the right of the row.
    pub(crate) fn letter(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Untracked => "U",
            Self::Deleted => "D",
            Self::Conflicted => "!",
        }
    }

    /// The text color of the letter (2.4 rule 3: `info`, `success` and
    /// `error`, which pass 4.5:1; the `diff_*` colors are for bars only).
    pub(crate) fn color(self, c: &ThemeColors) -> Hsla {
        match self {
            Self::Modified => c.info,
            Self::Added | Self::Untracked => c.success,
            Self::Deleted | Self::Conflicted => c.error,
        }
    }
}

/// The marks of one file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileMark {
    pub(crate) git: Option<GitMark>,
    pub(crate) errors: usize,
}

/// Every mark of the tree, by absolute path.
#[derive(Debug, Default)]
pub(crate) struct Decorations {
    files: HashMap<PathBuf, FileMark>,
    changed_dirs: HashSet<PathBuf>,
}

impl Decorations {
    /// Marks for the tree of `root`: `entries` are relative to `workdir`
    /// (the active repository), `errors` are absolute paths with their
    /// error counts. Folders between a changed file and `root` get the
    /// changes dot.
    pub(crate) fn build<'a>(
        root: &Path,
        workdir: Option<&Path>,
        entries: &[FileStatus],
        errors: impl IntoIterator<Item = (&'a Path, usize)>,
    ) -> Self {
        let mut out = Self::default();
        if let Some(workdir) = workdir {
            for entry in entries {
                let Some(mark) = GitMark::of(entry) else {
                    continue;
                };
                let path = workdir.join(&entry.path);
                if !path.starts_with(root) {
                    continue;
                }
                let mut dir = path.parent();
                while let Some(d) = dir.filter(|d| *d != root && d.starts_with(root)) {
                    if !out.changed_dirs.insert(d.to_path_buf()) {
                        break;
                    }
                    dir = d.parent();
                }
                out.files.entry(path).or_default().git = Some(mark);
            }
        }
        for (path, count) in errors {
            if count > 0 && path.starts_with(root) {
                out.files.entry(path.to_path_buf()).or_default().errors = count;
            }
        }
        out
    }

    /// The marks of the file at `path`.
    pub(crate) fn file(&self, path: &Path) -> FileMark {
        self.files.get(path).copied().unwrap_or_default()
    }

    /// Whether the folder at `path` holds changed files.
    pub(crate) fn dir_changed(&self, path: &Path) -> bool {
        self.changed_dirs.contains(path)
    }
}

/// The error counts of every file with diagnostics.
fn error_counts(bridge: &LspBridge) -> Vec<(PathBuf, usize)> {
    let store = bridge.store();
    store
        .paths()
        .map(|path| {
            let errors = store
                .for_path(path)
                .iter()
                .filter(|(_, d)| {
                    crate::lsp_bridge::convert::severity(d)
                        == arcode_editor::DiagnosticSeverity::Error
                })
                .count();
            (path.to_path_buf(), errors)
        })
        .filter(|(_, n)| *n > 0)
        .collect()
}

impl FileTree {
    /// Shows git status letters and error counts from `git` and `lsp`,
    /// updated whenever either changes (nothing runs while both are idle).
    pub fn follow_sources(
        &mut self,
        git: Entity<GitStore>,
        lsp: Entity<LspBridge>,
        cx: &mut Context<Self>,
    ) {
        let git_sub = cx.subscribe(&git, |tree, _, event: &GitEvent, cx| {
            if matches!(event, GitEvent::StatusChanged | GitEvent::ReposChanged) {
                tree.update_decorations(cx);
            }
        });
        let lsp_sub = cx.observe(&lsp, |tree, _, cx| tree.update_decorations(cx));
        self.sources = Some(Sources {
            git,
            lsp,
            _subs: [git_sub, lsp_sub],
        });
        self.update_decorations(cx);
    }

    fn update_decorations(&mut self, cx: &mut Context<Self>) {
        let Some(sources) = &self.sources else {
            return;
        };
        let store = sources.git.read(cx);
        let workdir = store.active_repo().map(|r| r.workdir.clone());
        let errors = error_counts(sources.lsp.read(cx));
        let decor = Decorations::build(
            &self.root,
            workdir.as_deref(),
            &store.snapshot().entries,
            errors.iter().map(|(p, n)| (p.as_path(), *n)),
        );
        self.decor = decor;
        cx.notify();
    }
}

/// The entities the tree takes its marks from.
pub(crate) struct Sources {
    git: Entity<GitStore>,
    lsp: Entity<LspBridge>,
    _subs: [gpui::Subscription; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, index: StatusCode, worktree: StatusCode) -> FileStatus {
        FileStatus {
            path: PathBuf::from(path),
            index,
            worktree,
            orig_path: None,
            conflict: None,
        }
    }

    #[test]
    fn marks_follow_the_status() {
        use StatusCode::*;
        assert_eq!(
            GitMark::of(&entry("a", Modified, Unmodified)),
            Some(GitMark::Modified)
        );
        assert_eq!(
            GitMark::of(&entry("a", Unmodified, Modified)),
            Some(GitMark::Modified)
        );
        assert_eq!(
            GitMark::of(&entry("a", Added, Modified)),
            Some(GitMark::Modified)
        );
        assert_eq!(
            GitMark::of(&entry("a", Added, Unmodified)),
            Some(GitMark::Added)
        );
        assert_eq!(
            GitMark::of(&entry("a", Untracked, Untracked)),
            Some(GitMark::Untracked)
        );
        assert_eq!(
            GitMark::of(&entry("a", Unmodified, Deleted)),
            Some(GitMark::Deleted)
        );
        assert_eq!(
            GitMark::of(&entry("a", Unmerged, Unmerged)),
            Some(GitMark::Conflicted)
        );
        assert_eq!(GitMark::of(&entry("a", Ignored, Ignored)), None);
        assert_eq!(GitMark::of(&entry("a", Unmodified, Unmodified)), None);
        let letters: Vec<_> = [
            GitMark::Modified,
            GitMark::Added,
            GitMark::Untracked,
            GitMark::Deleted,
            GitMark::Conflicted,
        ]
        .map(GitMark::letter)
        .to_vec();
        assert_eq!(letters, ["M", "A", "U", "D", "!"]);
    }

    #[test]
    fn folders_above_changes_get_the_dot() {
        use StatusCode::*;
        let root = Path::new("/w/demo");
        let entries = [
            entry("src/geometry/shape.rs", Unmodified, Modified),
            entry("src/util.rs", Added, Unmodified),
            entry("README.md", Unmodified, Unmodified),
        ];
        let errors = [
            (Path::new("/w/demo/src/main.rs"), 2),
            (Path::new("/w/demo/b.rs"), 0),
        ];
        let d = Decorations::build(root, Some(root), &entries, errors);
        assert!(d.dir_changed(Path::new("/w/demo/src")));
        assert!(d.dir_changed(Path::new("/w/demo/src/geometry")));
        assert!(!d.dir_changed(root), "the root is not a row");
        assert!(!d.dir_changed(Path::new("/w")));
        assert_eq!(
            d.file(Path::new("/w/demo/src/util.rs")).git,
            Some(GitMark::Added)
        );
        assert_eq!(d.file(Path::new("/w/demo/README.md")), FileMark::default());
        assert_eq!(d.file(Path::new("/w/demo/src/main.rs")).errors, 2);
        assert_eq!(d.file(Path::new("/w/demo/b.rs")), FileMark::default());
        // A parent repository's files outside the tree are left out.
        let d = Decorations::build(
            root,
            Some(Path::new("/w")),
            &[entry("x.rs", Added, Unmodified)],
            [],
        );
        assert_eq!(d.file(Path::new("/w/x.rs")), FileMark::default());
    }

    #[test]
    fn a_repository_inside_the_project_is_mapped_from_its_workdir() {
        use StatusCode::*;
        let root = Path::new("/w/demo");
        let workdir = Path::new("/w/demo/vendor/lib");
        let entries = [entry("src/lib.rs", Unmodified, Modified)];
        let d = Decorations::build(root, Some(workdir), &entries, []);
        assert_eq!(
            d.file(Path::new("/w/demo/vendor/lib/src/lib.rs")).git,
            Some(GitMark::Modified)
        );
        assert!(d.dir_changed(Path::new("/w/demo/vendor")));
        // No repository: no git marks, errors still count.
        let d = Decorations::build(root, None, &entries, [(Path::new("/w/demo/a.rs"), 1)]);
        assert!(!d.dir_changed(Path::new("/w/demo/src")));
        assert_eq!(d.file(Path::new("/w/demo/a.rs")).errors, 1);
    }
}
