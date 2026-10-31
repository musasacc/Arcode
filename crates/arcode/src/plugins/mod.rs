//! The WASM plugin system in the app (docs/PLUGINS.md §13 step 9): the
//! GPUI side of `arcode-plugin-host`, which itself has no GPUI.
//!
//! Each workspace owns a [`Plugins`]: a [`PluginClient`] whose
//! `PluginManager` is created lazily (manifests only; the wasmtime engine
//! starts on the first activation), the editors it publishes snapshots
//! of, the plugin panels it shows and the status-bar entries. The
//! workspace wires it up in `workspace/plugins*.rs`:
//!
//! - **Editor bridge** ([`bridge`], [`snapshot`]): document snapshots for
//!   `editor.text`, edits and opened files as requests to the UI thread.
//! - **Commands** ([`commands`]): contributed commands in the palette.
//! - **Lenses** ([`lenses`]): "▶ Send request" rows above lines.
//! - **Openers and panels** ([`panel`]): `.db` files open in the DB
//!   viewer; plugin views render as tabs, sidebar entries or panels.
//! - **Status** ([`status`]): status items and the task chip (Esc or a
//!   click cancels).
//! - **Consent** ([`consent`]): interim host prompts.
//! - **Log** ([`log`]): "Plugins: Show Log".

pub mod bridge;
pub mod client;
pub mod commands;
pub mod consent;
pub mod lenses;
pub mod log;
pub mod panel;
pub mod paths;
pub mod snapshot;
pub mod status;

use std::collections::HashMap;
use std::path::PathBuf;

use arcode_editor::Editor;
use arcode_plugin_host::bindings::PanelId;
use arcode_plugin_host::cancel::ReplySender;
use arcode_plugin_host::{PluginId, QualifiedId};
use gpui::{AppContext, Context, Entity, EntityId, SharedString, Subscription, Task, WeakEntity};

pub use client::{Contributions, PluginClient, SidebarPanel};
pub use commands::RunPluginCommand;
pub use panel::{PanelView, PanelViewEvent};
pub use status::PluginStatus;

use crate::list_picker::ListPicker;
use lenses::LensTarget;

/// Where a quick pick's answer goes (the plugin thread waits on it).
pub type QuickPickReply = ReplySender<Option<u32>>;

/// A plugin panel key: owner plus host-allocated id.
pub type PanelKey = (PluginId, PanelId);

/// An open editor the plugins know about.
pub struct TrackedEditor {
    /// The editor.
    pub editor: WeakEntity<Editor>,
    /// Lens click targets by lens item id.
    pub lens_targets: HashMap<u64, LensTarget>,
    /// The debounced lens request.
    pub lens_task: Option<Task<()>>,
    /// Buffer version the shown lenses were asked for.
    pub lens_version: Option<u64>,
    /// Editor events and release.
    pub subs: Vec<Subscription>,
}

/// Where a plugin panel is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// A tab of a pane (key of its page).
    Page(SharedString),
    /// The sidebar (an activity-bar entry).
    Sidebar(QualifiedId),
}

/// A shown plugin panel.
pub struct PanelSlot {
    /// The view (held strongly by the pane or the sidebar only).
    pub view: WeakEntity<PanelView>,
    /// Where it is.
    pub placement: Placement,
    /// Its events (messages to toast).
    pub sub: Subscription,
}

/// Longest host log kept (lines).
pub const MAX_HOST_LOG: usize = 2000;

/// The plugin state of one workspace.
pub struct Plugins {
    /// The manager handle.
    pub client: PluginClient,
    /// What the registry contributes (after loading).
    pub contributions: Contributions,
    /// Editors by entity id.
    pub editors: HashMap<EntityId, TrackedEditor>,
    /// Shown panels.
    pub panels: HashMap<PanelKey, PanelSlot>,
    /// Files open in an opener's panel.
    pub opened_files: HashMap<PathBuf, PanelKey>,
    /// Sidebar views by contribution (kept while hidden).
    pub sidebar_views: HashMap<QualifiedId, Entity<PanelView>>,
    /// The sidebar panel shown while the activity is `Extension`.
    pub sidebar_active: Option<usize>,
    /// Status items and task chips.
    pub status: Entity<PluginStatus>,
    /// The open quick pick and where its answer goes.
    pub quick_pick: Option<(Entity<ListPicker>, Option<QuickPickReply>)>,
    /// Host messages for the plugin log (notifications, crashes, unloads).
    pub host_log: Vec<String>,
    /// "Load Unpacked" folders of this session.
    pub dev_folders: Vec<PathBuf>,
    /// Background loops (outputs, editor requests, startup).
    pub tasks: Vec<Task<()>>,
    /// The window, for the palette's `when` filter.
    pub window: Option<gpui::WindowId>,
    /// The editor-request receiver until its loop starts.
    pub requests: Option<async_channel::Receiver<bridge::EditorRequest>>,
}

impl Plugins {
    /// Nothing is read or started here (see [`PluginClient`]).
    pub fn new<T: 'static>(cx: &mut Context<T>) -> Self {
        let (client, requests) = PluginClient::new();
        Self {
            client,
            contributions: Contributions::default(),
            editors: HashMap::new(),
            panels: HashMap::new(),
            opened_files: HashMap::new(),
            sidebar_views: HashMap::new(),
            sidebar_active: None,
            status: cx.new(|_| PluginStatus::default()),
            quick_pick: None,
            host_log: Vec::new(),
            dev_folders: paths::dev_folders_from_env(),
            tasks: Vec::new(),
            window: None,
            requests: Some(requests),
        }
    }

    /// Adds a line to the host part of the plugin log.
    pub fn log(&mut self, line: impl Into<String>) {
        if self.host_log.len() >= MAX_HOST_LOG {
            self.host_log.remove(0);
        }
        self.host_log.push(line.into());
    }

    /// The page key of a panel's tab.
    pub fn page_key(key: &PanelKey) -> SharedString {
        format!("plugin\u{0}{}\u{0}{}", key.0, key.1).into()
    }
}
