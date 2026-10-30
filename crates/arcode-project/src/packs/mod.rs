//! Language packs: the language server, debugger, formatter and linter a
//! language needs, how to check whether each is installed, and how to
//! install it.
//!
//! Nothing here installs anything. [`install_plan`] lists the exact steps;
//! the UI shows them (they download from the network), asks the user, and
//! runs them in the integrated terminal.

mod builtin;
mod check;
mod plan;
mod steps;

use serde::{Deserialize, Serialize};

use crate::command::command_line;

pub use builtin::packs;
pub use check::{
    CommandRunner, ComponentStatus, DEFAULT_CHECK_TIMEOUT, RunOutcome, SystemRunner, ToolState,
    check_component, check_installed, check_installed_on, check_requirements, find_executable,
    requirement_check,
};
pub use plan::{PlanStep, install_plan, install_plan_for, install_plan_missing};

/// LSP language ids the packs may refer to.
pub const KNOWN_LANGUAGE_IDS: [&str; 22] = [
    "c",
    "cpp",
    "css",
    "go",
    "gomod",
    "html",
    "javascript",
    "javascriptreact",
    "json",
    "jsonc",
    "less",
    "markdown",
    "objective-c",
    "objective-cpp",
    "plaintext",
    "python",
    "rust",
    "scss",
    "shellscript",
    "toml",
    "typescript",
    "typescriptreact",
];

/// An operating system an install step or check applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Os {
    /// Linux and other Unix-likes without a more specific entry.
    Linux,
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
}

impl Os {
    /// Every supported OS.
    pub const ALL: [Os; 3] = [Os::Linux, Os::MacOs, Os::Windows];

    /// The OS this binary was built for.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }

    /// The Python executable on this OS (`python` on Windows, else `python3`).
    pub fn python(self) -> &'static str {
        match self {
            Self::Windows => "python",
            Self::Linux | Self::MacOs => "python3",
        }
    }
}

/// The role of a pack component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComponentKind {
    /// Language server.
    Lsp,
    /// Debug adapter.
    Debugger,
    /// Formatter.
    Formatter,
    /// Linter.
    Linter,
}

/// A command whose success (exit code 0) means a component is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckCommand {
    /// Systems this check applies to.
    pub os: Vec<Os>,
    /// Program name, looked up on `PATH`.
    pub program: String,
    /// Arguments.
    pub args: Vec<String>,
}

impl CheckCommand {
    /// The command line for display.
    pub fn command_line(&self) -> String {
        command_line(&self.program, &self.args)
    }
}

/// What an install step does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallAction {
    /// Run this command in the integrated terminal.
    Run {
        /// Program name, looked up on `PATH`.
        program: String,
        /// Arguments.
        args: Vec<String>,
    },
    /// Display-only instructions; nothing is run.
    Manual {
        /// Text to show the user.
        instructions: String,
    },
}

/// One step of installing a component on some systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallStep {
    /// Systems this step applies to.
    pub os: Vec<Os>,
    /// What the step does, in one sentence.
    pub description: String,
    /// The command to run, or manual instructions.
    pub action: InstallAction,
    /// Whether the step downloads from the network.
    pub network: bool,
}

impl InstallStep {
    /// The command line for [`InstallAction::Run`] steps.
    pub fn command_line(&self) -> Option<String> {
        match &self.action {
            InstallAction::Run { program, args } => Some(command_line(program, args)),
            InstallAction::Manual { .. } => None,
        }
    }

    /// Whether this step only shows instructions.
    pub fn is_manual(&self) -> bool {
        matches!(self.action, InstallAction::Manual { .. })
    }
}

/// A tool in a language pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    /// Language server, debugger, formatter or linter.
    pub kind: ComponentKind,
    /// Tool name, e.g. `rust-analyzer`.
    pub name: String,
    /// Installation checks per OS (none: the state is always unknown).
    pub checks: Vec<CheckCommand>,
    /// Install steps for all systems; filter with [`Component::install_steps`].
    pub install: Vec<InstallStep>,
    /// Programs that must exist first, e.g. `npm`, `python`, `go`, `rustup`.
    pub requires: Vec<String>,
    /// Alternatives and extras: not part of the default install plan.
    pub optional: bool,
}

impl Component {
    /// The check command for `os`, if any.
    pub fn check_for(&self, os: Os) -> Option<&CheckCommand> {
        self.checks.iter().find(|c| c.os.contains(&os))
    }

    /// The install steps for `os`, in order.
    pub fn install_steps(&self, os: Os) -> impl Iterator<Item = &InstallStep> {
        self.install.iter().filter(move |s| s.os.contains(&os))
    }

    /// Whether installing on `os` is display-only (no command to run).
    pub fn is_display_only(&self, os: Os) -> bool {
        self.install_steps(os).all(InstallStep::is_manual)
    }
}

/// The tools for one language (or a family of languages).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguagePack {
    /// Stable id, e.g. `rust`, `typescript`, `cpp`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// LSP language ids this pack serves.
    pub languages: Vec<String>,
    /// The tools.
    pub components: Vec<Component>,
}

impl LanguagePack {
    /// The component named `name`.
    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}

/// The pack serving the LSP language id `language_id`.
pub fn pack_for_language(language_id: &str) -> Option<LanguagePack> {
    packs()
        .into_iter()
        .find(|p| p.languages.iter().any(|l| l == language_id))
}

/// The pack with id `id`.
pub fn pack_by_id(id: &str) -> Option<LanguagePack> {
    packs().into_iter().find(|p| p.id == id)
}
