//! The menu bar in a window: opening with the mouse and the keyboard,
//! moving between menus, running items where focus was, the disabled
//! debug items and collapsing into the menu button.

use gpui::{
    Entity, Focusable, Modifiers, Point, TestAppContext, VisualTestContext, Window, point, px, size,
};

use super::Workspace;
use super::tests::open;
use crate::menu_bar::MenuBar;

fn wide(cx: &mut VisualTestContext) {
    cx.simulate_resize(size(px(1600.), px(900.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

fn bar(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<MenuBar> {
    ws.read_with(cx, |ws, _| ws.menu_bar().clone())
}

/// The open popups as text (see `OpenMenu::describe`), or `None`.
fn shown(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<Vec<Vec<String>>> {
    let bar = bar(ws, cx);
    bar.read_with(cx, |bar, _| bar.open_menu().map(|o| o.describe()))
}

fn title(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<usize> {
    let bar = bar(ws, cx);
    bar.read_with(cx, |bar, _| bar.open_menu().map(|o| o.title))
}

fn center(cx: &mut VisualTestContext, selector: &'static str) -> Point<gpui::Pixels> {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center()
}

fn editor_focused(ws: &Entity<Workspace>, window: &Window, cx: &gpui::App) -> bool {
    ws.read(cx)
        .active_pane()
        .read(cx)
        .focus_handle(cx)
        .contains_focused(window, cx)
}

#[gpui::test]
fn click_opens_hover_switches_escape_closes(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    let file = center(cx, "menu-title-0");
    cx.simulate_click(file, Modifiers::none());
    cx.run_until_parked();
    let menus = shown(&ws, cx).expect("File is open");
    assert_eq!(menus.len(), 1);
    let ctrl = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    assert!(
        menus[0].contains(&format!("Open Folder…\t{ctrl}+O")),
        "bindings are shown: {menus:#?}"
    );
    assert!(
        menus[0].iter().any(|r| r == "(Open Recent) >"),
        "no recent projects yet: {menus:#?}"
    );
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("menu-popup-0").is_some(),
        "the popup is drawn"
    );

    // Moving over another title opens that menu.
    let edit = center(cx, "menu-title-1");
    cx.simulate_mouse_move(edit, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(title(&ws, cx), Some(1));
    let go = center(cx, "menu-title-4");
    cx.simulate_mouse_move(go, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(title(&ws, cx), Some(4));

    // A click on the open title closes it.
    cx.simulate_click(go, Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none());

    // Escape closes too, and a click outside.
    cx.simulate_click(file, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    assert!(shown(&ws, cx).is_none());
    cx.simulate_click(file, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(point(px(800.), px(500.)), Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none());
}

#[gpui::test]
fn alt_letters_and_arrows_navigate(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    cx.simulate_keystrokes("alt-f");
    assert_eq!(title(&ws, cx), Some(0));
    let menus = shown(&ws, cx).unwrap();
    assert!(menus[0][0].starts_with("> New Project"), "{menus:#?}");
    cx.simulate_keystrokes("right");
    assert_eq!(title(&ws, cx), Some(1));
    cx.simulate_keystrokes("left left");
    assert_eq!(
        title(&ws, cx),
        Some(crate::menu_bar::model::MENUS.len() - 1)
    );
    // Alt+letter while open switches menus.
    cx.simulate_keystrokes("alt-v");
    assert_eq!(title(&ws, cx), Some(3));
    cx.simulate_keystrokes("down down");
    let menus = shown(&ws, cx).unwrap();
    assert!(menus[0][3].starts_with("> Toggle Panel"), "{menus:#?}");
    cx.simulate_keystrokes("escape");
    assert!(shown(&ws, cx).is_none());

    // File > Preferences is a submenu: its mnemonic opens it, Left closes.
    cx.simulate_keystrokes("alt-f e");
    let menus = shown(&ws, cx).unwrap();
    assert_eq!(menus.len(), 2);
    assert!(menus[1][0].starts_with("> Settings"), "{menus:#?}");
    cx.simulate_keystrokes("left");
    assert_eq!(shown(&ws, cx).unwrap().len(), 1);
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn a_tap_on_alt_activates_the_bar(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    let bar_entity = bar(&ws, cx);
    let bar_only = bar_entity.read_with(cx, |b, _| b.open_menu().map(|o| o.bar_only));
    assert_eq!(bar_only, Some(true), "the bar is active, no menu open");
    cx.simulate_keystrokes("right enter");
    assert_eq!(title(&ws, cx), Some(1));
    assert_eq!(shown(&ws, cx).unwrap().len(), 1);
    // Another tap closes it.
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none());
    // Alt held for a shortcut is not a tap.
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_keystrokes("alt-]");
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none());
}

#[gpui::test]
fn items_run_where_focus_was(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    crate::test_support::settle(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("a.txt"), window, cx)
    });
    crate::test_support::settle(cx);
    let editor = ws.read_with(cx, |ws, cx| ws.active_editor(cx).expect("a.txt is open"));
    ws.update_in(cx, |ws, window, cx| {
        ws.active_pane().read(cx).focus(window, cx)
    });
    let selection =
        |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.selections().primary().range());
    assert_eq!(selection(cx), 0..0);

    // Selection > Select All by its mnemonics: the editor command runs in
    // the editor that had focus, which has it again afterwards.
    cx.simulate_keystrokes("alt-s a");
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none(), "picking closes the menu");
    assert_eq!(selection(cx), 0..12);
    assert!(
        cx.update(|window, cx| editor_focused(&ws, window, cx)),
        "focus is back in the editor"
    );

    // The mouse runs items too: View > Toggle Sidebar.
    let sidebar = ws.read_with(cx, |ws, _| ws.sidebar_visible);
    let view = center(cx, "menu-title-3");
    cx.simulate_click(view, Modifiers::none());
    cx.run_until_parked();
    let menus = shown(&ws, cx).unwrap();
    let row = menus[0]
        .iter()
        .position(|r| r.starts_with("Toggle Sidebar"))
        .expect("Toggle Sidebar is listed");
    let selector: &'static str = Box::leak(format!("menu-row-0-{row}").into_boxed_str());
    let at = center(cx, selector);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none());
    assert_eq!(ws.read_with(cx, |ws, _| ws.sidebar_visible), !sidebar);

    // Editor commands are disabled when focus is not in an editor.
    ws.update_in(cx, |ws, window, _| window.focus(&ws.focus_handle));
    cx.simulate_keystrokes("alt-e");
    let edit = &shown(&ws, cx).unwrap()[0];
    assert!(edit.iter().any(|r| r.starts_with("(Undo)")), "{edit:#?}");
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn debug_items_are_disabled_without_a_session(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    cx.simulate_keystrokes("alt-r");
    let run = &shown(&ws, cx).unwrap()[0];
    for disabled in ["(Step Over)", "(Pause)", "(Stop Debugging)", "(Continue)"] {
        assert!(
            run.iter().any(|r| r.starts_with(disabled)),
            "{disabled} in {run:#?}"
        );
    }
    assert!(run.iter().any(|r| r == "(Step Over)\tF10"), "{run:#?}");
    // Down never stops on a disabled row.
    for _ in 0..30 {
        cx.simulate_keystrokes("down");
        let rows = &shown(&ws, cx).unwrap()[0];
        let sel = rows.iter().find(|r| r.starts_with("> ")).unwrap();
        assert!(!sel.starts_with("> ("), "{sel}");
    }
    cx.simulate_keystrokes("escape");
    // Without a project, Close Project is disabled too.
    cx.simulate_keystrokes("alt-f");
    let file = &shown(&ws, cx).unwrap()[0];
    assert!(file.iter().any(|r| r == "(Close Project)"), "{file:#?}");
}

#[gpui::test]
fn recent_projects_are_a_submenu(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    let dir = tempfile::tempdir().unwrap();
    cx.update(|_, cx| crate::app_state::touch_recent(dir.path(), cx));
    cx.simulate_keystrokes("alt-f r");
    let menus = shown(&ws, cx).unwrap();
    assert_eq!(menus.len(), 2, "{menus:#?}");
    let name = arcode_config::state::project_name(dir.path());
    assert_eq!(
        menus[1],
        [
            format!("> {name}"),
            "-".into(),
            "Clear Recent Projects".into()
        ]
    );
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("menu-popup-1").is_some(),
        "the submenu is drawn"
    );
    let parent = cx.debug_bounds("menu-popup-0").unwrap();
    let sub = cx.debug_bounds("menu-popup-1").unwrap();
    assert!(sub.left() >= parent.right() - px(8.), "{parent:?} {sub:?}");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let root = ws.read_with(cx, |ws, _| ws.project_root().map(|p| p.to_path_buf()));
    assert_eq!(
        root.map(|r| std::fs::canonicalize(r).unwrap()),
        Some(std::fs::canonicalize(dir.path()).unwrap())
    );
}

#[gpui::test]
fn narrow_windows_and_the_setting_collapse_the_menus(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    let collapsed = |cx: &mut VisualTestContext| bar(&ws, cx).read_with(cx, |b, _| b.collapsed());
    assert!(!collapsed(cx));
    assert!(cx.debug_bounds("menu-title-0").is_some());
    // The titles sit right after the logo, before the project chip.
    let file = cx.debug_bounds("menu-title-0").unwrap();
    assert!(file.left() >= arcode_ui_kit::metrics::RAIL_W, "{file:?}");

    cx.simulate_resize(size(px(900.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(collapsed(cx), "900 px is too narrow for every title");
    let button = center(cx, "menu-button");
    cx.simulate_click(button, Modifiers::none());
    cx.run_until_parked();
    let menus = shown(&ws, cx).unwrap();
    assert_eq!(menus[0][0], "File >", "the button lists the menus");
    // Alt+F opens File inside the list.
    cx.simulate_keystrokes("escape alt-f");
    let menus = shown(&ws, cx).unwrap();
    assert_eq!(menus.len(), 2);
    assert_eq!(menus[0][0], "> File >");
    cx.simulate_keystrokes("escape escape");
    assert!(shown(&ws, cx).is_none());

    // ui.menu_bar = false collapses a wide window too.
    wide(cx);
    assert!(!collapsed(cx));
    cx.update(|_, cx| {
        let mut config = arcode_ui_kit::settings(cx).clone();
        config.ui.menu_bar = false;
        arcode_ui_kit::set_settings(config, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(collapsed(cx), "the setting collapses a wide window too");
}

/// A project with `a.txt` open in the editor, which has focus.
fn with_file(
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> (tempfile::TempDir, Entity<arcode_editor::Editor>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    crate::test_support::settle(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("a.txt"), window, cx)
    });
    crate::test_support::settle(cx);
    let editor = ws.read_with(cx, |ws, cx| ws.active_editor(cx).expect("a.txt is open"));
    ws.update_in(cx, |ws, window, cx| {
        ws.active_pane().read(cx).focus(window, cx)
    });
    (dir, editor)
}

#[gpui::test]
fn alt_with_the_mouse_is_not_a_tap(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    let (_dir, editor) = with_file(&ws, cx);
    // Alt+click (add a cursor), then Alt up: the bar stays off and the
    // keys typed next reach the editor.
    let in_editor = point(px(800.), px(120.));
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_click(in_editor, Modifiers::alt());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none(), "Alt+click left the bar off");
    assert!(
        cx.update(|window, cx| editor_focused(&ws, window, cx)),
        "focus stays in the editor"
    );
    let before = editor.read_with(cx, |e, _| e.buffer().len_bytes());
    cx.simulate_keystrokes("z h e");
    let after = editor.read_with(cx, |e, _| e.buffer().len_bytes());
    assert!(after > before, "the letters were typed into the editor");

    // Alt+Shift+drag with Shift released first.
    let alt_shift = Modifiers {
        alt: true,
        shift: true,
        ..Modifiers::default()
    };
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_modifiers_change(alt_shift);
    cx.simulate_mouse_down(in_editor, gpui::MouseButton::Left, alt_shift);
    cx.simulate_mouse_up(in_editor, gpui::MouseButton::Left, alt_shift);
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none(), "a column selection is not a tap");

    // A plain tap still works afterwards.
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_some(), "a tap activates the bar");
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn alt_leaves_an_open_palette_alone(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    ws.update_in(cx, |ws, window, cx| ws.open_palette("> tog", window, cx));
    cx.run_until_parked();
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_modifiers_change(Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none(), "a tap on Alt");
    cx.simulate_keystrokes("alt-f");
    assert!(shown(&ws, cx).is_none(), "Alt+F");
    assert!(ws.read_with(cx, |ws, _| ws.palette.is_some()));
}

#[gpui::test]
fn submenus_are_drawn_over_their_menu(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    // Half a 1366 px screen: the menus fold into the button and File's
    // Preferences submenu has no room on either side.
    cx.simulate_resize(size(px(683.), px(700.)));
    cx.run_until_parked();
    cx.simulate_keystrokes("alt-f e");
    let menus = shown(&ws, cx).unwrap();
    assert_eq!(menus.len(), 3, "{menus:#?}");
    let row = menus[2]
        .iter()
        .position(|r| r.contains("Editor Font"))
        .unwrap();
    let selector: &'static str = Box::leak(format!("menu-row-2-{row}").into_boxed_str());
    let at = center(cx, selector);
    let sub = cx.debug_bounds("menu-popup-2").unwrap();
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(
        sub.left() >= px(0.) && sub.right() <= viewport.width,
        "{sub:?}"
    );
    // The mouse over the submenu's row selects it, whatever is under it.
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    let menus = shown(&ws, cx).unwrap();
    assert_eq!(menus.len(), 3, "the submenu stays open: {menus:#?}");
    assert!(
        menus[2][row].starts_with("> Editor Font"),
        "{:#?}",
        menus[2]
    );
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(shown(&ws, cx).is_none(), "the item ran");
}

#[gpui::test]
fn long_menus_scroll_in_short_windows(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    cx.simulate_resize(size(px(1440.), px(380.)));
    cx.run_until_parked();
    cx.simulate_keystrokes("alt-r");
    let menus = shown(&ws, cx).unwrap();
    let last = menus[0].len() - 1;
    let popup = center_bounds(cx, "menu-popup-0");
    assert!(
        popup.top() >= px(crate::title_bar::TITLE_BAR_HEIGHT - 2.),
        "the menu stays below the titles: {popup:?}"
    );
    assert!(popup.bottom() <= px(380.), "{popup:?}");
    // End selects the last row and scrolls it into view.
    cx.simulate_keystrokes("end");
    let menus = shown(&ws, cx).unwrap();
    assert!(menus[0][last].starts_with("> "), "{:#?}", menus[0]);
    let selector: &'static str = Box::leak(format!("menu-row-0-{last}").into_boxed_str());
    let row = center_bounds(cx, selector);
    assert!(
        row.top() >= popup.top() && row.bottom() <= popup.bottom(),
        "{row:?} in {popup:?}"
    );
    cx.simulate_keystrokes("escape");
}

fn center_bounds(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Bounds<gpui::Pixels> {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
}

#[gpui::test]
fn file_items_follow_the_active_editor(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    wide(cx);
    cx.simulate_keystrokes("alt-g");
    let go = shown(&ws, cx).unwrap().remove(0);
    for disabled in [
        "(Back)",
        "(Forward)",
        "(Go to Symbol in File…)",
        "(Go to Definition)",
        "(Find All References)",
    ] {
        assert!(
            go.iter().any(|r| r.starts_with(disabled)),
            "{disabled} in {go:#?}"
        );
    }
    cx.simulate_keystrokes("escape alt-f");
    let file = shown(&ws, cx).unwrap().remove(0);
    assert!(
        file.iter().any(|r| r.starts_with("(Reload from Disk)")),
        "{file:#?}"
    );
    cx.simulate_keystrokes("escape");

    // With a file open and focus on the workspace (not the editor), the
    // editor items apply and show the editor's key bindings.
    let (_dir, _editor) = with_file(&ws, cx);
    ws.update_in(cx, |ws, window, _| window.focus(&ws.focus_handle));
    cx.simulate_keystrokes("alt-g");
    let go = shown(&ws, cx).unwrap().remove(0);
    assert!(go.iter().any(|r| r == "Go to Definition\tF12"), "{go:#?}");
    cx.simulate_keystrokes("escape alt-f");
    let file = shown(&ws, cx).unwrap().remove(0);
    assert!(
        file.iter().any(|r| r.starts_with("Reload from Disk")),
        "{file:#?}"
    );
    cx.simulate_keystrokes("escape");
}
