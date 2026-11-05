use std::collections::HashSet;

use gpui::TestAppContext;

use super::model::{Facts, Item, MENUS, When, command_labels, menu_for_mnemonic, mnemonic};
use super::rows::{Resolved, Row, Sources, build, build_root};
use super::state::{NavKey, OpenMenu, Outcome};
use crate::commands::commands;

#[test]
fn mnemonics_parse() {
    assert_eq!(mnemonic("&File"), ("File".into(), Some((0, 'f'))));
    assert_eq!(mnemonic("G&it"), ("Git".into(), Some((1, 'i'))));
    assert_eq!(mnemonic("Save && Close"), ("Save & Close".into(), None));
    assert_eq!(mnemonic("Plain"), ("Plain".into(), None));
    assert_eq!(mnemonic("Trailing&"), ("Trailing".into(), None));
    // The offset is in bytes of the text without the marker.
    assert_eq!(mnemonic("Go…&x"), ("Go…x".into(), Some((5, 'x'))));
}

/// Checks `items` (and nested submenus): every entry that can be picked
/// has a mnemonic, and no two share one.
fn check_mnemonics(path: &str, items: &[Item]) {
    let mut seen = HashSet::new();
    for item in items {
        let label = match item {
            Item::Command { label, .. } | Item::Recent { label } | Item::Themes { label } => *label,
            Item::Submenu { label, items } => {
                check_mnemonics(&format!("{path} > {label}"), items);
                *label
            }
            Item::Separator => continue,
        };
        let (_, mark) = mnemonic(label);
        let (_, key) = mark.unwrap_or_else(|| panic!("{path} > {label} has no mnemonic"));
        assert!(
            seen.insert(key),
            "{path}: mnemonic {key:?} of {label:?} is used twice"
        );
    }
}

#[test]
fn every_menu_has_unique_mnemonics() {
    let mut titles = HashSet::new();
    for menu in MENUS {
        let (text, mark) = mnemonic(menu.title);
        let (_, key) = mark.expect("menu title has a mnemonic");
        assert!(titles.insert(key), "two menus use Alt+{key}");
        check_mnemonics(&text, menu.items);
    }
    assert_eq!(menu_for_mnemonic('f'), Some(0));
    assert_eq!(menu_for_mnemonic('h'), Some(MENUS.len() - 1));
    assert_eq!(menu_for_mnemonic('z'), None);
}

/// Alt+letter must not take a key the app already binds with Alt alone.
#[gpui::test]
fn title_mnemonics_do_not_shadow_alt_bindings(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for menu in MENUS {
            let (_, mark) = mnemonic(menu.title);
            let key = mark.expect("mnemonic").1;
            let stroke = format!("alt-{key}");
            let taken: Vec<String> = keymap
                .bindings()
                .filter(|b| {
                    b.keystrokes().len() == 1
                        && b.keystrokes()[0].inner().unparse() == stroke
                        && b.predicate().is_none_or(|p| {
                            // Bindings inside a find bar or search field
                            // win there, which is intended.
                            let p = format!("{p}");
                            p.contains("Workspace") || p.contains("Editor") || p.contains("Pane")
                        })
                })
                .map(|b| format!("{:?}", b.action().name()))
                .collect();
            assert!(taken.is_empty(), "Alt+{key} is bound to {taken:?}");
        }
    });
}

/// Every item names a registered command (so no item is dead), and the
/// rows built from the registry keep every item.
#[gpui::test]
fn every_item_runs_a_registered_command(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    cx.update(|cx| {
        let registered: HashSet<String> = commands(cx).iter().map(|c| c.label()).collect();
        let labels = command_labels();
        assert!(labels.len() > 100, "{}", labels.len());
        for (path, command) in &labels {
            assert!(
                registered.contains(*command),
                "{path} names {command:?}, which is not a registered command"
            );
        }
        let lookup = |label: &str| {
            commands(cx)
                .iter()
                .find(|c| c.label() == label)
                .map(|c| Resolved {
                    action: c.action(),
                    available: true,
                    keys: None,
                })
        };
        let src = Sources {
            command: &lookup,
            facts: Facts::default(),
            recent: &[],
            themes: &[],
        };
        let root = build_root(&src);
        assert_eq!(root.len(), MENUS.len());
        fn count(rows: &[Row]) -> usize {
            rows.iter()
                .map(|r| match &r.kind {
                    super::rows::RowKind::Run(_) => 1,
                    super::rows::RowKind::Sub(sub) => count(sub),
                    _ => 0,
                })
                .sum()
        }
        assert_eq!(count(&root), labels.len(), "every item became a row");
    });
}

