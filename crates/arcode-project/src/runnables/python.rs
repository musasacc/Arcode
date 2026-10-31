//! Commands for Python runnables (`python -m pytest`, `python <file>`).

use std::path::Path;

use super::{RunnableKind, label};
use crate::command::{RunCommand, RunKind, RunSource, args};
use crate::{fsutil, toolchain};

pub(super) fn command(
    file: &Path,
    root: &Path,
    kind: RunnableKind,
    name: &str,
) -> Option<RunCommand> {
    let python = toolchain::python_program(&fsutil::start_dir(file), Some(root));
    let rel = fsutil::rel_slash(file, root);
    let (list, run_kind) = match kind {
        RunnableKind::Main => (vec![rel], RunKind::Run),
        RunnableKind::Test | RunnableKind::TestModule | RunnableKind::Benchmark => {
            let node = if name.is_empty() {
                rel
            } else {
                format!("{rel}::{}", name.replace('.', "::"))
            };
            let mut list = args(&["-m", "pytest"]);
            list.push(node);
            (list, RunKind::Test)
        }
    };
    let shown = if kind == RunnableKind::Main { "" } else { name };
    Some(
        RunCommand::new(python, list, root, run_kind, RunSource::Python)
            .with_label(label(kind, shown)),
    )
}
