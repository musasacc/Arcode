use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::langs::{crate_name, output_path, parse_shebang};
use super::*;
use crate::{RunKind, RunSource, debug_target};

/// Tools that "find" the listed programs (at `/usr/bin/<name>`).
struct Fake {
    programs: HashSet<&'static str>,
    node: Option<(u32, u32)>,
}

impl Fake {
    fn with(programs: &[&'static str]) -> Self {
        Self {
            programs: programs.iter().copied().collect(),
            node: None,
        }
    }

    fn node(mut self, version: (u32, u32)) -> Self {
        self.programs.insert("node");
        self.node = Some(version);
        self
    }
}

impl Tools for Fake {
    fn find(&self, program: &str) -> Option<PathBuf> {
        self.programs
            .contains(program)
            .then(|| PathBuf::from("/usr/bin").join(program))
    }

    fn node_version(&self) -> Option<(u32, u32)> {
        self.node
    }
}

/// A project folder with a separate build folder.
struct Dirs {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    build: PathBuf,
}

fn dirs() -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(tmp.path()).unwrap();
    let root = base.join("proj");
    let build = base.join("build");
    fs::create_dir_all(&root).unwrap();
    Dirs {
        _tmp: tmp,
        root,
        build,
    }
}

impl Dirs {
    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn run(&self, file: &Path, language_id: &str, tools: &Fake) -> Result<RunCommand, String> {
        file_command(
            &FileRunInput {
                file,
                language_id,
                project_root: Some(&self.root),
                build_dir: &self.build,
            },
            tools,
        )
    }
}

#[test]
fn python_runs_the_file_from_the_project_folder() {
    let d = dirs();
    let file = d.write("scripts/test.py", "print('hi')\n");
    let cmd = d.run(&file, "python", &Fake::with(&["python3"])).unwrap();
    assert_eq!(cmd.command_line(), "python3 scripts/test.py");
    assert_eq!(cmd.cwd, d.root);
    assert_eq!(cmd.label, "run test.py");
    assert_eq!(cmd.source, RunSource::Python);
    let target = debug_target(&cmd).expect("debuggable");
    assert_eq!(target.language, "python");
    assert_eq!(target.program.as_deref(), Some(file.as_path()));
}

#[test]
fn python_prefers_the_virtualenv_and_says_what_to_install() {
    let d = dirs();
    let file = d.write("test.py", "");
    let python = crate::toolchain::venv_python(&d.root.join(".venv"));
    fs::create_dir_all(python.parent().unwrap()).unwrap();
    fs::write(&python, "").unwrap();
    let cmd = d.run(&file, "python", &Fake::with(&[])).unwrap();
    assert_eq!(cmd.program, python.to_string_lossy());

    fs::remove_dir_all(d.root.join(".venv")).unwrap();
    let err = d.run(&file, "python", &Fake::with(&[])).unwrap_err();
    assert_eq!(err, "Install Python 3 to run test.py");
}

#[test]
fn a_loose_rust_file_is_compiled_into_the_build_folder() {
    let d = dirs();
    let file = d.write("test.rs", "fn main() { println!(\"hello\"); }\n");
    let cmd = d.run(&file, "rust", &Fake::with(&["rustc"])).unwrap();
    let out = output_path(&d.build, &file);
    assert!(out.starts_with(&d.build), "{out:?}");
    assert!(!out.parent().unwrap().exists());
    create_output_dir(&cmd).unwrap();
    assert!(
        out.parent().unwrap().is_dir(),
        "the build folder is created"
    );
    assert_eq!(cmd.program, out.to_string_lossy());
    assert_eq!(cmd.source, RunSource::File);
    let compile = cmd.compile.as_deref().expect("compile step");
    assert_eq!(compile.program, "rustc");
    assert_eq!(
        compile.args,
        [
            "--edition",
            "2024",
            "-g",
            "test.rs",
            "-o",
            &*out.to_string_lossy()
        ]
    );
    assert_eq!(compile.kind, RunKind::Build);
    let line = cmd.command_line();
    assert!(
        line.starts_with("rustc --edition 2024 -g test.rs -o "),
        "{line}"
    );
    // Nothing is written next to the source.
    let names: Vec<_> = fs::read_dir(&d.root).unwrap().collect();
    assert_eq!(names.len(), 1);

    let target = debug_target(&cmd).expect("debuggable");
    assert_eq!(target.language, "rust");
    assert_eq!(target.build.as_ref(), Some(compile));
    assert_eq!(target.program.as_deref(), Some(out.as_path()));
}

