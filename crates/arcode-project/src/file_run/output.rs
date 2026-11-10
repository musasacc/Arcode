//! Where a program compiled from one source file goes, and the record of
//! the programs Arcode built next to their sources.
//!
//! Next to the source (`test.rs` → `test`, `test.exe` on Windows) a
//! program is only written over when Arcode built it: a folder, a file
//! that is not a program, or a program built some other way keeps its
//! name, and the program goes to the cache folder instead. The debug
//! information compilers write beside it (`test.pdb` on Windows, a
//! `test.dSYM` folder on macOS) follows the program's rule. The record is
//! a small text file in the cache (one line per program: its path and the
//! modification time it had, or the time a build into it started).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fsutil;

/// The most programs the record keeps (the newest ones).
const MAX_RECORDS: usize = 400;

/// File times can trail the clock a little (coarse file system clocks):
/// a program modified this long before a build started still counts as
/// the build's.
const CLOCK_SLACK_NANOS: u128 = 2_000_000_000;

/// How long after a build started a program written there still counts
/// as the build's, until [`finish_started_build`] records its exact time.
pub const BUILD_WINDOW: std::time::Duration = std::time::Duration::from_secs(600);

/// Where [`super::file_command`] puts a compiled program.
#[derive(Debug, Clone, Copy)]
pub enum ProgramOutput<'a> {
    /// Next to the source, run from the file's folder; `cache` when the
    /// name is taken by something `built` does not hold.
    NextToSource {
        /// The cache folder for programs that cannot go next to the source.
        cache: &'a Path,
        /// The programs Arcode built next to their sources.
        built: &'a BuiltPrograms,
    },
    /// A folder per source file in `cache`; the project folder is the
    /// working directory.
    Cache(&'a Path),
}

/// Why a program cannot be written next to its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameTaken {
    /// A folder has the program's name.
    Folder,
    /// A file that is not a program has the name.
    NotAProgram,
    /// A program Arcode did not build has the name.
    NotBuiltByArcode,
    /// Debug information the compiler writes beside the program
    /// ([`debug_files`]) is there, and Arcode did not build the program.
    DebugFiles,
}

/// When Arcode last wrote a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stamp {
    /// The program's modification time after a build (nanoseconds since
    /// the Unix epoch).
    Built(u128),
    /// A build into it started then (in a terminal, so its end is not
    /// seen): a file modified at or after that time (give or take
    /// [`CLOCK_SLACK_NANOS`]) and within [`BUILD_WINDOW`] is the build's.
    /// [`finish_started_build`] turns it into [`Stamp::Built`] once the
    /// program is there.
    Started(u128),
}

impl Stamp {
    /// Whether a program modified at `modified` is the one this stamp
    /// records.
    fn covers(self, modified: u128) -> bool {
        match self {
            Self::Built(at) => at == modified,
            Self::Started(at) => {
                modified + CLOCK_SLACK_NANOS >= at && modified <= at + BUILD_WINDOW.as_nanos()
            }
        }
    }
}

/// The programs Arcode built next to their sources (see the module
/// documentation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltPrograms {
    entries: Vec<(PathBuf, Stamp)>,
}

impl BuiltPrograms {
    /// Reads the record at `path`; empty when it is missing or unreadable
    /// (lines it does not understand are skipped).
    pub fn load(path: &Path) -> Self {
        let text = fs::read_to_string(path).unwrap_or_default();
        let entries = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, ' ');
                let kind = parts.next()?;
                let nanos: u128 = parts.next()?.parse().ok()?;
                let path = PathBuf::from(parts.next()?);
                let stamp = match kind {
                    "built" => Stamp::Built(nanos),
                    "started" => Stamp::Started(nanos),
                    _ => return None,
                };
                Some((path, stamp))
            })
            .collect();
        Self { entries }
    }

    /// Whether the file at `program` is one Arcode built there and has not
    /// changed since (or was written after a build into it started).
    pub fn holds(&self, program: &Path) -> bool {
        let Some(modified) = modified_nanos(program) else {
            return false;
        };
        self.entries
            .iter()
            .filter(|(path, _)| path == program)
            .any(|(_, stamp)| stamp.covers(modified))
    }

    /// Records `stamp` for `program`. A started build keeps the program's
    /// last build: if this one fails, the old program is still Arcode's.
    fn set(&mut self, program: &Path, stamp: Stamp) {
        let keep_built = matches!(stamp, Stamp::Started(_));
        self.entries.retain(|(path, old)| {
            path != program || (keep_built && matches!(old, Stamp::Built(_)))
        });
        self.entries.push((program.to_path_buf(), stamp));
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        let mut text = String::new();
        let keep = self.entries.len().saturating_sub(MAX_RECORDS);
        for (program, stamp) in self.entries.iter().skip(keep) {
            let Some(shown) = program.to_str().filter(|p| !p.contains(['\n', '\r'])) else {
                continue;
            };
            // Programs that are gone need no record.
            if !program.exists() && matches!(stamp, Stamp::Built(_)) {
                continue;
            }
            let (kind, nanos) = match stamp {
                Stamp::Built(n) => ("built", n),
                Stamp::Started(n) => ("started", n),
            };
            text.push_str(&format!("{kind} {nanos} {shown}\n"));
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)
    }
}

/// Notes in the record at `record` that a build into `program` starts now
/// (Run Current File: the build runs in a terminal). Returns the build's
/// stamp, for [`abandon_started_build`].
pub fn record_build_started(record: &Path, program: &Path) -> io::Result<u128> {
    let mut built = BuiltPrograms::load(record);
    let at = now_nanos();
    built.set(program, Stamp::Started(at));
    built.save(record)?;
    Ok(at)
}

