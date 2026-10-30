//! The result model of the Search view: files in path order with their
//! matching lines, collapsible, flattened into list rows.

use std::collections::HashSet;
use std::path::PathBuf;

use arcode_search::{FileMatches, LineMatch};

/// One list row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The header of file `file`.
    File(usize),
    /// Line `line` of file `file`.
    Line(usize, usize),
}

/// Streamed results, kept sorted by relative path.
#[derive(Default)]
pub struct Results {
    files: Vec<FileMatches>,
    collapsed: HashSet<PathBuf>,
    matches: usize,
}

impl Results {
    /// Drops every file (collapsed state is kept for the next run).
    pub fn clear(&mut self) {
        self.files.clear();
        self.matches = 0;
    }

    /// Adds a file's matches, keeping the path order. A file already
    /// listed is replaced.
    pub fn insert(&mut self, file: FileMatches) {
        match self
            .files
            .binary_search_by(|f| f.relative.cmp(&file.relative))
        {
            Ok(ix) => {
                self.matches -= self.files[ix].match_count();
                self.matches += file.match_count();
                self.files[ix] = file;
            }
            Err(ix) => {
                self.matches += file.match_count();
                self.files.insert(ix, file);
            }
        }
    }

    /// Removes the file at `path`, if listed.
    pub fn remove(&mut self, path: &std::path::Path) {
        if let Some(ix) = self.files.iter().position(|f| f.path == path) {
            self.matches -= self.files[ix].match_count();
            self.files.remove(ix);
        }
    }

    /// The files in display order.
    pub fn files(&self) -> &[FileMatches] {
        &self.files
    }

    /// Total matches over all files.
    pub fn match_count(&self) -> usize {
        self.matches
    }

    /// Whether the file is collapsed.
    pub fn is_collapsed(&self, file: &FileMatches) -> bool {
        self.collapsed.contains(&file.path)
    }

    /// Collapses or expands the file at index `ix`.
    pub fn toggle(&mut self, ix: usize) {
        if let Some(path) = self.files.get(ix).map(|f| f.path.clone())
            && !self.collapsed.remove(&path)
        {
            self.collapsed.insert(path);
        }
    }

    /// Collapses every file, or expands all when all are collapsed.
    pub fn toggle_all(&mut self) {
        let all = self.files.iter().all(|f| self.collapsed.contains(&f.path));
        if all {
            self.collapsed.clear();
        } else {
            self.collapsed
                .extend(self.files.iter().map(|f| f.path.clone()));
        }
    }

    /// The rows to display.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::with_capacity(self.files.len() + self.matches);
        for (fx, file) in self.files.iter().enumerate() {
            rows.push(Row::File(fx));
            if !self.is_collapsed(file) {
                rows.extend((0..file.matches.len()).map(|lx| Row::Line(fx, lx)));
            }
        }
        rows
    }

    /// The line behind a row.
    pub fn line(&self, row: Row) -> Option<(&FileMatches, &LineMatch)> {
        match row {
            Row::Line(fx, lx) => {
                let file = self.files.get(fx)?;
                Some((file, file.matches.get(lx)?))
            }
            Row::File(_) => None,
        }
    }
}

/// Chars of context kept before the first match of a result line.
const LEAD: usize = 24;

/// The line text with leading whitespace removed, cut to start shortly
/// before the first match (marked with `…`), and the match ranges shifted
/// to fit (char ranges).
pub fn trimmed_line(line: &LineMatch) -> (String, Vec<std::ops::Range<usize>>) {
    let indent = line
        .line_text
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();
    let first = line.ranges.first().map_or(indent, |r| r.start);
    let (skip, prefix) = if first > indent + LEAD {
        (first - LEAD / 2, "…")
    } else {
        (indent.min(first), "")
    };
    let shift = prefix.chars().count();
    let text: String = prefix
        .chars()
        .chain(line.line_text.chars().skip(skip))
        .collect();
    let ranges = line
        .ranges
        .iter()
        .filter(|r| r.start >= skip)
        .map(|r| r.start - skip + shift..r.end - skip + shift)
        .collect();
    (text, ranges)
}

/// File name and the folder it is in (`src/git`, with `/`), for display.
pub fn split_path(path: &std::path::Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path
        .parent()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    (name, dir)
}

/// Char ranges to byte ranges of `text`.
pub fn char_to_byte_ranges(
    text: &str,
    ranges: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    let offsets: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    ranges
        .iter()
        .filter_map(|r| Some(*offsets.get(r.start)?..*offsets.get(r.end)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel: &str, lines: &[usize]) -> FileMatches {
        FileMatches {
            path: PathBuf::from("/p").join(rel),
            relative: PathBuf::from(rel),
            matches: lines
                .iter()
                .map(|&line| LineMatch {
                    line,
                    line_text: "  x = x".into(),
                    ranges: vec![2..3, 6..7],
                    columns: vec![2..3, 6..7],
                })
                .collect(),
        }
    }

    #[test]
    fn files_stay_sorted_and_counted() {
        let mut r = Results::default();
        r.insert(file("src/b.rs", &[1]));
        r.insert(file("a.rs", &[0, 4]));
        r.insert(file("src/b.rs", &[2, 3]));
        let names: Vec<_> = r.files().iter().map(|f| f.relative.clone()).collect();
        assert_eq!(names, [PathBuf::from("a.rs"), PathBuf::from("src/b.rs")]);
        assert_eq!(r.match_count(), 8);
        r.remove(&PathBuf::from("/p/a.rs"));
        assert_eq!(r.match_count(), 4);
    }

    #[test]
    fn rows_follow_collapsing() {
        let mut r = Results::default();
        r.insert(file("a.rs", &[0, 4]));
        r.insert(file("b.rs", &[1]));
        assert_eq!(r.rows().len(), 5);
        r.toggle(0);
        assert_eq!(r.rows(), vec![Row::File(0), Row::File(1), Row::Line(1, 0)]);
        r.toggle_all();
        assert_eq!(r.rows(), vec![Row::File(0), Row::File(1)]);
        r.toggle_all();
        assert_eq!(r.rows().len(), 5);
        let (f, l) = r.line(Row::Line(0, 1)).unwrap();
        assert_eq!((f.relative.to_str(), l.line), (Some("a.rs"), 4));
    }

    #[test]
    fn trimming_keeps_match_ranges() {
        let line = LineMatch {
            line: 0,
            line_text: "    let é = foo;".into(),
            ranges: std::iter::once(12..15).collect(),
            columns: std::iter::once(12..15).collect(),
        };
        let (text, ranges) = trimmed_line(&line);
        assert_eq!(text, "let é = foo;");
        assert_eq!(ranges, vec![8..11]);
        assert_eq!(char_to_byte_ranges(&text, &ranges), vec![9..12]);
        let long = LineMatch {
            line: 0,
            line_text: format!("{}needle", "x".repeat(60)),
            ranges: std::iter::once(60..66).collect(),
            columns: std::iter::once(60..66).collect(),
        };
        let (text, ranges) = trimmed_line(&long);
        assert!(text.starts_with('…'));
        let shown: String = text.chars().skip(ranges[0].start).take(6).collect();
        assert_eq!(shown, "needle");
    }
}
