//! How to install a language server that is missing: one command for this
//! machine, picked by what is on `PATH` (npm before pipx before pip, the
//! platform package manager for native servers), or the server's homepage
//! when nothing fits.
//!
//! Nothing here runs anything. The UI shows the command and runs it only
//! when the user clicks Install, in a terminal tab they can see; see also
//! [`rustup`] for rustup's placeholder executables.

pub mod rustup;
#[cfg(test)]
mod tests;

use crate::command::command_line;
use crate::packs::Os;

/// How to get a missing server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallHint {
    /// Run this command in a terminal.
    Command(InstallCommandLine),
    /// No command fits this machine: the page that says how.
    Link(&'static str),
}

/// A program and its arguments (run without a shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommandLine {
    /// Program name, looked up on `PATH`.
    pub program: String,
    /// Arguments, one per entry.
    pub args: Vec<String>,
}

impl InstallCommandLine {
    /// The command as one line, e.g. `npm install -g pyright`.
    pub fn display(&self) -> String {
        command_line(&self.program, &self.args)
    }
}

impl InstallHint {
    /// The command line or the link, for display.
    pub fn display(&self) -> String {
        match self {
            Self::Command(command) => command.display(),
            Self::Link(url) => (*url).to_string(),
        }
    }
}

/// A system package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Manager {
    AptGet,
    Dnf,
    Pacman,
    Brew,
    Winget,
}

impl Manager {
    /// The program, whether it needs `sudo`, its install arguments, and
    /// the systems it is looked for on.
    fn spec(self) -> (&'static str, bool, &'static [&'static str], &'static [Os]) {
        const LINUX: &[Os] = &[Os::Linux];
        match self {
            Self::AptGet => ("apt-get", true, &["install"], LINUX),
            Self::Dnf => ("dnf", true, &["install"], LINUX),
            Self::Pacman => ("pacman", true, &["-S"], LINUX),
            Self::Brew => ("brew", false, &["install"], &[Os::MacOs, Os::Linux]),
            Self::Winget => ("winget", false, &["install", "-e", "--id"], &[Os::Windows]),
        }
    }
}

/// One way to install a server, tried in order.
#[derive(Debug, Clone, Copy)]
enum Way {
    /// `program args…`, when `program` is on `PATH`.
    Run(&'static str, &'static [&'static str]),
    /// `python3 -m pip install --user <package>` (`python` on Windows).
    Pip(&'static str),
    /// The package of the first package manager found.
    Package(&'static [(Manager, &'static str)]),
}

/// How each server command is installed.
struct Recipe {
    /// The server commands it provides.
    commands: &'static [&'static str],
    /// Where its install instructions are.
    homepage: &'static str,
    ways: &'static [Way],
}

const fn npm(packages: &'static [&'static str]) -> Way {
    Way::Run("npm", packages)
}

