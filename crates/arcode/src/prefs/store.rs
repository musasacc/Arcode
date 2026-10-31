//! Writing settings changed in the UI to `config.toml`.
//!
//! Each change is applied to the file in place (comments kept, see
//! `arcode_config::apply_edits`) on the background executor, the file is
//! loaded again there, and the result becomes the active configuration on
//! the UI thread. Changes (and reloads) wait in one queue and run one at a
//! time in the order they were made, so the last change wins both in the
//! file and on screen.

use std::collections::VecDeque;
use std::path::PathBuf;

use arcode_config::config::edit::edit_config_file;
use arcode_config::{Config, SettingEdit, SettingValue, apply_edits};
use gpui::{App, AppContext, Global, Task};

/// Changes a file's text (`Err` leaves the file alone).
type EditFn = Box<dyn FnOnce(&str) -> Result<String, String> + Send>;

/// One queued change.
struct Job {
    path: PathBuf,
    /// `None` only reloads the file.
    edit: Option<EditFn>,
    /// The setting edits, for [`pending_value`].
    edits: Vec<SettingEdit>,
    done: async_channel::Sender<Result<(), String>>,
}

/// The queue of changes.
#[derive(Default)]
struct ConfigWriter {
    queue: VecDeque<Job>,
    /// Edits of the job being written.
    current: Vec<SettingEdit>,
    running: bool,
}

impl Global for ConfigWriter {}

/// Overrides the file written (tests); `None` uses
/// `arcode_config::paths::config_file`.
pub struct ConfigPathOverride(pub PathBuf);

impl Global for ConfigPathOverride {}

/// The configuration file the Settings page edits.
pub fn config_path(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<ConfigPathOverride>() {
        Some(path) => Some(path.0.clone()),
        None => arcode_config::paths::config_file(),
    }
}

/// Writes `edits` to the configuration file and applies the result. The
/// task resolves to an error message when the file could not be changed
/// (for example when it has a TOML syntax error).
pub fn write_settings(edits: Vec<SettingEdit>, cx: &mut App) -> Task<Result<(), String>> {
    let applied = edits.clone();
    let edit: EditFn = Box::new(move |source| apply_edits(source, &applied));
    enqueue(Some(edit), edits, cx)
}

/// Changes the configuration file with `edit` (queued with the other
/// writes) and applies the result.
pub fn edit_config(
    edit: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
    cx: &mut App,
) -> Task<Result<(), String>> {
    enqueue(Some(Box::new(edit)), Vec::new(), cx)
}

/// Loads `config.toml` again on the background executor and applies it
/// (after the writes queued before).
pub fn reload(cx: &mut App) {
    enqueue(None, Vec::new(), cx).detach();
}

/// The last value written or waiting to be written for `path`, while a
/// write is queued: the value a control should step from, as the active
/// settings lag behind quick clicks.
pub fn pending_value(path: &str, cx: &App) -> Option<SettingValue> {
    let writer = cx.try_global::<ConfigWriter>()?;
    let queued = writer.queue.iter().flat_map(|job| job.edits.iter());
    writer
        .current
        .iter()
        .chain(queued)
        .rfind(|e| e.path == path && e.guard.is_none())
        .and_then(|e| e.value.clone())
}

fn enqueue(
    edit: Option<EditFn>,
    edits: Vec<SettingEdit>,
    cx: &mut App,
) -> Task<Result<(), String>> {
    let Some(path) = config_path(cx) else {
        return Task::ready(Err("No configuration folder is available".into()));
    };
    let (done, result) = async_channel::bounded(1);
    let writer = cx.default_global::<ConfigWriter>();
    writer.queue.push_back(Job {
        path,
        edit,
        edits,
        done,
    });
    if !writer.running {
        writer.running = true;
        cx.spawn(async move |cx| run_queue(cx).await).detach();
    }
    cx.background_spawn(async move {
        result
            .recv()
            .await
            .unwrap_or_else(|_| Err("The application is closing".into()))
    })
}

/// Runs the queued jobs one after the other until the queue is empty.
async fn run_queue(cx: &mut gpui::AsyncApp) {
    loop {
        let next = cx.update(|cx| {
            let writer = cx.default_global::<ConfigWriter>();
            let job = writer.queue.pop_front();
            match &job {
                Some(job) => writer.current = job.edits.clone(),
                None => writer.running = false,
            }
            job
        });
        let Ok(Some(job)) = next else {
            return;
        };
        let Job {
            path, edit, done, ..
        } = job;
        let loaded = cx
            .background_spawn(async move {
                if let Some(edit) = edit {
                    edit_config_file(&path, edit)
                        .map_err(|err| format!("Cannot update config.toml: {err}"))?;
                }
                Ok::<_, String>(Config::load_from(&path))
            })
            .await;
        let result = cx.update(|cx| {
            cx.default_global::<ConfigWriter>().current.clear();
            loaded.map(|(config, warnings)| crate::app::apply_settings(config, &warnings, cx))
        });
        let result = result.unwrap_or_else(|_| Err("The application is closing".into()));
        done.try_send(result).ok();
    }
}

/// Writes `edits` and shows a failure as a toast in the window of `ws`.
pub fn write_and_report(
    edits: Vec<SettingEdit>,
    ws: gpui::WeakEntity<crate::workspace::Workspace>,
    cx: &mut App,
) {
    let task = write_settings(edits, cx);
    cx.spawn(async move |cx| {
        if let Err(err) = task.await {
            ws.update(cx, |ws, cx| ws.show_toast(err, cx)).ok();
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    async fn writes_apply_in_order_and_keep_comments(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# keep me\n[editor]\nfont_size = 14.0\n").unwrap();
        crate::test_support::init(cx);
        cx.update(|cx| cx.set_global(ConfigPathOverride(path.clone())));
        let first =
            cx.update(|cx| write_settings(vec![SettingEdit::set("editor.font_size", 15.0f32)], cx));
        let second =
            cx.update(|cx| write_settings(vec![SettingEdit::set("editor.font_size", 18.0f32)], cx));
        cx.update(|cx| {
            let pending = pending_value("editor.font_size", cx);
            assert_eq!(pending, Some(SettingValue::from(18.0f32)));
        });
        first.await.unwrap();
        second.await.unwrap();
        cx.run_until_parked();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# keep me\n"), "{text}");
        assert!(text.contains("font_size = 18.0"), "{text}");
        cx.update(|cx| {
            assert_eq!(arcode_ui_kit::settings(cx).editor.font_size, 18.0);
            assert_eq!(pending_value("editor.font_size", cx), None);
        });
    }

    #[gpui::test]
    async fn a_broken_file_is_reported_and_left_alone(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[editor\n").unwrap();
        crate::test_support::init(cx);
        cx.update(|cx| cx.set_global(ConfigPathOverride(path.clone())));
        let task =
            cx.update(|cx| write_settings(vec![SettingEdit::set("ui.font_size", 15.0f32)], cx));
        let err = task.await.unwrap_err();
        assert!(err.contains("not valid TOML"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[editor\n");
    }

    #[gpui::test]
    async fn a_reload_runs_after_the_writes_before_it(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[ui]\nfont_size = 14.0\n").unwrap();
        crate::test_support::init(cx);
        cx.update(|cx| cx.set_global(ConfigPathOverride(path.clone())));
        let write =
            cx.update(|cx| write_settings(vec![SettingEdit::set("ui.font_size", 17.0f32)], cx));
        cx.update(reload);
        write.await.unwrap();
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(arcode_ui_kit::settings(cx).ui.font_size, 17.0));
    }
}