#[test]
fn rust_names_the_crate_when_the_file_name_is_not_one() {
    let d = dirs();
    let file = d.write("my-tool.rs", "fn main() {}\n");
    let cmd = d.run(&file, "rust", &Fake::with(&["rustc"])).unwrap();
    let compile = cmd.compile.unwrap();
    assert!(
        compile
            .args
            .windows(2)
            .any(|w| w == ["--crate-name", "my_tool"]),
        "{:?}",
        compile.args
    );
    assert_eq!(crate_name("1st"), "_1st");
    assert_eq!(crate_name("ok_name"), "ok_name");
}

#[test]
fn rust_inside_a_cargo_package_keeps_cargo() {
    let d = dirs();
    d.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let main = d.write("src/main.rs", "fn main() {}\n");
    let tools = Fake::with(&["cargo", "rustc"]);
    let cmd = d.run(&main, "rust", &tools).unwrap();
    assert_eq!(cmd.command_line(), "cargo run --package demo --bin demo");
    assert_eq!(cmd.label, "run main.rs");
    assert_eq!(cmd.source, RunSource::Cargo);
    // A script no target builds is compiled on its own.
    let script = d.write("scripts/tool.rs", "fn main() {}\n");
    let cmd = d.run(&script, "rust", &tools).unwrap();
    assert_eq!(cmd.compile.unwrap().program, "rustc");
    let err = d.run(&main, "rust", &Fake::with(&["rustc"])).unwrap_err();
    assert_eq!(err, "Install Rust (cargo) to run main.rs");
}

#[test]
fn a_build_script_runs_inside_cargo() {
    let d = dirs();
    d.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    d.write("src/main.rs", "fn main() {}\n");
    let build = d.write("build.rs", "fn main() {}\n");
    let tools = Fake::with(&["cargo", "rustc"]);
    let cmd = d.run(&build, "rust", &tools).unwrap();
    assert_eq!(cmd.command_line(), "cargo run --package demo");
    assert_eq!(cmd.source, RunSource::Cargo);
    assert!(cmd.compile.is_none());

    // A build script named in the manifest, and `build = false`.
    d.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nbuild = \"gen/make.rs\"\n",
    );
    let custom = d.write("gen/make.rs", "fn main() {}\n");
    assert_eq!(
        d.run(&custom, "rust", &tools).unwrap().source,
        RunSource::Cargo
    );
    assert!(d.run(&build, "rust", &tools).unwrap().compile.is_some());
    d.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nbuild = false\n",
    );
    assert!(d.run(&build, "rust", &tools).unwrap().compile.is_some());
}

#[test]
fn c_and_cpp_use_the_first_compiler_found() {
    let d = dirs();
    let c = d.write("hello.c", "int main(void) { return 0; }\n");
    let cmd = d.run(&c, "c", &Fake::with(&["gcc", "clang"])).unwrap();
    let compile = cmd.compile.as_deref().unwrap();
    assert_eq!(compile.program, "gcc");
    assert_eq!(&compile.args[..2], ["-g", "hello.c"]);
    assert_eq!(debug_target(&cmd).unwrap().language, "c");
    let err = d.run(&c, "c", &Fake::with(&[])).unwrap_err();
    assert_eq!(err, "Install a C compiler (gcc or clang) to run hello.c");

    let cpp = d.write("app.cpp", "int main() {}\n");
    let cmd = d.run(&cpp, "cpp", &Fake::with(&["c++"])).unwrap();
    assert_eq!(cmd.compile.as_deref().unwrap().program, "c++");
    assert_eq!(debug_target(&cmd).unwrap().language, "cpp");
}

