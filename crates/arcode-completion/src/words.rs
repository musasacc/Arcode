//! Buffer-word index: the fallback completion source when no server helps.

use std::collections::HashMap;
use std::ops::Range;

use arcode_buffer::{Buffer, Edit};

use crate::candidate::{CandidateKind, CandidateSource, CompletionCandidate};

/// Minimum word length in chars.
pub const MIN_WORD_LEN: usize = 3;
/// Words longer than this (in chars) are ignored (minified code, hashes).
pub const MAX_WORD_LEN: usize = 64;

/// Word occurrence counts per line, kept in sync with a document.
///
/// Words are identifiers: a letter (any Unicode letter) or `_`, followed by
/// letters, digits or `_`; at least [`MIN_WORD_LEN`] chars.
#[derive(Debug, Clone, Default)]
pub struct WordIndex {
    /// Words of each line; `None` marks a line pending re-indexing.
    lines: Vec<Option<Vec<Box<str>>>>,
    counts: HashMap<Box<str>, u32>,
}

impl WordIndex {
    /// An empty index.
    pub fn new() -> Self {
        WordIndex::default()
    }

    /// An index of `text`.
    pub fn from_text(text: &str) -> Self {
        let mut index = WordIndex::new();
        index.index_text(text);
        index
    }

    /// Replaces the whole index with the words of `text`.
    pub fn index_text(&mut self, text: &str) {
        self.lines.clear();
        self.counts.clear();
        for line in text.split('\n') {
            let words = self.add_line_words(line);
            self.lines.push(Some(words));
        }
    }

    /// Replaces lines `lines` (clamped to the index) with the lines of
    /// `new_text` (split on `\n`; an empty string yields one empty line).
    pub fn update_lines(&mut self, lines: Range<usize>, new_text: &str) {
        let start = lines.start.min(self.lines.len());
        let end = lines.end.clamp(start, self.lines.len());
        self.remove_lines(start..end);
        let new: Vec<_> = new_text
            .split('\n')
            .map(|line| Some(self.add_line_words(line)))
            .collect();
        self.lines.splice(start..start, new);
    }

    /// Applies buffer edits (in the order returned by the buffer) and then
    /// re-indexes the touched lines from `buffer` (its state after them).
    pub fn apply_edits(&mut self, edits: &[Edit], buffer: &Buffer) {
        for edit in edits {
            let start = edit.start_point.line.min(self.lines.len());
            let end = (edit.old_end_point.line + 1).clamp(start, self.lines.len());
            self.remove_lines(start..end);
            let added = edit.new_end_point.line - edit.start_point.line + 1;
            self.lines
                .splice(start..start, std::iter::repeat_n(None, added));
        }
        // Keep the line count in sync with the rope even after odd input.
        let want = buffer.line_count();
        if self.lines.len() > want {
            let len = self.lines.len();
            self.remove_lines(want..len);
        }
        self.lines.resize(want, None);
        for i in 0..self.lines.len() {
            if self.lines[i].is_none() {
                let line = buffer.line_text(i);
                let words = self.add_line_words(&line);
                self.lines[i] = Some(words);
            }
        }
    }

    /// Number of indexed lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Number of distinct words.
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    /// Whether the index holds no words.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// How often `word` occurs.
    pub fn count(&self, word: &str) -> u32 {
        self.counts.get(word).copied().unwrap_or(0)
    }

    /// Word candidates whose text fuzzy-contains `prefix` (case-insensitive
    /// subsequence; empty matches all), most frequent first.
    ///
    /// `exclude` is the word under the cursor: it is skipped when it occurs
    /// only once (that occurrence is the one being typed).
    pub fn candidates_for(&self, prefix: &str, exclude: Option<&str>) -> Vec<CompletionCandidate> {
        let needle: Vec<char> = prefix.chars().flat_map(char::to_lowercase).collect();
        let mut hits: Vec<(&str, u32)> = self
            .counts
            .iter()
            .filter(|(w, n)| !(exclude == Some(&***w) && **n <= 1))
            .filter(|(w, _)| is_subsequence(&needle, w))
            .map(|(w, n)| (&**w, *n))
            .collect();
        hits.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let width = hits.len().to_string().len();
        hits.into_iter()
            .enumerate()
            .map(|(i, (word, _))| {
                let mut c = CompletionCandidate::new(
                    word,
                    CandidateKind::Word,
                    CandidateSource::BufferWord,
                );
                c.sort_text = Some(format!("{i:0width$}"));
                c
            })
            .collect()
    }

    fn add_line_words(&mut self, line: &str) -> Vec<Box<str>> {
        let words: Vec<Box<str>> = words_in(line).map(Box::from).collect();
        for w in &words {
            *self.counts.entry(w.clone()).or_insert(0) += 1;
        }
        words
    }

    fn remove_lines(&mut self, range: Range<usize>) {
        for words in self.lines.drain(range).flatten() {
            for w in words {
                if let Some(n) = self.counts.get_mut(&w) {
                    *n -= 1;
                    if *n == 0 {
                        self.counts.remove(&w);
                    }
                }
            }
        }
    }
}

fn is_subsequence(needle: &[char], word: &str) -> bool {
    let mut it = needle.iter().peekable();
    for c in word.chars().flat_map(char::to_lowercase) {
        if it.peek() == Some(&&c) {
            it.next();
        }
    }
    it.peek().is_none()
}

fn is_word_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Iterates the indexable words of `text` (see [`WordIndex`]).
pub fn words_in(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (start, c) = rest.next()?;
            if !is_word_char(c) {
                continue;
            }
            let mut end = start + c.len_utf8();
            let mut len = 1;
            while let Some(&(i, c)) = rest.peek() {
                if !is_word_char(c) {
                    break;
                }
                end = i + c.len_utf8();
                len += 1;
                rest.next();
            }
            if is_word_start(c) && (MIN_WORD_LEN..=MAX_WORD_LEN).contains(&len) {
                return Some(&text[start..end]);
            }
        }
    })
}

/// The word around char column `col` of `line` as `(start_col, word)`,
/// e.g. the partially typed word before the cursor plus its tail.
pub fn word_at(line: &str, col: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let col = col.min(chars.len());
    let mut start = col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    (start < end).then(|| (start, chars[start..end].iter().collect()))
}

#[cfg(test)]
#[path = "words_tests.rs"]
mod tests;
