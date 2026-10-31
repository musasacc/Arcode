//! The workspace side of the plugin system (`crate::plugins`): loading the
//! manager after the first frame, following editors (snapshots and code
//! lenses), running plugin commands, and the Plugins actions. Panels,
//! outputs and editor requests are in `plugins_ui.rs`.

use std::time::Duration;

use arcode_editor::{Editor, EditorEvent};
use arcode_plugin_host::bridge::EditorBridge;
use arcode_plugin_host::{CallError, CallOutput};
use gpui::{Context, Entity, Window};

use super::{Workspace, WorkspaceEvent};
use crate::plugins::lenses::{LENS_DEBOUNCE, LENS_SET, build};
use crate::plugins::snapshot::{doc_id, snapshot};
use crate::plugins::{TrackedEditor, commands};

/// The manager is created this long after the window opens, so startup
/// does not even read manifests.
const LOAD_DELAY: Duration = Duration::from_millis(250);

impl Workspace {
    /// Sets up plugins for a new workspace: the status-bar entry, the
    /// editor-request loop and a deferred, manifest-only load.
    pub(super) fn with_plugins(mut self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        self.plugins.window = Some(window.window_handle().window_id());
        let status = self.plugins.status.clone();
        self.status_bar
            .update(cx, |bar, cx| bar.set_plugin_view(status.into(), cx));
        self._subscriptions
            .push(cx.subscribe_self(|ws, event: &WorkspaceEvent, cx| {
                ws.plugins_workspace_event(event, cx)
            }));
        self.plugins_page_init(window, cx);
        if let Some(requests) = self.plugins.requests.take() {
            let task = cx.spawn_in(window, async move |this, cx| {
                while let Ok(request) = requests.recv().await {
                    let alive = this.update_in(cx, |ws, window, cx| {
                        ws.plugin_editor_request(request, window, cx)
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            });
            self.plugins.tasks.push(task);
        }
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(LOAD_DELAY).await;
            this.update_in(cx, |ws, window, cx| ws.ensure_plugins(window, cx))
                .ok();
        });
        self.plugins.tasks.push(task);
        self
    }

    /// Loads the plugin manager (manifests only) unless loaded; returns
    /// whether plugins are available. Keeps its project in step.
    pub(crate) fn ensure_plugins(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.plugins.client.ensure() {
            self.plugins_loaded(window, cx);
        }
        self.plugins_sync_project(cx);
        self.plugins.client.is_loaded()
    }

    pub(super) fn plugins_loaded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.plugins.client.clone();
        self.plugins_apply_settings(cx);
        self.plugins.contributions = client.contributions();
        commands::register_commands(&self.plugins.contributions.commands, cx);
        client.index().set_files(self.project_files.clone());
        if let Some(outputs) = client.take_outputs() {
            let task = cx.spawn_in(window, async move |this, cx| {
                while let Ok(output) = outputs.recv().await {
                    let alive = this
                        .update_in(cx, |ws, window, cx| ws.on_plugin_output(output, window, cx));
                    if alive.is_err() {
                        break;
                    }
                }
            });
            self.plugins.tasks.push(task);
        }
        self.plugins_sync_project(cx);
        for editor in self.editors(cx) {
            self.plugin_track_editor(&editor, cx);
        }
        self.plugin_refresh_visible(cx);
        self.plugin_update_command_context(cx);
        self.refresh_status(cx);
        cx.notify();
    }

    /// Tells the palette the active document's language (`when` clauses).
    pub(super) fn plugin_update_command_context(&self, cx: &mut Context<Self>) {
        if let Some(window) = self.plugins.window {
            let language = self.plugin_active_language(cx);
            crate::plugins::commands::set_window_language(window, language, cx);
        }
    }

    /// Tells the manager about the open project if it changed; that stops
    /// every instance, so their panels are retired.
    pub(super) fn plugins_sync_project(&mut self, cx: &mut Context<Self>) {
        let (root, name) = match &self.project {
            Some(p) => (Some(p.root.clone()), Some(p.name.clone())),
            None => (None, None),
        };
        if self.plugins.client.set_project(root, name) {
            self.plugin_retire_panels("the project changed", None, cx);
        }
    }