#[test]
fn conditions_follow_the_facts() {
    let none = Facts::default();
    let project = Facts {
        project: true,
        ..none
    };
    let repo = Facts {
        repo: true,
        ..project
    };
    let running = Facts {
        debugging: true,
        ..project
    };
    let paused = Facts {
        paused: true,
        ..running
    };
    assert!(When::Always.holds(none));
    assert!(!When::Project.holds(none) && When::Project.holds(project));
    assert!(When::NoRepo.holds(project) && !When::NoRepo.holds(repo));
    assert!(
        !When::NoRepo.holds(none),
        "no project: nothing to initialize"
    );
    assert!(When::Repo.holds(repo) && !When::Repo.holds(project));
    assert!(When::NotDebugging.holds(project) && !When::NotDebugging.holds(running));
    assert!(When::Running.holds(running) && !When::Running.holds(paused));
    assert!(When::Paused.holds(paused) && !When::Paused.holds(running));
    assert!(When::Debugging.holds(paused) && !When::Debugging.holds(project));
    let untitled = Facts {
        editor: true,
        ..none
    };
    let file = Facts {
        file: true,
        ..untitled
    };
    assert!(!When::Editor.holds(none) && When::Editor.holds(untitled));
    assert!(!When::File.holds(untitled) && When::File.holds(file));
    let back = Facts { back: true, ..none };
    assert!(When::Back.holds(back) && !When::Forward.holds(back));
}

const DEMO_SUB: &[Item] = &[
    Item::Command {
        label: "&Inner",
        command: "inner",
        when: When::Always,
    },
    Item::Command {
        label: "&Second",
        command: "second",
        when: When::Always,
    },
];

const DEMO: &[Item] = &[
    Item::Separator,
    Item::Command {
        label: "&Alpha",
        command: "a",
        when: When::Always,
    },
    Item::Command {
        label: "&Beta",
        command: "b",
        when: When::Paused,
    },
    Item::Separator,
    Item::Separator,
    Item::Command {
        label: "&Missing",
        command: "missing",
        when: When::Always,
    },
    Item::Submenu {
        label: "S&ub",
        items: DEMO_SUB,
    },
    Item::Recent { label: "&Recent" },
    Item::Themes { label: "&Theme" },
    Item::Separator,
];

fn demo_rows() -> Vec<Row> {
    let lookup = |label: &str| {
        (label != "missing").then(|| Resolved {
            action: Box::new(crate::actions::ToggleSidebar),
            available: true,
            keys: (label == "a").then(|| "Ctrl+A".to_owned()),
        })
    };
    let themes = vec![
        ("system".to_owned(), "Follow System".to_owned(), false),
        ("Arcode Ember".to_owned(), "Arcode Ember".to_owned(), true),
    ];
    let src = Sources {
        command: &lookup,
        facts: Facts::default(),
        recent: &[],
        themes: &themes,
    };
    build(DEMO, &src)
}

#[test]
fn rows_resolve_commands_and_tidy_separators() {
    let rows = demo_rows();
    let shown: Vec<String> = rows.iter().map(Row::describe).collect();
    assert_eq!(
        shown,
        [
            "Alpha\tCtrl+A",
            "(Beta)",
            "-",
            "Sub >",
            "(Recent) >",
            "Theme >"
        ]
    );
    let themes: Vec<String> = rows[5]
        .children()
        .unwrap()
        .iter()
        .map(Row::describe)
        .collect();
    assert_eq!(themes, ["[ ] Follow System", "[x] Arcode Ember"]);
}

/// A bar whose every menu is the demo menu.
fn demo_menu(collapsed: bool) -> OpenMenu {
    let root: Vec<Row> = MENUS
        .iter()
        .map(|m| {
            let mut row = demo_rows().remove(3);
            let (text, mark) = mnemonic(m.title);
            row.label = text.into();
            row.mnemonic = mark;
            row.kind = super::rows::RowKind::Sub(demo_rows());
            row
        })
        .collect();
    OpenMenu::new(root, collapsed, 0, true)
}

#[test]
fn keys_move_through_rows_and_menus() {
    let mut m = demo_menu(false);
    assert_eq!(m.depth(), 1);
    assert_eq!(m.sel, [Some(0)], "keyboard open selects the first row");
    // Down skips the disabled row and the separator.
    assert_eq!(m.key(NavKey::Down), Outcome::Moved);
    assert_eq!(m.sel, [Some(3)]);
    // Into the submenu and back.
    m.key(NavKey::Right);
    assert_eq!(m.sel, [Some(3), Some(0)]);
    assert_eq!(m.describe()[1], ["> Inner", "Second"]);
    m.key(NavKey::Up);
    assert_eq!(m.sel, [Some(3), Some(1)], "wraps around");
    assert_eq!(m.key(NavKey::Enter), Outcome::Pick { level: 1, row: 1 });
    m.key(NavKey::Left);
    assert_eq!(m.sel, [Some(3)]);
    // Right on a plain row moves to the next menu; Left back.
    m.key(NavKey::Home);
    m.key(NavKey::Right);
    assert_eq!((m.title, m.sel.clone()), (1, vec![Some(0)]));
    m.key(NavKey::Left);
    m.key(NavKey::Left);
    assert_eq!(m.title, MENUS.len() - 1, "wraps to the last menu");
    // Escape closes a submenu first, then the menu.
    m.key(NavKey::End);
    m.key(NavKey::Enter);
    assert_eq!(m.depth(), 2);
    assert_eq!(m.key(NavKey::Escape), Outcome::Moved);
    assert_eq!(m.key(NavKey::Escape), Outcome::Close);
}

