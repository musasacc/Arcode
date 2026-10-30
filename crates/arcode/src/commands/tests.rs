use super::*;
use crate::actions::{ShowSourceControl, ToggleFinder};

#[test]
fn labels_split_into_category_and_name() {
    assert_eq!(
        split_label("Git: Toggle Blame", "git::ToggleBlame"),
        ("Git".to_owned(), "Toggle Blame".to_owned())
    );
    assert_eq!(
        split_label("Go to Definition", "editor::GoToDefinition"),
        ("Editor".to_owned(), "Go to Definition".to_owned())
    );
    assert_eq!(split_label("Odd: ", "x::Y").0, "X");
}

#[gpui::test]
fn registry_dedupes_by_label_and_lists_defaults(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        let before = commands(cx).len();
        assert!(before > 40, "every app and editor command is registered");
        let labels: Vec<String> = commands(cx).iter().map(Command::label).collect();
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
        assert!(labels.iter().any(|l| l == "Search: Find in Files"));
        assert!(labels.iter().any(|l| l == "Editor: Undo"));
        register(cx, "Git: Show Source Control", Box::new(ShowSourceControl));
        assert_eq!(commands(cx).len(), before);
        register(cx, "Custom: Finder", Box::new(ToggleFinder));
        assert_eq!(commands(cx).len(), before + 1);
        let quit = commands(cx)
            .iter()
            .find(|c| c.name.as_ref() == "Quit")
            .expect("quit");
        assert!(quit.global);
    });
}
