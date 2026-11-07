use super::*;

fn rust() -> Language {
    Language {
        key: "rust".into(),
        name: "Rust".into(),
    }
}

fn editing(file: &str, language: Language) -> Shown {
    Shown {
        view: View::Editing {
            file_name: file.into(),
            language,
        },
        project: Some("secret-project".into()),
        started: Some(1_700_000_000),
    }
}

fn settings(file: bool, project: bool, elapsed: bool) -> Settings {
    Settings {
        enabled: true,
        show_file_name: file,
        show_project_name: project,
        show_elapsed: elapsed,
        application_id: String::new(),
    }
}

#[test]
fn every_privacy_combination_shows_only_what_is_allowed() {
    let shown = editing("main.rs", rust());
    for file in [false, true] {
        for project in [false, true] {
            for elapsed in [false, true] {
                let a = activity(&shown, &settings(file, project, elapsed), false);
                let text = a.to_string();
                assert_eq!(text.contains("main.rs"), file, "{text}");
                assert_eq!(text.contains("secret-project"), project, "{text}");
                assert_eq!(a.get("timestamps").is_some(), elapsed, "{text}");
                assert!(!text.contains('/') && !text.contains('\\'), "{text}");
                let details = a["details"].as_str().unwrap();
                if file {
                    assert_eq!(details, "Editing main.rs");
                } else {
                    assert_eq!(details, "Editing a Rust file");
                }
                if project {
                    assert_eq!(a["state"], "In secret-project");
                } else {
                    assert!(a.get("state").is_none(), "{text}");
                }
                if elapsed {
                    assert_eq!(a["timestamps"]["start"], 1_700_000_000);
                }
                assert_eq!(a["assets"]["large_image"], "arcode");
                assert_eq!(a["assets"]["large_text"], "Arcode");
                assert_eq!(a["assets"]["small_image"], "rust");
                assert_eq!(a["assets"]["small_text"], "Rust");
            }
        }
    }
}

#[test]
fn a_path_is_never_sent() {
    let mut shown = editing("/home/me/work/secret/main.rs", rust());
    shown.project = Some("/home/me/work/secret-project".into());
    let a = activity(&shown, &settings(true, true, true), false);
    let text = a.to_string();
    assert_eq!(a["details"], "Editing main.rs");
    assert_eq!(a["state"], "In secret-project");
    assert!(!text.contains("home") && !text.contains("work"), "{text}");
    let windows = editing(r"C:\Users\me\src\lib.rs", rust());
    let a = activity(&windows, &settings(true, false, false), false);
    assert_eq!(a["details"], "Editing lib.rs");
    assert!(!a.to_string().contains("Users"));
    // A name that is all path: the language instead.
    let a = activity(
        &editing("dir/", rust()),
        &settings(true, false, false),
        false,
    );
    assert_eq!(a["details"], "Editing a Rust file");
}

#[test]
fn views_and_idle() {
    let s = settings(true, true, true);
    let choosing = activity(&Shown::default(), &s, false);
    assert_eq!(choosing["details"], "Choosing a project");
    assert!(choosing.get("state").is_none() && choosing.get("timestamps").is_none());
    let browsing = Shown {
        view: View::Browsing,
        project: Some("app".into()),
        started: Some(5),
    };
    let a = activity(&browsing, &s, false);
    assert_eq!(a["details"], "Browsing files");
    assert_eq!(a["state"], "In app");
    assert!(a["assets"].get("small_image").is_none());
    let idle = activity(&editing("main.rs", rust()), &s, true);
    assert_eq!(idle["details"], "Idle");
    assert!(!idle.to_string().contains("main.rs"));
    assert!(idle["assets"].get("small_image").is_none());
}

#[test]
fn articles_and_short_language_keys() {
    let lang = |key: &str, name: &str| Language {
        key: key.into(),
        name: name.into(),
    };
    let details = |l: Language| {
        activity(&editing("x", l), &settings(false, false, false), false)["details"].clone()
    };
    assert_eq!(details(lang("html", "HTML")), "Editing an HTML file");
    assert_eq!(
        details(lang("objc", "Objective-C")),
        "Editing an Objective-C file"
    );
    assert_eq!(details(lang("python", "Python")), "Editing a Python file");
    assert_eq!(
        details(lang("plaintext", "Plain Text")),
        "Editing a text file"
    );
    let c = activity(
        &editing("a.c", lang("c", "C")),
        &settings(false, false, false),
        false,
    );
    assert_eq!(c["assets"]["small_image"], "clang");
    assert_eq!(c["assets"]["small_text"], "C language");
    let odd = activity(
        &editing("a", lang("Type Script!", "TypeScript")),
        &settings(true, false, false),
        false,
    );
    assert_eq!(odd["assets"]["small_image"], "typescript");
    // "Editing a" is long enough; a one-character project is not.
    assert_eq!(odd["details"], "Editing a");
}

#[test]
fn long_text_is_cut_on_a_char_boundary() {
    assert_eq!(field_text(""), None);
    assert_eq!(field_text(" x "), None);
    assert_eq!(field_text("ab").as_deref(), Some("ab"));
    let exact = "é".repeat(MAX_FIELD_CHARS);
    assert_eq!(field_text(&exact).as_deref(), Some(exact.as_str()));
    let long = "é".repeat(300);
    let cut = field_text(&long).unwrap();
    assert_eq!(cut.chars().count(), MAX_FIELD_CHARS);
    assert!(cut.ends_with('…'));
    let emoji = "🦀".repeat(200);
    assert_eq!(field_text(&emoji).unwrap().chars().count(), MAX_FIELD_CHARS);
    let shown = editing(&format!("{}.rs", "a".repeat(400)), rust());
    let a = activity(&shown, &settings(true, false, false), false);
    assert_eq!(
        a["details"].as_str().unwrap().chars().count(),
        MAX_FIELD_CHARS
    );
    // Control characters never reach Discord.
    assert_eq!(field_text("a\nb\tc").as_deref(), Some("abc"));
}

#[test]
fn application_ids() {
    let mut s = Settings::default();
    assert_eq!(
        s.effective_application_id(),
        None.or(non_empty(crate::DEFAULT_APPLICATION_ID))
    );
    s.application_id = " 123456789012345678 ".into();
    assert_eq!(s.effective_application_id(), Some("123456789012345678"));
    assert_eq!(s.active_application_id(), None, "off");
    s.enabled = true;
    assert_eq!(s.active_application_id(), Some("123456789012345678"));
}

fn non_empty(id: &str) -> Option<&str> {
    (!id.is_empty()).then_some(id)
}