#[test]
fn letters_pick_by_mnemonic() {
    let mut m = demo_menu(false);
    assert_eq!(m.key(NavKey::Char('a')), Outcome::Pick { level: 0, row: 0 });
    // A disabled row's mnemonic does nothing.
    assert_eq!(m.key(NavKey::Char('b')), Outcome::Moved);
    // A submenu's mnemonic opens it.
    assert_eq!(m.key(NavKey::Char('u')), Outcome::Moved);
    assert_eq!(m.depth(), 2);
    assert_eq!(m.key(NavKey::Char('s')), Outcome::Pick { level: 1, row: 1 });
}

#[test]
fn the_bar_alone_waits_for_a_menu() {
    let mut m = demo_menu(false);
    m.bar_only = true;
    m.sel.clear();
    assert_eq!(m.depth(), 0);
    m.key(NavKey::Right);
    m.key(NavKey::Right);
    assert_eq!((m.title, m.depth()), (2, 0));
    m.key(NavKey::Down);
    assert_eq!((m.title, m.depth()), (2, 1));
    let mut m = demo_menu(false);
    m.bar_only = true;
    m.key(NavKey::Char('h'));
    assert_eq!(
        (m.title, m.depth(), m.bar_only),
        (MENUS.len() - 1, 1, false)
    );
    let mut m = demo_menu(false);
    m.bar_only = true;
    assert_eq!(m.key(NavKey::Escape), Outcome::Close);
}

#[test]
fn hover_opens_and_closes_submenus() {
    let mut m = demo_menu(false);
    m.hover(0, 3);
    assert_eq!(
        m.sel,
        [Some(3), None],
        "a submenu opens, nothing selected in it"
    );
    m.hover(1, 0);
    assert_eq!(m.sel, [Some(3), Some(0)]);
    // Back over the submenu row keeps the submenu.
    m.hover(0, 3);
    assert_eq!(m.sel, [Some(3), Some(0)]);
    m.hover(0, 0);
    assert_eq!(m.sel, [Some(0)], "another row closes it");
    m.hover(0, 1);
    assert_eq!(m.sel, [None], "a disabled row is not selected");
    assert_eq!(m.click(0, 1), Outcome::Moved);
    assert_eq!(m.click(0, 0), Outcome::Pick { level: 0, row: 0 });
}

#[test]
fn collapsed_menus_nest_under_the_button() {
    let mut m = demo_menu(true);
    // Opened on menu 0 (Alt+F): its submenu is open under the list.
    assert_eq!(m.depth(), 2);
    assert_eq!(m.sel, [Some(0), Some(0)]);
    assert_eq!(m.describe()[0][0], "> File >");
    m.key(NavKey::Left);
    assert_eq!(m.sel, [Some(0)]);
    m.key(NavKey::Left);
    assert_eq!(m.sel, [Some(0)], "nothing left of the list");
    m.key(NavKey::Down);
    m.key(NavKey::Right);
    assert_eq!(m.sel, [Some(1), Some(0)]);
    let list = OpenMenu::list(demo_menu(true).root, false);
    assert_eq!((list.depth(), list.sel.clone()), (1, vec![None]));
}

#[gpui::test]
fn native_menus_mirror_the_bar(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    cx.update(|cx| {
        let menus = super::native::menus(Facts::default(), cx);
        let titles: Vec<&str> = menus.iter().map(|m| m.name.as_ref()).collect();
        assert_eq!(
            titles,
            [
                "Arcode",
                "File",
                "Edit",
                "Selection",
                "View",
                "Go",
                "Run",
                "Terminal",
                "Git",
                "Help"
            ]
        );
        let items = super::native::describe(&menus);
        for expected in [
            "Arcode > Settings…",
            "Arcode > Quit Arcode",
            "File > Open Folder…",
            "File > Preferences > Select Theme…",
            "View > Select Theme…",
            // Nothing to step without a debug session, no project to
            // close, no file to reload.
            "Run > (Step Over)",
            "File > (Close Project)",
            "File > (Reload from Disk)",
            "Git > (Commit)",
            "Run > Start Debugging",
        ] {
            assert!(
                items.iter().any(|i| i == expected),
                "{expected} in {items:#?}"
            );
        }
        assert!(
            !items.iter().any(|i| i == "File > Quit"),
            "Quit is in the app menu"
        );
        // While paused, the stepping items apply and Start Debugging not.
        let paused = Facts {
            project: true,
            debugging: true,
            paused: true,
            ..Facts::default()
        };
        let items = super::native::describe(&super::native::menus(paused, cx));
        for expected in [
            "Run > Step Over",
            "Run > Continue",
            "Run > (Pause)",
            "Run > (Start Debugging)",
            "File > Close Project",
        ] {
            assert!(
                items.iter().any(|i| i == expected),
                "{expected} in {items:#?}"
            );
        }
        for menu in &menus {
            let first = menu
                .items
                .first()
                .map(|i| matches!(i, gpui::MenuItem::Separator));
            let last = menu
                .items
                .last()
                .map(|i| matches!(i, gpui::MenuItem::Separator));
            assert_eq!((first, last), (Some(false), Some(false)), "{}", menu.name);
        }
    });
}
