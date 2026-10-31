//! The sidebar `databases` panel (§10): a tree of the project's SQLite
//! files, their tables and views (on expand), and their columns (on
//! expand). `find-files` runs only when the panel is shown or refreshed.
//!
//! Activating a database or table opens the file through the editor, which
//! routes it to the `sqlite` opener; the plugin remembers which table to
//! select when that view arrives.

use std::collections::BTreeMap;
use std::path::Path;

use arcode_plugin::prelude::*;
use arcode_plugin::ui::{self, Element, Tone, TreeItem};

use super::db::Intent;
use crate::db::{ColumnInfo, Database, Kind, Object};

/// Glob for SQLite files (§10).
pub const DB_GLOB: &str = "**/*.{sqlite,sqlite3,db}";

/// Most files listed.
pub const MAX_FILES: u32 = 200;

/// What activating an item asks the plugin to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    /// Project-relative path to open.
    pub path: String,
    /// What the opened view should show.
    pub intent: Intent,
}

/// The sidebar panel's state.
#[derive(Debug)]
pub struct Sidebar {
    panel: Panel,
    /// Database files, or why they could not be listed.
    pub files: Result<Vec<String>, String>,
    /// Expanded databases: their tables and views.
    pub objects: BTreeMap<usize, Result<Vec<Object>, String>>,
    /// Expanded tables: their columns.
    pub columns: BTreeMap<(usize, usize), Result<Vec<ColumnInfo>, String>>,
}

/// "1 file", "3 files".
fn file_count(n: usize) -> String {
    if n == 1 {
        "1 file".to_owned()
    } else {
        format!("{n} files")
    }
}

/// Lists the project's database files (sorted, at most [`MAX_FILES`]).
pub fn find_databases() -> Result<Vec<String>, String> {
    workspace::find_files(DB_GLOB, MAX_FILES)
        .map(|mut files| {
            files.sort();
            files
        })
        .map_err(|e| format!("Could not list files: {e:?}"))
}

/// Parses `d<i>`, `t<i>.<j>` and `c<i>.<j>.<k>` keys into indices.
fn parse(key: &str, prefix: char) -> Option<Vec<usize>> {
    key.strip_prefix(prefix)?
        .split('.')
        .map(|n| n.parse().ok())
        .collect()
}

impl Sidebar {
    /// Shows the panel: lists files and renders.
    pub fn open(panel: Panel) -> Result<Self> {
        let mut sidebar = Self {
            panel,
            files: Ok(Vec::new()),
            objects: BTreeMap::new(),
            columns: BTreeMap::new(),
        };
        sidebar.refresh()?;
        Ok(sidebar)
    }

    /// Lists files again and collapses everything.
    pub fn refresh(&mut self) -> Result<()> {
        self.files = find_databases();
        self.objects.clear();
        self.columns.clear();
        self.render()
    }

    fn file(&self, i: usize) -> Option<&str> {
        self.files.as_ref().ok()?.get(i).map(String::as_str)
    }

    fn object(&self, i: usize, j: usize) -> Option<&Object> {
        self.objects.get(&i)?.as_ref().ok()?.get(j)
    }

    /// Handles an event; returns a file to open, if any.
    pub fn handle(&mut self, root: &Path, event: Event) -> Result<Option<OpenRequest>> {
        let node = Some(event.node);
        let (refresh, dbs) = (self.panel.node_id("refresh"), self.panel.node_id("dbs"));
        match event.kind {
            EventKind::Click if node == refresh => self.refresh().map(|()| None),
            EventKind::TreeToggle(key) if node == dbs => {
                self.toggle(root, &key);
                self.render().map(|()| None)
            }
            EventKind::TreeActivate(key) if node == dbs => Ok(self.activate(&key)),
            _ => Ok(None),
        }
    }

    fn toggle(&mut self, root: &Path, key: &str) {
        if let Some([i]) = parse(key, 'd').as_deref() {
            self.toggle_database(root, *i);
        } else if let Some([i, j]) = parse(key, 't').as_deref() {
            self.toggle_table(root, *i, *j);
        }
    }

    /// Expands database `i` (reads its tables and views) or collapses it.
    fn toggle_database(&mut self, root: &Path, i: usize) {
        if self.objects.remove(&i).is_some() {
            return;
        }
        if let Some(path) = self.file(i) {
            let listed = Database::open(root, path, false).and_then(|mut db| db.objects());
            self.objects.insert(i, listed.map_err(|e| e.message()));
        }
    }