#[test]
fn go_runs_the_file_outside_a_module_and_the_package_inside() {
    let d = dirs();
    let file = d.write("tools/hello.go", "package main\nfunc main() {}\n");
    let tools = Fake::with(&["go"]);
    let cmd = d.run(&file, "go", &tools).unwrap();
    assert_eq!(cmd.command_line(), "go run tools/hello.go");
    let target = debug_target(&cmd).expect("Delve");
    assert_eq!(target.language, "go");
    assert_eq!(target.program.as_deref(), Some(file.as_path()));
    assert_eq!(target.go_mode, Some("debug"));

    d.write("go.mod", "module x\n");
    let cmd = d.run(&file, "go", &tools).unwrap();
    assert_eq!(cmd.command_line(), "go run ./tools");
    assert_eq!(cmd.label, "run hello.go");
    assert_eq!(
        d.run(&file, "go", &Fake::with(&[])).unwrap_err(),
        "Install Go to run hello.go"
    );
}

#[test]
fn javascript_and_typescript() {
    let d = dirs();
    let js = d.write("a.js", "");
    let cmd = d.run(&js, "javascript", &Fake::with(&["node"])).unwrap();
    assert_eq!(cmd.command_line(), "node a.js");
    assert!(debug_target(&cmd).is_none());
    let cmd = d.run(&js, "javascript", &Fake::with(&["deno"])).unwrap();
    assert_eq!(cmd.command_line(), "deno run a.js");
    assert_eq!(
        d.run(&js, "javascript", &Fake::with(&[])).unwrap_err(),
        "Install Node.js to run a.js"
    );

    let ts = d.write("b.ts", "");
    let cmd = d
        .run(&ts, "typescript", &Fake::with(&[]).node((22, 6)))
        .unwrap();
    assert_eq!(cmd.command_line(), "node --experimental-strip-types b.ts");
    let cmd = d
        .run(&ts, "typescript", &Fake::with(&["bun"]).node((20, 11)))
        .unwrap();
    assert_eq!(cmd.command_line(), "bun b.ts");
    let err = d
        .run(&ts, "typescript", &Fake::with(&[]).node((20, 11)))
        .unwrap_err();
    assert!(
        err.contains("22.6 or newer") && err.contains("20.11"),
        "{err}"
    );
    let err = d.run(&ts, "typescript", &Fake::with(&[])).unwrap_err();
    assert!(err.starts_with("Install Node.js 22.6"), "{err}");
}

#[test]
fn interpreters_for_the_other_languages() {
    let d = dirs();
    let cases = [
        ("Main.java", "java", &["java"][..], "java Main.java"),
        ("x.rb", "ruby", &["ruby"][..], "ruby x.rb"),
        ("x.php", "php", &["php"][..], "php x.php"),
        ("x.lua", "lua", &["luajit"][..], "luajit x.lua"),
        ("x.sh", "shellscript", &["bash", "sh"][..], "bash x.sh"),
    ];
    for (name, id, programs, line) in cases {
        let file = d.write(name, "echo\n");
        let cmd = d.run(&file, id, &Fake::with(programs)).unwrap();
        assert_eq!(cmd.command_line(), line, "{name}");
        assert_eq!(cmd.source, RunSource::File);
        assert!(debug_target(&cmd).is_none(), "{name}");
        let err = d.run(&file, id, &Fake::with(&[])).unwrap_err();
        assert!(err.starts_with("Install ") && err.ends_with(name), "{err}");
    }
    let zsh = d.write("z.sh", "#!/usr/bin/env zsh\necho\n");
    let cmd = d
        .run(&zsh, "shellscript", &Fake::with(&["bash", "zsh"]))
        .unwrap();
    assert_eq!(cmd.program, "zsh");
}

