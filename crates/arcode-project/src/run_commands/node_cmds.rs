//! Run commands from `package.json` scripts.

use std::path::Path;

use super::kind_for_name;
use crate::command::{RunCommand, RunSource, args};
use crate::{fsutil, node};

/// One `<pm> run <script>` per script; `start` and `dev` come first, the
/// rest by name.
pub(super) fn detect(root: &Path) -> Vec<RunCommand> {
    let Some(json) = fsutil::read_json(&root.join("package.json")) else {
        return Vec::new();
    };
    let (pm, _) = node::node_package_manager(root, None);
    let mut scripts = node::scripts(&json);
    // Sorted explicitly: map order depends on serde_json's features.
    scripts.sort_by(|a, b| (rank(a), a).cmp(&(rank(b), b)));
    scripts
        .into_iter()
        .map(|name| {
            let kind = kind_for_name(&name);
            RunCommand::new(
                pm.program(),
                args(&["run", &name]),
                root,
                kind,
                RunSource::Node,
            )
        })
        .collect()
}

fn rank(script: &str) -> u8 {
    match script {
        "start" => 0,
        "dev" => 1,
        _ => 2,
    }
}
