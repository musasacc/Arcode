# Building UI

Plugins never draw. A plugin describes its panel as a tree of widgets,
and Arcode draws that tree with its own native widgets, in the user's
theme and font, following low-end mode. This page covers the widgets,
how updates work, events, and tables with millions of rows.

```rust
use arcode_plugin::prelude::*;
use arcode_plugin::ui::*;
```

## A first panel

```toml
[[contributes.panels]]
id = "stats"
title = "Stats"
location = "sidebar"
icon = "info"
```

```rust
#[derive(Default)]
struct Stats {
    panel: Option<Panel>,
    clicks: u32,
}

impl Plugin for Stats {
    fn open_view(&mut self, panel: PanelId, contribution: &str, _args: Json) -> Result<()> {
        if contribution != "stats" {
            return Err(Error::msg(format!("no view for `{contribution}`")));
        }
        self.panel = Some(Panel::attach(panel));
        self.redraw()
    }

    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
        let Some(p) = self.panel.as_ref().filter(|p| p.id() == panel) else {
            return Ok(());
        };
        if event.kind == EventKind::Click && p.node_id("add") == Some(event.node) {
            self.clicks += 1;
            self.redraw()?;
        }
        Ok(())
    }
}

impl Stats {
    fn redraw(&mut self) -> Result<()> {
        let root = vstack([
            text("Clicks").bold(),
            text(self.clicks.to_string()).mono().key("count"),
            button("Add one").primary().key("add"),
        ]);
        match self.panel.as_mut() {
            Some(panel) => panel.rerender(&root),
            None => Ok(()),
        }
    }
}
```

The sidebar entry appears from the manifest alone. Clicking it activates
the plugin and calls `open_view`; the plugin draws the tree. A click on
the button calls `ui_event`, the plugin builds the tree again, and the SDK
sends Arcode a single change: the new text of the `count` node.

## How updates work

You always build the **whole tree** from your state and call
`panel.rerender(&root)`. You never patch by hand.

1. The SDK gives every element a stable node id from its **key path**:
   the keys from the root down to it. An element's key is what you set
   with `.key("...")`, or its position among its siblings if you set none.
2. The first `rerender` sends the full tree. Later calls compare the new
   tree with the previous one inside your plugin and send only patches:
   changed nodes, removed nodes, a new root.
3. Arcode validates the patches and applies them all at once, or none of
   them. On an error `rerender` returns `Err` and the next `rerender`
   sends the full tree again.
4. If nothing changed, nothing is sent.

Use explicit keys:

- on every element you need to find again (`panel.node_id("add")`), such
  as buttons, inputs, tables and trees;
- on items of lists that change (`.key(item.id.clone())`), so inserting
  one item at the top does not rewrite all the others.

Two siblings with the same explicit key are an error.

## Widgets

Builders return an `Element`. Containers take children; the others are
leaves. The modifiers in the last column return the element, so they
chain: `text("x").bold().tone(Tone::Muted)`.

| Builder | Widget | Children | Events | Modifiers |
|---|---|---|---|---|
| `vstack(children)` | vertical stack, gap 8 px | any | | `.grow()` on children |
| `hstack(children)` | horizontal stack, gap 8 px, wraps when narrow | any | | |
| `stack_with(axis, gap, padding, children)` | stack with gap and padding in 4 px units | any | | |
| `split(axis, ratio, first, second)` | two panes with a draggable divider; `ratio` is the first pane's share (0.0 to 1.0) | exactly 2 | | |
| `tabs(active, [(title, child), ...])` | tabs, one child per title | one per title | `Tab(index)` | |
| `scroll(child)` | scroll container | exactly 1 | | |
| `text(s)` | text | 0 | | `.tone()`, `.mono()`, `.bold()`, `.wrap()` |
| `badge(s, tone)` | small coloured label | 0 | | `.tone()` |
| `button(label)` | button | 0 | `Click` | `.primary()`, `.icon(name)`, `.enabled(bool)` |
| `input(value, placeholder)` | text input | 0 | `Submit`, `Change(text)` | `.multiline()`, `.language(id)`, `.submit_on_enter()`, `.notify_changes()` |
| `select(options, selected)` | drop-down | 0 | `Select(index)` | |
| `checkbox(label, checked)` | checkbox | 0 | `Toggle(checked)` | |
| `key_values(pairs)` | two-column list of pairs | 0 | | |
| `table(columns, row_count, generation)` | virtual table | 0 | `RowActivate(row)`, `Sort((column, ascending))` | |
| `tree(items)` | flat, virtual tree | 0 | `TreeToggle(key)`, `TreeActivate(key)` | |
| `code(text, language)` | read-only code with highlighting and line numbers | 0 | | |
| `json(text)` | JSON viewer (pretty-printed) | 0 | | |
| `spinner(label)` | progress spinner | 0 | | |
| `empty_state(title, hint)` | centred title and hint for empty panels | 0 | | |
| `divider()` | thin line | 0 | | |
| `spacer()` | flexible empty space (grows) | 0 | | |

