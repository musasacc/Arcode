//! `ui-big` fixture: a million-row table served page by page, and views
//! sized around the host's UI caps.
//!
//! - `open-view(big)`: renders a title and a `table` keyed `table` with
//!   1,000,000 rows.
//! - `fetch-rows`: logs `{"export": "fetch-rows", "start", "count"}` and
//!   answers generated rows (or a page over the 4 MiB cap after `huge`).
//! - command `render` with one of `{"nodes": n}` (a stack of n nodes in
//!   total), `{"depth": d}` (d nested levels), `{"rows-changed": n}`, or
//!   `{"huge": true}`; reports `{"op": "render", "result": "ok" | error}`.

use arcode_plugin::prelude::*;
use arcode_plugin::ui::{self, Cell, Element};
use serde_json::{Value, json};

/// Rows in the big table.
const ROWS: u64 = 1_000_000;

#[derive(Default)]
struct Big {
    panel: Option<Panel>,
    huge: bool,
}

fn report(value: Value) {
    host::log(host::Level::Info, &value.to_string());
}

fn table_view(rows: u64) -> Element {
    ui::vstack([
        ui::text("Big").key("title"),
        ui::table(vec![ui::column("n"), ui::column("text")], Some(rows), 1)
            .key("table")
            .grow(),
    ])
}

fn nodes(n: u64) -> Element {
    ui::vstack((1..n).map(|_| ui::text("")))
}

fn deep(levels: u64) -> Element {
    let mut e = ui::text("bottom");
    for _ in 1..levels {
        e = ui::vstack([e]);
    }
    e
}

impl Big {
    fn panel(&mut self) -> Result<&mut Panel> {
        if self.panel.is_none() {
            self.panel = Some(Panel::open("big", "Big", None)?);
        }
        self.panel.as_mut().ok_or_else(|| Error::msg("no panel"))
    }

    fn render(&mut self, args: &Value) -> Result<()> {
        if args["huge"].as_bool() == Some(true) {
            self.huge = true;
            return Ok(());
        }
        let panel = self.panel()?;
        if let Some(n) = args["rows-changed"].as_u64() {
            let table = panel
                .node_id("table")
                .ok_or_else(|| Error::msg("no table"))?;
            return panel.rows_changed(table, Some(n));
        }
        let tree = match (args["nodes"].as_u64(), args["depth"].as_u64()) {
            (Some(n), _) => nodes(n),
            (_, Some(d)) => deep(d),
            _ => table_view(ROWS),
        };
        panel.rerender(&tree)
    }
}

impl Plugin for Big {
    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        if id != "render" {
            return Err(Error::msg(format!("unknown command `{id}`")));
        }
        let args: Value = args.parse()?;
        let result = match self.render(&args) {
            Ok(()) => json!("ok"),
            Err(e) => json!(e.message()),
        };
        report(json!({"op": "render", "result": result}));
        Ok(())
    }

    fn open_view(&mut self, panel: PanelId, _contribution: &str, _args: Json) -> Result<()> {
        let mut p = Panel::attach(panel);
        p.rerender(&table_view(ROWS))?;
        self.panel = Some(p);
        Ok(())
    }

    fn fetch_rows(
        &mut self,
        _panel: PanelId,
        _table: NodeId,
        start: u64,
        count: u32,
    ) -> Result<RowPage> {
        report(json!({"export": "fetch-rows", "start": start, "count": count}));
        let end = (start + u64::from(count)).min(ROWS);
        let filler = if self.huge {
            "x".repeat(64 * 1024)
        } else {
            String::new()
        };
        let rows = (start..end)
            .map(|i| {
                vec![
                    Cell::Int(i64::try_from(i).unwrap_or(i64::MAX)),
                    Cell::Text(format!("row {i}{filler}")),
                ]
            })
            .collect();
        Ok(RowPage {
            start,
            rows,
            at_end: end >= ROWS,
        })
    }
}

arcode_plugin::export_plugin!(Big);
