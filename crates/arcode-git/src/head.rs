//! Branch detection by reading `HEAD` and refs from disk (no process spawn,
//! except for the rare reftable ref backend).

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::Repo;

/// Number of hex digits shown for a detached `HEAD`.
const SHORT_SHA_LEN: usize = 7;

/// Branch name git writes into `HEAD` when refs live in a reftable.
const REFTABLE_PLACEHOLDER: &str = ".invalid";

/// What `HEAD` points at.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Head {
    /// On a branch that has at least one commit.
    Branch(String),
    /// Detached at a commit; holds the abbreviated (7 hex digit) id. Empty
    /// when the state could not be determined (see [`Repo::head`]).
    Detached(String),
    /// On a branch without commits yet (freshly initialized repository).
    Unborn(String),
}

impl Head {
    /// The branch name, for both born and unborn branches.
    pub fn branch_name(&self) -> Option<&str> {
        match self {
            Head::Branch(name) | Head::Unborn(name) => Some(name),
            Head::Detached(_) => None,
        }
    }
}

impl fmt::Display for Head {
    /// Formats like a status bar: `main` or `detached@abc1234`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Head::Branch(name) | Head::Unborn(name) => f.write_str(name),
            Head::Detached(sha) if sha.is_empty() => f.write_str("detached"),
            Head::Detached(sha) => write!(f, "detached@{sha}"),
        }
    }
}

impl Repo {
    /// Reads the current `HEAD`.
    ///
    /// Never fails: if `HEAD` is unreadable or malformed this returns
    /// `Head::Detached(String::new())`. Use [`Repo::try_head`] to see errors.
    pub fn head(&self) -> Head {
        self.try_head()
            .unwrap_or_else(|_| Head::Detached(String::new()))
    }

    /// Reads the current `HEAD`, reporting I/O errors and malformed content.
    ///
    /// Works purely from files (`HEAD`, loose refs, `packed-refs`). Only for
    /// repositories using the reftable ref backend, whose refs are binary,
    /// it falls back to the `git` CLI.
    pub fn try_head(&self) -> io::Result<Head> {
        let text = fs::read_to_string(self.git_dir.join("HEAD"))?;
        let content = text.trim();
        if let Some(target) = content.strip_prefix("ref:") {
            let refname = target.trim();
            if refname.is_empty() {
                return Err(malformed("empty symbolic ref in HEAD"));
            }
            let name = refname.strip_prefix("refs/heads/").unwrap_or(refname);
            if name == REFTABLE_PLACEHOLDER && self.uses_reftable() {
                return crate::cli::head_via_cli(self)
                    .ok_or_else(|| io::Error::other("cannot resolve reftable HEAD"));
            }
            return Ok(if self.ref_exists(refname) {
                Head::Branch(name.to_owned())
            } else {
                Head::Unborn(name.to_owned())
            });
        }
        parse_object_id(content)
            .map(|sha| Head::Detached(sha[..SHORT_SHA_LEN].to_owned()))
            .ok_or_else(|| malformed("HEAD is neither a ref nor an object id"))
    }

    /// The branch for a status bar: `main`, or `detached@abc1234`.
    pub fn branch_display(&self) -> String {
        self.head().to_string()
    }

    /// True if `refname` (e.g. `refs/heads/main`) exists as a loose ref or
    /// in `packed-refs`.
    fn ref_exists(&self, refname: &str) -> bool {
        let rel = Path::new(refname);
        if self.common_dir.join(rel).is_file() || self.git_dir.join(rel).is_file() {
            return true;
        }
        [&self.common_dir, &self.git_dir].iter().any(|dir| {
            fs::read_to_string(dir.join("packed-refs"))
                .is_ok_and(|packed| packed_refs_contain(&packed, refname))
        })
    }

    fn uses_reftable(&self) -> bool {
        self.common_dir.join("reftable").is_dir() || self.git_dir.join("reftable").is_dir()
    }
}

fn malformed(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}

/// Returns `text` if it is a full SHA-1 (40) or SHA-256 (64) hex object id.
fn parse_object_id(text: &str) -> Option<&str> {
    let valid_len = text.len() == 40 || text.len() == 64;
    (valid_len && text.bytes().all(|b| b.is_ascii_hexdigit())).then_some(text)
}

/// Checks a `packed-refs` file body for an entry named exactly `refname`.
fn packed_refs_contain(packed: &str, refname: &str) -> bool {
    packed
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .filter_map(|line| line.split_once(' '))
        .any(|(_, name)| name.trim_end() == refname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids() {
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(parse_object_id(sha1), Some(sha1));
        assert_eq!(parse_object_id(&"a".repeat(64)).map(str::len), Some(64));
        assert_eq!(parse_object_id("abc"), None);
        assert_eq!(parse_object_id(&"g".repeat(40)), None);
    }

    #[test]
    fn packed_refs_lookup() {
        let packed = "# pack-refs with: peeled fully-peeled sorted \n\
            1111111111111111111111111111111111111111 refs/heads/main\n\
            2222222222222222222222222222222222222222 refs/tags/v1\n\
            ^3333333333333333333333333333333333333333\n";
        assert!(packed_refs_contain(packed, "refs/heads/main"));
        assert!(packed_refs_contain(packed, "refs/tags/v1"));
        assert!(!packed_refs_contain(packed, "refs/heads/ma"));
        assert!(!packed_refs_contain(packed, "refs/heads/dev"));
    }

    #[test]
    fn display_forms() {
        assert_eq!(Head::Branch("main".into()).to_string(), "main");
        assert_eq!(Head::Unborn("trunk".into()).to_string(), "trunk");
        assert_eq!(
            Head::Detached("abc1234".into()).to_string(),
            "detached@abc1234"
        );
        assert_eq!(Head::Detached(String::new()).to_string(), "detached");
        assert_eq!(Head::Unborn("x".into()).branch_name(), Some("x"));
        assert_eq!(Head::Detached("a".into()).branch_name(), None);
    }
}
