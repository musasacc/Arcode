//! Local branches: listing and switching (never fetches).

use crate::exec::{exec, exec_text};
use crate::{GitError, GitResult, Repo};

/// A local branch (`refs/heads/*`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Branch {
    /// Branch name without `refs/heads/`.
    pub name: String,
    /// True for the checked-out branch.
    pub is_current: bool,
    /// Abbreviated id of the tip commit.
    pub commit: String,
    /// Subject of the tip commit.
    pub summary: String,
    /// Configured upstream, e.g. `origin/main` (as last fetched; no network).
    pub upstream: Option<String>,
    /// Commits on this branch that the upstream lacks.
    pub ahead: usize,
    /// Commits on the upstream that this branch lacks.
    pub behind: usize,
    /// True when the upstream is configured but its ref no longer exists.
    pub upstream_gone: bool,
}

/// `for-each-ref` format: NUL-separated fields, one branch per line.
const FORMAT: &str = "--format=%(refname)%00%(HEAD)%00%(objectname:short)%00\
                      %(upstream:short)%00%(upstream:track,nobracket)%00%(subject)";

/// Lists local branches sorted by name. An unborn current branch (no
/// commits yet) has no ref and is not listed; see [`Repo::head`].
pub fn branches(repo: &Repo) -> GitResult<Vec<Branch>> {
    let out = exec_text(repo, &["for-each-ref", FORMAT, "refs/heads"], None)?;
    out.lines()
        .filter(|l| !l.is_empty())
        .map(|line| parse_branch(line).ok_or_else(|| GitError::Parse(line.to_owned())))
        .collect()
}

fn parse_branch(line: &str) -> Option<Branch> {
    let mut f = line.splitn(6, '\0');
    let name = f.next()?.strip_prefix("refs/heads/")?.to_owned();
    let is_current = f.next()? == "*";
    let commit = f.next()?.to_owned();
    let upstream = Some(f.next()?.to_owned()).filter(|u| !u.is_empty());
    let track = f.next()?;
    let summary = f.next()?.to_owned();
    let (ahead, behind) = parse_track(track);
    Some(Branch {
        name,
        is_current,
        commit,
        summary,
        upstream,
        ahead,
        behind,
        upstream_gone: track == "gone",
    })
}

/// Parses `ahead 1, behind 2` (either part optional).
fn parse_track(track: &str) -> (usize, usize) {
    let (mut ahead, mut behind) = (0, 0);
    for part in track.split(", ") {
        match part.split_once(' ') {
            Some(("ahead", n)) => ahead = n.parse().unwrap_or(0),
            Some(("behind", n)) => behind = n.parse().unwrap_or(0),
            _ => {}
        }
    }
    (ahead, behind)
}

/// Switches the working tree to the existing local branch `name`.
///
/// `name` must be a branch name exactly as listed by [`branches`]:
/// revision expressions such as `main~1`, `main^` or `main@{1}` are
/// rejected with [`GitError::BranchNotFound`] and HEAD is never detached.
/// Never creates a branch from a remote-tracking one (`--no-guess`) and
/// never fetches: in a partial clone whose needed objects are missing
/// locally this fails with [`GitError::NetworkRequired`]. Local changes
/// that would be overwritten make git refuse ([`GitError::Command`]).
pub fn checkout_branch(repo: &Repo, name: &str) -> GitResult<()> {
    if !is_local_branch(repo, name) {
        return Err(GitError::BranchNotFound(name.to_owned()));
    }
    // `switch` only accepts branches (no implicit detach), unlike `checkout`.
    let args = ["switch", "-q", "--no-guess", name];
    exec(repo, &args, None).map(drop).map_err(network_error)
}

/// True when `refs/heads/<name>` is a well-formed ref that exists exactly
/// (no revision syntax, no `@{-N}` shorthand, no abbreviation).
fn is_local_branch(repo: &Repo, name: &str) -> bool {
    if name.is_empty() || name.starts_with('-') {
        return false;
    }
    let refname = format!("refs/heads/{name}");
    let well_formed = exec(repo, &["check-ref-format", refname.as_str()], None).is_ok();
    well_formed
        && exec(
            repo,
            &["show-ref", "--verify", "--quiet", refname.as_str()],
            None,
        )
        .is_ok()
}

/// git's messages when a missing object could only come from a remote.
const NETWORK_ERRORS: &[&str] = &["promisor", "lazy fetch", "transport", "not allowed"];

fn network_error(err: GitError) -> GitError {
    match err {
        GitError::Command { stderr, .. } if NETWORK_ERRORS.iter().any(|m| stderr.contains(m)) => {
            GitError::NetworkRequired(stderr)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_lines() {
        let b =
            parse_branch("refs/heads/feat/x\0*\0abc1234\0origin/feat/x\0ahead 2, behind 1\0Fix it")
                .unwrap();
        assert_eq!(b.name, "feat/x");
        assert!(b.is_current);
        assert_eq!(b.upstream.as_deref(), Some("origin/feat/x"));
        assert_eq!((b.ahead, b.behind), (2, 1));
        assert_eq!(b.summary, "Fix it");
        let b = parse_branch("refs/heads/old\0 \0def5678\0origin/old\0gone\0msg").unwrap();
        assert!(b.upstream_gone && !b.is_current);
        let b = parse_branch("refs/heads/m\0 \0d\0\0\0subject\0with nul").unwrap();
        assert_eq!(b.upstream, None);
        assert_eq!(b.summary, "subject\0with nul");
        assert!(parse_branch("refs/tags/v1\0 \0d\0\0\0s").is_none());
    }

    #[test]
    fn parses_track() {
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("behind 3"), (0, 3));
        assert_eq!(parse_track("ahead 4"), (4, 0));
    }
}