Every element also has `.key(k)`, `.grow()` (take the remaining space of
the parent stack), `.child(e)` and `.children(iter)`.

Details:

- **Text** is always plain text, never markup. Without `.wrap()` it stays
  on one line and is clipped at the panel edge. Users can copy any text,
  badge, key-value or table cell through the right-click menu.
- **`Tone`** is `Normal`, `Muted`, `Accent`, `Success`, `Warning` or
  `Danger`. It applies to `text` and `badge`; the colours come from the
  user's theme.
- **Axis** is `Axis::Horizontal` or `Axis::Vertical`.
- **Icons** (`button(...).icon("play")`) use the names listed in
  [The manifest](manifest.md#contributespanels).
- **`input` with `.multiline().language("sql")`** becomes a small code
  editor with syntax highlighting for that language id.
- **`code(text, Some("rust".into()))`** is a read-only editor, so it
  handles large text well.
- **`json(text)`** takes raw JSON text and shows it pretty-printed;
  invalid JSON is shown as received with a note.

The raw widget types (`Widget`, `TextProps`, `ButtonProps`, ...) are
exported from `arcode_plugin::ui` too, if you want to build an `Element`
with `Element::new(widget)`.

## Events

`ui_event(panel, event)` receives:

```rust
pub struct Event {
    pub node: NodeId,                    // the widget that sent it
    pub kind: EventKind,
    pub values: Vec<(NodeId, String)>,   // inputs edited since the previous event
}

pub enum EventKind {
    Click,                   // button
    Submit,                  // input: Enter (Ctrl/Cmd+Enter when multiline) with submit_on_enter
    Change(String),          // input with notify_changes, debounced 300 ms
    Toggle(bool),            // checkbox
    Select(u32),             // select: option index
    Tab(u32),                // tabs: tab index
    Sort((u32, bool)),       // table: (column, ascending)
    RowActivate(u64),        // table: double-click or Enter on a row
    TreeToggle(String),      // tree: expand or collapse, item key
    TreeActivate(String),    // tree: double-click or Enter on an item, item key
}
```

Match `event.node` against `panel.node_id("your-key")`.

**User actions.** `Click`, `Submit`, `Select`, `Tab`, `Sort`,
`RowActivate` and `TreeActivate` count as user actions: during them your
plugin may send HTTP requests and call `host::begin_task`. `Change`,
`Toggle` and `TreeToggle` do not.

## Inputs are uncontrolled

Arcode owns what the user types. There is no event per keystroke.

- Every event carries `values`: the current text of each input the user
  edited since the previous event. A "Run" button click therefore
  delivers the query text in the same call.
- If an input was not edited since the last event, it is not in
  `values`. Remember the last value in your state if you need it later.
- A `rerender` overwrites what the user typed only when **you** change
  that input's `value` compared to your previous tree. Keep passing the
  same value (often `""`) and the user's text stays.
- `.notify_changes()` opts in to `Change(text)` events, debounced by
  300 ms, for things like a filter box.

```rust
fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
    let Some(p) = self.panel.as_ref().filter(|p| p.id() == panel) else { return Ok(()) };
    let query_input = p.node_id("query");
    if let Some((_, text)) = event.values.iter().find(|(n, _)| Some(*n) == query_input) {
        self.query = text.clone();
    }
    if event.kind == EventKind::Click && p.node_id("run") == Some(event.node) {
        self.run_query()?;
    }
    Ok(())
}
```

Other state the user changes in a panel also stays on Arcode's side:
scroll positions, split ratios, the active tab and which tree items are
expanded. Events tell you about tab switches and tree toggles so you can
follow along.

## Tables

A table never sends all its rows. You declare the columns and the row
count; Arcode asks for rows a page at a time as the user scrolls.

```rust
fn results_view(&self) -> Element {
    let columns = vec![
        Column { title: "id".into(), width: Some(80), numeric: true, sortable: true },
        column("name"),
        column("email"),
    ];
    table(columns, Some(self.rows.len() as u64), self.generation).key("results")
}

fn fetch_rows(&mut self, _panel: PanelId, _table: NodeId, start: u64, count: u32) -> Result<RowPage> {
    let from = (start as usize).min(self.rows.len());
    let to = (from + count as usize).min(self.rows.len());
    let rows = self.rows[from..to]
        .iter()
        .map(|r| vec![Cell::Int(r.id), Cell::Text(r.name.clone()), Cell::Text(r.email.clone())])
        .collect();
    Ok(RowPage { start, rows, at_end: to == self.rows.len() })
}
```

- `column(title)` makes a plain column; set `width` (pixels), `numeric`
  (right-aligned) and `sortable` on a `Column` directly. Clicking a
  sortable header sends `Sort((column, ascending))`.
- `row_count` is `Some(n)`, or `None` when you do not know it yet. With
  `None`, Arcode keeps asking for pages until one comes back with
  `at_end: true`.
- `fetch_rows` gets `start` and `count` (200 rows per page). Return a
  page that starts at `start`; return fewer rows at the end and set
  `at_end`. If several tables share a panel, the `table` argument is the
  node id to compare with `panel.node_id(...)`.
- Cells: `Cell::Empty`, `Cell::Int(i64)`, `Cell::Real(f64)`,
  `Cell::Text(String)`, `Cell::Boolean(bool)`, `Cell::Blob(size)`.
- When the rows change (a new query, a new sort order), bump
  `generation` in the next tree, or call
  `panel.rows_changed(table_id, Some(new_count))`. Both make Arcode drop
  its cached pages and ask again; a new generation also scrolls back to
  the top.

Arcode keeps at most a few pages per table, asks for the visible page
first and one page ahead, and coalesces requests while the user scrolls,
so a million-row table stays cheap on both sides.

`TablePager` stores rows on the plugin side with a row and byte cap and
answers `fetch_rows` for you:

```rust
let mut pager = TablePager::new(100_000, 64 * 1024 * 1024);
for row in query_results {
    if !pager.push(row) {
        break; // a cap was reached; pager.truncated() is now true
    }
}
// in fetch_rows:
Ok(pager.page(start, count))
```

Limits: a page may hold at most 4 MiB, counting 32 bytes per cell plus
its text; text cells are shown cut to 4 KiB.

## Trees

A tree is a flat list of items that carry their depth. You decide which
children to include based on which items are expanded.

```rust
TreeItem {
    key: "db/users".into(),        // what events carry
    label: "users".into(),
    detail: Some("1,204 rows".into()),
    icon: Some("table".into()),
    depth: 1,
    expandable: true,
    expanded: self.open.contains("db/users"),
}
```

When the user expands an item, Arcode shows it expanded with a spinner
right away and sends `TreeToggle(key)`. Add the children to your state
and rerender. `TreeActivate(key)` is a double-click or Enter on an item.

## Incremental updates in practice

- Build the tree from state in one function (`fn view(&self) -> Element`)
  and call it after every change.
- Keep the `Panel` value for as long as the panel is open, so it can
  diff. When `Panel::open` returns the id you already have, keep your
  existing value.
- Rerendering a big tree costs your plugin CPU time for the diff, not the
  editor's. Arcode's validation of each change also runs on your plugin's
  thread.
- `panel.invalidate()` forces a full render next time; you rarely need
  it.

## Limits

| What | Limit |
|---|---|
| Widgets per panel | 20,000 |
| Depth | 64 levels |
| Text per panel | 8 MiB, counting the text of every widget plus the size of every list entry (tab title, option, column, pair, tree item) |
| Panel title | 256 bytes |
| Table page | 4 MiB |

A tree that breaks a rule is refused as a whole, and `rerender` returns
`Err` with `UiError::InvalidTree(reason)` or `UiError::TooLarge`. Child
counts are part of the rules: `split` needs exactly 2 children, `scroll`
exactly 1, `tabs` one per title, and leaf widgets none.

## Panel lifetime

- `open_view` is called when Arcode shows your panel: from the activity
  bar, for an opener, or again after your plugin restarted while the
  panel was open. Always draw something in it.
- `view_closed` is called when the user closes it. Drop your `Panel`.
- While one of your panels is open, your plugin is not unloaded for
  being idle.
- If your plugin crashes, its panels show "<Plugin> stopped: <reason>"
  until the next event restarts it.
