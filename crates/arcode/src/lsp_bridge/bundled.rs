//! The language servers bundled with Arcode: the bundle is looked up the
//! first time a server starts (next to the executable, then in the local
//! data folder), and every configured server that is neither a path nor on
//! `PATH` runs from it. Node-based servers run with the bundled Node.js,
//! jdtls with the user's Java. All of it runs off the UI thread: in the
//! language server runtime as each server starts (so a restart sees a Java
//! installed since), and in the background for the Languages page.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use arcode_lsp::{LaunchHook, ServerSpec};
use arcode_project::bundled::{
    Bundle, Launch, LaunchContext, ServerSource, bundle_dirs, resolve_server,
};
use arcode_project::packs::find_executable;

/// The bundle of this installation, if there is one. Looked up once: a
/// bundle fetched while Arcode runs is used after a restart.
pub fn bundle() -> Option<&'static Bundle> {
    static BUNDLE: OnceLock<Option<Bundle>> = OnceLock::new();
    BUNDLE.get_or_init(find_bundle).as_ref()
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

/// The hook that points each server the user has not installed at its
/// bundled copy as it starts, keeping `notes` up to date.
pub fn launch_hook(notes: RuntimeNotes) -> LaunchHook {
    Arc::new(move |spec: &mut ServerSpec| {
        let note = resolve_spec(spec, bundle(), &|command| {
            find_executable(command).is_some()
        });
        let mut notes = notes.lock().unwrap_or_else(|e| e.into_inner());
        match note {
            Some(note) => notes.insert(spec.name.clone(), note),
            None => notes.remove(&spec.name),
        };
    })
}

/// Points `spec` at its bundled copy when its command is not a path, not
/// on `PATH` (`on_path`) and in `bundle`. Returns what to tell the user if
/// it fails to start (a missing runtime).
pub fn resolve_spec(
    spec: &mut ServerSpec,
    bundle: Option<&Bundle>,
    on_path: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let bundle = bundle?;
    // Only commands the bundle has cost a PATH lookup.
    bundle.manifest().command(&spec.command)?;
    let resolved = resolve_server(&spec.command, &spec.args, on_path, |command| {
        bundle.launch(command, &LaunchContext::current(state_dir()))
    });
    if resolved.source != ServerSource::Bundled {
        return None;
    }
    log::debug!("{}: bundled {}", spec.name, resolved.command);
    spec.command = resolved.command;
    spec.args = resolved.args;
    resolved.note
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
mod tests {
    use std::fs;

    use arcode_project::bundled::MANIFEST_FILE;

    use super::*;

    const MANIFEST: &str = r#"
schema = 1
[node]
version = "24.0.0"
license = "MIT"
homepage = "https://nodejs.org"
platforms = {}
[[server]]
name = "pyright"
version = "1.1.400"
languages = ["python"]
license = "MIT"
homepage = "https://example.org"
source = "npm"
packages = ["pyright@1.1.400"]
commands = [{ name = "pyright-langserver", node = "lib/npm/node_modules/pyright/langserver.index.js" }]
[[server]]
name = "rust-analyzer"
version = "2026-01-01"
languages = ["rust"]
license = "MIT"
homepage = "https://example.org"
source = "archive"
commands = [{ name = "rust-analyzer", exec = "bin/rust-analyzer" }]
"#;

    fn bundle() -> (tempfile::TempDir, Bundle) {
        let dir = tempfile::tempdir().unwrap();
        let exe = if cfg!(windows) { ".exe" } else { "" };
        for file in [
            format!("node/bin/node{exe}"),
            format!("node/node{exe}"),
            "lib/npm/node_modules/pyright/langserver.index.js".into(),
            format!("bin/rust-analyzer{exe}"),
        ] {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        fs::write(dir.path().join(MANIFEST_FILE), MANIFEST).unwrap();
        let bundle = Bundle::load(dir.path()).unwrap().unwrap();
        (dir, bundle)
    }

    fn spec(name: &str, command: &str) -> ServerSpec {
        ServerSpec {
            args: vec!["--stdio".into()],
            ..ServerSpec::new(name, command)
        }
    }

    #[test]
    fn missing_servers_run_from_the_bundle() {
        let (dir, bundle) = bundle();
        let mut servers = [
            spec("pyright-langserver", "pyright-langserver"),
            spec("rust-analyzer", "rust-analyzer"),
            spec("custom", "/opt/rust-analyzer"),
            spec("typos", "typos-lsp"),
        ];
        // rust-analyzer is installed by the user; pyright is not.
        for server in &mut servers {
            let note = resolve_spec(server, Some(&bundle), &|c| c == "rust-analyzer");
            assert_eq!(note, None);
        }
        let node = bundle.node(arcode_project::packs::Os::current());
        assert_eq!(servers[0].command, node.to_string_lossy());
        assert_eq!(
            servers[0].args,
            [
                dir.path()
                    .join("lib/npm/node_modules/pyright/langserver.index.js")
                    .to_string_lossy()
                    .into_owned(),
                "--stdio".to_string()
            ]
        );
        assert_eq!(servers[1].command, "rust-analyzer");
        assert_eq!(servers[2].command, "/opt/rust-analyzer");
        assert_eq!(servers[3].command, "typos-lsp");
        // Without a bundle nothing changes.
        let mut plain = spec("pyright-langserver", "pyright-langserver");
        resolve_spec(&mut plain, None, &|_| false);
        assert_eq!(plain.command, "pyright-langserver");
    }

    #[test]
    fn the_hook_keeps_notes_per_server() {
        let notes = RuntimeNotes::default();
        notes
            .lock()
            .unwrap()
            .insert("jdtls".into(), "needs Java".into());
        let hook = launch_hook(notes.clone());
        // Unit tests see no bundle: nothing changes, and a server that
        // starts without a problem drops its old note.
        let mut jdtls = spec("jdtls", "jdtls");
        hook(&mut jdtls);
        assert_eq!(jdtls.command, "jdtls");
        assert!(notes.lock().unwrap().is_empty());
    }
}