    pub(super) fn plugins_workspace_event(
        &mut self,
        event: &WorkspaceEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.plugins.client.is_loaded() {
            return;
        }
        match event {
            WorkspaceEvent::ProjectChanged => {
                self.plugins_sync_project(cx);
                self.plugins
                    .client
                    .index()
                    .set_files(self.project_files.clone());
                self.plugin_refresh_visible(cx);
            }
            WorkspaceEvent::EditorOpened(editor) => {
                self.plugin_track_editor(editor, cx);
                self.plugin_refresh_visible(cx);
            }
            WorkspaceEvent::ActiveEditorChanged(editor) => {
                let doc = editor.as_ref().map(doc_id);
                if let Some(editor) = editor {
                    self.plugin_publish(editor, cx);
                }
                self.plugins.client.editor().set_focused(doc);
                self.plugin_update_command_context(cx);
                self.plugin_refresh_visible(cx);
            }
        }
    }

    /// Publishes `editor`'s snapshot (UI thread; the rope clone is O(1)).
    pub(super) fn plugin_publish(&self, editor: &Entity<Editor>, cx: &gpui::App) {
        let root = self.project.as_ref().map(|p| p.root.as_path());
        let snap = snapshot(editor, root, &self.plugins.contributions.languages, cx);
        self.plugins.client.editor().publish(snap);
    }

