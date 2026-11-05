//! The menus as GPUI application menus, for macOS's native menu bar: an
//! "Arcode" application menu with Settings and Quit, then the same menus
//! as the in-window bar. The OS shows each item's key binding and greys
//! items whose action is not available where focus is. GPUI asks only
//! that, so an item whose condition does not hold (Step Over with no
//! debug session) runs [`Unavailable`](super::Unavailable) instead, which
//! nothing handles; the menus are rebuilt when the conditions change
//! ([`sync_native_menus`](super::sync_native_menus)). Such an item shows
//! no key binding while it is greyed.

use gpui::{App, Menu, MenuItem};

use super::model::{Facts, Item, MENUS, mnemonic};
use crate::commands::commands;

/// Commands the application menu holds, so the other menus leave them out.
const APP_MENU: &[(&str, &str)] = &[
    ("Settings…", "Preferences: Open Settings"),
    ("Quit Arcode", "Application: Quit"),
];

/// The application menus. Recent projects and the theme list need data
/// only the open window has: the theme list becomes "Select Theme…" and
/// recent projects are left out.
pub fn menus(facts: Facts, cx: &App) -> Vec<Menu> {
    let action = |label: &str| {
        commands(cx)
            .iter()
            .find(|c| c.label() == label)
            .map(|c| c.action())
    };
    let mut app_items = Vec::new();
    for (ix, (name, command)) in APP_MENU.iter().enumerate() {
        if ix == APP_MENU.len() - 1 {
            app_items.push(MenuItem::separator());
        }
        if let Some(action) = action(command) {
            app_items.push(MenuItem::Action {
                name: (*name).into(),
                action,
                os_action: None,
            });
        }
    }
    let mut menus = vec![Menu {
        name: "Arcode".into(),
        items: app_items,
    }];
    for menu in MENUS {
        menus.push(Menu {
            name: mnemonic(menu.title).0.into(),
            items: items(menu.items, facts, &action),
        });
    }
    menus
}

fn items(
    items: &[Item],
    facts: Facts,
    action: &dyn Fn(&str) -> Option<Box<dyn gpui::Action>>,
) -> Vec<MenuItem> {
    let mut out: Vec<MenuItem> = Vec::new();
    for item in items {
        match item {
            Item::Separator => out.push(MenuItem::separator()),
            Item::Command {
                label,
                command,
                when,
            } => {
                if APP_MENU.iter().any(|(_, c)| c == command) {
                    continue;
                }
                if let Some(action) = action(command) {
                    let action = if when.holds(facts) {
                        action
                    } else {
                        Box::new(super::Unavailable)
                    };
                    out.push(MenuItem::Action {
                        name: mnemonic(label).0.into(),
                        action,
                        os_action: None,
                    });
                }
            }
            Item::Submenu { label, items: sub } => out.push(MenuItem::submenu(Menu {
                name: mnemonic(label).0.into(),
                items: self::items(sub, facts, action),
            })),
            Item::Themes { .. } => {
                if let Some(action) = action("Preferences: Select Theme") {
                    out.push(MenuItem::Action {
                        name: "Select Theme…".into(),
                        action,
                        os_action: None,
                    });
                }
            }
            // Recent projects need the window; clearing them does not.
            Item::Recent { .. } => {
                if let Some(action) = action(super::model::CLEAR_RECENT) {
                    out.push(MenuItem::Action {
                        name: "Clear Recent Projects".into(),
                        action,
                        os_action: None,
                    });
                }
            }
        }
    }
    // No separator at either end or twice in a row.
    let mut tidy: Vec<MenuItem> = Vec::with_capacity(out.len());
    for item in out {
        let sep = matches!(item, MenuItem::Separator);
        if sep && tidy.last().is_none_or(|l| matches!(l, MenuItem::Separator)) {
            continue;
        }
        tidy.push(item);
    }
    if tidy
        .last()
        .is_some_and(|l| matches!(l, MenuItem::Separator))
    {
        tidy.pop();
    }
    tidy
}

/// The menus as text, for tests: `Menu > Item` per action (in
/// parentheses when it does not apply), `Menu > Sub >` for a submenu's
/// items.
pub fn describe(menus: &[Menu]) -> Vec<String> {
    use gpui::Action as _;
    fn walk(path: &str, items: &[MenuItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                MenuItem::Action { name, action, .. } => {
                    if action.name() == super::Unavailable.name() {
                        out.push(format!("{path} > ({name})"));
                    } else {
                        out.push(format!("{path} > {name}"));
                    }
                }
                MenuItem::Submenu(menu) => {
                    walk(&format!("{path} > {}", menu.name), &menu.items, out)
                }
                MenuItem::Separator => out.push(format!("{path} > -")),
                MenuItem::SystemMenu(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        walk(&menu.name, &menu.items, &mut out);
    }
    out
}
