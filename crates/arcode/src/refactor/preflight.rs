//! Checks the resource operations of a workspace edit against the disk
//! before anything runs, so an edit that would fail half-way (e.g. a
//! rename onto an existing file after the text edits that refer to the new
//! name) is refused with nothing changed.

use std::path::{Path, PathBuf};

use super::files::is_case_change;
use crate::lsp_bridge::workspace_edit::FileOp;

/// What the earlier operations of the edit did to the paths.
#[derive(Default)]
struct Sim {
    /// `(path, exists afterwards)`, latest last.
    changes: Vec<(PathBuf, bool)>,
}

impl Sim {
    /// Whether `path` exists at this point of the edit; `None` when an
    /// earlier operation moved or removed a folder above it (unknown).
    fn exists(&self, path: &Path) -> Option<bool> {
        for (changed, exists) in self.changes.iter().rev() {
            if changed == path {
                return Some(*exists);
            }
            if path.starts_with(changed) {
                return None;
            }
        }
        Some(path.exists())
    }

    fn set(&mut self, path: &Path, exists: bool) {
        self.changes.push((path.to_path_buf(), exists));
    }
}

fn name(path: &Path) -> String {
    path.display().to_string()
}

/// Refuses `ops` when one of them is bound to fail. `is_open` tells
/// whether a path has an open editor (a text edit needs a file or one).
pub(crate) fn check_resources(
    ops: &[FileOp],
    is_open: impl Fn(&Path) -> bool,
) -> Result<(), String> {
    let mut sim = Sim::default();
    for op in ops {
        match op {
            FileOp::Edit { path, .. } => {
                if sim.exists(path) == Some(false) && !is_open(path) {
                    return Err(format!("{} does not exist", name(path)));
                }
            }
            FileOp::Create {
                path,
                overwrite,
                ignore_if_exists,
            } => {
                if sim.exists(path) == Some(true) && !overwrite && !ignore_if_exists {
                    return Err(format!("{} already exists", name(path)));
                }
                sim.set(path, true);
            }
            FileOp::Rename {
                from,
                to,
                overwrite,
                ignore_if_exists,
            } => {
                if sim.exists(from) == Some(false) {
                    return Err(format!("{} does not exist", name(from)));
                }
                let clash = sim.exists(to) == Some(true) && !overwrite && !is_case_change(from, to);
                if clash && !ignore_if_exists {
                    return Err(format!("{} already exists", name(to)));
                }
                if !clash {
                    sim.set(from, false);
                    sim.set(to, true);
                }
            }
            FileOp::Delete {
                path,
                ignore_if_not_exists,
                ..
            } => {
                if sim.exists(path) == Some(false) && !ignore_if_not_exists {
                    return Err(format!("{} does not exist", name(path)));
                }
                sim.set(path, false);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn rename(from: &Path, to: &Path) -> FileOp {
        FileOp::Rename {
            from: from.into(),
            to: to.into(),
            overwrite: false,
            ignore_if_exists: false,
        }
    }

    fn edit(path: &Path) -> FileOp {
        FileOp::Edit {
            path: path.into(),
            version: None,
            edits: Vec::new(),
        }
    }

    #[test]
    fn follows_earlier_operations() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a.rs"),
            dir.path().join("b.rs"),
            dir.path().join("c.rs"),
        );
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        let closed = |_: &Path| false;
        // A rename onto an existing file fails even after text edits.
        let err = check_resources(&[edit(&a), rename(&a, &b)], closed).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        // Moving the target away first makes room.
        assert!(check_resources(&[rename(&b, &c), rename(&a, &b), edit(&b)], closed).is_ok());
        // An edit of a file an earlier operation deleted needs an editor.
        let delete = FileOp::Delete {
            path: a.clone(),
            recursive: false,
            ignore_if_not_exists: false,
        };
        assert!(check_resources(&[delete.clone(), edit(&a)], closed).is_err());
        assert!(check_resources(&[delete.clone(), edit(&a)], |_| true).is_ok());
        // Creating what was deleted is fine; creating twice is not.
        let create = FileOp::Create {
            path: a.clone(),
            overwrite: false,
            ignore_if_exists: false,
        };
        assert!(check_resources(&[delete, create.clone()], closed).is_ok());
        assert!(check_resources(&[create], closed).is_err());
        // Paths below a moved folder are not judged.
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let moved = dir.path().join("moved");
        assert!(
            check_resources(&[rename(&sub, &moved), edit(&moved.join("x.rs"))], closed).is_ok()
        );
    }
}
