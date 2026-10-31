use std::path::{Path, PathBuf};

use super::*;
use crate::AppState;

fn rich() -> ProjectSession {
    let mut files = BTreeMap::new();
    files.insert(
        "/p/src/main.rs".to_owned(),
        FileSession {
            selections: vec![
                SavedSelection {
                    anchor: (3, 0),
                    head: (3, 4),
                },
                SavedSelection::cursor(9, 2),
            ],
            scroll_line: 120,
            seen: 1_700_000_000,
        },
    );
    ProjectSession {
        panes: vec![
            PaneSession {
                files: vec!["/p/src/main.rs".into(), "/p/README.md".into()],
                active: 1,
            },
            PaneSession {
                files: vec!["/p/Cargo.toml".into()],
                active: 0,
            },
        ],
        active_pane: 1,
        files,
        layout: Some(LayoutSession {
            sidebar_visible: false,
            sidebar_width: 310.,
            sidebar_view: "git".into(),
            panel_visible: true,
            panel_height: 180.,
            panel_tab: "problems".into(),
        }),
        expanded_dirs: vec!["/p/src".into()],
        window: Some(WindowState {
            x: 5.,
            y: 6.,
            width: 900.,
            height: 700.,
            maximized: false,
        }),
        ..Default::default()
    }
}

#[test]
fn rich_session_round_trips_through_the_state_file() {
    let mut state = AppState::default();
    state.touch_recent_at(Path::new("/p"), 1);
    state.set_session(Path::new("/p"), rich());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.toml");
    state.save_to(&path).unwrap();
    let loaded = AppState::load_from(&path).unwrap();
    assert_eq!(loaded.session(Path::new("/p")), Some(&rich()));
    assert_eq!(loaded, state);
}

#[test]
fn old_sessions_become_one_pane() {
    let mut old = ProjectSession {
        open_files: vec!["/p/a.rs".into(), "/p/b.rs".into()],
        active_file: Some("/p/b.rs".into()),
        ..Default::default()
    };
    old.cursors.insert("/p/a.rs".into(), (40, 3));
    let panes = old.pane_layout();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].files, [PathBuf::from("/p/a.rs"), "/p/b.rs".into()]);
    assert_eq!(panes[0].active, 1);
    let a = old.file(Path::new("/p/a.rs")).unwrap();
    assert_eq!(a.selections, [SavedSelection::cursor(40, 3)]);
    assert_eq!(a.scroll_line, 30);
    assert_eq!(old.file(Path::new("/p/b.rs")), None);

    // Parsing a file written before panes existed.
    let text = "[sessions.\"/p\"]\nopen_files = [\"/p/a.rs\"]\n\n[sessions.\"/p\".cursors]\n\"/p/a.rs\" = [2, 1]\n";
    let state: AppState = toml::from_str(text).unwrap();
    let session = state.session(Path::new("/p")).unwrap();
    assert_eq!(session.pane_layout()[0].files, [PathBuf::from("/p/a.rs")]);
    assert_eq!(
        session.file(Path::new("/p/a.rs")).unwrap().selections[0],
        SavedSelection::cursor(2, 1)
    );
}

#[test]
fn pane_layout_drops_empty_panes_and_clamps() {
    let session = ProjectSession {
        panes: vec![
            PaneSession::default(),
            PaneSession {
                files: vec!["/p/a".into()],
                active: 7,
            },
        ],
        ..Default::default()
    };
    let panes = session.pane_layout();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].active, 0);
}

#[test]
fn compact_keeps_open_files_and_bounds_the_rest() {
    let mut session = rich();
    session.open_files = vec!["/old".into()];
    for i in 0..(MAX_FILE_STATES + 50) {
        session
            .files
            .insert(format!("/p/f{i:04}"), FileSession::default());
    }
    session.expanded_dirs = (0..MAX_EXPANDED_DIRS + 3)
        .map(|i| PathBuf::from(format!("/p/d{i}")))
        .collect();
    session.compact();
    assert!(session.open_files.is_empty());
    assert_eq!(session.files.len(), MAX_FILE_STATES);
    assert!(session.files.contains_key("/p/src/main.rs"));
    assert_eq!(session.expanded_dirs.len(), MAX_EXPANDED_DIRS);
}

#[test]
fn compact_drops_the_least_recently_seen_files_first() {
    let mut session = ProjectSession::default();
    for i in 0..(MAX_FILE_STATES + 2) {
        let seen = if i < 2 { 1_000 } else { i as u64 + 2_000 };
        session.files.insert(
            format!("/p/f{i:04}"),
            FileSession {
                seen,
                ..Default::default()
            },
        );
    }
    // The two alphabetically last files are the oldest.
    session.files.get_mut("/p/f0201").unwrap().seen = 1;
    session.files.get_mut("/p/f0200").unwrap().seen = 2;
    session.compact();
    assert_eq!(session.files.len(), MAX_FILE_STATES);
    assert!(!session.files.contains_key("/p/f0201"));
    assert!(!session.files.contains_key("/p/f0200"));
    assert!(session.files.contains_key("/p/f0000"));
}

#[test]
fn sessions_are_capped_keeping_known_projects() {
    let mut state = AppState::default();
    state.touch_recent_at(Path::new("/recent"), 1);
    state.add_open_project(Path::new("/open"));
    state.add_open_project(Path::new("/open"));
    assert_eq!(state.open_projects.len(), 1);
    state.set_session(Path::new("/recent"), rich());
    state.set_session(Path::new("/open"), rich());
    for i in 0..crate::state::MAX_SESSIONS + 5 {
        state.set_session(
            &PathBuf::from(format!("/x{i:03}")),
            ProjectSession::default(),
        );
    }
    assert_eq!(state.sessions.len(), crate::state::MAX_SESSIONS);
    assert!(state.session(Path::new("/recent")).is_some());
    assert!(state.session(Path::new("/open")).is_some());
    let last = format!("/x{:03}", crate::state::MAX_SESSIONS + 4);
    assert!(state.session(Path::new(&last)).is_some());
    state.remove_open_project(Path::new("/open"));
    assert!(state.open_projects.is_empty());
}

#[test]
fn breakpoints_round_trip_and_default_to_enabled() {
    let session = ProjectSession {
        breakpoints: vec![super::SavedBreakpoint {
            path: "/p/src/main.rs".into(),
            line: 4,
            condition: Some("x > 1".into()),
            ..Default::default()
        }],
        run_command: Some("cargo run --release".into()),
        ..Default::default()
    };
    let text = toml::to_string(&session).unwrap();
    let back: ProjectSession = toml::from_str(&text).unwrap();
    assert_eq!(back, session);
    let old: ProjectSession =
        toml::from_str("[[breakpoints]]\npath = \"/p/a.rs\"\nline = 2\n").unwrap();
    assert!(old.breakpoints[0].enabled);
    assert_eq!(old.run_command, None);
}
