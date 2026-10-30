//! File-system path completion inside string literals.

use std::path::{Path, PathBuf};

use crate::candidate::{CandidateKind, CandidateSource, CompletionCandidate, InsertAction};

/// At most this many directory entries are returned.
pub const MAX_PATH_ENTRIES: usize = 2000;

/// Whether `typed` looks like a path: `./`, `../`, `/`, `~/` (and `.\`,
/// `..\`, `~\`, `C:\`, `C:/` on Windows).
pub fn looks_like_path(typed: &str) -> bool {
    const UNIX: [&str; 4] = ["./", "../", "/", "~/"];
    if UNIX.iter().any(|p| typed.starts_with(p)) {
        return true;
    }
    cfg!(windows) && looks_like_windows_path(typed)
}

fn looks_like_windows_path(typed: &str) -> bool {
    const WIN: [&str; 4] = [".\\", "..\\", "~\\", "\\"];
    if WIN.iter().any(|p| typed.starts_with(p)) {
        return true;
    }
    let b = typed.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Splits `typed` into its directory part (with trailing separator) and the
/// partial file name after the last separator.
pub fn split_typed(typed: &str) -> (&str, &str) {
    match typed.rfind(is_separator) {
        Some(i) => typed.split_at(i + 1),
        None => ("", typed),
    }
}

/// The partial file name to rank path candidates against and to replace on
/// accept (the text after the last separator).
pub fn path_query(typed: &str) -> &str {
    split_typed(typed).1
}

fn resolve_dir(base_dir: &Path, dir: &str) -> Option<PathBuf> {
    let rest = dir
        .strip_prefix("~/")
        .or_else(|| cfg!(windows).then(|| dir.strip_prefix("~\\")).flatten());
    if let Some(rest) = rest {
        return Some(std::env::home_dir()?.join(rest));
    }
    let path = Path::new(dir);
    if path.is_absolute() || dir.starts_with('/') {
        Some(path.to_path_buf())
    } else {
        Some(base_dir.join(path))
    }
}

/// Lists the entries of the directory `typed` points into, relative to
/// `base_dir`. Returns nothing unless [`looks_like_path`].
///
/// Labels are entry names; folders get a trailing `/` (`\` on Windows when
/// the user typed backslashes). Hidden (dot) entries appear only when the
/// partial name starts with `.`. Folders sort first, then by name; the
/// fuzzy filtering on [`path_query`] is left to [`crate::rank`].
pub fn path_completions(base_dir: &Path, typed: &str) -> Vec<CompletionCandidate> {
    if !looks_like_path(typed) {
        return Vec::new();
    }
    let (dir, partial) = split_typed(typed);
    let Some(dir_path) = resolve_dir(base_dir, dir) else {
        return Vec::new();
    };
    let Ok(read) = std::fs::read_dir(&dir_path) else {
        return Vec::new();
    };
    let sep = if cfg!(windows) && typed.contains('\\') && !typed.contains('/') {
        '\\'
    } else {
        '/'
    };
    let show_hidden = partial.starts_with('.');
    let mut entries: Vec<(bool, String)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with('.') && !show_hidden {
                return None;
            }
            // Follows symlinks, so a link to a folder counts as a folder.
            let is_dir = std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir());
            Some((is_dir, name))
        })
        .take(MAX_PATH_ENTRIES)
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    entries
        .into_iter()
        .enumerate()
        .map(|(i, (is_dir, name))| {
            let label = if is_dir {
                format!("{name}{sep}")
            } else {
                name.clone()
            };
            let kind = if is_dir {
                CandidateKind::Folder
            } else {
                CandidateKind::File
            };
            let mut c = CompletionCandidate::new(label.clone(), kind, CandidateSource::Path);
            c.filter_text = Some(name);
            c.sort_text = Some(format!("{i:06}"));
            c.insert = InsertAction::PlainText(label);
            c.data = i as u64;
            c
        })
        .collect()
}

/// If char column `cursor_col` of `line_text` is inside a string literal
/// (`"`, `'` or `` ` ``) whose content up to the cursor looks like a path,
/// returns `(start_col, typed)`: the char column where the content starts
/// and the content up to the cursor.
pub fn detect_path_prefix(line_text: &str, cursor_col: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = line_text.chars().collect();
    if cursor_col > chars.len() {
        return None;
    }
    let mut open: Option<(char, usize)> = None;
    let mut i = 0;
    while i < cursor_col {
        let c = chars[i];
        match open {
            // A backslash escapes the next char (so `\\` stays inside).
            Some(_) if c == '\\' => i += 1,
            Some((q, _)) if c == q => open = None,
            Some(_) => {}
            None if matches!(c, '"' | '\'' | '`') => open = Some((c, i + 1)),
            None => {}
        }
        i += 1;
    }
    let (_, start) = open?;
    // An escape right before the cursor may have stepped past it.
    let typed: String = chars[start..cursor_col.max(start)].iter().collect();
    looks_like_path(&typed).then_some((start, typed))
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