    /// Expands table `j` of database `i` (reads its columns) or collapses it.
    fn toggle_table(&mut self, root: &Path, i: usize, j: usize) {
        if self.columns.remove(&(i, j)).is_some() {
            return;
        }
        if let (Some(path), Some(object)) = (self.file(i), self.object(i, j)) {
            let name = object.name.clone();
            let listed = Database::open(root, path, false).and_then(|mut db| db.columns(&name));
            self.columns.insert((i, j), listed.map_err(|e| e.message()));
        }
    }

    fn activate(&self, key: &str) -> Option<OpenRequest> {
        let (path, table) = match parse(key, 'd').or_else(|| parse(key, 't'))?.as_slice() {
            [i] => (self.file(*i)?, None),
            [i, j] => (self.file(*i)?, Some(self.object(*i, *j)?.name.clone())),
            _ => return None,
        };
        Some(OpenRequest {
            path: path.to_owned(),
            intent: Intent {
                table,
                query: false,
            },
        })
    }

    /// Sends the view to the host.
    pub fn render(&mut self) -> Result<()> {
        let body = match &self.files {
            Err(message) => ui::text(message.clone()).tone(Tone::Danger).wrap(),
            Ok(files) if files.is_empty() => ui::empty_state(
                "No databases",
                "No *.sqlite, *.sqlite3 or *.db files in this project.",
            ),
            Ok(files) => ui::tree(self.items(files)),
        };
        let bar = ui::hstack([
            ui::text(file_count(self.files.as_ref().map_or(0, Vec::len)))
                .tone(Tone::Muted)
                .key("count"),
            ui::spacer(),
            ui::button("Refresh").icon("refresh").key("refresh"),
        ])
        .key("bar");
        let tree: Element = ui::vstack([bar, body.key("dbs").grow()]).key("sidebar");
        self.panel.rerender(&tree)
    }

    fn items(&self, files: &[String]) -> Vec<TreeItem> {
        let item =
            |key: String, label: String, detail: Option<String>, depth: u8, open: Option<bool>| {
                TreeItem {
                    key,
                    label,
                    detail,
                    icon: None,
                    depth,
                    expandable: open.is_some(),
                    expanded: open == Some(true),
                }
            };
        let mut items = Vec::new();
        for (i, path) in files.iter().enumerate() {
            let tables = self.objects.get(&i);
            items.push(TreeItem {
                icon: Some("database".to_owned()),
                ..item(
                    format!("d{i}"),
                    path.clone(),
                    None,
                    0,
                    Some(tables.is_some()),
                )
            });
            match tables {
                Some(Err(e)) => items.push(item(format!("d{i}.error"), e.clone(), None, 1, None)),
                Some(Ok(objects)) => {
                    for (j, o) in objects.iter().enumerate() {
                        let columns = self.columns.get(&(i, j));
                        let kind = match o.kind {
                            Kind::View => "view",
                            Kind::Table if o.without_rowid => "without rowid",
                            Kind::Table => "table",
                        };
                        items.push(item(
                            format!("t{i}.{j}"),
                            o.name.clone(),
                            Some(kind.to_owned()),
                            1,
                            Some(columns.is_some()),
                        ));
                        match columns {
                            Some(Ok(cols)) => {
                                items.extend(cols.iter().enumerate().map(|(k, c)| {
                                    item(
                                        format!("c{i}.{j}.{k}"),
                                        c.name.clone(),
                                        Some(c.detail()),
                                        2,
                                        None,
                                    )
                                }))
                            }
                            Some(Err(e)) => {
                                items.push(item(
                                    format!("t{i}.{j}.error"),
                                    e.clone(),
                                    None,
                                    2,
                                    None,
                                ));
                            }
                            None => {}
                        }
                    }
                }
                None => {}
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::{file_count, parse};

    #[test]
    fn file_count_is_singular_for_one() {
        assert_eq!(file_count(1), "1 file");
        assert_eq!(file_count(0), "0 files");
        assert_eq!(file_count(3), "3 files");
    }

    #[test]
    fn keys_parse() {
        assert_eq!(parse("d3", 'd'), Some(vec![3]));
        assert_eq!(parse("t3.4", 't'), Some(vec![3, 4]));
        assert_eq!(parse("t3.error", 't'), None);
        assert_eq!(parse("d3", 't'), None);
    }
}
