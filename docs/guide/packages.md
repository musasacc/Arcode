# Packages

The **Packages** view (box icon in the activity bar, or **Packages: Show
Packages**) lists the dependencies of every package manifest in your
project, including nested ones, and lets you add, remove and update them
with each package manager's own commands.

## Supported managers

| Manager | Manifest | Installed versions read from |
|---|---|---|
| Cargo | `Cargo.toml` | `Cargo.lock` |
| npm | `package.json` | `package-lock.json` |
| pnpm | `package.json` | `pnpm-lock.yaml` |
| Yarn (classic and Berry) | `package.json` | `yarn.lock` |
| Bun | `package.json` | `bun.lock` (the binary `bun.lockb` is not read) |
| uv | `pyproject.toml` | `uv.lock` |
| Poetry | `pyproject.toml` | `poetry.lock` |
| Pipenv | `Pipfile` | `Pipfile.lock` |
| pip | `requirements*.txt`, `pyproject.toml` | the project's virtual environment |
| Go modules | `go.mod` | `go.mod` |
| Composer | `composer.json` | `composer.lock` |
| Bundler | `Gemfile` | `Gemfile.lock` |

## What runs when

Reading the list is **offline**: Arcode parses the manifests and
lockfiles itself and starts no program. Nothing is read until you first
open the Packages view; after that, the list refreshes shortly after a
manifest or lockfile changes.

Everything else runs the manager's own command in a terminal tab, only
when you click, so you can see exactly what happens. Commands are run as
separate arguments, never through a shell, and package names and
versions are checked first.

| Action | Cargo | npm | pnpm / Bun | Yarn | Go |
|---|---|---|---|---|---|
| Add | `cargo add <name>` (`--dev`) | `npm install <name>` (`--save-dev`) | `pnpm add` / `bun add` | `yarn add` (`--dev`) | `go get <name>` |
| Remove | `cargo remove <name>` | `npm uninstall <name>` | `pnpm remove` / `bun remove` | `yarn remove` | `go get <name>@none` |
| Update one | `cargo update -p <name>` | `npm update <name>` | `pnpm update` / `bun update` | `yarn upgrade` (`yarn up` on Berry) | `go get -u <name>` |
| Update all | `cargo update` | `npm update` | `pnpm update` / `bun update` | `yarn upgrade` (`yarn up '*'`) | `go get -u ./...` |

| Action | uv | Poetry | Pipenv | pip | Composer | Bundler |
|---|---|---|---|---|---|---|
| Add | `uv add <name>` (`--dev`) | `poetry add` (`--group dev`) | `pipenv install` (`--dev`) | `python -m pip install` | `composer require` (`--dev`) | `bundle add` |
| Remove | `uv remove` | `poetry remove` | `pipenv uninstall` | `python -m pip uninstall -y` | `composer remove` | `bundle remove` |
| Update one | `uv lock --upgrade-package <name>` | `poetry update <name>` | `pipenv update <name>` | `python -m pip install -U <name>` | `composer update <name>` | `bundle update <name>` |
| Update all | `uv lock --upgrade` | `poetry update` | `pipenv update` | `python -m pip install -U -r <file>` | `composer update` | `bundle update` |

pip runs with the project's virtual environment's Python when there is
one (`.venv`), otherwise with `python3` (`python` on Windows).

**Check for updates** (**Packages: Check for Updates**) asks the manager
which newer versions exist (`cargo update --dry-run`, `npm outdated`,
`pip list --outdated`, `go list -m -u all`, ...). This is the only part
of the view that reads from the network on its own initiative, and it
runs only when you click it. It can be cancelled, and it stops by itself
if it takes too long.

**Documentation** in a dependency's context menu opens its page on the
registry (docs.rs, npmjs.com, PyPI, pkg.go.dev, ...) in your browser.

## The "packages are not installed" banner

When a project's dependencies are missing (for example `package.json`
without `node_modules`, or a Python project without a virtual
environment), a banner above the editor offers **Install packages**. The
check is offline and cheap. Nothing is installed until you click; the
install commands then run in terminal tabs:

| Project | Install command |
|---|---|
| npm, pnpm, Yarn, Bun | `<manager> install` in the workspace root |
| `pyproject.toml` with `uv.lock` / `poetry.lock` / `pdm.lock` | `uv sync` / `poetry install` / `pdm install` |
| other `pyproject.toml` | `python -m pip install -e .` (in a new `.venv` if there is none) |
| `requirements.txt` | `python -m pip install -r requirements.txt` (in a new `.venv` if there is none) |
| `Pipfile` | `pipenv install` |
| `go.mod` | `go mod download` |
| `composer.json` | `composer install` |
| `Gemfile` | `bundle install` |

**Packages: Check for Missing Packages** runs the check again, and
**Packages: Install Missing Packages** installs without the banner. Turn
the banner off with `[workspace] package_banner = false`.

Package managers download from their registries; that is their job, and
Arcode only starts them when you ask. See [Privacy](privacy.md).
