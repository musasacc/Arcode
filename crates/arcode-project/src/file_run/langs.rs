//! The command of each [`FileLanguage`] for [`super::file_command`].

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use super::output::{check_next_to, program_next_to, stem, taken_note};
use super::{FileLanguage, FileRun, FileRunInput, ProgramOutput, Tools, file_name, working_dir};
use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::runnables::{RunnableKind, go, rust};
use crate::{fsutil, toolchain};

/// The oldest Node.js that runs TypeScript (`--experimental-strip-types`).
const NODE_STRIP_TYPES: (u32, u32) = (22, 6);

/// Everything the language builders share.
struct Ctx<'a> {
    input: &'a FileRunInput<'a>,
    tools: &'a dyn Tools,
    /// The file name, for labels and messages.
    name: String,
    cwd: PathBuf,
    /// The file relative to `cwd` (or absolute when outside it).
    rel: String,
    /// Where a compiled program went instead of next to the source.
    note: RefCell<Option<String>>,
}

impl Ctx<'_> {
    fn label(&self) -> String {
        format!("run {}", self.name)
    }

    /// The first of `programs` that is installed.
    fn first(&self, programs: &[&'static str]) -> Option<&'static str> {
        programs
            .iter()
            .copied()
            .find(|p| self.tools.find(p).is_some())
    }

    /// `<program> <before...> <file>`, run where the project is.
    fn interpreted(&self, program: &str, before: &[&str], source: RunSource) -> RunCommand {
        let mut list = args(before);
        list.push(self.rel.clone());
        RunCommand::new(program, list, &self.cwd, RunKind::Run, source).with_label(self.label())
    }

    fn missing(&self, what: &str) -> String {
        format!("Install {what} to run {}", self.name)
    }
}

pub(super) fn command(
    language: FileLanguage,
    input: &FileRunInput<'_>,
    tools: &dyn Tools,
) -> Result<FileRun, String> {
    let cwd = working_dir(input.file, input.project_root);
    let ctx = Ctx {
        input,
        tools,
        name: file_name(input.file),
        rel: fsutil::rel_slash(input.file, &cwd),
        cwd,
        note: RefCell::new(None),
    };
    let command = language_command(language, &ctx)?;
    Ok(FileRun {
        command,
        note: ctx.note.take(),
    })
}

fn language_command(language: FileLanguage, ctx: &Ctx<'_>) -> Result<RunCommand, String> {
    let (input, tools) = (ctx.input, ctx.tools);
    let unix = !cfg!(windows);
    match language {
        FileLanguage::Python => python(ctx),
        FileLanguage::Rust => rust_file(ctx),
        FileLanguage::C => native(
            ctx,
            &["cc", "gcc", "clang"],
            &[],
            "a C compiler (gcc or clang)",
        ),
        FileLanguage::Cpp => native(
            ctx,
            &["c++", "g++", "clang++"],
            &[],
            "a C++ compiler (g++ or clang++)",
        ),
        FileLanguage::Go => go_file(ctx),
        FileLanguage::JavaScript => match ctx.first(&["node", "bun", "deno"]) {
            Some("deno") => Ok(ctx.interpreted("deno", &["run"], RunSource::File)),
            Some(program) => Ok(ctx.interpreted(program, &[], RunSource::File)),
            None => Err(ctx.missing("Node.js")),
        },
        FileLanguage::TypeScript => typescript(ctx),
        FileLanguage::Java => match ctx.first(&["java"]) {
            Some(java) => Ok(ctx.interpreted(java, &[], RunSource::File)),
            None => Err(ctx.missing("a JDK (11 or newer)")),
        },
        FileLanguage::Ruby => simple(ctx, &["ruby"], "Ruby"),
        FileLanguage::Php => simple(ctx, &["php"], "PHP"),
        FileLanguage::Lua => simple(ctx, &["lua", "lua5.4", "lua5.3", "lua5.1", "luajit"], "Lua"),
        FileLanguage::Shell => {
            let shebang = shebang_program(input.file);
            let mut programs: Vec<String> = shebang.into_iter().collect();
            programs.extend(["bash".to_string(), "sh".to_string()]);
            if !unix {
                programs.retain(|p| p != "sh");
            }
            match programs.iter().find(|p| tools.find(p).is_some()) {
                Some(shell) => Ok(ctx.interpreted(shell, &[], RunSource::File)),
                None => Err(ctx.missing("bash")),
            }
        }
    }
}

fn simple(ctx: &Ctx<'_>, programs: &[&'static str], what: &str) -> Result<RunCommand, String> {
    match ctx.first(programs) {
        Some(program) => Ok(ctx.interpreted(program, &[], RunSource::File)),
        None => Err(ctx.missing(what)),
    }
}

