//! [`PackagesStore`]: the project's manifests with their dependencies
//! (`arcode_project::deps::scan_manifests`, offline) and the results of
//! "Check for updates" per manifest.
//!
//! Nothing is read until the Packages view is first shown
//! ([`PackagesStore::ensure_loaded`]); after that, changed manifests and
//! lockfiles rescan the project after a short quiet time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use arcode_project::deps::{
    self, CancelFlag, OutdatedFormat, PackageCommand, ProjectManifest, find_program, install_hint,
};
use gpui::{AppContext, Context, EventEmitter, Task};

/// Quiet time after a manifest or lockfile change before rescanning.
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(400);
/// A check for updates is stopped after this long.
const CHECK_TIMEOUT: Duration = Duration::from_secs(300);

/// The state of "Check for updates" for one manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckState {
    /// The check runs in the background.
    Running,
    /// Newest versions by package name (only packages with a newer one).
    Done(HashMap<String, String>),
    /// The check failed (the message says why).
    Failed(String),
}

/// What the store asks the workspace to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackagesStoreEvent {
    /// Show a short message.
    Message(String),
    /// Run a command in a terminal tab (a check whose output is not
    /// machine readable, or could not be parsed).
    RunInTerminal {
        /// The manifest the command is for.
        manifest: PathBuf,
        /// The command.
        command: PackageCommand,
    },
}

/// The manifests of the project.
#[derive(Default)]
pub struct PackagesStore {
    root: Option<PathBuf>,
    manifests: Vec<ProjectManifest>,
    loaded: bool,
    loading: bool,
    scans: usize,
    scan_task: Option<Task<()>>,
    refresh_task: Option<Task<()>>,
    checks: HashMap<PathBuf, CheckState>,
    check_tasks: HashMap<PathBuf, (CancelFlag, Task<()>)>,
}

impl EventEmitter<PackagesStoreEvent> for PackagesStore {}

impl PackagesStore {
    /// An empty store.
    pub fn new(_: &mut Context<Self>) -> Self {
        Self::default()
    }

    /// The project root.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The manifests, the root's first.
    pub fn manifests(&self) -> &[ProjectManifest] {
        &self.manifests
    }

    /// The manifest at `path`.
    pub fn manifest(&self, path: &Path) -> Option<&ProjectManifest> {
        self.manifests.iter().find(|m| m.path == path)
    }

    /// The first scan finished.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// A scan runs.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// How many scans ran (tests).
    pub fn scans(&self) -> usize {
        self.scans
    }

    /// The check state of the manifest at `path`.
    pub fn check(&self, path: &Path) -> Option<&CheckState> {
        self.checks.get(path)
    }