    pub(super) fn plugin_track_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        if self.plugins.editors.contains_key(&id) {
            return;
        }
        let events = cx.subscribe(editor, |ws, editor, event: &EditorEvent, cx| {
            ws.plugin_editor_event(&editor, event, cx)
        });
        let doc = doc_id(editor);
        let released = cx.observe_release(editor, move |ws, _, _| {
            ws.plugins.editors.remove(&id);
            ws.plugins.client.editor().forget(doc);
        });
        self.plugins.editors.insert(
            id,
            TrackedEditor {
                editor: editor.downgrade(),
                lens_targets: Default::default(),
                lens_task: None,
                lens_version: None,
                pending_click: None,
                subs: vec![events, released],
            },
        );
        self.plugin_publish(editor, cx);
    }

    pub(super) fn plugin_editor_event(
        &mut self,
        editor: &Entity<Editor>,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::Edited { .. } => {
                // The rows move with their lines at once, but the targets
                // (opaque plugin args, e.g. a line number) are for the old
                // text until the plugin answers again.
                if let Some(tracked) = self.plugins.editors.get_mut(&editor.entity_id()) {
                    tracked.lens_targets.clear();
                }
                self.plugin_publish(editor, cx);
                self.plugin_schedule_lenses(editor, true, cx);
            }
            EditorEvent::CodeLensClicked { set, line, id } if set == LENS_SET => {
                self.plugin_lens_clicked(editor, *line, *id, cx)
            }
            _ => {}
        }
    }

    /// Runs the lens `id` on `line`. After an edit its target is gone: the
    /// click is remembered and runs once the lenses of the current text
    /// are back, if one with the same label is still on that line.
    fn plugin_lens_clicked(
        &mut self,
        editor: &Entity<Editor>,
        line: usize,
        id: u64,
        cx: &mut Context<Self>,
    ) {
        let key = editor.entity_id();
        let target = self
            .plugins
            .editors
            .get(&key)
            .and_then(|t| t.lens_targets.get(&id))
            .cloned();
        if let Some(target) = target {
            self.plugin_publish(editor, cx);
            self.plugin_run(&target.command, &target.args, Some(doc_id(editor)), cx);
            return;
        }
        let label = editor
            .read(cx)
            .code_lenses(LENS_SET)
            .iter()
            .flat_map(|l| &l.items)
            .find(|item| item.id == id)
            .map(|item| item.label.clone());
        let (Some(label), Some(tracked)) = (label, self.plugins.editors.get_mut(&key)) else {
            return;
        };
        tracked.pending_click = Some((line, label));
        self.plugin_schedule_lenses(editor, false, cx);
    }

    /// Runs a click remembered by [`Self::plugin_lens_clicked`] if a lens
    /// of the fresh `set` matches it.
    fn plugin_run_pending_click(
        &mut self,
        editor: &Entity<Editor>,
        click: (usize, gpui::SharedString),
        set: &crate::plugins::lenses::LensSet,
        cx: &mut Context<Self>,
    ) {
        let (line, label) = click;
        let target = set
            .lenses
            .iter()
            .filter(|l| l.line == line)
            .flat_map(|l| &l.items)
            .find(|item| item.label == label)
            .and_then(|item| set.targets.get(&item.id))
            .cloned();
        if let Some(target) = target {
            self.plugin_run(&target.command, &target.args, Some(doc_id(editor)), cx);
        }
    }

    /// Whether `editor` is the shown tab of some pane.
    pub(super) fn plugin_editor_visible(&self, editor: &Entity<Editor>, cx: &gpui::App) -> bool {
        self.panes
            .iter()
            .any(|p| p.read(cx).active_editor() == Some(editor))
    }

    /// Asks for lenses of every visible editor whose shown lenses are not
    /// for its current version.
    pub(super) fn plugin_refresh_visible(&mut self, cx: &mut Context<Self>) {
        let visible: Vec<Entity<Editor>> = self
            .panes
            .iter()
            .filter_map(|p| p.read(cx).active_editor().cloned())
            .collect();
        for editor in visible {
            let version = editor.read(cx).buffer().version();
            let current = self
                .plugins
                .editors
                .get(&editor.entity_id())
                .is_some_and(|t| t.lens_version == Some(version));
            if !current {
                self.plugin_schedule_lenses(&editor, false, cx);
            }
        }
    }

    /// Requests lenses for `editor` now or after the edit debounce, if a
    /// plugin provides lenses for its language and it is visible.
    pub(super) fn plugin_schedule_lenses(
        &mut self,
        editor: &Entity<Editor>,
        debounce: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(language) = self
            .plugins
            .client
            .editor()
            .snapshot(doc_id(editor))
            .map(|s| s.language)
        else {
            return;
        };
        // Plain text is never handed to a lens provider: it may be a key,
        // a certificate or notes (see `snapshot::language_for`).
        let wanted = language != arcode_syntax::plain_text().id
            && self
                .plugins
                .client
                .manager()
                .is_some_and(|m| !m.registry().lens_providers(&language).is_empty());
        if !wanted || !self.plugin_editor_visible(editor, cx) {
            return;
        }
        let weak = editor.downgrade();
        let task = cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor().timer(LENS_DEBOUNCE).await;
            }
            let Some(editor) = weak.upgrade() else {
                return;
            };
            this.update(cx, |ws, cx| ws.plugin_request_lenses(&editor, cx))
                .ok();
        });
        if let Some(tracked) = self.plugins.editors.get_mut(&editor.entity_id()) {
            tracked.lens_task = Some(task);
        }
    }

    pub(super) fn plugin_request_lenses(
        &mut self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) {
        self.plugin_publish(editor, cx);
        let version = editor.read(cx).buffer().version();
        let calls = match self.plugins.client.manager() {
            Some(m) => m.code_lenses(doc_id(editor)),
            None => return,
        };
        if let Some(tracked) = self.plugins.editors.get_mut(&editor.entity_id()) {
            tracked.lens_version = Some(version);
        }
        let weak = editor.downgrade();
        let task = cx.spawn(async move |this, cx| {
            let mut answers = Vec::new();
            for call in calls {
                match call.pending.result.recv().await {
                    Ok(Ok(CallOutput::Lenses(lenses))) => answers.push((call.plugin, lenses)),
                    Ok(Err(CallError::Superseded)) => return,
                    Ok(Err(e)) => log::info!("lenses from {}: {e}", call.plugin),
                    _ => {}
                }
            }
            let Some(editor) = weak.upgrade() else {
                return;
            };
            this.update(cx, |ws, cx| {
                let line_count = editor.read(cx).line_count();
                let click = ws
                    .plugins
                    .editors
                    .get_mut(&editor.entity_id())
                    .and_then(|t| t.pending_click.take());
                if editor.read(cx).buffer().version() != version {
                    return;
                }
                let set = build(answers, line_count);
                editor.update(cx, |e, cx| {
                    e.set_code_lenses(LENS_SET, set.lenses.clone(), cx)
                });
                if let Some(click) = click {
                    ws.plugin_run_pending_click(&editor, click, &set, cx);
                }
                if let Some(tracked) = ws.plugins.editors.get_mut(&editor.entity_id()) {
                    tracked.lens_targets = set.targets;
                }
            })
            .ok();
        });
        if let Some(tracked) = self.plugins.editors.get_mut(&editor.entity_id()) {
            tracked.lens_task = Some(task);
        }
    }
}
