use super::*;

fn sample() -> AppState {
    let mut state = AppState {
        first_run_complete: true,
        open_projects: vec![PathBuf::from("/work/arcode")],
        window: Some(WindowState {
            x: 10.0,
            y: 20.5,
            width: 1280.0,
            height: 800.0,
            maximized: true,
        }),
        ..AppState::default()
    };
    state.touch_recent_at(Path::new("/work/other"), 100);
    state.touch_recent_at(Path::new("/work/arcode"), 200);
    let session = state.session_mut(Path::new("/work/arcode"));
    session.open_files = vec![
        "/work/arcode/src/main.rs".into(),
        "/work/arcode/README.md".into(),
    ];
    session.active_file = Some("/work/arcode/src/main.rs".into());
    session
        .cursors
        .insert("/work/arcode/src/main.rs".into(), (41, 7));
    state
}

#[test]
fn save_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep/state.toml");
    let state = sample();
    state.save_to(&path).unwrap();
    assert_eq!(AppState::load_from(&path).unwrap(), state);

    // Overwrite with a different state; no temp files left behind.
    AppState::default().save_to(&path).unwrap();
    assert_eq!(AppState::load_from(&path).unwrap(), AppState::default());
    let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
    assert_eq!(leftovers, 1);
}

#[test]
fn missing_file_is_default_and_corrupt_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.toml");
    assert_eq!(AppState::load_from(&path).unwrap(), AppState::default());
    std::fs::write(&path, "recent_projects = 5").unwrap();
    let err = AppState::load_from(&path).unwrap_err();
    assert!(format!("{err:#}").contains("state.toml"));
}

#[test]
fn partial_file_fills_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.toml");
    std::fs::write(&path, "first_run_complete = true\n[window]\nwidth = 900\n").unwrap();
    let state = AppState::load_from(&path).unwrap();
    assert!(state.first_run_complete);
    assert_eq!(state.window.unwrap().width, 900.0);
    assert!(state.recent_projects.is_empty());
}

#[test]
fn touch_recent_dedupes_and_orders() {
    let mut state = AppState::default();
    state.touch_recent_at(Path::new("/a/one"), 1);
    state.touch_recent_at(Path::new("/a/two"), 2);
    state.touch_recent_at(Path::new("/a/one/"), 3);
    let names: Vec<_> = state
        .recent_projects
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["one", "two"]);
    assert_eq!(state.recent_projects[0].last_opened, 3);

    state.touch_recent(Path::new("/a/two"));
    assert_eq!(state.recent_projects[0].name, "two");
    assert!(state.recent_projects[0].last_opened > 1_600_000_000);
}

#[test]
fn recent_list_is_capped() {
    let mut state = AppState::default();
    for i in 0..30 {
        state.touch_recent_at(&PathBuf::from(format!("/p/{i}")), i);
    }
    assert_eq!(state.recent_projects.len(), MAX_RECENT_PROJECTS);
    assert_eq!(state.recent_projects[0].name, "29");
    assert_eq!(state.recent_projects.last().unwrap().name, "10");
}

#[test]
fn remove_recent() {
    let mut state = sample();
    assert!(state.remove_recent(Path::new("/work/other")));
    assert!(!state.remove_recent(Path::new("/work/other")));
    assert_eq!(state.recent_projects.len(), 1);
}

#[test]
fn prune_missing_drops_vanished_projects() {
    let dir = tempfile::tempdir().unwrap();
    let alive = dir.path().join("alive");
    std::fs::create_dir(&alive).unwrap();
    let gone = dir.path().join("gone");

    let mut state = AppState::default();
    state.touch_recent_at(&alive, 1);
    state.touch_recent_at(&gone, 2);
    state.open_projects = vec![alive.clone(), gone.clone()];
    state.session_mut(&alive);
    state.session_mut(&gone);

    assert_eq!(state.prune_missing(), 1);
    assert_eq!(state.recent_projects.len(), 1);
    assert_eq!(state.recent_projects[0].path, alive);
    assert_eq!(state.open_projects, std::slice::from_ref(&alive));
    assert!(state.session(&alive).is_some());
    assert!(state.session(&gone).is_none());
}

#[test]
fn project_names() {
    assert_eq!(project_name(Path::new("/home/me/code")), "code");
    assert_eq!(project_name(Path::new("/")), "/");
}

#[test]
fn sessions_by_path() {
    let state = sample();
    let session = state.session(Path::new("/work/arcode")).unwrap();
    assert_eq!(session.cursors["/work/arcode/src/main.rs"], (41, 7));
    assert_eq!(session.open_files.len(), 2);
    assert!(state.session(Path::new("/work/other")).is_none());
}
