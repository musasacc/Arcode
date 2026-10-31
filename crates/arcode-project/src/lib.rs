//! Knowledge about projects, toolchains and language packs for Arcode.
//!
//! Pure logic plus filesystem inspection. This crate never installs
//! anything and never touches the network: it returns commands that the UI
//! shows and, on the user's request, runs in the integrated terminal.
//!
//! - [`detect`] finds the package manifest for an opened file and the
//!   command that installs its dependencies; [`install_state`] /
//!   [`needs_install`] tell "Packages not installed" from "Reinstall";
//!   [`missing_packages`] lists a project's manifests whose dependencies
//!   are missing (cheap, offline checks) for the editor banner.
//! - [`deps`] lists every manifest of a project with its package manager
//!   and declared dependencies (offline, from manifests and lockfiles) and
//!   builds the manager commands of the Packages view.
//! - [`detect_run_commands`] / [`default_run_command`] list a project's run,
//!   test and build commands (Cargo, package.json, Go, Python, just, make).
//! - [`command_for_runnable`] builds the command for a main function, test or
//!   benchmark located by tree-sitter queries elsewhere.
//! - [`debug_target`] turns such a command into what a debugger launches
//!   (a Cargo build whose JSON output names the executable, a script, a
//!   Python module, a Go package).
//! - [`packs()`] / [`pack_for_language`] describe language packs;
//!   [`check_installed`] runs their (offline) check commands and
//!   [`install_plan`] lists the install steps to confirm before running.
//! - [`toolchain`] helpers read virtualenvs and version files.
//! - [`files`] creates, renames, moves and deletes files (to the trash when
//!   the platform has one) and reveals them in the system file manager.
//! - [`templates`] lists new-project templates and creates project folders;
//!   a template's command is only returned, never run here.
//!
//! Everything that reads files or spawns processes blocks: call it off the
//! UI thread.

mod cargo;
mod command;
mod debug_target;
pub mod deps;
pub mod files;
mod fsutil;
mod gofile;
mod gomod;
mod install_state;
mod missing;
mod node;
mod package_files;
pub mod packs;
pub mod proc_group;
pub mod procs;
mod run_commands;
mod runnables;
pub mod templates;
pub mod toolchain;
mod trash;

pub use command::{RunCommand, RunKind, RunSource, command_line, quote_arg};
pub use debug_target::{
    CargoExecutable, DebugTarget, cargo_message_text, debug_target, executables_from_cargo_json,
};
pub use install_state::{PackagesState, install_state, needs_install};
pub use missing::{
    MissingPackages, has_dependencies, install_steps, is_missing, missing_packages,
    missing_packages_with,
};
pub use node::{NodePackageManager, node_package_manager};
pub use package_files::{
    InstallCommand, PackageFile, PackageKind, PythonManifest, detect, detect_within,
    package_file_for_manifest,
};
pub use packs::{
    CheckCommand, CommandRunner, Component, ComponentKind, ComponentStatus, InstallAction,
    InstallStep, KNOWN_LANGUAGE_IDS, LanguagePack, Os, PlanStep, RunOutcome, SystemRunner,
    ToolState, check_installed, install_plan, pack_by_id, pack_for_language, packs,
};
pub use run_commands::{default_run_command, detect_run_commands};
pub use runnables::{RUNNABLE_LANGUAGES, Runnable, RunnableKind, command_for_runnable};
pub use toolchain::{
    PythonVenv, detect_node_version_file, detect_python_venv, rust_toolchain_channel,
};
