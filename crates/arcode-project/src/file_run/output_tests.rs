//! Programs built next to their sources: names, the record of what Arcode
//! built, and the names it refuses to write over.

use std::fs;
use std::path::{Path, PathBuf};

use super::tests::{Dirs, Fake, dirs};
use super::*;

const EXE: &str = std::env::consts::EXE_SUFFIX;

impl Dirs {
    pub(super) fn record(&self) -> PathBuf {
        self.build.join("built-programs")
    }

    fn run_next(&self, file: &Path, language_id: &str, tools: &Fake) -> FileRun {
        let built = BuiltPrograms::load(&self.record());
        file_run(
            &FileRunInput {
                file,
                language_id,
                project_root: Some(&self.root),
                output: ProgramOutput::NextToSource {
                    cache: &self.build,
                    built: &built,
                },
            },
            tools,
        )
        .unwrap()
    }
}

/// Writes a file standing in for a program (executable on Unix).
pub(super) fn fake_program(path: &Path) {
    fs::write(path, "binary").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn programs_are_named_after_the_file_next_to_it() {
    let dir = Path::new("/p/src");
    assert_eq!(
        program_next_to(&dir.join("test.rs")),
        dir.join(format!("test{EXE}"))
    );
    assert_eq!(
        program_next_to(&dir.join("my tool.cpp")),
        dir.join(format!("my tool{EXE}"))
    );
    assert_eq!(
        program_next_to(&dir.join("main.c")),
        dir.join(format!("main{EXE}"))
    );
}

#[cfg(unix)]
#[test]
fn a_rust_file_is_built_next_to_it_and_run_from_its_folder() {
    let d = dirs();
    let file = d.write("tools/test.rs", "fn main() {}\n");
    let run = d.run_next(&file, "rust", &Fake::with(&["rustc"]));
    assert_eq!(run.note, None);
    let cmd = run.command;
    let folder = d.root.join("tools");
    assert_eq!(cmd.cwd, folder);
    assert_eq!(
        cmd.command_line(),
        "rustc --edition 2024 -g test.rs -o test && ./test"
    );
    assert_eq!(cmd.display_line(), "rustc --edition 2024 -g test.rs");
    assert_eq!(compiled_program(&cmd), Some(folder.join("test")));
    // Nothing to create: the folder is the source's.
    create_output_dir(&cmd).unwrap();
    assert!(!d.build.exists(), "the cache is not used");
    // Debug builds and starts the same program.
    let target = crate::debug_target(&cmd).expect("debuggable");
    assert_eq!(
        target.program.as_deref(),
        Some(folder.join("test").as_path())
    );
    assert_eq!(target.cwd, folder);
    assert_eq!(target.build.as_ref(), cmd.compile.as_deref());
}

#[cfg(unix)]
#[test]
fn c_and_cpp_are_built_next_to_them() {
    let d = dirs();
    let c = d.write("hello.c", "int main(void) { return 0; }\n");
    let cmd = d.run_next(&c, "c", &Fake::with(&["gcc"])).command;
    assert_eq!(cmd.command_line(), "gcc -g hello.c -o hello && ./hello");
    assert_eq!(cmd.cwd, d.root);
    let cpp = d.write("app/main.cpp", "int main() {}\n");
    let cmd = d.run_next(&cpp, "cpp", &Fake::with(&["clang++"])).command;
    assert_eq!(cmd.command_line(), "clang++ -g main.cpp -o main && ./main");
    assert_eq!(cmd.cwd, d.root.join("app"));
}

#[test]
fn interpreted_files_never_get_a_note() {
    let d = dirs();
    let file = d.write("test.py", "");
    // A folder named like the "program" does not matter to Python.
    fs::create_dir_all(d.root.join("test")).unwrap();
    let run = d.run_next(&file, "python", &Fake::with(&["python3"]));
    assert_eq!(run.note, None);
    assert_eq!(run.command.compile, None);
}

#[test]
fn a_folder_with_the_name_is_left_alone() {
    let d = dirs();
    let file = d.write("test.rs", "fn main() {}\n");
    fs::create_dir_all(d.root.join(format!("test{EXE}"))).unwrap();
    let run = d.run_next(&file, "rust", &Fake::with(&["rustc"]));
    let note = run.note.expect("a note");
    assert!(note.contains(&format!("test{EXE} is a folder")), "{note}");
    assert!(note.contains("cache"), "{note}");
    assert!(run.command.program.starts_with(&*d.build.to_string_lossy()));
}

#[cfg(unix)]
#[test]
fn a_file_that_is_not_a_program_is_left_alone() {
    let d = dirs();
    let file = d.write("notes.c", "int main(void) { return 0; }\n");
    let notes = d.write("notes", "my notes\n");
    let run = d.run_next(&file, "c", &Fake::with(&["cc"]));
    let note = run.note.expect("a note");
    assert_eq!(
        note,
        "Built notes.c in Arcode's cache: notes is not a program"
    );
    assert!(run.command.program.starts_with(&*d.build.to_string_lossy()));
    assert_eq!(fs::read_to_string(notes).unwrap(), "my notes\n");
}

#[test]
fn a_program_arcode_did_not_build_is_left_alone() {
    let d = dirs();
    let file = d.write("test.rs", "fn main() {}\n");
    let program = d.root.join(format!("test{EXE}"));
    fake_program(&program);
    let run = d.run_next(&file, "rust", &Fake::with(&["rustc"]));
    let note = run.note.expect("a note");
    assert!(note.contains("Arcode did not build test"), "{note}");
    assert_eq!(
        check_next_to(&program, &BuiltPrograms::default()),
        Err(NameTaken::NotBuiltByArcode)
    );
}

#[test]
fn a_program_arcode_built_is_built_again_in_place() {
    let d = dirs();
    let file = d.write("test.rs", "fn main() {}\n");
    let program = d.root.join(format!("test{EXE}"));
    // A build in a terminal starts, then writes the program.
    record_build_started(&d.record(), &program).unwrap();
    fake_program(&program);
    let run = d.run_next(&file, "rust", &Fake::with(&["rustc"]));
    assert_eq!(run.note, None);
    assert_eq!(compiled_program(&run.command), Some(program.clone()));

    // Debug Current File records the program it built.
    record_built(&d.record(), &program).unwrap();
    let built = BuiltPrograms::load(&d.record());
    assert!(built.holds(&program));
    assert_eq!(check_next_to(&program, &built), Ok(()));
}

#[test]
fn a_program_changed_after_the_build_is_not_arcodes_any_more() {
    let d = dirs();
    let program = d.root.join(format!("tool{EXE}"));
    fake_program(&program);
    record_built(&d.record(), &program).unwrap();
    assert!(BuiltPrograms::load(&d.record()).holds(&program));
    // Rewritten by something else: its time changes.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(&program)
        .unwrap()
        .set_modified(later)
        .unwrap();
    let built = BuiltPrograms::load(&d.record());
    assert!(!built.holds(&program));
    assert_eq!(
        check_next_to(&program, &built),
        Err(NameTaken::NotBuiltByArcode)
    );
    // A program older than the build that started is not the build's.
    let old = d.root.join(format!("old{EXE}"));
    fake_program(&old);
    let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(earlier)
        .unwrap();
    record_build_started(&d.record(), &old).unwrap();
    assert!(!BuiltPrograms::load(&d.record()).holds(&old));
}

#[test]
fn a_finished_terminal_build_is_recorded_exactly() {
    let d = dirs();
    let program = d.root.join(format!("test{EXE}"));
    record_build_started(&d.record(), &program).unwrap();
    // Not built yet: nothing to record.
    assert!(!finish_started_build(&d.record(), &program).unwrap());
    fake_program(&program);
    assert!(finish_started_build(&d.record(), &program).unwrap());
    assert!(BuiltPrograms::load(&d.record()).holds(&program));
    // Recorded once; the build's stamp is gone.
    assert!(!finish_started_build(&d.record(), &program).unwrap());
    // Written again by something else afterwards: not Arcode's.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(&program)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(!BuiltPrograms::load(&d.record()).holds(&program));

    // A build that fails leaves the last program Arcode's.
    record_build_started(&d.record(), &program).unwrap();
    record_built(&d.record(), &program).unwrap();
    record_build_started(&d.record(), &program).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&program)
        .unwrap()
        .set_modified(old)
        .unwrap();
    record_built(&d.record(), &program).unwrap();
    record_build_started(&d.record(), &program).unwrap();
    assert!(BuiltPrograms::load(&d.record()).holds(&program));
    assert!(!finish_started_build(&d.record(), &program).unwrap());

    // A program written long after a build started is not that build's.
    let slow = d.root.join(format!("slow{EXE}"));
    record_build_started(&d.record(), &slow).unwrap();
    fake_program(&slow);
    let much_later = std::time::SystemTime::now() + BUILD_WINDOW * 2;
    fs::File::options()
        .write(true)
        .open(&slow)
        .unwrap()
        .set_modified(much_later)
        .unwrap();
    assert!(!BuiltPrograms::load(&d.record()).holds(&slow));
    assert!(!finish_started_build(&d.record(), &slow).unwrap());
}

