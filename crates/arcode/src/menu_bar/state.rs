//! Where the user is in an open menu, and how keys and the mouse move
//! that: which menu of the bar is open, and the selected row of each open
//! popup (the menu, then any submenus). Pure state, tested without a
//! window.

use super::model::MENUS;
use super::rows::{Row, next_selectable};

/// A key the open menu understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Enter,
    Escape,
    /// A letter or digit: the mnemonic of a row (or, with only the bar
    /// active, of a menu).
    Char(char),
}

/// What a key or click did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The selection moved (or nothing happened).
    Moved,
    /// Close the menu, giving focus back.
    Close,
    /// Run row `row` of popup `level`, then close.
    Pick { level: usize, row: usize },
}

/// An open menu.
pub struct OpenMenu {
    /// One submenu row per menu of the bar.
    pub root: Vec<Row>,
    /// Opened from the single menu button: the first popup lists the
    /// menus themselves.
    pub collapsed: bool,
    /// The menu of the bar that is open or highlighted (not collapsed).
    pub title: usize,
    /// Only the bar is active (after a tap on Alt): the title is
    /// highlighted and no popup is open yet.
    pub bar_only: bool,
    /// Opened with the keyboard: mnemonics are underlined.
    pub keyboard: bool,
    /// The selected row of each open popup, outermost first. Every popup
    /// but the last has its submenu row selected.
    pub sel: Vec<Option<usize>>,
}

impl OpenMenu {
    /// Opens menu `title` of the bar (or the menu button's list when
    /// `collapsed`, with `title`'s submenu open). With `keyboard` the first
    /// row is selected.
    pub fn new(root: Vec<Row>, collapsed: bool, title: usize, keyboard: bool) -> Self {
        let mut menu = OpenMenu {
            root,
            collapsed,
            title: title.min(MENUS.len().saturating_sub(1)),
            bar_only: false,
            keyboard,
            sel: Vec::new(),
        };
        menu.open_title(menu.title, keyboard);
        menu
    }

    /// The menu button's list of menus, nothing open under it yet; with
    /// `keyboard` its first row is selected.
    pub fn list(root: Vec<Row>, keyboard: bool) -> Self {
        let mut menu = OpenMenu {
            root,
            collapsed: true,
            title: 0,
            bar_only: false,
            keyboard,
            sel: vec![None],
        };
        if keyboard {
            menu.select_first();
        }
        menu
    }

    /// Only the bar, `title` highlighted (a tap on Alt).
    pub fn bar(root: Vec<Row>, title: usize) -> Self {
        OpenMenu {
            root,
            collapsed: false,
            title,
            bar_only: true,
            keyboard: true,
            sel: Vec::new(),
        }
    }

    /// The rows of popup `level` (0 is the menu under the bar).
    pub fn rows(&self, level: usize) -> Option<&[Row]> {
        if self.bar_only || level >= self.sel.len() {
            return None;
        }
        let mut rows: &[Row] = if self.collapsed {
            &self.root
        } else {
            self.root.get(self.title)?.children()?
        };
        for depth in 0..level {
            rows = rows.get(self.sel[depth]?)?.children()?;
        }
        Some(rows)
    }

    /// How many popups are open.
    pub fn depth(&self) -> usize {
        if self.bar_only { 0 } else { self.sel.len() }
    }

    /// Opens menu `title` of the bar (or, collapsed, its submenu).
    pub fn open_title(&mut self, title: usize, select_first: bool) {
        self.bar_only = false;
        self.title = title;
        if self.collapsed {
            self.sel = vec![Some(title), None];
        } else {
            self.sel = vec![None];
        }
        if select_first {
            self.select_first();
        }
    }

    fn select_first(&mut self) {
        let level = self.sel.len() - 1;
        let first = self
            .rows(level)
            .and_then(|rows| next_selectable(rows, 0, 1));
        self.sel[level] = first;
    }

    /// The mouse is over row `row` of popup `level`: select it and open its
    /// submenu (closing deeper ones).
    pub fn hover(&mut self, level: usize, row: usize) {
        let Some(rows) = self.rows(level) else {
            return;
        };
        let Some(target) = rows.get(row) else {
            return;
        };
        let selectable = target.selectable();
        let opens = selectable && target.children().is_some();
        if self.sel.get(level) == Some(&Some(row)) && (!opens || self.sel.len() > level + 1) {
            if !opens {
                self.sel.truncate(level + 1);
            }
            return;
        }
        self.sel.truncate(level + 1);
        self.sel[level] = selectable.then_some(row);
        if opens {
            self.sel.push(None);
        }
    }

