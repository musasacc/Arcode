//! Rows of the TODO list: files with their items, filtered by tag chips.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use arcode_search::TodoItem;

/// One list row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A file header with the number of (shown) items.
    File {
        /// Absolute path.
        path: PathBuf,
        /// Shown items.
        count: usize,
        /// Whether the items are hidden.
        collapsed: bool,
    },
    /// Item `index` of the file's list.
    Item {
        /// Absolute path of the file.
        path: PathBuf,
        /// Index into the file's items.
        index: usize,
    },
}

/// Items per tag over all files, in the order of `tags` (tags without
/// items included, with 0).
pub fn tag_counts(
    files: &BTreeMap<PathBuf, Vec<TodoItem>>,
    tags: &[String],
) -> Vec<(String, usize)> {
    tags.iter()
        .map(|tag| {
            let n = files
                .values()
                .flatten()
                .filter(|item| &item.tag == tag)
                .count();
            (tag.clone(), n)
        })
        .collect()
}

/// Whether `item` passes the tag filter (empty filter = every tag).
pub fn shown(item: &TodoItem, filter: &HashSet<String>) -> bool {
    filter.is_empty() || filter.contains(&item.tag)
}

/// The rows for `files`, keeping only items whose tag is in `filter`
/// (all when empty) and skipping the items of collapsed files. Files with
/// nothing shown are left out.
pub fn build_rows(
    files: &BTreeMap<PathBuf, Vec<TodoItem>>,
    filter: &HashSet<String>,
    collapsed: &HashSet<PathBuf>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (path, items) in files {
        let visible: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, i)| shown(i, filter))
            .map(|(ix, _)| ix)
            .collect();
        if visible.is_empty() {
            continue;
        }
        let is_collapsed = collapsed.contains(path);
        rows.push(Row::File {
            path: path.clone(),
            count: visible.len(),
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            rows.extend(visible.into_iter().map(|index| Row::Item {
                path: path.clone(),
                index,
            }));
        }
    }
    rows
}

/// `path` relative to `root` with `/` separators, for display.
pub fn relative_label(root: Option<&Path>, path: &Path) -> String {
    let rel = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, line: usize, tag: &str) -> TodoItem {
        TodoItem {
            path: PathBuf::from(path),
            relative: PathBuf::from(path.trim_start_matches("/p/")),
            line,
            column: 3,
            tag: tag.into(),
            text: format!("{tag} text"),
            author: None,
        }
    }

    fn files() -> BTreeMap<PathBuf, Vec<TodoItem>> {
        let mut f = BTreeMap::new();
        f.insert(
            PathBuf::from("/p/a.rs"),
            vec![item("/p/a.rs", 1, "TODO"), item("/p/a.rs", 5, "FIXME")],
        );
        f.insert(PathBuf::from("/p/b.rs"), vec![item("/p/b.rs", 0, "NOTE")]);
        f
    }

    #[test]
    fn counts_per_tag_in_tag_order() {
        let tags: Vec<String> = ["FIXME", "TODO", "HACK"].map(String::from).to_vec();
        assert_eq!(
            tag_counts(&files(), &tags),
            vec![("FIXME".into(), 1), ("TODO".into(), 1), ("HACK".into(), 0)]
        );
    }

    #[test]
    fn rows_filter_and_collapse() {
        let none = HashSet::new();
        assert_eq!(build_rows(&files(), &none, &HashSet::new()).len(), 5);
        let filter: HashSet<String> = ["FIXME".to_string()].into();
        let rows = build_rows(&files(), &filter, &HashSet::new());
        assert_eq!(
            rows,
            vec![
                Row::File {
                    path: "/p/a.rs".into(),
                    count: 1,
                    collapsed: false
                },
                Row::Item {
                    path: "/p/a.rs".into(),
                    index: 1
                },
            ]
        );
        let collapsed: HashSet<PathBuf> = [PathBuf::from("/p/a.rs")].into();
        let rows = build_rows(&files(), &HashSet::new(), &collapsed);
        assert_eq!(rows.len(), 3);
        assert!(matches!(
            &rows[0],
            Row::File {
                count: 2,
                collapsed: true,
                ..
            }
        ));
        assert_eq!(
            relative_label(Some(Path::new("/p")), Path::new("/p/x/y.rs")),
            "x/y.rs"
        );
    }
}
