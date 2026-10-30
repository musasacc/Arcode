//! Loading a directory of user themes.
//!
//! All files are parsed first; themes are then built in file order, each
//! `extends` target being built on demand, so a theme may extend a user
//! theme from any file. Cycles are reported and broken by inheriting from
//! the built-in theme of the same appearance.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::registry::builtin_base;
use super::{Appearance, Theme, ThemeError, ThemeRegistry, parse};

/// One `*.toml` file of the themes directory.
struct ThemeFile {
    /// Path for messages.
    shown: String,
    /// The file name without extension (fallback theme name).
    stem: Option<String>,
    /// Parsed content; `None` when unreadable or not valid TOML.
    table: Option<Table>,
    /// The theme name the table will produce.
    name: Option<String>,
    /// Warnings for this file, without the path prefix.
    warnings: Vec<String>,
}

/// Build progress of one file.
#[derive(Clone)]
enum Slot {
    Pending,
    Building,
    Built(Option<Box<Theme>>),
}

/// Adds every theme in `dir` to `registry`, returning warnings prefixed with
/// the file path. A missing directory yields nothing.
pub(super) fn load_dir(registry: &mut ThemeRegistry, dir: &Path) -> Vec<String> {
    let paths = match theme_files(dir) {
        Ok(paths) => paths,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => return vec![format!("cannot read themes in {}: {err}", dir.display())],
    };
    let files: Vec<ThemeFile> = paths.iter().map(|path| read_file(path)).collect();
    let mut builder = Builder {
        slots: vec![Slot::Pending; files.len()],
        files,
        registry: &*registry,
    };
    for index in 0..builder.files.len() {
        builder.build(index);
    }
    let Builder { files, slots, .. } = builder;

    let mut warnings = Vec::new();
    let mut user_names: Vec<String> = Vec::new();
    for (file, slot) in files.into_iter().zip(slots) {
        let shown = file.shown;
        warnings.extend(file.warnings.into_iter().map(|w| format!("{shown}: {w}")));
        let Slot::Built(Some(theme)) = slot else {
            continue;
        };
        if user_names.contains(&theme.name) {
            warnings.push(format!(
                "{shown}: replaces earlier theme \"{}\"",
                theme.name
            ));
        }
        user_names.push(theme.name.clone());
        registry.insert(*theme);
    }
    warnings
}

fn read_file(path: &Path) -> ThemeFile {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let mut file = ThemeFile {
        shown: path.display().to_string(),
        stem,
        table: None,
        name: None,
        warnings: Vec::new(),
    };
    let parsed = fs::read_to_string(path)
        .map_err(|err| format!("cannot read: {err}"))
        .and_then(|source| parse::parse_table(&source).map_err(|err| format!("{err}; skipped")));
    match parsed {
        Ok(table) => {
            file.name = parse::theme_name(&table, file.stem.as_deref());
            file.table = Some(table);
        }
        Err(err) => file.warnings.push(err),
    }
    file
}

struct Builder<'a> {
    files: Vec<ThemeFile>,
    slots: Vec<Slot>,
    /// The registry before any user theme is added (built-ins).
    registry: &'a ThemeRegistry,
}

impl Builder<'_> {
    /// Builds file `index` (and, first, the theme it extends).
    fn build(&mut self, index: usize) -> Option<Theme> {
        match &self.slots[index] {
            Slot::Built(theme) => return theme.as_deref().cloned(),
            Slot::Building => return None,
            Slot::Pending => {}
        }
        let Some(table) = self.files[index].table.take() else {
            self.slots[index] = Slot::Built(None);
            return None;
        };
        self.slots[index] = Slot::Building;
        let mut warnings = Vec::new();
        let base = base_theme(&table, &mut warnings, |name| self.lookup(index, name));
        let fallback = self.files[index].stem.clone();
        let theme = match Theme::from_table(&table, Some(&base), fallback.as_deref()) {
            Ok((theme, theme_warnings)) => {
                warnings.extend(theme_warnings);
                Some(theme)
            }
            Err(err) => {
                warnings.push(format!("{err}; skipped"));
                None
            }
        };
        self.files[index].warnings.extend(warnings);
        self.slots[index] = Slot::Built(theme.clone().map(Box::new));
        theme
    }

    /// Resolves an `extends` target for file `from`: the last other user
    /// file defining that name (exact, then case-insensitive), else a
    /// built-in theme.
    fn lookup(&mut self, from: usize, name: &str) -> Result<Theme, String> {
        let name = name.trim();
        let defines = |exact: bool| {
            self.files
                .iter()
                .enumerate()
                .rev()
                .filter(|(i, _)| *i != from)
                .find(|(_, f)| {
                    f.name.as_deref().is_some_and(|n| {
                        if exact {
                            n == name
                        } else {
                            n.eq_ignore_ascii_case(name)
                        }
                    })
                })
                .map(|(i, _)| i)
        };
        let Some(target) = defines(true).or_else(|| defines(false)) else {
            return self
                .registry
                .get(name)
                .cloned()
                .ok_or_else(|| format!("unknown theme \"{name}\""));
        };
        if matches!(self.slots[target], Slot::Building) {
            return Err(format!("cycle through theme \"{name}\""));
        }
        self.build(target)
            .ok_or_else(|| format!("theme \"{name}\" could not be loaded"))
    }
}

/// The theme a user theme table inherits from: its `extends` target as
/// found by `lookup`, else the built-in theme of its appearance (also used,
/// with a warning, when the target cannot be resolved).
pub(super) fn base_theme(
    table: &Table,
    warnings: &mut Vec<String>,
    lookup: impl FnOnce(&str) -> Result<Theme, String>,
) -> Theme {
    let appearance = table
        .get("appearance")
        .and_then(Value::as_str)
        .and_then(Appearance::parse)
        .unwrap_or_default();
    let fallback = || builtin_base(appearance).clone();
    match table.get("extends") {
        None => fallback(),
        Some(Value::String(name)) => lookup(name).unwrap_or_else(|err| {
            warnings.push(format!("`extends`: {err}"));
            fallback()
        }),
        Some(other) => {
            warnings.push(format!(
                "`extends` must be a theme name, found {}; ignored",
                other.type_str()
            ));
            fallback()
        }
    }
}

/// `*.toml` files in `dir`, sorted by path for deterministic loading.
fn theme_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_toml = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
        if is_toml && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Parses one user theme on top of `registry`'s themes (for `extends`).
pub(super) fn user_theme(
    registry: &ThemeRegistry,
    source: &str,
    fallback_name: Option<&str>,
) -> Result<(Theme, Vec<String>), ThemeError> {
    let table = parse::parse_table(source)?;
    let mut warnings = Vec::new();
    let base = base_theme(&table, &mut warnings, |name| {
        registry
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown theme \"{}\"", name.trim()))
    });
    let (theme, theme_warnings) = Theme::from_table(&table, Some(&base), fallback_name)?;
    warnings.extend(theme_warnings);
    Ok((theme, warnings))
}