/// The build that [`record_build_started`] noted (stamp `started`) ended
/// without writing `program` (it failed): forgets it, so a program put
/// there later by something else is not taken for Arcode's. The program
/// of an earlier build stays Arcode's.
pub fn abandon_started_build(record: &Path, program: &Path, started: u128) -> io::Result<()> {
    let mut built = BuiltPrograms::load(record);
    let before = built.entries.len();
    built
        .entries
        .retain(|(path, stamp)| path != program || *stamp != Stamp::Started(started));
    if built.entries.len() == before {
        return Ok(());
    }
    built.save(record)
}

/// Once the build that [`record_build_started`] noted has written
/// `program`, records the program's exact modification time, so a program
/// written there later by something else is not taken for Arcode's.
/// Returns whether it did (`false`: not built yet, or not by that build).
pub fn finish_started_build(record: &Path, program: &Path) -> io::Result<bool> {
    let mut built = BuiltPrograms::load(record);
    let Some(modified) = modified_nanos(program) else {
        return Ok(false);
    };
    let entries = || built.entries.iter().filter(|(path, _)| path == program);
    let by_build = entries().any(|(_, s)| matches!(s, Stamp::Started(_)) && s.covers(modified));
    // Still the program of the last build: the new one is not written yet.
    let unchanged = entries().any(|(_, s)| matches!(s, Stamp::Built(_)) && s.covers(modified));
    if !by_build || unchanged {
        return Ok(false);
    }
    built.set(program, Stamp::Built(modified));
    built.save(record)?;
    Ok(true)
}

/// Notes in the record at `record` that `program` was just built (Debug
/// Current File builds it itself and sees the end).
pub fn record_built(record: &Path, program: &Path) -> io::Result<()> {
    let modified = modified_nanos(program)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the program was not built"))?;
    let mut built = BuiltPrograms::load(record);
    built.set(program, Stamp::Built(modified));
    built.save(record)
}

/// The program built from `file` next to it: `<folder>/<stem>`
/// (`<stem>.exe` on Windows).
pub fn program_next_to(file: &Path) -> PathBuf {
    let name = format!("{}{}", stem(file), std::env::consts::EXE_SUFFIX);
    fsutil::start_dir(file).join(name)
}

/// Whether the program built from `file` may be written next to it:
/// nothing has its name, or Arcode built what has it. The same goes for
/// the debug information the compiler writes beside it ([`debug_files`]):
/// it is Arcode's when the program is.
pub fn check_next_to(program: &Path, built: &BuiltPrograms) -> Result<(), NameTaken> {
    check_next_to_on(program, built, std::env::consts::OS)
}

pub(crate) fn check_next_to_on(
    program: &Path,
    built: &BuiltPrograms,
    os: &str,
) -> Result<(), NameTaken> {
    check_program(program, built)?;
    let ours = built.holds(program);
    let debug = debug_files(program, os);
    if !ours && debug.iter().any(|f| fs::symlink_metadata(f).is_ok()) {
        return Err(NameTaken::DebugFiles);
    }
    Ok(())
}

/// What the compilers write beside a program built with `-g` on `os`:
/// on Windows the linker's `<stem>.pdb` (and `<stem>.ilk` when it links
/// incrementally), on macOS a `<stem>.dSYM` folder; nothing on Linux.
pub(crate) fn debug_files(program: &Path, os: &str) -> Vec<PathBuf> {
    let extensions: &[&str] = match os {
        "windows" => &["pdb", "ilk"],
        "macos" => &["dSYM"],
        _ => &[],
    };
    let base = match os {
        // `test.exe` → `test.pdb`
        "windows" => program.with_extension(""),
        _ => program.to_path_buf(),
    };
    extensions
        .iter()
        .map(|ext| {
            let mut name = base.as_os_str().to_owned();
            name.push(".");
            name.push(ext);
            PathBuf::from(name)
        })
        .collect()
}

fn check_program(program: &Path, built: &BuiltPrograms) -> Result<(), NameTaken> {
    let Ok(meta) = fs::symlink_metadata(program) else {
        return Ok(());
    };
    if meta.is_dir() {
        return Err(NameTaken::Folder);
    }
    if built.holds(program) {
        return Ok(());
    }
    if meta.is_file() && !is_executable(&meta) {
        return Err(NameTaken::NotAProgram);
    }
    Err(NameTaken::NotBuiltByArcode)
}

/// The one-line note shown when the program of `file` went to the cache
/// because `taken`.
pub fn taken_note(file: &Path, program: &Path, taken: NameTaken) -> String {
    let source = super::file_name(file);
    let name = super::file_name(program);
    let why = match taken {
        NameTaken::Folder => format!("{name} is a folder"),
        NameTaken::NotAProgram => format!("{name} is not a program"),
        NameTaken::NotBuiltByArcode => format!("Arcode did not build {name}"),
        NameTaken::DebugFiles => format!("Arcode did not build the debug files of {name}"),
    };
    format!("Built {source} in Arcode's cache: {why}")
}

/// The file name stem a program is named after (`main` for a file
/// without one).
pub(crate) fn stem(file: &Path) -> String {
    file.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "main".to_string())
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

/// On Windows the name ends in `.exe`: a file with it is a program.
#[cfg(not(unix))]
fn is_executable(_: &fs::Metadata) -> bool {
    true
}

fn modified_nanos(path: &Path) -> Option<u128> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_nanos())
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}
