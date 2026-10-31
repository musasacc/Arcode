//! Plugin commands in the workspace: the palette's `when` filter, running
//! a command as a user gesture, the Plugins actions (log, reload, load
//! unpacked, cancel; Esc cancels a running task) and failure toasts.

use std::path::Path;

use arcode_plugin_host::CallError;
use arcode_plugin_host::QualifiedId;
use arcode_plugin_host::bindings::DocId;
use arcode_plugin_host::bridge::EditorBridge;
use gpui::{Context, Window};

use super::Workspace;
use crate::plugins::commands;
use crate::plugins::commands::{
    CancelPluginTask, LoadUnpackedPlugin, ReloadPlugins, RunPluginCommand, ShowPluginLog,
    when_holds,
};
use crate::plugins::snapshot::doc_id;

impl Workspace {
    /// The language plugins see for the active editor.
    pub(super) fn plugin_active_language(&self, cx: &gpui::App) -> Option<String> {
        let editor = self.active_editor(cx)?;
        self.plugins
            .client
            .editor()
            .snapshot(doc_id(&editor))
            .map(|s| s.language)
    }

    /// Whether the palette lists the plugin command `id` here.
    pub(crate) fn plugin_command_enabled(&self, id: &str, cx: &gpui::App) -> bool {
        let Some(command) = self
            .plugins
            .contributions
            .commands
            .iter()
            .find(|c| c.id.as_str() == id)
        else {
            return false;
        };
        let language = self.plugin_active_language(cx);
        when_holds(command.when.as_ref(), language.as_deref())
    }

    /// Runs a plugin command as a user gesture about `about` (else the
    /// active document); failures become toasts.
    pub(crate) fn plugin_run(
        &mut self,
        command: &QualifiedId,
        args: &str,
        about: Option<DocId>,
        cx: &mut Context<Self>,
    ) {
        self.plugins_sync_project(cx);
        let active = self.active_editor(cx);
        if let Some(editor) = &active {
            self.plugin_publish(editor, cx);
        }
        let about = about.or_else(|| active.as_ref().map(doc_id));
        let name = self.plugins.client.plugin_name(&plugin_of(command));
        let result = match self.plugins.client.manager() {
            Some(m) => m.run_command(command, args, about),
            None => return,
        };
        match result {
            Ok(pending) => {
                cx.spawn(async move |this, cx| {
                    let Ok(Err(error)) = pending.result.recv().await else {
                        return;
                    };
                    this.update(cx, |ws, cx| ws.plugin_call_failed(&name, error, cx))
                        .ok();
                })
                .detach();
            }
            Err(e) => self.show_toast(format!("{name}: {e}"), cx),
        }
    }

    /// A call failed: say so unless it is noise or a crash (crashes arrive
    /// as their own output).
    pub(super) fn plugin_call_failed(
        &mut self,
        name: &str,
        error: CallError,
        cx: &mut Context<Self>,
    ) {
        match error {
            CallError::Trapped(_) | CallError::Superseded | CallError::Stopped => {}
            other => {
                let message = format!("{name}: {other}");
                self.plugins.log(message.clone());
                self.show_toast(message, cx);
            }
        }
    }

