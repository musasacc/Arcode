//! Run commands from `Makefile` targets and `justfile` recipes.

use std::fs;
use std::path::Path;

use super::kind_for_name;
use crate::command::{RunCommand, RunSource};

/// One `make <target>` per explicit target of the first Makefile found.
pub(super) fn detect_make(root: &Path) -> Vec<RunCommand> {
    let Some(text) = read_first(root, &["GNUmakefile", "makefile", "Makefile"]) else {
        return Vec::new();
    };
    make_targets(&text)
        .into_iter()
        .map(|t| {
            RunCommand::new(
                "make",
                vec![t.clone()],
                root,
                kind_for_name(&t),
                RunSource::Make,
            )
        })
        .collect()
}

/// One `just <recipe>` per public recipe of the justfile.
pub(super) fn detect_just(root: &Path) -> Vec<RunCommand> {
    let Some(text) = read_first(root, &["justfile", "Justfile", ".justfile"]) else {
        return Vec::new();
    };
    just_recipes(&text)
        .into_iter()
        .map(|r| {
            RunCommand::new(
                "just",
                vec![r.clone()],
                root,
                kind_for_name(&r),
                RunSource::Just,
            )
        })
        .collect()
}

fn read_first(root: &Path, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| fs::read_to_string(root.join(n)).ok())
}

/// Target names from `name [name...]: deps` lines, in file order.
///
/// Skips recipe lines (indented), variable assignments (`=`, `:=`, `::=`,
/// `?=`, `+=`), special targets (`.PHONY`), pattern rules (`%`) and
/// variable references (`$`) and the bodies of `define ... endef` blocks.
pub(crate) fn make_targets(text: &str) -> Vec<String> {
    let mut targets: Vec<String> = Vec::new();
    let mut define_depth = 0usize;
    for line in text.lines() {
        // A tab-indented line outside a block is a recipe, not a directive.
        let recipe = define_depth == 0 && line.starts_with('\t');
        let directive = if recipe { None } else { define_directive(line) };
        if directive == Some(true) {
            define_depth += 1;
            continue;
        }
        if define_depth > 0 {
            if directive == Some(false) {
                define_depth -= 1;
            }
            continue;
        }
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        let (head, rest) = line.split_at(colon);
        // `A = b:c`, `A := b`, `A ::= b`.
        if head.contains('=') || rest.trim_start_matches(':').starts_with('=') {
            continue;
        }
        for name in head.split_whitespace() {
            let special = name.starts_with('.') || name.contains(['%', '$', '(', ')']);
            if !special && !targets.iter().any(|t| t == name) {
                targets.push(name.to_string());
            }
        }
    }
    targets
}

/// `Some(true)` for a `define` line (with optional `override`/`export`
/// prefixes), `Some(false)` for `endef`, `None` otherwise.
fn define_directive(line: &str) -> Option<bool> {
    let mut words = line
        .split_whitespace()
        .skip_while(|w| matches!(*w, "override" | "export"));
    match words.next()? {
        "define" => Some(true),
        "endef" => Some(false),
        _ => None,
    }
}

/// Public recipe names (not starting with `_`, not marked `[private]`), in
/// file order.
pub(crate) fn just_recipes(text: &str) -> Vec<String> {
    const KEYWORDS: [&str; 6] = ["set", "alias", "import", "mod", "export", "unexport"];
    let mut recipes = Vec::new();
    let mut private = false;
    for line in text.lines() {
        if line.starts_with([' ', '\t', '#']) || line.trim().is_empty() {
            continue;
        }
        if line.starts_with('[') {
            private |= line.contains("private");
            continue;
        }
        let was_private = std::mem::take(&mut private);
        let Some(colon) = line.find(':') else {
            continue;
        };
        if line[colon..].starts_with(":=") {
            continue;
        }
        let head = line[..colon].trim_start_matches('@');
        let Some(name) = head.split_whitespace().next() else {
            continue;
        };
        let valid = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid || KEYWORDS.contains(&name) || name.starts_with('_') || was_private {
            continue;
        }
        recipes.push(name.to_string());
    }
    recipes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makefile_targets() {
        let text = "CC := gcc\nFLAGS = -O2\nX ?= 1\n.PHONY: all test\nall: build\n\
                    build test: deps\n\t$(CC) main.c\n%.o: %.c\n\techo\nrun:\n# c: x\nfmt :\n";
        assert_eq!(
            make_targets(text),
            ["all", "build", "test", "run", "fmt"]
                .map(String::from)
                .to_vec()
        );
    }

    #[test]
    fn define_blocks_are_skipped() {
        let text = "define HELP\nUsage: make <target>\n  define NESTED\nx: y\n  endef\nendef\n\
                    export define SCRIPT =\nrun: it\nendef\nbuild:\n\tcc\n";
        assert_eq!(make_targets(text), vec!["build".to_string()]);
    }

    #[test]
    fn justfile_recipes() {
        let text = "set shell := [\"bash\", \"-c\"]\nversion := \"1\"\nalias b := build\n\
                    default:\n  just --list\nbuild target='x':\n  cargo build\n@test *args: build\n  cargo test\n\
                    _helper:\n  echo\n[private]\nhidden:\n  echo\n[group('x')]\nlint:\n  echo\n";
        assert_eq!(
            just_recipes(text),
            ["default", "build", "test", "lint"]
                .map(String::from)
                .to_vec()
        );
    }
}
