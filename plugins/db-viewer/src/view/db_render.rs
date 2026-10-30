//! The view tree of a [`DbView`]. Every ancestor of the SQL input and the
//! two tables is keyed, so their node ids (and the text the user is typing)
//! survive every re-render.

use arcode_plugin::ui::{self, Axis, Element, Tone, TreeItem};

use super::db::{DbView, QueryState, SNAPSHOT_NOTE};
use crate::db::Kind;
use crate::recent;
use crate::sql::OpenMode;

/// The Query tab's status while a query runs.
pub const RUNNING: &str = "Running query… (Esc or [Cancel] in the status bar stops it)";

/// Share of the width the schema tree gets.
const TREE_RATIO: f32 = 0.28;

/// The whole panel.
pub fn view(v: &DbView) -> Element {
    if let Err(message) = &v.db {
        return ui::vstack([
            ui::text(v.path.clone()).bold().key("path"),
            ui::text(message.clone())
                .tone(Tone::Danger)
                .wrap()
                .key("error"),
        ])
        .key("failed");
    }
    let left = ui::vstack([header(v).key("header"), schema(v).key("schema").grow()]).key("left");
    let right = ui::tabs(
        v.tab,
        [
            ("Data".to_owned(), data_tab(v).key("data-tab")),
            ("Query".to_owned(), query_tab(v).key("query-tab")),
        ],
    )
    .key("tabs");
    ui::split(Axis::Horizontal, TREE_RATIO, left, right).key("root")
}

fn header(v: &DbView) -> Element {
    let (label, tone) = match (v.mode(), v.writes) {
        (Some(OpenMode::Snapshot), _) => ("snapshot", Tone::Warning),
        (_, true) => ("writes on", Tone::Danger),
        _ => ("read-only", Tone::Muted),
    };
    let mut items = vec![
        ui::hstack([
            ui::text(v.file_name().to_owned()).bold().key("name"),
            ui::badge(label, tone).key("mode"),
        ])
        .key("title"),
    ];
    if v.mode() == Some(OpenMode::Snapshot) {
        items.push(
            ui::text(SNAPSHOT_NOTE)
                .tone(Tone::Warning)
                .wrap()
                .key("snapshot"),
        );
    }
    if let Some((tone, text)) = &v.notice {
        items.push(ui::text(text.clone()).tone(*tone).wrap().key("notice"));
    }
    ui::vstack(items)
}

fn schema(v: &DbView) -> Element {
    if v.objects.is_empty() {
        return ui::empty_state("No tables", "This database has no tables or views.");
    }
    let mut items = Vec::new();
    for (i, o) in v.objects.iter().enumerate() {
        let (detail, icon) = match (o.kind, o.without_rowid) {
            (Kind::View, _) => ("view", "eye"),
            (Kind::Table, true) => ("without rowid", "table"),
            (Kind::Table, false) => ("table", "table"),
        };
        let expanded = v.expanded.get(&i);
        items.push(TreeItem {
            key: format!("o{i}"),
            label: o.name.clone(),
            detail: Some(detail.to_owned()),
            icon: Some(icon.to_owned()),
            depth: 0,
            expandable: true,
            expanded: expanded.is_some(),
        });
        match expanded {
            Some(Ok(columns)) => items.extend(columns.iter().enumerate().map(|(k, c)| TreeItem {
                key: format!("o{i}.{k}"),
                label: c.name.clone(),
                detail: Some(c.detail()),
                icon: None,
                depth: 1,
                expandable: false,
                expanded: false,
            })),
            Some(Err(message)) => items.push(TreeItem {
                key: format!("o{i}.error"),
                label: message.clone(),
                detail: None,
                icon: None,
                depth: 1,
                expandable: false,
                expanded: false,
            }),
            None => {}
        }
    }
    ui::tree(items)
}

fn data_tab(v: &DbView) -> Element {
    let mut items = Vec::new();
    if let (Some(grid), Some(object)) = (&v.grid, v.selected_object()) {
        let count = match grid.shape.row_count {
            Some(n) => format!("{} rows", group(n)),
            None => "rows: unknown".to_owned(),
        };
        let mut head = vec![
            ui::text(object.name.clone()).bold().key("table-name"),
            ui::badge(count, Tone::Muted).key("count"),
        ];
        if grid.shape.row_count.is_none() {
            // Counting took longer than 200 ms; counting on request is a
            // cancellable task.
            head.push(ui::button("Count").icon("refresh").key("count-rows"));
        }
        if let Some(sort) = &grid.source.sort {
            let dir = if sort.ascending { "asc" } else { "desc" };
            head.push(ui::text(format!("sorted by {} {dir}", sort.column)).tone(Tone::Muted));
        }
        items.push(ui::hstack(head).key("data-head"));
        if let Some(e) = &v.grid_error {
            items.push(error_line(e).key("data-error"));
        }
        items.push(
            ui::table(grid.grid_columns(), grid.shape.row_count, grid.generation)
                .key("data")
                .grow(),
        );
    } else {
        if let Some(e) = &v.grid_error {
            items.push(error_line(e).key("data-error"));
        }
        items.push(
            ui::empty_state("No table selected", "Pick a table or view on the left.")
                .key("data-empty")
                .grow(),
        );
    }
    ui::vstack(items)
}

fn query_tab(v: &DbView) -> Element {
    let running = v.query == QueryState::Running;
    let mut actions = vec![
        ui::button("Run")
            .primary()
            .icon("play")
            .enabled(!running)
            .key("run"),
        ui::checkbox("Allow writes", v.writes).key("writes"),
    ];
    if !v.recent.is_empty() {
        let labels = v.recent.iter().map(|q| recent::label(q));
        actions.push(ui::select(labels, None).key("recent"));
    }
    actions.push(ui::spacer());
    let mut items = vec![
        ui::input(v.sql.clone(), "SELECT * FROM …  (Ctrl+Enter runs)")
            .multiline()
            .language("sql")
            .submit_on_enter()
            .key("sql"),
        ui::hstack(actions).key("actions"),
    ];
    if running {
        // No in-panel Cancel button: calls into a plugin are serial, so its
        // click would arrive after the query. The task chip and Esc cancel
        // the running call itself (docs/PLUGINS.md, Appendix B, Step 11).
        items.push(ui::spinner(RUNNING).key("status"));
    } else if let Some((tone, text)) = v.status_line() {
        items.push(ui::text(text).tone(tone).wrap().key("status"));
    }
    match &v.results {
        Some(outcome) if outcome.has_rows => {
            if let Some(note) = outcome.truncation() {
                items.push(ui::text(note).tone(Tone::Warning).key("truncated"));
            }
            items.push(
                ui::table(
                    outcome.columns.clone(),
                    Some(outcome.rows.len()),
                    v.result_generation,
                )
                .key("result")
                .grow(),
            );
        }
        _ => items.push(
            ui::empty_state("No results", "Run a query to see its rows here.")
                .key("result-empty")
                .grow(),
        ),
    }
    ui::vstack(items)
}

fn error_line(message: &str) -> Element {
    ui::text(message.to_owned()).tone(Tone::Danger).wrap()
}

/// `1234567` → `1,234,567`.
pub fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn digits_are_grouped() {
        assert_eq!(super::group(0), "0");
        assert_eq!(super::group(999), "999");
        assert_eq!(super::group(1_000_000), "1,000,000");
    }
}