    /// A click on row `row` of popup `level`.
    pub fn click(&mut self, level: usize, row: usize) -> Outcome {
        let Some(target) = self.rows(level).and_then(|rows| rows.get(row)) else {
            return Outcome::Moved;
        };
        if !target.selectable() {
            return Outcome::Moved;
        }
        if target.children().is_some() {
            self.hover(level, row);
            return Outcome::Moved;
        }
        Outcome::Pick { level, row }
    }

    /// Handles a key.
    pub fn key(&mut self, key: NavKey) -> Outcome {
        if self.bar_only {
            return self.bar_key(key);
        }
        let level = self.sel.len() - 1;
        let Some(rows) = self.rows(level) else {
            return Outcome::Close;
        };
        let last = rows.len().saturating_sub(1);
        let current = self.sel[level];
        match key {
            NavKey::Down => {
                self.sel[level] = next_selectable(rows, current.map_or(0, |s| s + 1), 1);
            }
            NavKey::Up => {
                self.sel[level] = next_selectable(rows, current.map_or(last, |s| s + last), -1);
            }
            NavKey::Home => self.sel[level] = next_selectable(rows, 0, 1),
            NavKey::End => self.sel[level] = next_selectable(rows, last, -1),
            NavKey::Right => {
                if let Some(row) = current.filter(|&r| rows[r].children().is_some()) {
                    self.enter_submenu(level, row);
                } else if !self.collapsed {
                    self.open_title((self.title + 1) % MENUS.len(), true);
                }
            }
            NavKey::Left => {
                let top = if self.collapsed { 1 } else { 0 };
                if level > top {
                    self.sel.pop();
                } else if !self.collapsed {
                    let n = MENUS.len();
                    self.open_title((self.title + n - 1) % n, true);
                } else if level == 1 {
                    self.sel.pop();
                }
            }
            NavKey::Enter => {
                let Some(row) = current else {
                    return Outcome::Moved;
                };
                if rows[row].children().is_some() {
                    self.enter_submenu(level, row);
                } else {
                    return Outcome::Pick { level, row };
                }
            }
            NavKey::Escape => {
                if level > 0 {
                    self.sel.pop();
                } else {
                    return Outcome::Close;
                }
            }
            NavKey::Char(ch) => {
                let found = rows
                    .iter()
                    .position(|r| r.selectable() && r.mnemonic.map(|(_, c)| c) == Some(ch));
                if let Some(row) = found {
                    let opens = rows[row].children().is_some();
                    self.sel[level] = Some(row);
                    if opens {
                        self.enter_submenu(level, row);
                    } else {
                        return Outcome::Pick { level, row };
                    }
                }
            }
        }
        Outcome::Moved
    }

    fn enter_submenu(&mut self, level: usize, row: usize) {
        self.sel.truncate(level + 1);
        self.sel[level] = Some(row);
        self.sel.push(None);
        self.select_first();
    }

    fn bar_key(&mut self, key: NavKey) -> Outcome {
        let n = MENUS.len();
        match key {
            NavKey::Left => self.title = (self.title + n - 1) % n,
            NavKey::Right => self.title = (self.title + 1) % n,
            NavKey::Home => self.title = 0,
            NavKey::End => self.title = n - 1,
            NavKey::Down | NavKey::Up | NavKey::Enter => self.open_title(self.title, true),
            NavKey::Escape => return Outcome::Close,
            NavKey::Char(ch) => match super::model::menu_for_mnemonic(ch) {
                Some(title) => self.open_title(title, true),
                None => return Outcome::Moved,
            },
        }
        Outcome::Moved
    }

    /// The row a [`Outcome::Pick`] names.
    pub fn row(&self, level: usize, row: usize) -> Option<&Row> {
        self.rows(level)?.get(row)
    }

    /// The open popups as text, for tests: each popup's rows with the
    /// selected one prefixed with `> `.
    pub fn describe(&self) -> Vec<Vec<String>> {
        (0..self.depth())
            .filter_map(|level| {
                let rows = self.rows(level)?;
                Some(
                    rows.iter()
                        .enumerate()
                        .map(|(ix, r)| {
                            let mark = if self.sel[level] == Some(ix) {
                                "> "
                            } else {
                                ""
                            };
                            format!("{mark}{}", r.describe())
                        })
                        .collect(),
                )
            })
            .collect()
    }
}
