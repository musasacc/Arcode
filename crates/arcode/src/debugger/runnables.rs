//! Run buttons: runnable items (mains, tests) of a file with the command
//! that runs each. Parsing runs on the background executor over a rope
//! snapshot, so the editor's own tree is not needed.

use std::path::{Path, PathBuf};

use arcode_buffer::Rope;
use arcode_config::RunOutput;
use arcode_project::{
    BuiltPrograms, FileRun, FileRunInput, ProgramOutput, RunCommand, Runnable,
    RunnableKind as ProjectKind, SystemTools,
};
use arcode_syntax::{Language, RunnableItem, RunnableKind, Syntax};

/// Where Run Current File puts the programs it compiles in the cache
/// (`run.output = "cache"`, or when a name next to the source is taken):
/// `run/` in the cache folder, one folder per source file.
pub fn file_build_dir() -> PathBuf {
    arcode_config::paths::cache_dir()
        .unwrap_or_else(|| std::env::temp_dir().join("arcode"))
        .join("run")
}

/// The record of the programs Arcode built next to their sources (see
/// [`arcode_project::BuiltPrograms`]).
pub fn built_record() -> PathBuf {
    file_build_dir().join("built-programs")
}

/// The command that runs `file` on its own with the program placed as
/// `output` says, and a note when it went to the cache instead. Blocking.
pub fn single_file_run(
    file: &Path,
    language_id: &str,
    project_root: Option<&Path>,
    output: RunOutput,
) -> Result<FileRun, String> {
    let cache = file_build_dir();
    let built;
    let output = match output {
        RunOutput::Cache => ProgramOutput::Cache(&cache),
        RunOutput::NextToSource => {
            built = BuiltPrograms::load(&built_record());
            ProgramOutput::NextToSource {
                cache: &cache,
                built: &built,
            }
        }
    };
    let input = FileRunInput {
        file,
        language_id,
        project_root,
        output,
    };
    arcode_project::file_run(&input, &SystemTools)
}

/// The program a compiled single-file command writes next to its source
/// (in the folder its compile step runs in), which the record must hold.
/// `None` for other commands and programs in the cache.
pub fn program_next_to_source(command: &RunCommand) -> Option<PathBuf> {
    let compile = command.compile.as_deref()?;
    let program = arcode_project::compiled_program(command)?;
    (program.parent() == Some(compile.cwd.as_path())).then_some(program)
}

/// The source file a compiled single-file command builds (its compile
/// step's argument with a runnable extension, in the step's folder).
pub fn compiled_source(command: &RunCommand) -> Option<PathBuf> {
    compile_step_source(command.compile.as_deref()?)
}

/// The source file the compile step `compile` builds.
pub fn compile_step_source(compile: &RunCommand) -> Option<PathBuf> {
    compile
        .args
        .iter()
        .map(Path::new)
        .find(|arg| arcode_project::FileLanguage::from_path(arg).is_some())
        .map(|arg| compile.cwd.join(arg))
}

/// The runnable items of `text` (a file in the project at `root`) with
/// their commands. Empty for languages without runnables. A main of a
/// loose file is built as `output` says. Blocking.
pub fn find_runnables(
    language: &'static Language,
    text: &Rope,
    file: &Path,
    root: &Path,
    output: RunOutput,
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
        let kind = project_kind(item.kind);
        let runnable = Runnable::new(language.id, file, root, item.line, kind, &name)
            .or_else(|| file_main(language.id, file, root, item.line, kind, output));
        let Some(runnable) = runnable else {
            continue;
        };
        // One button per line (a `describe` that is also a test, ...).
        if out.iter().all(|r| r.line != runnable.line) {
            out.push(runnable);
        }
    }
    out
}

/// A main function the project has no command for (a loose `test.rs`
/// without `Cargo.toml`): the file is run on its own.
fn file_main(
    language_id: &str,
    file: &Path,
    root: &Path,
    line: usize,
    kind: ProjectKind,
    output: RunOutput,
) -> Option<Runnable> {
    if kind != ProjectKind::Main {
        return None;
    }
    let command = single_file_run(file, language_id, Some(root), output)
        .ok()?
        .command;
    Some(Runnable {
        line,
        kind,
        name: "main".to_string(),
        command,
    })
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
        let found = find_runnables(lang, &Rope::from_str(text), &file, root, RunOutput::Cache);
        let lines: Vec<usize> = found.iter().map(|r| r.line).collect();
        assert_eq!(lines, [0, 3, 5], "{found:#?}");
        assert_eq!(found[0].command.args[0], "run");
        let test = &found[2].command;
        assert_eq!(test.args[0], "test");
        assert!(test.args.contains(&"tests::adds".to_string()), "{test:?}");
        assert!(test.args.contains(&"--exact".to_string()), "{test:?}");
    }

    #[test]
    fn a_loose_rust_main_runs_the_file() {
        if arcode_project::packs::find_executable("rustc").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let text = "fn main() {\n    println!(\"hi\");\n}\n";
        let file = root.join("test.rs");
        std::fs::write(&file, text).unwrap();
        let lang = arcode_syntax::language_by_id("rust").unwrap();
        let found = find_runnables(lang, &Rope::from_str(text), &file, root, RunOutput::Cache);
        assert_eq!(found.len(), 1, "{found:#?}");
        let command = &found[0].command;
        assert_eq!(command.label, "run test.rs");
        let compile = command.compile.as_deref().expect("compiled first");
        assert_eq!(compile.program, "rustc");
        assert!(!command.program.starts_with(&*root.to_string_lossy()));
        // The gutter's Debug builds it with the same compile step.
        assert!(super::super::launch::LaunchChoice::for_command(command).is_some());
    }

    #[test]
    fn a_loose_main_builds_next_to_its_file() {
        if arcode_project::packs::find_executable("rustc").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let text = "fn main() {}\n";
        let file = root.join("tools/test.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        let lang = arcode_syntax::language_by_id("rust").unwrap();
        let found = find_runnables(
            lang,
            &Rope::from_str(text),
            &file,
            &root,
            RunOutput::NextToSource,
        );
        let command = &found[0].command;
        let program = root
            .join("tools")
            .join(format!("test{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(command.cwd, root.join("tools"));
        assert_eq!(program_next_to_source(command), Some(program));
        assert_eq!(compiled_source(command), Some(file.clone()));

        // The cache choice: the program is not next to the file.
        let cached = single_file_run(&file, "rust", Some(&root), RunOutput::Cache).unwrap();
        assert_eq!(program_next_to_source(&cached.command), None);
        assert_eq!(compiled_source(&cached.command), Some(file));
    }

    #[test]
    fn plain_text_has_none() {
        let lang = arcode_syntax::plain_text();
        let found = find_runnables(
            lang,
            &Rope::from_str("main"),
            Path::new("/x"),
            Path::new("/"),
            RunOutput::NextToSource,
        );
        assert!(found.is_empty());
    }
}