/// `python file.py`, with the nearest usable virtualenv's interpreter.
fn python(ctx: &Ctx<'_>) -> Result<RunCommand, String> {
    let start = fsutil::start_dir(ctx.input.file);
    if let Some(venv) = toolchain::find_usable_python_venv(&start, ctx.input.project_root) {
        let python = venv.python.to_string_lossy().into_owned();
        return Ok(ctx.interpreted(&python, &[], RunSource::Python));
    }
    let candidates: &[&'static str] = if cfg!(windows) {
        &["python", "py", "python3"]
    } else {
        &["python3", "python"]
    };
    match ctx.first(candidates) {
        Some(python) => Ok(ctx.interpreted(python, &[], RunSource::Python)),
        None => Err(ctx.missing("Python 3")),
    }
}

/// `cargo run` for a file of a Cargo package, else `rustc` and the
/// program.
fn rust_file(ctx: &Ctx<'_>) -> Result<RunCommand, String> {
    let root = ctx.input.project_root.unwrap_or(&ctx.cwd);
    if let Some(command) = rust::package_run_command(ctx.input.file, root) {
        if ctx.tools.find("cargo").is_none() {
            return Err(ctx.missing("Rust (cargo)"));
        }
        return Ok(command.with_label(ctx.label()));
    }
    let Some(rustc) = ctx.first(&["rustc"]) else {
        return Err(ctx.missing("Rust (rustc)"));
    };
    let mut flags = args(&["--edition", "2024"]);
    // rustc names the crate after the file, which fails for `my-tool.rs`.
    let stem = stem(ctx.input.file);
    let crate_name = crate_name(&stem);
    if crate_name != stem {
        flags.extend(["--crate-name".to_string(), crate_name]);
    }
    Ok(compiled(ctx, rustc, flags))
}

