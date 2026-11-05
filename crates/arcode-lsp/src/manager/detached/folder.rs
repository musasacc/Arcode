//! Whether a loose file's folder is small enough to analyse.
//!
//! For a detached file rust-analyzer reads every `.rs` file in the file's
//! folder and in all folders below it, hidden ones and `node_modules`
//! included (it only skips `target`). A file directly in the home folder
//! would load `~/.cargo/registry`, `~/.rustup` and every checkout, which
//! takes gigabytes and minutes. Such a folder is counted first, with a
//! limit on what the count itself reads, and refused when it is too big.

use std::path::Path;

/// How much of a folder rust-analyzer may read for a loose file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FolderLimit {
    /// Rust files in the folder and the folders below it.
    pub(crate) rust_files: usize,
    /// Files and folders looked at while counting them.
    pub(crate) entries: usize,
}

impl FolderLimit {
    /// The limit in normal mode.
    pub(crate) const NORMAL: FolderLimit = FolderLimit {
        rust_files: 1000,
        entries: 20_000,
    };
    /// The limit in low-end mode.
    pub(crate) const LOW_END: FolderLimit = FolderLimit {
        rust_files: 200,
        entries: 5000,
    };
}

/// Whether `dir` and the folders below it hold no more than `limit` allows.
/// `target` folders are not counted (rust-analyzer skips them), nor is
/// `.git` (no Rust files, many entries). Symbolic links are not followed.
pub(crate) fn small_enough(dir: &Path, limit: FolderLimit) -> bool {
    let mut pending = vec![dir.to_path_buf()];
    let (mut entries, mut rust_files) = (0, 0);
    while let Some(dir) = pending.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            entries += 1;
            if entries > limit.entries {
                return false;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            if kind.is_dir() {
                if name != "target" && name != ".git" {
                    pending.push(entry.path());
                }
            } else if Path::new(&name).extension().is_some_and(|e| e == "rs") {
                rust_files += 1;
                if rust_files > limit.rust_files {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: FolderLimit = FolderLimit {
        rust_files: 3,
        entries: 10,
    };

    #[test]
    fn counts_rust_files_below_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        for f in ["a.rs", ".hidden/b.rs", "notes.txt"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        // Build output does not count.
        for i in 0..20 {
            std::fs::write(dir.join(format!("target/debug/{i}.rs")), "").unwrap();
        }
        assert!(small_enough(dir, SMALL));
        std::fs::write(dir.join(".hidden/c.rs"), "").unwrap();
        assert!(small_enough(dir, SMALL));
        std::fs::write(dir.join("d.rs"), "").unwrap();
        assert!(!small_enough(dir, SMALL), "four Rust files");
    }

    #[test]
    fn many_entries_are_too_many_without_rust_files() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("node_modules/x");
        std::fs::create_dir_all(&deep).unwrap();
        for i in 0..8 {
            std::fs::write(deep.join(format!("{i}.js")), "").unwrap();
        }
        assert!(small_enough(tmp.path(), SMALL));
        std::fs::write(deep.join("8.js"), "").unwrap();
        assert!(!small_enough(tmp.path(), SMALL), "eleven entries");
        // A folder that cannot be read holds nothing to load.
        assert!(small_enough(&tmp.path().join("missing"), SMALL));
    }
}
