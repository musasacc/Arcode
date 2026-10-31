//! Run buttons: runnable items (mains, tests) of a file with the command
//! that runs each. Parsing runs on the background executor over a rope
//! snapshot, so the editor's own tree is not needed.

use std::path::Path;

use arcode_buffer::Rope;
use arcode_project::{Runnable, RunnableKind as ProjectKind};
use arcode_syntax::{Language, RunnableItem, RunnableKind, Syntax};

/// The runnable items of `text` (a file in the project at `root`) with
/// their commands. Empty for languages without runnables. Blocking.
pub fn find_runnables(
    language: &'static Language,
    text: &Rope,
    file: &Path,
    root: &Path,
) -> Vec<Runnable> {
    if !arcode_project::RUNNABLE_LANGUAGES.contains(&language.id) {
        return Vec::new();
    }
    let Some(syntax) = Syntax::new(language, text) else {
        return Vec::new();
    };
    let mut out: Vec<Runnable> = Vec::new();
    for item in syntax.runnables(text) {
        let name = item_name(language.id, &item);
        let Some(runnable) = Runnable::new(
            language.id,
            file,
            root,
            item.line,
            project_kind(item.kind),
            &name,
        ) else {
            continue;
        };
        // One button per line (a `describe` that is also a test, ...).
        if out.iter().all(|r| r.line != runnable.line) {
            out.push(runnable);
        }
    }
    out
}

fn project_kind(kind: RunnableKind) -> ProjectKind {
    match kind {
        RunnableKind::Main => ProjectKind::Main,
        RunnableKind::Test => ProjectKind::Test,
        RunnableKind::TestModule => ProjectKind::TestModule,
        RunnableKind::Bench => ProjectKind::Benchmark,
    }
}

/// The name `command_for_runnable` expects for each language.
fn item_name(language_id: &str, item: &RunnableItem) -> String {
    match language_id {
        "rust" | "python" => item.qualified_name("::"),
        "javascript" | "typescript" | "javascriptreact" | "typescriptreact" => {
            item.qualified_name(" ")
        }
        _ => item.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_mains_and_tests_get_cargo_commands() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let text = "fn main() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {}\n}\n";
        let file = root.join("src/main.rs");
        std::fs::write(&file, text).unwrap();
        let lang = arcode_syntax::language_by_id("rust").unwrap();
        let found = find_runnables(lang, &Rope::from_str(text), &file, root);
        let lines: Vec<usize> = found.iter().map(|r| r.line).collect();
        assert_eq!(lines, [0, 3, 5], "{found:#?}");
        assert_eq!(found[0].command.args[0], "run");
        let test = &found[2].command;
        assert_eq!(test.args[0], "test");
        assert!(test.args.contains(&"tests::adds".to_string()), "{test:?}");
        assert!(test.args.contains(&"--exact".to_string()), "{test:?}");
    }

    #[test]
    fn plain_text_has_none() {
        let lang = arcode_syntax::plain_text();
        let found = find_runnables(
            lang,
            &Rope::from_str("main"),
            Path::new("/x"),
            Path::new("/"),
        );
        assert!(found.is_empty());
    }
}
