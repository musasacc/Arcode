use super::*;

fn project(path: &str, last_opened: u64, pinned: bool) -> RecentProject {
    RecentProject {
        path: PathBuf::from(path),
        name: arcode_config::state::project_name(Path::new(path)),
        last_opened,
        pinned,
    }
}

#[test]
fn pinned_projects_come_first() {
    let recent = [
        project("/w/newest", 50, false),
        project("/w/pinned-new", 40, true),
        project("/w/middle", 30, false),
        project("/w/pinned-old", 20, true),
    ];
    let missing = HashSet::from([PathBuf::from("/w/middle")]);
    let rows = entries(&recent, &missing, Some(Path::new("/w")), 60);
    let names: Vec<_> = rows.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["pinned-new", "pinned-old", "newest", "middle"]);
    assert!(rows[0].pinned && rows[1].pinned && !rows[2].pinned);
    assert!(rows[3].missing && !rows[2].missing);
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(rows[2].shown, format!("~{sep}newest"));
    assert_eq!(rows[2].when, "just now");
}

#[test]
fn filter_matches_name_and_path_in_display_order() {
    let recent = [
        project("/home/ada/code/arcode", 9, false),
        project("/home/ada/work/billing-api", 8, true),
        project("/home/ada/code/notes", 7, false),
        project("/srv/arc-tools", 6, false),
    ];
    let rows = entries(&recent, &HashSet::new(), Some(Path::new("/home/ada")), 10);
    let names = |hits: Vec<usize>| -> Vec<String> {
        hits.into_iter().map(|i| rows[i].name.clone()).collect()
    };
    assert_eq!(filter(&rows, "").len(), 4);
    assert_eq!(filter(&rows, "   ").len(), 4);
    assert_eq!(names(filter(&rows, "arc")), ["arcode", "arc-tools"]);
    // Fuzzy, and the path counts too.
    assert_eq!(names(filter(&rows, "bapi")), ["billing-api"]);
    assert_eq!(names(filter(&rows, "work")), ["billing-api"]);
    assert_eq!(names(filter(&rows, "code")), ["arcode", "notes"]);
    assert!(filter(&rows, "zzz").is_empty());
}

#[test]
fn filter_ranks_names_and_ignores_letters_of_shared_folders() {
    let names = [
        "billing-api",
        "weather-cli",
        "zebra-tools",
        "ops-scripts",
        "client-web",
        "notes",
    ];
    let mut recent: Vec<RecentProject> = names
        .iter()
        .enumerate()
        .map(|(i, n)| project(&format!("/home/ada/projects/{n}"), 100 - i as u64, false))
        .collect();
    recent[0].pinned = true;
    let rows = entries(&recent, &HashSet::new(), Some(Path::new("/home/ada")), 200);
    let found = |query: &str| -> Vec<String> {
        filter(&rows, query)
            .into_iter()
            .map(|i| rows[i].name.clone())
            .collect()
    };
    // The letters of "~/projects" do not count.
    for query in ["ts", "ps", "rt", "ects"] {
        let hits = found(query);
        assert!(hits.len() < names.len(), "{query}: {hits:?}");
    }
    assert_eq!(found("ects"), Vec::<String>::new());
    // The best match comes first, even ahead of a pinned project.
    let cli = found("cli");
    assert_eq!(
        cli.first().map(String::as_str),
        Some("client-web"),
        "{cli:?}"
    );
    assert!(cli.contains(&"weather-cli".to_owned()));
    assert!(!cli.contains(&"billing-api".to_owned()), "{cli:?}");
    assert_eq!(found("wcli"), ["weather-cli"]);
    // A folder on the path, by the start of its name or written out.
    assert_eq!(found("proj").len(), names.len());
    assert_eq!(found("projects/zeb"), ["zebra-tools"]);
    assert_eq!(found("PROJ").len(), names.len());
}

#[test]
fn selection_steps_stop_at_the_ends() {
    assert_eq!(step(None, 0, 1), None);
    assert_eq!(step(None, 3, 1), Some(0));
    assert_eq!(step(None, 3, -1), Some(2));
    assert_eq!(step(Some(0), 3, -1), Some(0));
    assert_eq!(step(Some(1), 3, 1), Some(2));
    assert_eq!(step(Some(2), 3, 1), Some(2));
    // A selection past a shrunk list comes back inside it.
    assert_eq!(step(Some(9), 3, 0), Some(2));
}

#[test]
fn times_read_like_a_person_would_say_them() {
    let now = 1_000_000_000;
    let ago = |secs: u64| relative_time(now - secs, now);
    assert_eq!(ago(0), "just now");
    assert_eq!(ago(59), "just now");
    assert_eq!(ago(60), "1 minute ago");
    assert_eq!(ago(5 * 60 + 30), "5 minutes ago");
    assert_eq!(ago(3600), "1 hour ago");
    assert_eq!(ago(2 * 3600), "2 hours ago");
    assert_eq!(ago(30 * 3600), "yesterday");
    assert_eq!(ago(3 * 86400), "3 days ago");
    assert_eq!(ago(8 * 86400), "last week");
    assert_eq!(ago(21 * 86400), "3 weeks ago");
    assert_eq!(ago(45 * 86400), "last month");
    assert_eq!(ago(120 * 86400), "4 months ago");
    assert_eq!(ago(400 * 86400), "over a year ago");
    // A clock that went backwards, or a project never opened.
    assert_eq!(relative_time(now + 10, now), "just now");
    assert_eq!(relative_time(0, now), "over a year ago");
}

#[test]
fn paths_under_home_start_with_a_tilde() {
    let home = Path::new("/home/ada");
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        tilde_path(Path::new("/home/ada/code/demo"), Some(home)),
        format!("~{sep}code/demo")
    );
    assert_eq!(tilde_path(home, Some(home)), "~");
    assert_eq!(tilde_path(Path::new("/srv/demo"), Some(home)), "/srv/demo");
    assert_eq!(
        tilde_path(Path::new("/home/adam"), Some(home)),
        "/home/adam"
    );
    assert_eq!(tilde_path(Path::new("/x"), None), "/x");
}