#[test]
fn the_record_survives_odd_lines_and_drops_missing_programs() {
    let d = dirs();
    fs::create_dir_all(&d.build).unwrap();
    fs::write(d.record(), "nonsense\nbuilt x /nope\nweird 1 /a\n").unwrap();
    assert_eq!(BuiltPrograms::load(&d.record()), BuiltPrograms::default());
    assert_eq!(
        BuiltPrograms::load(&d.build.join("missing")),
        BuiltPrograms::default()
    );
    let program = d.root.join(format!("gone{EXE}"));
    fake_program(&program);
    record_built(&d.record(), &program).unwrap();
    fs::remove_file(&program).unwrap();
    let other = d.root.join(format!("kept{EXE}"));
    fake_program(&other);
    record_built(&d.record(), &other).unwrap();
    let text = fs::read_to_string(d.record()).unwrap();
    assert!(!text.contains("gone"), "{text}");
    assert!(text.contains("kept"), "{text}");
    assert!(record_built(&d.record(), &d.root.join("never-built")).is_err());
}

#[test]
fn windows_names_and_commands() {
    // The `.exe` name and the `.\` run step are what Windows gets.
    let d = dirs();
    let file = d.write("test.rs", "fn main() {}\n");
    let cmd = d.run_next(&file, "rust", &Fake::with(&["rustc"])).command;
    let compile = cmd.compile.as_deref().unwrap();
    let out = format!("test{EXE}");
    assert_eq!(compile.args.last(), Some(&out));
    assert_eq!(
        cmd.program,
        format!(".{}{out}", std::path::MAIN_SEPARATOR),
        "{cmd:?}"
    );
    assert_eq!(compiled_program(&cmd), Some(d.root.join(&out)));
    // A Windows-style command still names its program and folder.
    let windows = RunCommand::new(
        ".\\test.exe",
        Vec::new(),
        "C:\\p",
        crate::RunKind::Run,
        crate::RunSource::File,
    )
    .with_compile(RunCommand::new(
        "rustc",
        vec![
            "-g".into(),
            "test.rs".into(),
            "-o".into(),
            "test.exe".into(),
        ],
        "C:\\p",
        crate::RunKind::Build,
        crate::RunSource::File,
    ));
    assert_eq!(windows.display_line(), "rustc -g test.rs");
    assert_eq!(
        windows.command_line_in(crate::Shell::PowerShell),
        "rustc -g test.rs -o test.exe; if ($?) { .\\test.exe }"
    );
    assert_eq!(
        windows.command_line_in(crate::Shell::Cmd),
        "rustc -g test.rs -o test.exe && .\\test.exe"
    );
}

#[test]
fn the_cache_choice_keeps_the_old_folder() {
    let d = dirs();
    let file = d.write("test.rs", "fn main() {}\n");
    let run = file_run(
        &FileRunInput {
            file: &file,
            language_id: "rust",
            project_root: Some(&d.root),
            output: ProgramOutput::Cache(&d.build),
        },
        &Fake::with(&["rustc"]),
    )
    .unwrap();
    assert_eq!(run.note, None);
    assert!(run.command.program.starts_with(&*d.build.to_string_lossy()));
    assert_eq!(run.command.cwd, d.root);
}
