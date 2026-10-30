//! Events, queries and row pages of a [`DbView`].

use std::path::Path;

use arcode_plugin::prelude::*;
use arcode_plugin::ui::Tone;

use super::db::{DbView, QueryState};
use crate::cells::fit_page;
use crate::query;
use crate::recent::{self, Settings};

/// The schema tree's key for object `i` is `o<i>`; columns are `o<i>.<k>`.
pub fn object_index(key: &str) -> Option<usize> {
    key.strip_prefix('o')?.parse().ok()
}

impl DbView {
    fn is(&self, key: &str, node: NodeId) -> bool {
        self.panel.node_id(key) == Some(node)
    }

    /// Handles one widget event and re-renders.
    pub fn handle(&mut self, root: &Path, event: Event) -> Result<()> {
        if let Some(sql) = self.panel.node_id("sql") {
            for (node, text) in &event.values {
                if *node == sql {
                    self.sql.clone_from(text);
                }
            }
        }
        let node = event.node;
        match event.kind {
            EventKind::Click if self.is("run", node) => return self.run(),
            EventKind::Click if self.is("count-rows", node) => self.count_rows(),
            EventKind::Submit if self.is("sql", node) => return self.run(),
            EventKind::Change(text) if self.is("sql", node) => self.sql = text,
            EventKind::Toggle(on) if self.is("writes", node) => self.set_writes(root, on),
            EventKind::Tab(tab) if self.is("tabs", node) => self.tab = tab,
            EventKind::Select(i) if self.is("recent", node) => {
                if let Some(sql) = self.recent.get(i as usize) {
                    self.sql.clone_from(sql);
                    self.tab = 1;
                }
            }
            EventKind::Sort((column, ascending)) if self.is("data", node) => {
                if let Some(grid) = self.grid.as_mut() {
                    grid.sort(column as usize, ascending);
                }
            }
            EventKind::TreeToggle(key) if self.is("schema", node) => {
                if let Some(i) = object_index(&key) {
                    self.toggle(i);
                }
            }
            EventKind::TreeActivate(key) if self.is("schema", node) => {
                if let Some(i) = object_index(&key) {
                    self.select(Some(i));
                    self.tab = 0;
                }
            }
            _ => return Ok(()),
        }
        self.render()
    }

    /// Runs the Query tab's SQL as a cancellable task.
    pub fn run(&mut self) -> Result<()> {
        if self.sql.trim().is_empty() {
            self.query = QueryState::Failed("Type a query first.".to_owned());
            return self.render();
        }
        // Raises the CPU budget to limits.task-seconds (gesture calls only)
        // and shows "Running query… [Cancel]" in the status bar.
        host::begin_task("Running query");
        self.query = QueryState::Running;
        self.tab = 1;
        self.render()?;
        let sql = self.sql.clone();
        recent::remember(&mut self.recent, &sql);
        let max_rows = Settings::load().max_rows();
        let result = match self.db.as_mut() {
            Ok(db) => query::run(db, &sql, max_rows).map_err(|e| e.message()),
            Err(e) => Err(e.clone()),
        };
        match result {
            Ok(outcome) => {
                self.query = QueryState::Done(outcome.summary());
                if outcome.wrote {
                    self.refresh();
                }
                self.results = Some(outcome);
                self.result_generation = self.result_generation.wrapping_add(1);
            }
            Err(message) => self.query = QueryState::Failed(message),
        }
        self.render()
    }

    /// Counts the grid's rows as a cancellable task ("Count" button).
    fn count_rows(&mut self) {
        host::begin_task("Counting rows");
        if let (Some(grid), Ok(db)) = (self.grid.as_mut(), self.db.as_mut()) {
            self.grid_error = grid.count_now(db).err().map(|e| e.message());
        }
    }

    /// Answers `fetch-rows` for the Data grid or the query results.
    pub fn fetch(&mut self, table: NodeId, start: u64, count: u32) -> Result<RowPage> {
        if self.is("result", table) {
            let mut page = match &self.results {
                Some(outcome) => outcome.rows.page(start, count),
                None => RowPage {
                    start,
                    rows: Vec::new(),
                    at_end: true,
                },
            };
            fit_page(&mut page.rows);
            return Ok(page);
        }
        if !self.is("data", table) {
            return Err(Error::msg(format!("no table with node id {table}")));
        }
        let (Some(grid), Ok(db)) = (self.grid.as_mut(), self.db.as_mut()) else {
            return Err(Error::msg("no table selected"));
        };
        match grid.fetch(db, start, count) {
            Ok(page) => {
                if self.grid_error.take().is_some() {
                    self.render()?;
                }
                Ok(page)
            }
            Err(e) => {
                let message = e.message();
                self.grid_error = Some(message.clone());
                self.render()?;
                Err(Error::msg(message))
            }
        }
    }

    /// The `new-query` command: show the Query tab.
    pub fn show_query(&mut self) -> Result<()> {
        self.tab = 1;
        self.render()
    }

    /// Tone and text of the Query tab's status line.
    pub fn status_line(&self) -> Option<(Tone, String)> {
        match &self.query {
            QueryState::Idle => None,
            QueryState::Running => Some((Tone::Muted, "Running…".to_owned())),
            QueryState::Done(summary) => Some((Tone::Success, summary.clone())),
            QueryState::Failed(message) => Some((Tone::Danger, message.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::object_index;

    #[test]
    fn tree_keys() {
        assert_eq!(object_index("o12"), Some(12));
        assert_eq!(object_index("o1.3"), None);
        assert_eq!(object_index("x1"), None);
    }
}