#[test]
fn unknown_languages_say_why() {
    let d = dirs();
    let readme = d.write("README.md", "# hi\n");
    let err = d
        .run(&readme, "markdown", &Fake::with(&["python3"]))
        .unwrap_err();
    assert!(err.starts_with("Cannot run README.md: "), "{err}");
    // A known extension runs even when the file is open as plain text.
    let py = d.write("tool.py", "");
    assert!(d.run(&py, "plaintext", &Fake::with(&["python3"])).is_ok());
    assert!(!FileLanguage::Lua.can_debug());
    let msg = debug_unsupported_message(Some(FileLanguage::Ruby), "ruby", Path::new("/p/x.rb"));
    assert!(
        msg.starts_with("Cannot debug x.rb: there is no debugger for Ruby files"),
        "{msg}"
    );
    let msg = debug_unsupported_message(None, "Markdown", &readme);
    assert!(
        msg.starts_with("Cannot debug README.md: there is no debugger for Markdown files"),
        "{msg}"
    );
    assert!(unsupported_message(&readme, "Markdown").contains("no runner for Markdown files"));
}

#[test]
fn headers_do_not_run() {
    let d = dirs();
    let tools = Fake::with(&["cc", "c++"]);
    for (name, language_id) in [("util.h", "c"), ("vec.hpp", "cpp"), ("x.HH", "cpp")] {
        let header = d.write(name, "int helper(void);\n");
        assert_eq!(FileLanguage::detect(language_id, &header), None, "{name}");
        let err = d.run(&header, language_id, &tools).unwrap_err();
        assert_eq!(
            err,
            format!(
                "Cannot run {name}: a header does not run on its own. Run a .c or .cpp file \
                 that includes it."
            )
        );
        let msg = debug_unsupported_message(None, "C", &header);
        assert!(
            msg.starts_with(&format!("Cannot debug {name}: a header")),
            "{msg}"
        );
    }
    // Nothing was compiled.
    assert!(!d.build.exists());
    let source = d.write("util.c", "int helper(void) { return 1; }\n");
    assert_eq!(FileLanguage::detect("c", &source), Some(FileLanguage::C));
}

#[test]
fn a_file_outside_the_project_runs_in_its_own_folder() {
    let d = dirs();
    let other = d.root.parent().unwrap().join("elsewhere");
    fs::create_dir_all(&other).unwrap();
    let file = other.join("t.py");
    fs::write(&file, "").unwrap();
    let cmd = d.run(&file, "python", &Fake::with(&["python3"])).unwrap();
    assert_eq!(cmd.cwd, other);
    assert_eq!(cmd.command_line(), "python3 t.py");
    let no_project = file_command(
        &FileRunInput {
            file: &file,
            language_id: "python",
            project_root: None,
            build_dir: &d.build,
        },
        &Fake::with(&["python3"]),
    )
    .unwrap();
    assert_eq!(no_project.cwd, other);
}

#[test]
fn programs_of_same_named_files_do_not_collide() {
    let build = Path::new("/cache/run");
    let a = output_path(build, Path::new("/p/a/main.c"));
    let b = output_path(build, Path::new("/p/b/main.c"));
    assert_ne!(a, b);
    assert_eq!(
        a.file_name().unwrap().to_string_lossy(),
        format!("main{}", std::env::consts::EXE_SUFFIX)
    );
    assert_eq!(a, output_path(build, Path::new("/p/a/main.c")));
}

#[test]
fn versions_and_shebangs_parse() {
    assert_eq!(parse_node_version("v22.6.0\n"), Some((22, 6)));
    assert_eq!(parse_node_version("garbage"), None);
    assert_eq!(parse_shebang("#!/bin/zsh\n").as_deref(), Some("zsh"));
    assert_eq!(
        parse_shebang("#!/usr/bin/env -S bash -e").as_deref(),
        Some("bash")
    );
    assert_eq!(parse_shebang("echo"), None);
}
