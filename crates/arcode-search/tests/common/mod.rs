//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use arcode_search::{CancelToken, FileMatches, SearchQuery, SearchStats, search_collect};

/// Create files (relative path -> contents) under a fresh temp dir.
pub fn tree(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, contents) in files {
        write(dir.path(), rel, contents);
    }
    dir
}

pub fn write(root: &Path, rel: &str, contents: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, contents).expect("write");
}

pub fn run(root: &Path, query: &SearchQuery) -> (Vec<FileMatches>, SearchStats) {
    search_collect(root, query, &CancelToken::new()).expect("search")
}

/// Relative paths (with `/` separators) of the files that matched.
pub fn paths(files: &[FileMatches]) -> Vec<String> {
    files.iter().map(|f| slash(&f.relative)).collect()
}

pub fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn rel(path: &str) -> PathBuf {
    path.split('/').collect()
}

/// Text of `range` (char indices) in `s`.
pub fn chars(s: &str, range: &std::ops::Range<usize>) -> String {
    s.chars()
        .skip(range.start)
        .take(range.end - range.start)
        .collect()
}