    /// Registers the Plugins actions on the workspace root.
    pub(super) fn register_plugin_actions<E: gpui::InteractiveElement>(
        &self,
        el: E,
        cx: &mut Context<Self>,
    ) -> E {
        el.on_action(cx.listener(|ws, action: &RunPluginCommand, window, cx| {
            if !ws.ensure_plugins(window, cx) {
                return;
            }
            let Some(id) = QualifiedId::parse(&action.id) else {
                return;
            };
            if !ws.plugin_command_enabled(id.as_str(), cx) {
                // E.g. its key pressed outside its language: not ours.
                cx.propagate();
                return;
            }
            ws.plugin_run(&id, "null", None, cx);
        }))
        .on_action(cx.listener(|ws, _: &ShowPluginLog, window, cx| {
            ws.ensure_plugins(window, cx);
            ws.show_plugin_log(window, cx);
        }))
        .on_action(cx.listener(|ws, _: &ReloadPlugins, window, cx| {
            ws.reload_plugins(window, cx);
        }))
        .on_action(cx.listener(|ws, _: &LoadUnpackedPlugin, window, cx| {
            ws.prompt_folder("Load Plugin", window, cx, |ws, path, window, cx| {
                if !ws.plugins.dev_folders.contains(&path) {
                    ws.plugins.dev_folders.push(path);
                }
                ws.reload_plugins(window, cx);
            });
        }))
        .on_action(cx.listener(|ws, _: &CancelPluginTask, _, cx| {
            ws.plugins.status.update(cx, |s, cx| s.cancel_newest(cx));
        }))
    }

    /// Installs (once, when the first task starts) the Esc interceptor:
    /// while a plugin task runs, Esc in this window cancels the newest one
    /// before any key binding sees it (the editor's Escape would otherwise
    /// take it to clear a selection). Esc goes on as usual when no task
    /// runs or a consent prompt or quick pick is open (Esc answers those).
    pub(super) fn plugin_install_esc(&mut self, cx: &mut Context<Self>) {
        if self.plugins.esc_interceptor.is_some() {
            return;
        }
        let Some(window_id) = self.plugins.window else {
            return;
        };
        let this = cx.entity().downgrade();
        let interceptor = cx.intercept_keystrokes(move |event, window, cx| {
            let key = &event.keystroke;
            if key.key != "escape"
                || key.modifiers.modified()
                || window.window_handle().window_id() != window_id
            {
                return;
            }
            let cancelled = this
                .update(cx, |ws, cx| {
                    ws.plugins.prompts_open == 0
                        && ws.plugins.quick_pick.is_none()
                        && ws.plugins.status.update(cx, |s, cx| s.cancel_newest(cx))
                })
                .unwrap_or(false);
            if cancelled {
                cx.stop_propagation();
            }
        });
        self.plugins.esc_interceptor = Some(interceptor);
    }

    pub(super) fn reload_plugins(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ensure_plugins(window, cx) {
            return;
        }
        let dev = self.plugins.dev_folders.clone();
        self.plugins.client.reload(dev);
        self.plugin_retire_panels("Plugins: Reload", Some(window), cx);
        let old = std::mem::take(&mut self.plugins.contributions);
        self.plugins.contributions = self.plugins.client.contributions();
        let gone: Vec<_> = old
            .commands
            .into_iter()
            .map(|c| c.id)
            .filter(|id| {
                !self
                    .plugins
                    .contributions
                    .commands
                    .iter()
                    .any(|c| &c.id == id)
            })
            .collect();
        commands::unregister_commands(&gone, cx);
        commands::register_commands(&self.plugins.contributions.commands, cx);
        for tracked in self.plugins.editors.values_mut() {
            tracked.lens_version = None;
            tracked.lens_targets.clear();
        }
        self.plugin_refresh_visible(cx);
        let count = self
            .plugins
            .client
            .manager()
            .map_or(0, |m| m.registry().plugins().count());
        self.show_toast(format!("Plugins reloaded ({count} found)"), cx);
    }

    /// Whether a path is inside the project (plugins only reach those).
    pub(super) fn plugin_relative(&self, path: &Path) -> Option<String> {
        crate::plugins::snapshot::relative_path(
            self.project.as_ref().map(|p| p.root.as_path()),
            path,
        )
    }
}

/// The plugin id half of a namespaced id.
fn plugin_of(id: &QualifiedId) -> arcode_plugin_host::PluginId {
    arcode_plugin_host::PluginId::parse(id.plugin()).unwrap_or_else(|_| {
        arcode_plugin_host::PluginId::parse("unknown.plugin").expect("valid id")
    })
}
