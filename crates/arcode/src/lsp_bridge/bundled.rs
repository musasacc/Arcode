//! The language servers bundled with Arcode, and what "installed" means
//! for a server command.
//!
//! The bundle is looked up when a server starts (next to the executable,
//! then in the local data folder); while none is found it is looked for
//! again at later starts, so a bundle fetched while Arcode runs is used
//! without a restart. Every configured server that is neither a path nor
//! installed runs from it. Node-based servers run with the bundled
//! Node.js, jdtls with the user's Java.
//!
//! "Installed" means on `PATH`, except for rustup's placeholders: rustup
//! puts a `rust-analyzer` on `PATH` even without the component, which only
//! prints an error (see `arcode_project::server_install::rustup`). Such a
//! server counts as not installed: its bundled copy runs, or it fails as
//! "not installed" without being started. A configured path always runs
//! as given.
//!
//! All of it runs off the UI thread: in the language server runtime as
//! each server starts (so a restart sees a Java or a server installed
//! since), and in the background for the Languages page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arcode_lsp::{LaunchHook, ServerSpec};
use arcode_project::bundled::{
    Bundle, Launch, LaunchContext, ServerSource, bundle_dirs, resolve_server,
};
use arcode_project::packs::find_executable;
use arcode_project::server_install::rustup::{self, ProxyCheck};

/// How long "no bundle" is believed before looking again.
const RECHECK: Duration = Duration::from_secs(5);

/// The last bundle lookup.
struct Lookup {
    found: Option<Arc<Bundle>>,
    checked: Option<Instant>,
}

static LOOKUP: Mutex<Lookup> = Mutex::new(Lookup {
    found: None,
    checked: None,
});

/// The bundle of this installation, if there is one. A bundle once found
/// is kept; while there is none, it is looked for again after a few
/// seconds (or at once after [`recheck_bundle`]).
pub fn bundle() -> Option<Arc<Bundle>> {
    let mut lookup = LOOKUP.lock().unwrap_or_else(|e| e.into_inner());
    if lookup.found.is_none() && lookup.checked.is_none_or(|t| t.elapsed() >= RECHECK) {
        lookup.found = find_bundle().map(Arc::new);
        lookup.checked = Some(Instant::now());
    }
    lookup.found.clone()
}

/// Makes the next [`bundle`] look again when none was found (after an
/// install, which may have fetched one).
pub fn recheck_bundle() {
    LOOKUP.lock().unwrap_or_else(|e| e.into_inner()).checked = None;
}

fn find_bundle() -> Option<Bundle> {
    // Unit tests never pick up a bundle installed on the machine.
    if cfg!(test) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let data = arcode_config::paths::local_data_dir();
    let (found, problems) = Bundle::find(&bundle_dirs(&exe, data.as_deref()));
    for problem in problems {
        log::warn!("bundled language servers ignored: {problem}");
    }
    if let Some(bundle) = &found {
        log::info!("bundled language servers: {}", bundle.dir().display());
    }
    found
}

/// Where bundled servers keep state (jdtls workspaces): the local data
/// folder, which Windows roaming profiles do not copy around.
fn state_dir() -> PathBuf {
    arcode_config::paths::local_data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("language-servers-state")
}

/// Per server name, what to tell the user if it fails (a runtime its
/// bundled copy needs is missing or too old). Filled as servers start.
pub type RuntimeNotes = Arc<Mutex<HashMap<String, String>>>;

/// What a server command is on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Installed: runs from here.
    Installed(PathBuf),
    /// Only rustup's placeholder is on `PATH`; why it cannot run.
    Placeholder(String),
    /// Not on `PATH`.
    Missing,
}

/// Looks server `command` up on `PATH`; for a rustup placeholder asks
/// rustup (in `root`, whose toolchain file picks the toolchain) whether
/// its component is installed. Blocks for up to a few seconds.
pub fn find_server(command: &str, root: Option<&Path>) -> Found {
    classify(command, find_executable(command), root)
}

/// [`find_server`] for `command` found at `path` (`None`: not on `PATH`).
pub fn classify(command: &str, path: Option<PathBuf>, root: Option<&Path>) -> Found {
    let Some(path) = path else {
        return Found::Missing;
    };
    match rustup::check_proxy(command, &path, root, rustup::WHICH_TIMEOUT) {
        // Only a missing component makes it a placeholder. Any other
        // failure (e.g. the project's toolchain file names a toolchain that
        // is not installed) runs the proxy: rustup then does what it does
        // when run (installs that toolchain, or says why it cannot).
        ProxyCheck::Missing(said) => match arcode_lsp::rustup_missing_component(&said) {
            Some(reason) => Found::Placeholder(reason),
            None => Found::Installed(path),
        },
        ProxyCheck::NotProxy | ProxyCheck::Installed(_) | ProxyCheck::Unknown => {
            Found::Installed(path)
        }
    }
}

/// The hook that prepares each server as it starts (see [`prepare`]),
/// keeping `notes` up to date.
pub fn launch_hook(notes: RuntimeNotes) -> LaunchHook {
    Arc::new(move |spec: &mut ServerSpec, root: &Path| {
        let bundle = bundle();
        let note = prepare(spec, bundle.as_deref(), &|command| {
            find_server(command, Some(root))
        });
        let mut notes = notes.lock().unwrap_or_else(|e| e.into_inner());
        match note {
            Some(note) => notes.insert(spec.name.clone(), note),
            None => notes.remove(&spec.name),
        };
    })
}

/// Prepares `spec` to start: a command that is not a path and not
/// installed (`find`) runs from `bundle` when the bundle has it; a rustup
/// placeholder without a bundled copy is marked missing, so it fails as
/// "not installed" without running. Returns what to tell the user if the
/// bundled copy fails to start (a missing runtime).
pub fn prepare(
    spec: &mut ServerSpec,
    bundle: Option<&Bundle>,
    find: &dyn Fn(&str) -> Found,
) -> Option<String> {
    if is_path(&spec.command) {
        return None;
    }
    let bundle = bundle.filter(|b| b.manifest().command(&spec.command).is_some());
    // Only commands the bundle has, or rustup provides, cost a lookup.
    if bundle.is_none() && !rustup::is_proxied(&spec.command) {
        return None;
    }
    let found = find(&spec.command);
    if let Some(bundle) = bundle {
        let installed = matches!(found, Found::Installed(_));
        let resolved = resolve_server(
            &spec.command,
            &spec.args,
            |_| installed,
            |command| bundle.launch(command, &LaunchContext::current(state_dir())),
        );
        if resolved.source == ServerSource::Bundled {
            log::debug!("{}: bundled {}", spec.name, resolved.command);
            spec.command = resolved.command;
            spec.args = resolved.args;
            return resolved.note;
        }
    }
    if let Found::Placeholder(reason) = found {
        log::info!("{}: {reason}", spec.name);
        spec.missing = Some(reason);
    }
    None
}

/// Whether `command` names a file rather than a program on `PATH`.
fn is_path(command: &str) -> bool {
    command.contains(['/', '\\']) || Path::new(command).is_absolute()
}

/// How the bundled `command` would start with `cx`, if the bundle has it.
pub fn launch(command: &str, cx: &LaunchContext) -> Option<Launch> {
    bundle()?.launch(command, cx)
}

/// A launch context for this machine (looks for Java: off the UI thread).
pub fn launch_context() -> LaunchContext {
    LaunchContext::current(state_dir())
}

#[cfg(test)]
#[path = "bundled_tests.rs"]
mod tests;