const RECIPES: &[Recipe] = &[
    Recipe {
        commands: &["rust-analyzer"],
        homepage: "https://rust-analyzer.github.io/book/installation.html",
        ways: &[Way::Run("rustup", &["component", "add", "rust-analyzer"])],
    },
    Recipe {
        commands: &["pyright-langserver", "pyright"],
        homepage: "https://github.com/microsoft/pyright",
        ways: &[
            npm(&["install", "-g", "pyright"]),
            Way::Run("pipx", &["install", "pyright"]),
            Way::Pip("pyright"),
        ],
    },
    Recipe {
        commands: &["typescript-language-server"],
        homepage: "https://github.com/typescript-language-server/typescript-language-server",
        ways: &[npm(&[
            "install",
            "-g",
            "typescript",
            "typescript-language-server",
        ])],
    },
    Recipe {
        commands: &["gopls"],
        homepage: "https://pkg.go.dev/golang.org/x/tools/gopls",
        ways: &[Way::Run(
            "go",
            &["install", "golang.org/x/tools/gopls@latest"],
        )],
    },
    Recipe {
        commands: &[
            "vscode-json-language-server",
            "vscode-html-language-server",
            "vscode-css-language-server",
        ],
        homepage: "https://github.com/hrsh7th/vscode-langservers-extracted",
        ways: &[npm(&["install", "-g", "vscode-langservers-extracted"])],
    },
    Recipe {
        commands: &["bash-language-server"],
        homepage: "https://github.com/bash-lsp/bash-language-server",
        ways: &[npm(&["install", "-g", "bash-language-server"])],
    },
    Recipe {
        commands: &["yaml-language-server"],
        homepage: "https://github.com/redhat-developer/yaml-language-server",
        ways: &[npm(&["install", "-g", "yaml-language-server"])],
    },
    Recipe {
        commands: &["intelephense"],
        homepage: "https://intelephense.com",
        ways: &[npm(&["install", "-g", "intelephense"])],
    },
    Recipe {
        commands: &["taplo"],
        homepage: "https://taplo.tamasfe.dev/cli/installation/binary.html",
        ways: &[Way::Run(
            "cargo",
            &["install", "taplo-cli", "--locked", "--features", "lsp"],
        )],
    },
    Recipe {
        commands: &["ruby-lsp"],
        homepage: "https://shopify.github.io/ruby-lsp",
        ways: &[Way::Run("gem", &["install", "ruby-lsp"])],
    },
    // Not Homebrew's `llvm` nor winget's `LLVM.LLVM`: neither puts clangd
    // on `PATH` (the LLVM installer leaves "Add LLVM to the system PATH"
    // off), so the server would still not be found after the install.
    Recipe {
        commands: &["clangd"],
        homepage: "https://clangd.llvm.org/installation",
        ways: &[Way::Package(&[
            (Manager::AptGet, "clangd"),
            (Manager::Dnf, "clang-tools-extra"),
            (Manager::Pacman, "clang"),
        ])],
    },
    Recipe {
        commands: &["marksman"],
        homepage: "https://github.com/artempyanykh/marksman",
        ways: &[Way::Package(&[
            (Manager::Brew, "marksman"),
            (Manager::Winget, "Artempyanykh.Marksman"),
        ])],
    },
    Recipe {
        commands: &["lua-language-server"],
        homepage: "https://luals.github.io/#install",
        ways: &[Way::Package(&[
            (Manager::Pacman, "lua-language-server"),
            (Manager::Brew, "lua-language-server"),
            (Manager::Winget, "LuaLS.lua-language-server"),
        ])],
    },
    Recipe {
        commands: &["jdtls"],
        homepage: "https://github.com/eclipse-jdtls/eclipse.jdt.ls",
        ways: &[Way::Package(&[(Manager::Brew, "jdtls")])],
    },
];

/// The server commands this module knows how to install.
pub fn known_commands() -> impl Iterator<Item = &'static str> {
    RECIPES.iter().flat_map(|r| r.commands.iter().copied())
}

/// Where the install instructions of server `command` are.
pub fn homepage(command: &str) -> Option<&'static str> {
    recipe(command).map(|r| r.homepage)
}

/// How to install server `command` on `os`, given which programs are on
/// `PATH` (`on_path`): the first way whose program is there, else the
/// server's homepage. `None` for a server this module does not know, or
/// a command that is a path (the user put it there).
pub fn install_hint(command: &str, os: Os, on_path: &dyn Fn(&str) -> bool) -> Option<InstallHint> {
    let recipe = recipe(command)?;
    let found = recipe.ways.iter().find_map(|way| match *way {
        Way::Run(program, args) => on_path(program).then(|| line(program, args)),
        Way::Pip(package) => {
            let python = os.python();
            on_path(python).then(|| line(python, &["-m", "pip", "install", "--user", package]))
        }
        Way::Package(packages) => packages.iter().find_map(|(manager, package)| {
            let (program, sudo, args, systems) = manager.spec();
            if !systems.contains(&os) || !on_path(program) {
                return None;
            }
            let mut words: Vec<&str> = args.to_vec();
            words.push(package);
            Some(if sudo && on_path("sudo") {
                words.insert(0, program);
                line("sudo", &words)
            } else {
                line(program, &words)
            })
        }),
    });
    Some(match found {
        Some(command) => InstallHint::Command(command),
        None => InstallHint::Link(recipe.homepage),
    })
}

fn recipe(command: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.commands.contains(&command))
}

fn line(program: &str, args: &[&str]) -> InstallCommandLine {
    InstallCommandLine {
        program: program.to_string(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
    }
}
