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
//!   editor's change gutter.

mod cli;
mod diff;
mod head;
mod myers;
mod paths;
mod repo;

pub use diff::{ChangeKind, LineChange, line_diff, split_lines};
pub use head::Head;
pub use repo::{Repo, find_repo};
