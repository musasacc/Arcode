//! Plugin changes made in one window reach the others.
//!
//! Every window has its own plugin manager. Each one reads the shared
//! files again before it writes them (`plugins.toml`, the grants files),
//! so no window puts back what another one changed; and when a window
//! enables, disables, installs or uninstalls a plugin or revokes a grant,
//! it tells the other windows, which catch up at once: a plugin disabled
//! elsewhere stops and everything it showed goes away, a revoked file
//! grant recycles its instance. Nothing runs between changes.

use gpui::{Context, Window};

use super::Workspace;

impl Workspace {
    /// Tells the other windows that this one saved a plugin change.
    pub(super) fn plugins_tell_other_windows(&self, window: &Window, cx: &mut Context<Self>) {
        let me = window.window_handle().window_id();
        cx.defer(move |cx| {
            for handle in crate::windows::workspace_windows(cx) {
                if handle.window_id() != me {
                    handle
                        .update(cx, |ws, window, cx| {
                            ws.plugins_changed_elsewhere(window, cx)
                        })
                        .ok();
                }
            }
        });
    }

    /// Catches up with a change another window saved.
    pub(crate) fn plugins_changed_elsewhere(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plugins.client.is_loaded() {
            // It reads the files when it loads.
            return;
        }
        let gone = self.plugins.client.sync_from_disk();
        for plugin in &gone {
            self.plugins_take_down(plugin, window, cx);
        }
        self.plugin_sync_contributions(cx);
        self.plugins_page_refresh(cx);
    }
}
