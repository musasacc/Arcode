//! The rows an open menu shows: the [`model`](super::model) items with
//! their commands resolved where focus was when the menu opened, so each
//! row knows its action, its key binding and whether it applies.

use std::path::PathBuf;

use gpui::{Action, SharedString};

use super::model::{Facts, Item, MENUS, mnemonic};

/// A row of an open menu.
pub struct Row {
    /// The label without the `&`.
    pub label: SharedString,
    /// Byte offset of the mnemonic in `label` and the key (lowercase).
    pub mnemonic: Option<(usize, char)>,
    /// The key binding, as the palette shows it (`Ctrl+Shift+P`).
    pub keys: Option<SharedString>,
    /// Muted text at the right instead of keys (a recent project's path).
    pub detail: Option<SharedString>,
    pub enabled: bool,
    /// `Some` for rows of a choice (themes): whether this one is in use.
    pub checked: Option<bool>,
    pub kind: RowKind,
}

/// What picking a row does.
pub enum RowKind {
    Separator,
    /// Dispatches the action where focus was.
    Run(Box<dyn Action>),
    /// Opens a nested menu.
    Sub(Vec<Row>),
    /// Opens a recent project in this window.
    OpenRecent(PathBuf),
    /// Sets `ui.theme` (`"system"` or a theme name).
    SetTheme(SharedString),
}

impl Row {
    fn separator() -> Self {
        Row {
            label: SharedString::default(),
            mnemonic: None,
            keys: None,
            detail: None,
            enabled: false,
            checked: None,
            kind: RowKind::Separator,
        }
    }

    fn new(label: &str, kind: RowKind) -> Self {
        let (text, mark) = mnemonic(label);
        Row {
            label: text.into(),
            mnemonic: mark,
            keys: None,
            detail: None,
            enabled: true,
            checked: None,
            kind,
        }
    }

    /// Whether the row can be selected and picked.
    pub fn selectable(&self) -> bool {
        self.enabled && !matches!(self.kind, RowKind::Separator)
    }

    /// The nested rows of a submenu row.
    pub fn children(&self) -> Option<&[Row]> {
        match &self.kind {
            RowKind::Sub(rows) => Some(rows),
            _ => None,
        }
    }

    /// The row for tests: `-` for a separator, the label in parentheses
    /// when disabled, `[x]` / `[ ]` before a choice, `>` after a submenu
    /// and the keys after a tab.
    pub fn describe(&self) -> String {
        if matches!(self.kind, RowKind::Separator) {
            return "-".into();
        }
        let mut s = match self.checked {
            Some(true) => format!("[x] {}", self.label),
            Some(false) => format!("[ ] {}", self.label),
            None => self.label.to_string(),
        };
        if !self.enabled {
            s = format!("({s})");
        }
        if matches!(self.kind, RowKind::Sub(_)) {
            s.push_str(" >");
        }
        if let Some(keys) = &self.keys {
            s.push('\t');
            s.push_str(keys);
        }
        s
    }
}

/// A command resolved where focus is.
pub struct Resolved {
    pub action: Box<dyn Action>,
    /// Some view on the focus path (or the app) handles it.
    pub available: bool,
    pub keys: Option<String>,
}

/// What the rows are built from.
pub struct Sources<'a> {
    /// Resolves a palette command label; `None` when it is not registered.
    pub command: &'a dyn Fn(&str) -> Option<Resolved>,
    pub facts: Facts,
    /// Recent projects (path, name), newest first, without the current one.
    pub recent: &'a [(PathBuf, String)],
    /// Theme choices: (value of `ui.theme`, label, in use).
    pub themes: &'a [(String, String, bool)],
}

/// The rows of `items`. A command that is not registered is left out;
/// separators never lead, trail or follow each other.
pub fn build(items: &[Item], src: &Sources) -> Vec<Row> {
    let mut rows = Vec::new();
    for item in items {
        match item {
            Item::Separator => rows.push(Row::separator()),
            Item::Command {
                label,
                command,
                when,
            } => {
                let Some(resolved) = (src.command)(command) else {
                    continue;
                };
                let mut row = Row::new(label, RowKind::Run(resolved.action));
                row.keys = resolved.keys.map(Into::into);
                row.enabled = resolved.available && when.holds(src.facts);
                rows.push(row);
            }
            Item::Submenu { label, items } => {
                let children = build(items, src);
                let mut row = Row::new(label, RowKind::Sub(Vec::new()));
                row.enabled = children.iter().any(Row::selectable);
                row.kind = RowKind::Sub(children);
                rows.push(row);
            }
            Item::Recent { label } => {
                let children: Vec<Row> = src
                    .recent
                    .iter()
                    .map(|(path, name)| {
                        let mut row = Row::new("", RowKind::OpenRecent(path.clone()));
                        row.label = name.clone().into();
                        row.detail = Some(path.display().to_string().into());
                        row
                    })
                    .collect();
                let mut row = Row::new(label, RowKind::Sub(Vec::new()));
                row.enabled = !children.is_empty();
                row.kind = RowKind::Sub(children);
                rows.push(row);
            }
            Item::Themes { label } => {
                let children: Vec<Row> = src
                    .themes
                    .iter()
                    .map(|(value, name, current)| {
                        let mut row = Row::new("", RowKind::SetTheme(value.clone().into()));
                        row.label = name.clone().into();
                        row.checked = Some(*current);
                        row
                    })
                    .collect();
                let mut row = Row::new(label, RowKind::Sub(Vec::new()));
                row.enabled = !children.is_empty();
                row.kind = RowKind::Sub(children);
                rows.push(row);
            }
        }
    }
    tidy_separators(&mut rows);
    rows
}

/// One submenu row per menu of the bar (the menus themselves are their
/// children).
pub fn build_root(src: &Sources) -> Vec<Row> {
    MENUS
        .iter()
        .map(|menu| {
            let mut row = Row::new(menu.title, RowKind::Sub(build(menu.items, src)));
            row.enabled = true;
            row
        })
        .collect()
}

fn tidy_separators(rows: &mut Vec<Row>) {
    let is_sep = |r: &Row| matches!(r.kind, RowKind::Separator);
    let mut out: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        if is_sep(&row) && out.last().is_none_or(is_sep) {
            continue;
        }
        out.push(row);
    }
    if out.last().is_some_and(is_sep) {
        out.pop();
    }
    *rows = out;
}

/// The first selectable row at or after `from` going in `dir` (+1 or -1),
/// wrapping around.
pub fn next_selectable(rows: &[Row], from: usize, dir: isize) -> Option<usize> {
    let n = rows.len() as isize;
    if n == 0 {
        return None;
    }
    (0..n)
        .map(|step| (from as isize + step * dir).rem_euclid(n) as usize)
        .find(|&ix| rows[ix].selectable())
}
