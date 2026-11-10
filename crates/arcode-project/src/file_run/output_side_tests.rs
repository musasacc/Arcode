//! Programs next to their sources, continued: a failed terminal build
//! and the debug files compilers write beside a program.

use std::fs;
use std::path::{Path, PathBuf};

use super::output_tests::fake_program;
use super::tests::dirs;
use super::*;

const EXE: &str = std::env::consts::EXE_SUFFIX;

// A terminal build that ended without its program (it failed) is
// forgotten: before, a program written there in the next ten minutes by
// something else (`cc -o test` by hand) counted as Arcode's.
#[test]
fn a_failed_terminal_build_is_forgotten() {
    let d = dirs();
    let program = d.root.join(format!("test{EXE}"));
    let started = record_build_started(&d.record(), &program).unwrap();
    abandon_started_build(&d.record(), &program, started).unwrap();
    fake_program(&program);
    assert!(!BuiltPrograms::load(&d.record()).holds(&program));
    assert_eq!(
        check_next_to(&program, &BuiltPrograms::load(&d.record())),
        Err(NameTaken::NotBuiltByArcode)
    );

    // The program of an earlier build stays Arcode's, and a newer build's
    // stamp is not dropped by an older build's end.
    record_built(&d.record(), &program).unwrap();
    let first = record_build_started(&d.record(), &program).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = record_build_started(&d.record(), &program).unwrap();
    assert_ne!(first, second);
    abandon_started_build(&d.record(), &program, first).unwrap();
    let text = fs::read_to_string(d.record()).unwrap();
    assert!(text.contains(&format!("started {second} ")), "{text}");
    abandon_started_build(&d.record(), &program, second).unwrap();
    assert!(BuiltPrograms::load(&d.record()).holds(&program));
    let text = fs::read_to_string(d.record()).unwrap();
    assert!(!text.contains("started"), "{text}");
}

// Compilers write debug files beside the program on Windows and macOS:
// they are never written over unless Arcode built the program.
#[test]
fn debug_files_beside_the_program_follow_its_rule() {
    use super::output::{check_next_to_on, debug_files};
    let at = |p: &str| PathBuf::from(p);
    assert_eq!(
        debug_files(Path::new("/p/test.exe"), "windows"),
        [at("/p/test.pdb"), at("/p/test.ilk")]
    );
    assert_eq!(
        debug_files(Path::new("/p/test"), "macos"),
        [at("/p/test.dSYM")]
    );
    assert!(debug_files(Path::new("/p/test"), "linux").is_empty());

    let d = dirs();
    let none = BuiltPrograms::default();
    // Someone's own debug files, with no program: not written over.
    let win = d.root.join("test.exe");
    fs::write(d.root.join("test.pdb"), "pdb").unwrap();
    assert_eq!(
        check_next_to_on(&win, &none, "windows"),
        Err(NameTaken::DebugFiles)
    );
    let mac = d.root.join("tool");
    fs::create_dir(d.root.join("tool.dSYM")).unwrap();
    assert_eq!(
        check_next_to_on(&mac, &none, "macos"),
        Err(NameTaken::DebugFiles)
    );
    assert_eq!(check_next_to_on(&mac, &none, "linux"), Ok(()));
    let note = taken_note(&d.root.join("tool.c"), &mac, NameTaken::DebugFiles);
    assert!(note.contains("debug files of tool"), "{note}");
    // Beside a program Arcode built, they are that build's.
    fake_program(&mac);
    record_built(&d.record(), &mac).unwrap();
    let built = BuiltPrograms::load(&d.record());
    assert_eq!(check_next_to_on(&mac, &built, "macos"), Ok(()));
}