    /// Opens `root` (read on first use) or clears the store.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.cancel_all_checks();
        self.checks.clear();
        self.root = root;
        self.manifests.clear();
        self.loaded = false;
        self.loading = false;
        self.scan_task = None;
        self.refresh_task = None;
        cx.notify();
    }

    /// Reads the project unless it was read already (or is being read).
    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            self.refresh(cx);
        }
    }

    /// Reads the project's manifests again, in the background.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_task = None;
        let Some(root) = self.root.clone() else {
            return;
        };
        self.loading = true;
        self.scans += 1;
        cx.notify();
        let scan_root = root.clone();
        let work = cx.background_spawn(async move { deps::scan_manifests(&scan_root) });
        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let found = work.await;
            this.update(cx, |store, cx| {
                if store.root.as_deref() != Some(root.as_path()) {
                    return;
                }
                store.loading = false;
                store.loaded = true;
                store
                    .checks
                    .retain(|path, _| found.iter().any(|m| &m.path == path));
                store.manifests = found;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Rescans after a short quiet time (only once the project was read;
    /// before that the first look reads it anyway).
    pub fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.loaded && !self.loading {
            return;
        }
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REFRESH_DEBOUNCE).await;
            this.update(cx, |store, cx| store.refresh(cx)).ok();
        }));
    }

    /// Files changed on disk: rescans when a manifest or lockfile in the
    /// project is among them.
    pub fn paths_changed(&mut self, paths: &[PathBuf], overflow: bool, cx: &mut Context<Self>) {
        let Some(root) = &self.root else {
            return;
        };
        let relevant = paths
            .iter()
            .any(|p| p.starts_with(root) && deps::is_package_file(p));
        if overflow || relevant {
            self.schedule_refresh(cx);
        }
    }

    /// Looks for newer versions of the manifest's packages (uses the
    /// network, so only on request). Managers without machine-readable
    /// output show theirs in a terminal instead.
    pub fn check_updates(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(m) = self.manifest(path).cloned() else {
            return;
        };
        let Some((command, format)) = deps::outdated_command(&m, cfg!(windows)) else {
            cx.emit(PackagesStoreEvent::Message(format!(
                "{} has no command that lists outdated packages",
                if m.yarn_berry {
                    "Yarn 2+"
                } else {
                    m.manager.label()
                }
            )));
            return;
        };
        if find_program(&command.program).is_none() {
            cx.emit(PackagesStoreEvent::Message(not_installed(&m, &command)));
            return;
        }
        if format == OutdatedFormat::Raw {
            cx.emit(PackagesStoreEvent::RunInTerminal {
                manifest: m.path.clone(),
                command,
            });
            return;
        }
        self.cancel_check(path, cx);
        let cancel = CancelFlag::new();
        let flag = cancel.clone();
        let run = command.clone();
        let work = cx.background_spawn(async move { deps::capture(&run, &flag, CHECK_TIMEOUT) });
        let key = m.path.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |store, cx| {
                store.check_tasks.remove(&key);
                let state = match result {
                    Ok(out) if out.cancelled => None,
                    Ok(out) => {
                        match deps::parse_outdated(format, &out.stdout, &out.stderr, out.success) {
                            Some(found) => Some(CheckState::Done(
                                found.into_iter().map(|o| (o.name, o.latest)).collect(),
                            )),
                            None => {
                                cx.emit(PackagesStoreEvent::Message(format!(
                                    "Could not read the output of `{}`; it runs in a terminal now",
                                    command.display
                                )));
                                cx.emit(PackagesStoreEvent::RunInTerminal {
                                    manifest: key.clone(),
                                    command: command.clone(),
                                });
                                Some(CheckState::Failed("Check failed".into()))
                            }
                        }
                    }
                    Err(err) => {
                        let msg = format!("Cannot run `{}`: {err}", command.display);
                        cx.emit(PackagesStoreEvent::Message(msg.clone()));
                        Some(CheckState::Failed(msg))
                    }
                };
                match state {
                    Some(state) => store.checks.insert(key.clone(), state),
                    None => store.checks.remove(&key),
                };
                cx.notify();
            })
            .ok();
        });
        self.checks.insert(m.path.clone(), CheckState::Running);
        self.check_tasks.insert(m.path, (cancel, task));
        cx.notify();
    }

    /// Stops a running check of the manifest at `path`.
    pub fn cancel_check(&mut self, path: &Path, cx: &mut Context<Self>) {
        // Dropping the task drops its result; the process is killed by
        // the capture loop once it sees the flag.
        if let Some((flag, _task)) = self.check_tasks.remove(path) {
            flag.cancel();
            self.checks.remove(path);
            cx.notify();
        }
    }

    fn cancel_all_checks(&mut self) {
        for (_, (flag, _task)) in self.check_tasks.drain() {
            flag.cancel();
        }
    }
}

/// "`pnpm` is not installed. Run `corepack enable` ..."
pub fn not_installed(m: &ProjectManifest, command: &PackageCommand) -> String {
    format!(
        "{} is not installed (or not on PATH). {}",
        command.program,
        install_hint(m.manager)
    )
}
