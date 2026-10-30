//! Local-only git integration for Arcode.
//!
//! This crate never talks to the network: there is no fetch, pull or push.
//!
//! - Repository discovery ([`find_repo`]) and branch detection ([`Repo::head`])
//!   work purely by reading files under `.git`, so they are cheap enough to
//!   call on every project open or focus change and never spawn a process.
//! - Content lookups such as [`Repo::head_file_contents`] shell out to the
//!   `git` CLI with prompts disabled and return `None` when git is missing.
//! - [`line_diff`] is a pure line diff (Myers, linear space) used for the
//!   editor's change gutter; [`diff_against_index`] runs it against the
//!   staged version of a file.
//! - [`status`], [`stage_hunk`], [`stage_file`], [`commit`], [`blame`] and
//!   [`branches`] wrap the CLI and report failures as [`GitError`]. They
//!   block until git exits: call them off the UI thread.
//! - [`parse_conflicts`] and [`resolve`] work on merge conflict markers in
//!   plain text.
//! - [`gutter_hunks`] merges unstaged and staged hunks into editor gutter
//!   marks; [`inline_diff`] interleaves two versions for a diff viewer.
//! - [`init_repo`], [`create_branch`], [`discard_changes`], [`commit_with`]
//!   (amend) and [`discover_repos`] (repositories in sub-folders) back the
//!   source control view. [`init_repo`] and [`discard_changes`] change the
//!   disk: call them only on an explicit user request.

mod blame;
mod branch;
mod cli;
mod commit;
mod conflict;
mod diff;
mod error;
mod exec;
mod gutter;
mod head;
mod hunk;
mod inline_diff;
mod myers;
mod ops;
mod patch;
mod paths;
mod repo;
mod stage;
mod status;

pub use blame::{BlameLine, blame};
pub use branch::{Branch, branches, checkout_branch};
pub use commit::commit;
pub use conflict::{Conflict, Resolution, parse_conflicts, resolve};
pub use diff::{ChangeKind, LineChange, line_diff, split_lines};
pub use error::{GitError, GitResult};
pub use gutter::{GutterHunk, gutter_hunks};
pub use head::Head;
pub use hunk::{Hunk, HunkRevert, diff_against_index, diff_hunks, revert_hunk, staged_hunks};
pub use inline_diff::{DiffLineKind, InlineDiff, inline_diff};
pub use ops::{
    CommitOptions, commit_with, create_branch, discard_changes, discover_repos, init_repo,
    last_commit_message, stage_all, unstage_all,
};
pub use repo::{Repo, find_repo};
pub use stage::{stage_file, stage_hunk, stage_paths, unstage_file, unstage_hunk};
pub use status::{
    ConflictKind, FileStatus, StatusCode, StatusSummary, conflicted_files, status, status_summary,
    status_with_ignored,
};
