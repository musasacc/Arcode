//! What the Discord presence is told: the view of the workspace, never a
//! path; and the palette command that turns it on and off.

use std::fs;

use arcode_presence::View;
use gpui::TestAppContext;

use super::tests::{open, project_dir};
use crate::prefs::ToggleDiscordPresence;
use crate::prefs::store::ConfigPathOverride;

#[gpui::test]
fn the_presence_follows_the_project_and_the_active_editor(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    fs::write(root.join("script.py"), "print(1)\n").expect("write");
    cx.update(crate::presence::init_for_tests);
    let (ws, cx) = open(cx);
    // The window is activated, or its first report counts.
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx)
    });
    cx.run_until_parked();
    let shown = cx.update(|_, cx| crate::presence::current(cx));
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(shown.view, View::Browsing);
    assert_eq!(shown.project.as_deref(), Some(name.as_str()));
    let started = shown.started.expect("the project's opening time");

    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("src/main.rs"), window, cx)
    });
    cx.run_until_parked();
    let shown = cx.update(|_, cx| crate::presence::current(cx));
    let View::Editing {
        file_name,
        language,
    } = &shown.view
    else {
        panic!("{shown:?}");
    };
    assert_eq!(file_name, "main.rs", "the name only, never the path");
    assert_eq!(
        (language.key.as_str(), language.name.as_str()),
        ("rust", "Rust")
    );
    assert_eq!(shown.started, Some(started), "the clock runs per project");

    ws.update_in(cx, |ws, window, cx| {
        ws.open_file(&root.join("script.py"), window, cx)
    });
    cx.run_until_parked();
    let shown = cx.update(|_, cx| crate::presence::current(cx));
    assert!(
        matches!(&shown.view, View::Editing { file_name, language }
            if file_name == "script.py" && language.key == "python"),
        "{shown:?}"
    );
    let text = format!("{shown:?}");
    assert!(!text.contains(&*root.to_string_lossy()), "{text}");

    ws.update_in(cx, |ws, window, cx| ws.close_project(window, cx));
    cx.run_until_parked();
    let shown = cx.update(|_, cx| crate::presence::current(cx));
    assert_eq!(shown.view, View::Choosing);
    assert_eq!((shown.project, shown.started), (None, None));
}

#[gpui::test]
fn the_palette_command_toggles_the_setting(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(&path, "[discord]\n# Mine.\nenabled = false\n").expect("write");
    cx.update(crate::presence::init_for_tests);
    let (ws, cx) = open(cx);
    let p = path.clone();
    cx.update(|_, cx| {
        cx.set_global(ConfigPathOverride(p));
        crate::prefs::store::reload(cx);
    });
    cx.run_until_parked();
    cx.dispatch_action(ToggleDiscordPresence);
    cx.run_until_parked();
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("# Mine.\nenabled = true\n"), "{text}");
    cx.update(|_, cx| assert!(arcode_ui_kit::settings(cx).discord.enabled));
    let toast = ws.read_with(cx, |ws, _| ws.toast.as_ref().map(|t| t.to_string()));
    if arcode_presence::DEFAULT_APPLICATION_ID.is_empty() {
        assert!(toast.unwrap_or_default().contains("application ID"));
    }
    cx.dispatch_action(ToggleDiscordPresence);
    cx.run_until_parked();
    let text = fs::read_to_string(&path).expect("read");
    assert!(text.contains("enabled = false"), "{text}");
}

#[gpui::test]
fn scrolling_over_an_editor_counts_as_input(cx: &mut TestAppContext) {
    let dir = project_dir();
    let root = fs::canonicalize(dir.path()).expect("canonical");
    cx.update(crate::presence::init_for_tests);
    let (ws, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_project(root.clone(), window, cx);
        ws.open_file(&root.join("src/main.rs"), window, cx);
    });
    cx.run_until_parked();
    // The editor handles the wheel and stops it; the presence still sees
    // it (capture phase).
    let size = cx.update(|window, _| window.viewport_size());
    let before = crate::presence::touches();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(size.width * 0.6, size.height * 0.5),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., -3.)),
        ..Default::default()
    });
    assert!(crate::presence::touches() > before, "scrolling is input");
}