/// A C or C++ compiler, then the program.
fn native(
    ctx: &Ctx<'_>,
    compilers: &[&'static str],
    flags: &[&str],
    what: &str,
) -> Result<RunCommand, String> {
    match ctx.first(compilers) {
        Some(compiler) => Ok(compiled(ctx, compiler, args(flags))),
        None => Err(ctx.missing(what)),
    }
}

/// `<compiler> <flags> -g <file> -o <out>`, then `<out>`: next to the
/// source and from its folder (`rustc -g test.rs -o test && ./test`), or
/// in the cache from the project folder.
fn compiled(ctx: &Ctx<'_>, compiler: &str, mut flags: Vec<String>) -> RunCommand {
    let file = ctx.input.file;
    let cache = match ctx.input.output {
        ProgramOutput::Cache(cache) => cache,
        ProgramOutput::NextToSource { cache, built } => {
            let program = program_next_to(file);
            match check_next_to(&program, built) {
                Ok(()) => return next_to_source(ctx, compiler, flags, &program),
                Err(taken) => {
                    *ctx.note.borrow_mut() = Some(taken_note(file, &program, taken));
                    cache
                }
            }
        }
    };
    let out = output_path(cache, file).to_string_lossy().into_owned();
    flags.extend([
        "-g".to_string(),
        ctx.rel.clone(),
        "-o".to_string(),
        out.clone(),
    ]);
    let compile = RunCommand::new(compiler, flags, &ctx.cwd, RunKind::Build, RunSource::File);
    RunCommand::new(out, Vec::new(), &ctx.cwd, RunKind::Run, RunSource::File)
        .with_compile(compile)
        .with_label(ctx.label())
}

/// The compile and run steps of a program written next to its source,
/// with paths relative to the file's folder.
fn next_to_source(
    ctx: &Ctx<'_>,
    compiler: &str,
    mut flags: Vec<String>,
    program: &Path,
) -> RunCommand {
    let dir = fsutil::start_dir(ctx.input.file);
    let out = file_name(program);
    flags.extend([
        "-g".to_string(),
        ctx.name.clone(),
        "-o".to_string(),
        out.clone(),
    ]);
    let compile = RunCommand::new(compiler, flags, &dir, RunKind::Build, RunSource::File);
    let run = format!(".{}{out}", std::path::MAIN_SEPARATOR);
    RunCommand::new(run, Vec::new(), &dir, RunKind::Run, RunSource::File)
        .with_compile(compile)
        .with_label(ctx.label())
}

/// `go run file.go`, or the file's package inside a Go module.
fn go_file(ctx: &Ctx<'_>) -> Result<RunCommand, String> {
    if ctx.tools.find("go").is_none() {
        return Err(ctx.missing("Go"));
    }
    let root = ctx.input.project_root.unwrap_or(&ctx.cwd);
    go::command(ctx.input.file, root, RunnableKind::Main, "")
        .map(|c| c.with_label(ctx.label()))
        .ok_or_else(|| ctx.missing("Go"))
}

/// Node.js 22.6+ strips the types itself; else Bun or Deno.
fn typescript(ctx: &Ctx<'_>) -> Result<RunCommand, String> {
    let node = ctx.tools.find("node").map(|_| ctx.tools.node_version());
    if let Some(Some(version)) = node
        && version >= NODE_STRIP_TYPES
    {
        return Ok(ctx.interpreted("node", &["--experimental-strip-types"], RunSource::File));
    }
    match ctx.first(&["bun", "deno"]) {
        Some("deno") => return Ok(ctx.interpreted("deno", &["run"], RunSource::File)),
        Some(bun) => return Ok(ctx.interpreted(bun, &[], RunSource::File)),
        None => {}
    }
    let (major, minor) = NODE_STRIP_TYPES;
    Err(match node {
        Some(Some((found_major, found_minor))) => format!(
            "Running {} needs Node.js {major}.{minor} or newer (this one is \
             {found_major}.{found_minor}). Update Node.js, or install Bun or Deno.",
            ctx.name
        ),
        _ => ctx.missing(&format!(
            "Node.js {major}.{minor} or newer (or Bun or Deno)"
        )),
    })
}

/// Where the program built from `file` goes: a folder of `build_dir`
/// named after the file's full path (two `main.c` in different folders do
/// not overwrite each other), holding `<stem>` (`<stem>.exe` on Windows).
pub(crate) fn output_path(build_dir: &Path, file: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    file.hash(&mut hasher);
    let folder = format!("{:016x}", hasher.finish());
    let name = format!("{}{}", stem(file), std::env::consts::EXE_SUFFIX);
    build_dir.join(folder).join(name)
}

/// A valid crate name for a file stem: other characters become `_`, and a
/// leading digit gets a `_` before it.
pub(crate) fn crate_name(stem: &str) -> String {
    let mut name: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if name.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    name
}

/// The interpreter a `#!` line names: `#!/bin/zsh` → `zsh`,
/// `#!/usr/bin/env -S bash -e` → `bash`.
pub(crate) fn shebang_program(file: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader, Read};
    let file = std::fs::File::open(file).ok()?;
    let mut first = String::new();
    // One short line: a binary file has no `#!` line to find.
    BufReader::new(file.take(256)).read_line(&mut first).ok()?;
    parse_shebang(&first)
}

pub(crate) fn parse_shebang(line: &str) -> Option<String> {
    let rest = line.strip_prefix("#!")?.trim();
    let mut words = rest.split_whitespace();
    let first = words.next()?;
    let base = |w: &str| w.rsplit(['/', '\\']).next().unwrap_or(w).to_string();
    if base(first) == "env" {
        return words.find(|w| !w.starts_with('-')).map(base);
    }
    Some(base(first))
}
