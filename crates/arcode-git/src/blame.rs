//! Line authorship from `git blame --porcelain`.

use std::collections::HashMap;
use std::path::Path;

use crate::exec::{exec, repo_path};
use crate::hunk::{Base, blob_text};
use crate::{GitError, GitResult, Repo};

/// Hex digits kept for [`BlameLine::commit`].
const SHORT_SHA: usize = 7;

/// Who last changed one line.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlameLine {
    /// Zero-based line in the blamed text (git counts `\n` lines).
    pub line: usize,
    /// Abbreviated commit id (`0000000` for uncommitted lines).
    pub commit: String,
    /// Author name.
    pub author: String,
    /// Author time, seconds since the Unix epoch.
    pub author_time: i64,
    /// First line of the commit message.
    pub summary: String,
    /// True for lines changed in the working tree or buffer.
    pub is_uncommitted: bool,
}

/// Blames `rel_path` at `HEAD` plus local changes.
///
/// With `contents` (the editor buffer, for a dirty buffer) git blames that
/// text via `--contents -`, so line numbers match the buffer; otherwise the
/// file on disk is blamed. A file that is not in `HEAD` (new, untracked, or
/// a repository without commits) yields only uncommitted lines.
pub fn blame(repo: &Repo, rel_path: &Path, contents: Option<&str>) -> GitResult<Vec<BlameLine>> {
    let path = repo_path(repo, rel_path)?;
    match blob_text(repo, rel_path, "HEAD:") {
        Base::Text(_) => {}
        Base::Binary => return Err(GitError::Binary(path.rel)),
        _ => {
            let text = match contents {
                Some(text) => text.to_owned(),
                None => {
                    std::fs::read_to_string(repo.workdir.join(&path.rel)).map_err(GitError::Io)?
                }
            };
            let count = text.split_inclusive('\n').count();
            return Ok((0..count).map(uncommitted).collect());
        }
    }
    let mut args = vec!["blame", "--porcelain"];
    if contents.is_some() {
        args.extend(["--contents", "-"]);
    }
    args.extend(["--", &path.git]);
    let out = exec(repo, &args, contents.map(str::as_bytes))?;
    parse_porcelain(&String::from_utf8_lossy(&out))
}

fn uncommitted(line: usize) -> BlameLine {
    BlameLine {
        line,
        commit: "0".repeat(SHORT_SHA),
        author: "Not Committed Yet".into(),
        author_time: 0,
        summary: String::new(),
        is_uncommitted: true,
    }
}

/// Per-commit details, printed by git only the first time a commit appears.
#[derive(Default, Clone)]
struct CommitInfo {
    author: String,
    author_time: i64,
    summary: String,
}

/// Parses `git blame --porcelain` output into lines sorted by line number.
pub(crate) fn parse_porcelain(out: &str) -> GitResult<Vec<BlameLine>> {
    let mut commits: HashMap<&str, CommitInfo> = HashMap::new();
    let mut lines = Vec::new();
    let mut current: Option<(&str, usize)> = None;
    for raw in out.split('\n') {
        if raw.starts_with('\t') {
            // Content line: ends the entry.
            let (sha, line) = current.take().ok_or_else(|| parse_error(raw))?;
            let info = commits.get(sha).cloned().unwrap_or_default();
            let is_uncommitted = sha.bytes().all(|b| b == b'0');
            lines.push(BlameLine {
                line,
                commit: sha.chars().take(SHORT_SHA).collect(),
                author: info.author,
                author_time: info.author_time,
                summary: info.summary,
                is_uncommitted,
            });
            continue;
        }
        let Some((sha, _)) = current else {
            if raw.is_empty() {
                continue;
            }
            current = Some(parse_header(raw).ok_or_else(|| parse_error(raw))?);
            continue;
        };
        let (key, value) = raw.split_once(' ').unwrap_or((raw, ""));
        let info = commits.entry(sha).or_default();
        match key {
            "author" => info.author = value.to_owned(),
            "author-time" => info.author_time = value.parse().unwrap_or(0),
            "summary" => info.summary = value.to_owned(),
            _ => {}
        }
    }
    lines.sort_by_key(|l| l.line);
    Ok(lines)
}

/// `<sha> <orig line> <final line> [<group size>]`, final line 1-based.
fn parse_header(raw: &str) -> Option<(&str, usize)> {
    let mut parts = raw.split(' ');
    let sha = parts.next().filter(|s| s.len() >= 40)?;
    if !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let _orig = parts.next()?;
    let final_line: usize = parts.next()?.parse().ok()?;
    Some((sha, final_line.checked_sub(1)?))
}

fn parse_error(raw: &str) -> GitError {
    GitError::Parse(format!("blame: {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const Z: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn parses_groups_and_repeated_commits() {
        let out = format!(
            "{A} 1 1 2\nauthor Ann\nauthor-mail <a@x>\nauthor-time 100\n\
             author-tz +0000\nsummary First commit\nfilename f.txt\n\tone\n\
             {A} 2 2\n\ttwo\n\
             {Z} 3 3 1\nauthor Not Committed Yet\nauthor-time 200\n\
             summary Version of f.txt from f.txt\nfilename f.txt\n\tthree\n"
        );
        let lines = parse_porcelain(&out).unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].commit, "aaaaaaa");
        assert_eq!(lines[1].line, 1);
        assert_eq!(lines[1].author, "Ann");
        assert_eq!(lines[1].author_time, 100);
        assert_eq!(lines[1].summary, "First commit");
        assert!(!lines[1].is_uncommitted);
        assert!(lines[2].is_uncommitted);
        assert_eq!(lines[2].commit, "0000000");
    }

    #[test]
    fn content_may_look_like_headers() {
        let out = format!("{A} 1 1 1\nauthor B\nsummary s\n\t{A} 9 9\n");
        let lines = parse_porcelain(&out).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].author, "B");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_porcelain("nonsense\n").is_err());
        assert!(parse_porcelain("\tcontent without header\n").is_err());
        assert!(parse_porcelain("").unwrap().is_empty());
    }
}
