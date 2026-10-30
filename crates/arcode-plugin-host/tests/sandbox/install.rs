//! "Install from Folder…", `plugins.toml` and the hash check (§3, plan
//! step 12): installed plugins start disabled, enabling approves the
//! current `plugin.wasm`, a swapped file does not run, and an update that
//! asks for more stays disabled.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcode_plugin_host::consent::{ConsentAnswer, ScriptedConsent};
use arcode_plugin_host::grants::GrantScope;
use arcode_plugin_host::install::InstallError;
use arcode_plugin_host::memory_editor::MemoryEditor;
use arcode_plugin_host::paths::PluginPaths;
use arcode_plugin_host::{
    AppServices, CallError, CallOutput, ManagerConfig, ManagerError, PluginId, PluginManager,
    PluginOutput, PluginSource, QualifiedId,
};

use crate::common::*;

fn manager(base: &Path) -> (PluginManager, async_channel::Receiver<PluginOutput>) {
    let paths = PluginPaths::with_bases(
        None,
        &base.join("config"),
        &base.join("data"),
        &base.join("cache"),
    );
    let app = AppServices {
        editor: Arc::new(MemoryEditor::default()),
        index: None,
        http: Arc::new(FakeHttp::default()),
        consent: Some(Arc::new(ScriptedConsent::always(ConsentAnswer::Allow {
            scope: GrantScope::Global,
            write: false,
        }))),
    };
    PluginManager::new(ManagerConfig::new(paths), app)
}

/// A copy of the `echo` fixture folder; `edit` rewrites its manifest.
fn echo_source(dir: &Path, edit: impl Fn(String) -> String) -> PathBuf {
    let src = dir.join("echo-src");
    std::fs::create_dir_all(&src).unwrap();
    let fixture = fixture("echo").dir;
    let manifest = std::fs::read_to_string(fixture.join("plugin.toml")).unwrap();
    std::fs::write(src.join("plugin.toml"), edit(manifest)).unwrap();
    std::fs::copy(fixture.join("plugin.wasm"), src.join("plugin.wasm")).unwrap();
    std::fs::write(src.join("notes.txt"), "not copied").unwrap();
    src
}

fn echo(m: &PluginManager) -> Result<arcode_plugin_host::CallResult, ManagerError> {
    let command = QualifiedId::parse("test.echo/echo").unwrap();
    m.run_command(&command, r#"{"op": "root-name"}"#, None)
        .map(|p| p.wait())
}

#[test]
fn installed_plugins_start_disabled_and_run_only_the_approved_file() {
    let dirs = tempfile::tempdir().unwrap();
    let (mut m, outputs) = manager(dirs.path());
    let id = PluginId::parse("test.echo").unwrap();
    let src = echo_source(dirs.path(), |t| t);
    assert_eq!(m.install_from_folder(&src), Ok(id.clone()));
    let installed = m.registry().get(&id).expect("registered");
    assert_eq!(installed.source, PluginSource::Installed);
    assert!(
        !installed.dir.join("notes.txt").exists(),
        "only plugin files"
    );
    assert_eq!(m.runtime().cache().compiles(), 1, "precompiled at install");
    assert!(!m.is_enabled(&id));
    assert_eq!(echo(&m).err(), Some(ManagerError::Disabled(id.clone())));

    m.set_enabled(&id, true);
    assert_eq!(echo(&m), Ok(Ok(CallOutput::Done)));
    assert_eq!(
        m.runtime().cache().compiles(),
        1,
        "activation used the cache"
    );
    let state = std::fs::read_to_string(dirs.path().join("config/arcode/plugins.toml")).unwrap();
    assert!(
        state.contains("test.echo") && state.contains("sha256"),
        "{state}"
    );

    // Someone swaps plugin.wasm: it does not run until approved again.
    let wasm = m.registry().get(&id).unwrap().entry_path();
    std::fs::copy(fixture("spin").dir.join("plugin.wasm"), &wasm).unwrap();
    m.reload(Vec::new());
    let r = echo(&m).unwrap();
    assert!(
        matches!(&r, Err(CallError::Activation(e)) if e.contains("changed")),
        "{r:?}"
    );
    let asked = std::iter::from_fn(|| outputs.try_recv().ok())
        .any(|o| matches!(o, PluginOutput::NeedsApproval { plugin } if plugin == id));
    assert!(asked, "NeedsApproval was sent");

    // A fresh manager reads plugins.toml: still enabled, still checked.
    drop(m);
    let (m, _outputs) = manager(dirs.path());
    assert!(m.is_enabled(&id));
    assert!(matches!(echo(&m), Ok(Err(CallError::Activation(_)))));
}

#[test]
fn updates_asking_for_more_are_disabled_again() {
    let dirs = tempfile::tempdir().unwrap();
    let (mut m, _outputs) = manager(dirs.path());
    let id = PluginId::parse("test.echo").unwrap();
    m.install_from_folder(&echo_source(dirs.path(), |t| t))
        .unwrap();
    m.set_enabled(&id, true);
    // Same capabilities: stays enabled.
    m.install_from_folder(&echo_source(dirs.path(), |t| t))
        .unwrap();
    assert!(m.is_enabled(&id));
    assert_eq!(echo(&m), Ok(Ok(CallOutput::Done)));
    // More capabilities: disabled until the user enables it again.
    let more = |t: String| t.replace("editor = \"context\"", "editor = \"all-open\"");
    m.install_from_folder(&echo_source(dirs.path(), more))
        .unwrap();
    assert!(!m.is_enabled(&id));
}

#[test]
fn folders_that_are_not_installable_are_refused() {
    let dirs = tempfile::tempdir().unwrap();
    let (mut m, _outputs) = manager(dirs.path());
    let empty = dirs.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        m.install_from_folder(&empty),
        Err(InstallError::Invalid(_))
    ));
    let reserved = echo_source(dirs.path(), |t| t.replace("test.echo", "arcode.echo"));
    let r = m.install_from_folder(&reserved);
    assert!(
        matches!(&r, Err(InstallError::Invalid(e)) if e.contains("reserved")),
        "{r:?}"
    );
    #[cfg(unix)]
    {
        let linked = echo_source(&dirs.path().join("l"), |t| t);
        std::fs::remove_file(linked.join("plugin.wasm")).unwrap();
        std::os::unix::fs::symlink(
            fixture("echo").dir.join("plugin.wasm"),
            linked.join("plugin.wasm"),
        )
        .unwrap();
        let r = m.install_from_folder(&linked);
        assert!(
            matches!(&r, Err(InstallError::Invalid(e)) if e.contains("regular")),
            "{r:?}"
        );
    }
    assert_eq!(m.registry().plugins().count(), 0, "nothing was installed");
}
